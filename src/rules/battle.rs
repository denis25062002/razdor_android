//! Card battles, following the original's rules (mechanics.md 2 and 3.3).
//!
//! Each side stands in a [`Formation`] (front, back and optionally a reserve row). Units act
//! one at a time by initiative; each has `Manevres` actions per turn, spent on an attack, a
//! spell or a step. Damage is deterministic. The battle ends when a side is wiped out, or
//! undecided after `BattleEndTurn` turns. There is no retreat.
//!
//! Choices where the original is unknown are marked *(guess)*.

use std::sync::Arc;

use super::content::{Bonus, Content, HeroClass, MagicSchool, Nature, Stat, UnitId, MAX_XP_GAIN};
use super::formation::{Formation, Row, Slot};
use super::units::{Stats, Unit};

/// How many turns a blessing or curse lasts, the turn it is cast included *(guess: the
/// original's duration is unknown)*.
pub const EFFECT_TURNS: u32 = 3;
/// Share of max HP a poisoned unit loses each turn (bonus `Poison`).
const POISON_PERCENT: i32 = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Team {
    Player,
    Enemy,
}

impl Team {
    pub fn other(self) -> Team {
        match self {
            Team::Player => Team::Enemy,
            Team::Enemy => Team::Player,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ongoing,
    Victory,
    Defeat,
    /// `BattleEndTurn` reached with both sides standing.
    Stalemate,
}

/// What a unit does to a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    /// Warrior strike at an enemy front cell in columns c−1..c+1.
    Melee,
    /// Warrior strike through three empty cells at the nearest front unit on either side:
    /// halves the target's defence; `FlankStrike` doubles the attack.
    LongStrike,
    Shot,
    /// Hostile magic damage.
    Strike,
    /// Hostile magic debuff.
    Curse,
    Heal,
    /// Friendly magic buff.
    Bless,
}

impl ActionKind {
    pub fn is_physical(self) -> bool {
        matches!(self, ActionKind::Melee | ActionKind::LongStrike | ActionKind::Shot)
    }

    pub fn is_hostile(self) -> bool {
        !matches!(self, ActionKind::Heal | ActionKind::Bless)
    }

    pub fn label(self) -> &'static str {
        match self {
            ActionKind::Melee => "strike",
            ActionKind::LongStrike => "long strike",
            ActionKind::Shot => "shoot",
            ActionKind::Strike => "magic strike",
            ActionKind::Curse => "curse",
            ActionKind::Heal => "heal",
            ActionKind::Bless => "bless",
        }
    }
}

/// Stat changes of a blessing (positive) or curse (negative).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buff {
    /// Added to melee and ranged attack (only those the unit has).
    pub attack: i32,
    /// Added to melee and ranged defence.
    pub defence: i32,
    pub initiative: i32,
    pub actions: i32,
}

impl Buff {
    pub fn is_empty(&self) -> bool {
        *self == Buff::default()
    }

    /// "Attack -3, Defence -5".
    pub fn describe(&self) -> String {
        let parts: Vec<String> = [
            ("Attack", self.attack),
            ("Defence", self.defence),
            ("Initiative", self.initiative),
            ("Actions", self.actions),
        ]
        .iter()
        .filter(|(_, v)| *v != 0)
        .map(|(n, v)| format!("{n} {v:+}"))
        .collect();
        parts.join(", ")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Timed {
    buff: Buff,
    /// Last turn it is in effect.
    until: u32,
}

/// The expected effect of an action, for hover previews.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preview {
    Damage(i32),
    Heal(i32),
    Buff(Buff),
}

#[derive(Clone, Debug)]
pub struct Fighter {
    pub unit: UnitId,
    pub name: String,
    pub team: Team,
    pub hp: i32,
    pub slot: Slot,
    /// The player's hero (squad index 0).
    pub is_hero: bool,
    /// Index into the player's squad, for writing results back.
    pub squad_index: Option<usize>,
    pub level: i32,
    /// Stats at the start of the battle (level, items, potions).
    pub base: Stats,
    /// Current stats: base with magic drain, blessing and curse.
    pub stats: Stats,
    /// Magic power left after the per-turn drain.
    pub power: i32,
    blessing: Option<Timed>,
    curse: Option<Timed>,
    pub poisoned: bool,
    /// Damage dealt plus HP healed, for the XP split.
    pub dealt: i32,
    /// Strength estimate at the start, for XP.
    pub tactical: i32,
    /// Cell after deployment; written back to the squad.
    deployed: Slot,
}

impl Fighter {
    fn new(content: &Content, unit: &Unit, team: Team, squad_index: Option<usize>) -> Fighter {
        let base = unit.stats(content);
        Fighter {
            unit: unit.def,
            name: unit.name(content).to_string(),
            team,
            hp: unit.hp.min(base.max_hp()),
            slot: unit.slot,
            is_hero: squad_index == Some(0),
            squad_index,
            level: unit.level,
            power: base[Stat::MagicPower],
            stats: base.clone(),
            base,
            blessing: None,
            curse: None,
            poisoned: false,
            dealt: 0,
            tactical: content.tactical_cost(unit.def, unit.level),
            deployed: unit.slot,
        }
    }

    pub fn alive(&self) -> bool {
        self.hp > 0
    }

    pub fn max_hp(&self) -> i32 {
        self.stats.max_hp()
    }

    pub fn blessing(&self) -> Option<Buff> {
        self.blessing.map(|t| t.buff)
    }

    pub fn curse(&self) -> Option<Buff> {
        self.curse.map(|t| t.buff)
    }

    fn has_attack(&self) -> bool {
        self.base[Stat::AttackBlow] > 0 || self.base[Stat::AttackShot] > 0
    }
}

/// The result of one action, for the UI to animate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub target: usize,
    pub kind: ActionKind,
    /// Damage dealt or HP restored.
    pub amount: i32,
    /// Blessing or curse applied.
    pub buff: Buff,
    pub killed: bool,
    /// Counterblow damage taken by the actor.
    pub counter: Option<i32>,
    /// The actor died: killed a `DeathCurse`/`Ghost` unit, or fell to the counterblow.
    pub actor_died: bool,
}

/// One AI action, for the UI to animate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Act { actor: usize, hit: Hit },
    Move { actor: usize, from: Slot, to: Slot },
    Wait { actor: usize },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ActionError {
    NotDeploying,
    InvalidTarget,
    NotYourTurn,
}

/// A unit's share of the XP after the battle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XpAward {
    pub fighter: usize,
    pub xp: i32,
}

/// Where a player's unit ended up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FighterResult {
    pub squad_index: usize,
    /// 0 when dead. A fallen hero whose army survived comes back with 1.
    pub hp: i32,
    pub slot: Slot,
}

pub struct Battle {
    content: Arc<Content>,
    pub formation: Formation,
    pub fighters: Vec<Fighter>,
    /// Battle turn (all units act once per turn), 1-based once the fight starts.
    pub round: u32,
    pub log: Vec<String>,
    /// The side that started the fight: +1 initiative.
    pub attacker: Team,
    building_defence: [i32; 2],
    order: Vec<usize>,
    turn: usize,
    actions_left: i32,
    deploying: bool,
    stalemate: bool,
}

/// Bonuses whose attacks ignore the target's defence.
const PIERCING: [Bonus; 4] = [Bonus::ArmorIgnore, Bonus::VampirsGist, Bonus::OldVampirsGist, Bonus::Artillery];
/// Bonuses that give +1 action on the first turn.
const FAST_START: [Bonus; 3] = [Bonus::HorseAtack, Bonus::OldVampirsGist, Bonus::FastDead];

/// `f(P)` of mechanics.md 3.3: actions added or removed by Elemental magic.
fn actions_of_power(p: i32) -> i32 {
    match p {
        ..=19 => 0,
        20..=44 => 1,
        45..=99 => 2,
        _ => 3,
    }
}

fn god_bonus(s: &Stats) -> i32 {
    10 * i32::from(s.has(&Bonus::GodAnger)) + 20 * i32::from(s.has(&Bonus::GodStrike))
}

impl Battle {
    /// `player` entries are (squad index, unit); squad index 0 is the hero. Starts in the
    /// deploy phase; call [`Battle::begin`] to fight.
    pub fn new(content: Arc<Content>, player: &[(usize, &Unit)], enemies: &[Unit], attacker: Team) -> Battle {
        let mut fighters: Vec<Fighter> =
            player.iter().map(|&(idx, u)| Fighter::new(&content, u, Team::Player, Some(idx))).collect();
        fighters.extend(enemies.iter().map(|u| Fighter::new(&content, u, Team::Enemy, None)));
        Battle {
            formation: content.formation,
            content,
            fighters,
            round: 0,
            log: Vec::new(),
            attacker,
            building_defence: [0; 2],
            order: Vec::new(),
            turn: 0,
            actions_left: 0,
            deploying: true,
            stalemate: false,
        }
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    /// Extra defence of the building `team` fights in (garrisons). Set before [`Battle::begin`].
    pub fn set_building_defence(&mut self, team: Team, defence: i32) {
        self.building_defence[team.index()] = defence;
    }

    pub fn is_deploying(&self) -> bool {
        self.deploying
    }

    /// Living fighter of `team` standing on `slot`.
    pub fn at(&self, team: Team, slot: Slot) -> Option<usize> {
        self.fighters.iter().position(|f| f.alive() && f.team == team && f.slot == slot)
    }

    /// Deploy phase: move a player card to `to`, swapping with whoever is there.
    pub fn move_card(&mut self, from: Slot, to: Slot) -> Result<(), ActionError> {
        if !self.deploying {
            return Err(ActionError::NotDeploying);
        }
        if !self.formation.contains(to) {
            return Err(ActionError::InvalidTarget);
        }
        let a = self.at(Team::Player, from).ok_or(ActionError::InvalidTarget)?;
        if let Some(b) = self.at(Team::Player, to) {
            self.fighters[b].slot = from;
        }
        self.fighters[a].slot = to;
        Ok(())
    }

    /// End deployment and start turn 1.
    pub fn begin(&mut self) {
        if !self.deploying {
            return;
        }
        self.deploying = false;
        for f in &mut self.fighters {
            f.deployed = f.slot;
            // Garrison: stats ×2 inside a strong building (mechanics.md 1.3).
            if f.base.has(&Bonus::Garrison) && self.building_defence[f.team.index()] >= 10 {
                for st in [Stat::AttackBlow, Stat::AttackShot, Stat::DefenceBlow, Stat::DefenceShot] {
                    f.base[st] *= 2;
                }
                f.stats = f.base.clone();
            }
        }
        self.collapse(Team::Player);
        self.collapse(Team::Enemy);
        self.next_turn();
    }

    fn opt(&self) -> &super::content::GlobalOptions {
        &self.content.options
    }

    fn living(&self, team: Team) -> impl Iterator<Item = &Fighter> {
        self.fighters.iter().filter(move |f| f.alive() && f.team == team)
    }

    pub fn outcome(&self) -> Outcome {
        if self.living(Team::Player).next().is_none() {
            Outcome::Defeat
        } else if self.living(Team::Enemy).next().is_none() {
            Outcome::Victory
        } else if self.stalemate {
            Outcome::Stalemate
        } else {
            Outcome::Ongoing
        }
    }

    /// Fighter whose turn it is; `None` while deploying or once the battle is over.
    pub fn active(&self) -> Option<usize> {
        if self.deploying || self.outcome() != Outcome::Ongoing {
            return None;
        }
        self.order.get(self.turn).copied()
    }

    pub fn actions_left(&self) -> i32 {
        self.actions_left
    }

    /// Upcoming fighters this turn, starting with the active one.
    pub fn queue(&self) -> impl Iterator<Item = usize> + '_ {
        self.order.iter().skip(self.turn).copied().filter(|&i| self.fighters[i].alive())
    }

    /// Turn order key: `Artillery` first, then initiative (attacker +1), attacker on ties.
    fn order_key(&self, i: usize) -> (bool, i32, bool, usize) {
        let f = &self.fighters[i];
        let ini = f.stats[Stat::Initiative] + i32::from(f.team == self.attacker);
        (!f.stats.has(&Bonus::Artillery), -ini, f.team != self.attacker, i)
    }

    /// Starts the next battle turn; false once `BattleEndTurn` is reached.
    fn start_round(&mut self) -> bool {
        if self.round >= self.opt().battle_end_turn.max(1) as u32 {
            self.stalemate = true;
            self.log.push(format!("Turn {} ends the battle undecided", self.round));
            return false;
        }
        self.round += 1;
        if self.round > 1 {
            self.turn_effects();
        }
        let mut order: Vec<usize> = (0..self.fighters.len()).filter(|&i| self.fighters[i].alive()).collect();
        order.sort_by_key(|&i| self.order_key(i));
        self.order = order;
        self.turn = 0;
        self.log.push(format!("-- Turn {} --", self.round));
        true
    }

    /// Start-of-turn upkeep from turn 2: effects expire, magic power drains, regeneration
    /// and poison.
    fn turn_effects(&mut self) {
        let round = self.round;
        for i in 0..self.fighters.len() {
            let (dec, floor) = {
                let f = &self.fighters[i];
                match f.base.magic.filter(|_| f.base.is_mage()) {
                    Some(school) => (
                        f.base.mana_drain.unwrap_or(self.opt().dec_spell(school)),
                        f.base.min_magic_power.unwrap_or(self.opt().min_spell(school)),
                    ),
                    None => (0, 0),
                }
            };
            let f = &mut self.fighters[i];
            if !f.alive() {
                continue;
            }
            if f.blessing.is_some_and(|t| t.until < round) {
                f.blessing = None;
            }
            if f.curse.is_some_and(|t| t.until < round) {
                f.curse = None;
            }
            // Magic power drains to the floor, never below (or up to) it.
            if f.power > floor {
                f.power = (f.power - dec).max(floor);
            }
            let max = f.base.max_hp();
            // Regen: percent of max HP per turn (guess: the original's timing is unverified).
            let regen = f.base[Stat::Regen];
            if regen > 0 && f.hp < max {
                let healed = (max * regen / 100).max(1).min(max - f.hp);
                f.hp += healed;
                self.log.push(format!("{} regenerates +{healed}", f.name));
            }
            if f.poisoned {
                let loss = (max * POISON_PERCENT / 100).max(1).min(f.hp);
                f.hp -= loss;
                self.log.push(format!("{} suffers {loss} from poison", f.name));
            }
            self.refresh(i);
        }
        self.collapse(Team::Player);
        self.collapse(Team::Enemy);
    }

    /// Actions `id` gets this turn: `Manevres`, +1 on turn 1 for the fast-start bonuses.
    fn turn_actions(&self, id: usize) -> i32 {
        let f = &self.fighters[id];
        f.stats[Stat::Manevres] + i32::from(self.round == 1 && f.stats.has_any(&FAST_START))
    }

    /// Moves on to the next fighter able to act, starting new turns as needed.
    fn next_turn(&mut self) {
        loop {
            if self.outcome() != Outcome::Ongoing {
                return;
            }
            if self.turn >= self.order.len() {
                if !self.start_round() {
                    return;
                }
                continue;
            }
            let id = self.order[self.turn];
            if self.fighters[id].alive() {
                let actions = self.turn_actions(id);
                if actions > 0 {
                    self.actions_left = actions;
                    return;
                }
                self.log.push(format!("{} cannot act", self.fighters[id].name));
            }
            self.turn += 1;
        }
    }

    fn end_turn(&mut self) {
        self.turn += 1;
        self.next_turn();
    }

    fn spend_action(&mut self, id: usize) {
        self.actions_left -= 1;
        if self.actions_left <= 0 || !self.fighters[id].alive() {
            self.end_turn();
        }
    }

    /// End the active fighter's turn, forfeiting any remaining actions.
    pub fn skip(&mut self) {
        if let Some(id) = self.active() {
            self.log.push(format!("{} waits", self.fighters[id].name));
            self.end_turn();
        }
    }

    /// Recomputes current stats from base, drain, blessing and curse.
    fn refresh(&mut self, i: usize) {
        let f = &mut self.fighters[i];
        let mut s = f.base.clone();
        s[Stat::MagicPower] = f.power;
        for b in [f.blessing, f.curse].into_iter().flatten().map(|t| t.buff) {
            for st in [Stat::AttackBlow, Stat::AttackShot] {
                if s[st] > 0 {
                    s[st] += b.attack;
                }
            }
            s[Stat::DefenceBlow] += b.defence;
            s[Stat::DefenceShot] += b.defence;
            s[Stat::Initiative] += b.initiative;
            s[Stat::Manevres] += b.actions;
        }
        s.clamp();
        f.stats = s;
        if self.order.get(self.turn) == Some(&i) {
            self.actions_left = self.actions_left.min(f.stats[Stat::Manevres].max(0));
        }
    }

    fn row_occupied(&self, team: Team, row: Row) -> bool {
        self.living(team).any(|f| f.slot.row == row)
    }

    /// With its front row empty, a side's back row steps forward; with both empty, the
    /// reserve does *(guess: so a side is never left untargetable)*.
    fn collapse(&mut self, team: Team) {
        if self.row_occupied(team, Row::Front) {
            return;
        }
        let from = [Row::Back, Row::Reserve].into_iter().find(|&r| self.row_occupied(team, r));
        let Some(from) = from else { return };
        for f in self.fighters.iter_mut().filter(|f| f.alive() && f.team == team && f.slot.row == from) {
            f.slot.row = Row::Front;
        }
        let side = if team == Team::Player { "Your" } else { "The enemy" };
        let what = if from == Row::Back { "rear" } else { "reserve" };
        self.log.push(format!("{side} {what} steps forward"));
    }

    // ------------------------------------------------------------------------------------
    // Reach (mechanics.md 2.3)
    // ------------------------------------------------------------------------------------

    /// Occupied front cells of `team` in columns c−1..c+1.
    fn front_near(&self, team: Team, col: u8) -> Vec<u8> {
        self.living(team).filter(|f| f.slot.row == Row::Front && f.slot.col.abs_diff(col) <= 1).map(|f| f.slot.col).collect()
    }

    /// Long-strike targets from column `col`: the nearest front unit of `team` to the right
    /// (c+2, c+3 …) and to the left (c−2, c−3 …).
    fn long_strike_targets(&self, team: Team, col: u8) -> Vec<usize> {
        let front = |c: i32| (0..self.formation.cols as i32).contains(&c).then(|| self.at(team, Slot::new(Row::Front, c as u8))).flatten();
        let c = col as i32;
        let right = (c + 2..self.formation.cols as i32).find_map(front);
        let left = (0..=c - 2).rev().find_map(front);
        [right, left].into_iter().flatten().collect()
    }

    /// What `id`, standing on `from`, can do to `target`, the default action first.
    fn options_from(&self, id: usize, from: Slot, target: usize) -> Vec<ActionKind> {
        use ActionKind::*;
        let (f, t) = (&self.fighters[id], &self.fighters[target]);
        let mut v = Vec::new();
        if !f.alive() || !t.alive() || !from.row.is_active() || !t.slot.row.is_active() {
            return v;
        }
        let s = &f.stats;
        if t.team != f.team {
            let near = self.front_near(t.team, from.col);
            let blocked = !near.is_empty();
            let adjacent = t.slot.row == Row::Front && near.contains(&t.slot.col);
            if s.is_warrior() && from.row == Row::Front && t.slot.row == Row::Front {
                if adjacent {
                    v.push(Melee);
                } else if !blocked && self.long_strike_targets(t.team, from.col).contains(&target) {
                    v.push(LongStrike);
                }
            }
            if s.is_shooter() && (from.row == Row::Back || !blocked || adjacent) {
                v.push(Shot);
            }
            if s.is_mage() && s.magic_direction().hits_enemies() && (from.row == Row::Back || !blocked) {
                let pair = if s.magic == Some(MagicSchool::Life) { [Strike, Curse] } else { [Curse, Strike] };
                for k in pair {
                    let useful = match k {
                        Strike => self.magic_strike(id, target) > 0,
                        _ => !self.curse_buff(id, target).is_empty(),
                    };
                    if useful {
                        v.push(k);
                    }
                }
            }
        } else if s.is_mage() && s.magic_direction().helps_allies() {
            if t.hp < t.max_hp() && self.heal_power(id, target) > 0 {
                v.push(Heal);
            }
            if t.blessing.is_none() && !self.bless_buff(id, target).is_empty() {
                v.push(Bless);
            }
        }
        v
    }

    /// What the fighter `id` can do to `target` from where it stands, default first.
    pub fn options(&self, id: usize, target: usize) -> Vec<ActionKind> {
        self.options_from(id, self.fighters[id].slot, target)
    }

    pub fn can_target(&self, id: usize, target: usize) -> bool {
        !self.options(id, target).is_empty()
    }

    pub fn targets(&self, id: usize) -> Vec<usize> {
        (0..self.fighters.len()).filter(|&t| self.can_target(id, t)).collect()
    }

    fn has_hostile_option_from(&self, id: usize, from: Slot) -> bool {
        (0..self.fighters.len()).any(|t| self.options_from(id, from, t).iter().any(|k| k.is_hostile()))
    }

    /// A unit that cannot attack from where it stands (e.g. a warrior in the back row).
    pub fn helpless(&self, id: usize) -> bool {
        let f = &self.fighters[id];
        let s = &f.stats;
        let could = s.is_warrior() || s.is_shooter() || (s.is_mage() && s.magic_direction().hits_enemies());
        could && !self.has_hostile_option_from(id, f.slot)
    }

    /// Empty own cells the fighter could step to: columns c−1..c+1 of the front and back
    /// rows. From the reserve: any empty front or back cell. Into the reserve: any empty
    /// reserve cell, from the back row only *(guess: the original gates this on an unknown
    /// per-unit flag)*.
    pub fn moves(&self, id: usize) -> Vec<Slot> {
        let f = &self.fighters[id];
        if !f.alive() {
            return Vec::new();
        }
        let from = f.slot;
        self.formation
            .slots()
            .filter(|&s| s != from && self.at(f.team, s).is_none())
            .filter(|s| match (from.row, s.row) {
                (Row::Reserve, r) => r.is_active(),
                (Row::Back, Row::Reserve) => true,
                (_, Row::Reserve) => false,
                _ => s.col.abs_diff(from.col) <= 1,
            })
            .collect()
    }

    // ------------------------------------------------------------------------------------
    // Damage and magic (mechanics.md 2.4, 3.3)
    // ------------------------------------------------------------------------------------

    fn has_knight(&self, team: Team) -> bool {
        self.fighters.iter().any(|f| f.team == team && f.is_hero && HeroClass::of_unit(f.unit) == Some(HeroClass::Knight))
    }

    /// Physical damage of `a` on `t`, before capping at the target's HP. Deterministic.
    pub fn physical_damage(&self, a: usize, t: usize, kind: ActionKind) -> i32 {
        let (af, tf) = (&self.fighters[a], &self.fighters[t]);
        let (s, ts) = (&af.stats, &tf.stats);
        let shot = kind == ActionKind::Shot;
        let building = self.building_defence[tf.team.index()];
        let mut atk = if shot { s[Stat::AttackShot] } else { s[Stat::AttackBlow] };
        let mut def = if shot { ts[Stat::DefenceShot] } else { ts[Stat::DefenceBlow] } + building;
        if shot && tf.slot.row == Row::Back {
            def += self.opt().row2_def;
        }
        if !shot && self.round <= 1 && ts.has(&Bonus::SpearDefense) {
            def *= 3;
        }
        if kind == ActionKind::LongStrike {
            def /= 2;
            if s.has(&Bonus::FlankStrike) {
                atk *= 2;
            }
        }
        // Piercing ignores the unit's defence; the building's still counts.
        if s.has_any(&PIERCING) || s.has(&Bonus::Other("PoisonArmorIgnore".into())) {
            def = building;
        }
        let mut dmg = (atk - def).max(1);
        if ts.has_any(&[Bonus::VampirsGist, Bonus::OldVampirsGist, Bonus::Evasive]) {
            dmg = dmg * 2 / 3;
        }
        if ts.has(&Bonus::Garrison) && building >= 10 {
            dmg = dmg * 2 / 3;
        }
        if shot && ts.has_any(&[Bonus::Dead, Bonus::FastDead]) {
            dmg = dmg * 3 / 10;
        }
        if self.has_knight(tf.team) {
            dmg = dmg * 90 / 100;
        }
        if ts.has_any(&[Bonus::Unvulnerabe, Bonus::Ghost]) {
            dmg = 1;
        }
        dmg += god_bonus(s);
        (dmg * (100 - ts.evasion.clamp(0, 100)) / 100).max(1)
    }

    fn school(&self, a: usize) -> MagicSchool {
        self.fighters[a].stats.magic.unwrap_or(MagicSchool::Elemental)
    }

    /// Caster power against `t` for hostile magic: reduced by the target's protection %.
    fn hostile_power(&self, a: usize, t: usize) -> i32 {
        let p = self.fighters[a].stats[Stat::MagicPower];
        let prot = self.fighters[t].stats.protection(self.school(a)).clamp(0, 100);
        (p * (100 - prot) + 50) / 100
    }

    /// Magic strike damage, before capping at HP: Life ×2 on undead, Death ×½ on undead,
    /// Elemental ¾; ¾ on elementals for Life and Death; plus GodAnger/GodStrike.
    pub fn magic_strike(&self, a: usize, t: usize) -> i32 {
        let p = self.hostile_power(a, t);
        let nature = self.fighters[t].stats.nature;
        let dmg = match (self.school(a), nature) {
            (MagicSchool::Life, Nature::Undead) => 2 * p,
            (MagicSchool::Death, Nature::Undead) => p / 2,
            (MagicSchool::Elemental, _) | (_, Nature::Elemental) => p * 3 / 4,
            _ => p,
        };
        if dmg > 0 {
            dmg + god_bonus(&self.fighters[a].stats)
        } else {
            0
        }
    }

    /// HP a heal restores before capping: Life P (not on undead or elementals), Elemental
    /// P/2, Death P on undead only.
    fn heal_power(&self, a: usize, t: usize) -> i32 {
        let p = self.fighters[a].stats[Stat::MagicPower];
        let nature = self.fighters[t].stats.nature;
        match self.school(a) {
            MagicSchool::Life if matches!(nature, Nature::Undead | Nature::Elemental) => 0,
            MagicSchool::Life => p,
            MagicSchool::Elemental => p / 2,
            MagicSchool::Death if nature == Nature::Undead => p,
            MagicSchool::Death => 0,
        }
    }

    /// Blessing by school (friendly: power not reduced).
    pub fn bless_buff(&self, a: usize, t: usize) -> Buff {
        let p = self.fighters[a].stats[Stat::MagicPower];
        let o = self.opt();
        let (bm, bn, w) = (o.bless_main_spell.max(1), o.bless_next_spell.max(1), o.wizard_main_spell.max(1));
        let target = &self.fighters[t];
        let mut b = match self.school(a) {
            MagicSchool::Life if matches!(target.stats.nature, Nature::Undead | Nature::Elemental) => Buff::default(),
            MagicSchool::Life => Buff { defence: 3 * p / (2 * bm) + 1, attack: 3 * p / (2 * bn), ..Buff::default() },
            MagicSchool::Elemental => Buff { actions: actions_of_power(p), initiative: p / w + 1, ..Buff::default() },
            MagicSchool::Death => Buff { attack: p / bm + 1, defence: p / bn, ..Buff::default() },
        };
        if !target.has_attack() {
            b.attack = 0;
        }
        b
    }

    /// Curse by school (hostile: power reduced by protection).
    pub fn curse_buff(&self, a: usize, t: usize) -> Buff {
        let p = self.hostile_power(a, t);
        let o = self.opt();
        let (cm, cn, w) = (o.curse_main_spell.max(1), o.curse_next_spell.max(1), o.wizard_main_spell.max(1));
        let mut b = match self.school(a) {
            // Life uses ⅔ of CurseMainSpell and a fixed 10.
            MagicSchool::Life => Buff { defence: -(1 + 3 * p / (2 * cm)), attack: -(p / 10), ..Buff::default() },
            MagicSchool::Elemental => Buff { actions: -actions_of_power(p), initiative: -(1 + p / w), ..Buff::default() },
            MagicSchool::Death => Buff { attack: -(1 + p / cm), defence: -(p / cn), ..Buff::default() },
        };
        if !self.fighters[t].has_attack() {
            b.attack = 0;
        }
        if p == 0 {
            b = Buff::default();
        }
        b
    }

    /// Expected effect of `kind` by `a` on `t`, for hover previews.
    pub fn preview(&self, a: usize, t: usize, kind: ActionKind) -> Preview {
        let target = &self.fighters[t];
        match kind {
            ActionKind::Strike => Preview::Damage(self.magic_strike(a, t).min(target.hp)),
            ActionKind::Curse => Preview::Buff(self.curse_buff(a, t)),
            ActionKind::Heal => Preview::Heal(self.heal_power(a, t).min(target.max_hp() - target.hp)),
            ActionKind::Bless => Preview::Buff(self.bless_buff(a, t)),
            k => Preview::Damage(self.physical_damage(a, t, k).min(target.hp)),
        }
    }

    // ------------------------------------------------------------------------------------
    // Actions
    // ------------------------------------------------------------------------------------

    /// Step the active fighter to an empty own cell; costs one action.
    pub fn move_active(&mut self, to: Slot) -> Result<(), ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        if !self.moves(id).contains(&to) {
            return Err(ActionError::InvalidTarget);
        }
        self.fighters[id].slot = to;
        let team = self.fighters[id].team;
        self.log.push(format!("{} moves", self.fighters[id].name));
        self.collapse(team);
        self.spend_action(id);
        Ok(())
    }

    /// The active fighter's default action on `target`; costs one action.
    pub fn act(&mut self, target: usize) -> Result<Hit, ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        let kind = *self.options(id, target).first().ok_or(ActionError::InvalidTarget)?;
        self.act_with(target, kind)
    }

    /// The active fighter does `kind` to `target`; costs one action.
    pub fn act_with(&mut self, target: usize, kind: ActionKind) -> Result<Hit, ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        if !self.options(id, target).contains(&kind) {
            return Err(ActionError::InvalidTarget);
        }
        let mut hit = Hit { target, kind, amount: 0, buff: Buff::default(), killed: false, counter: None, actor_died: false };
        let (name, tname) = (self.fighters[id].name.clone(), self.fighters[target].name.clone());
        match kind {
            ActionKind::Heal => {
                let t = &self.fighters[target];
                let healed = self.heal_power(id, target).min(t.max_hp() - t.hp);
                self.fighters[target].hp += healed;
                self.fighters[id].dealt += healed;
                hit.amount = healed;
                self.log.push(format!("{name} heals {tname} +{healed}"));
            }
            ActionKind::Bless | ActionKind::Curse => {
                let buff = if kind == ActionKind::Bless { self.bless_buff(id, target) } else { self.curse_buff(id, target) };
                let timed = Some(Timed { buff, until: self.round + EFFECT_TURNS - 1 });
                if kind == ActionKind::Bless {
                    self.fighters[target].blessing = timed;
                } else {
                    self.fighters[target].curse = timed;
                }
                self.refresh(target);
                hit.buff = buff;
                self.log.push(format!("{name} {}s {tname}: {}", kind.label(), buff.describe()));
            }
            _ => {
                let raw = if kind == ActionKind::Strike { self.magic_strike(id, target) } else { self.physical_damage(id, target, kind) };
                self.deal(id, target, raw, &mut hit);
                if kind.is_physical() && self.fighters[id].stats.has(&Bonus::Poison) && hit.amount > 1 && !hit.killed {
                    self.fighters[target].poisoned = true;
                }
                // Counterblow: a warrior struck in melee hits back once.
                let melee = matches!(kind, ActionKind::Melee | ActionKind::LongStrike);
                let t = &self.fighters[target];
                if melee && t.alive() && t.stats.has(&Bonus::Counterblow) && t.stats.is_warrior() && self.fighters[id].alive() {
                    let dmg = self.physical_damage(target, id, ActionKind::Melee).min(self.fighters[id].hp);
                    self.fighters[id].hp -= dmg;
                    self.fighters[target].dealt += dmg;
                    hit.counter = Some(dmg);
                    self.log.push(format!("{tname} hits back for {dmg}"));
                    hit.actor_died |= !self.fighters[id].alive();
                }
            }
        }
        self.collapse(Team::Player);
        self.collapse(Team::Enemy);
        self.spend_action(id);
        Ok(hit)
    }

    /// Applies `raw` damage (capped at HP), vampirism and the killer-dies bonuses.
    fn deal(&mut self, id: usize, target: usize, raw: i32, hit: &mut Hit) {
        let dmg = raw.min(self.fighters[target].hp);
        self.fighters[target].hp -= dmg;
        let (name, tname) = (self.fighters[id].name.clone(), self.fighters[target].name.clone());
        let a = &mut self.fighters[id];
        a.dealt += dmg;
        let vamp = a.stats[Stat::Vampirizm];
        if vamp > 0 {
            a.hp = (a.hp + dmg * vamp / 100).min(a.max_hp());
        }
        hit.amount = dmg;
        hit.killed = !self.fighters[target].alive();
        let how = match hit.kind {
            ActionKind::LongStrike => " with a long strike",
            ActionKind::Strike => " with magic",
            _ => "",
        };
        if hit.killed {
            self.log.push(format!("{name} kills {tname}{how} ({dmg})"));
            if self.fighters[target].stats.has_any(&[Bonus::DeathCurse, Bonus::Ghost]) {
                self.fighters[id].hp = 0;
                hit.actor_died = true;
                self.log.push(format!("{name} dies by {tname}'s curse"));
            }
        } else {
            self.log.push(format!("{name} hits {tname}{how} for {dmg}"));
        }
    }

    // ------------------------------------------------------------------------------------
    // AI
    // ------------------------------------------------------------------------------------

    /// How dangerous a fighter is: its best attack or magic power.
    fn threat(&self, t: usize) -> i32 {
        let s = &self.fighters[t].stats;
        s[Stat::AttackBlow].max(s[Stat::AttackShot]).max(s[Stat::MagicPower])
    }

    /// Action the AI would pick for the active fighter:
    /// 1. heal an ally below half HP (the most wounded);
    /// 2. a damaging action that kills (the most dangerous victim);
    /// 3. a curse on an uncursed enemy, for casters whose default is the curse (non-Life);
    /// 4. the damaging action needing the fewest hits to kill, then the most damage;
    /// 5. heal any wounded ally, else bless the most dangerous unblessed ally.
    pub fn ai_choice(&self) -> Option<(usize, ActionKind)> {
        let id = self.active()?;
        let opts: Vec<(usize, ActionKind)> =
            (0..self.fighters.len()).flat_map(|t| self.options(id, t).into_iter().map(move |k| (t, k))).collect();
        let hp_frac = |t: usize| self.fighters[t].hp * 1000 / self.fighters[t].max_hp();
        let heals = || opts.iter().copied().filter(|o| o.1 == ActionKind::Heal);
        if let Some(o) = heals().filter(|o| hp_frac(o.0) < 500).min_by_key(|o| hp_frac(o.0)) {
            return Some(o);
        }
        let damage = |o: &(usize, ActionKind)| match self.preview(id, o.0, o.1) {
            Preview::Damage(d) => Some(d),
            _ => None,
        };
        let damaging: Vec<((usize, ActionKind), i32)> = opts.iter().filter_map(|o| damage(o).map(|d| (*o, d))).collect();
        if let Some((o, _)) = damaging.iter().filter(|(o, d)| *d >= self.fighters[o.0].hp).max_by_key(|(o, _)| (self.threat(o.0), std::cmp::Reverse(o.0))) {
            return Some(*o);
        }
        let curse_first = self.fighters[id].stats.magic != Some(MagicSchool::Life);
        if curse_first {
            let curse = opts
                .iter()
                .filter(|o| o.1 == ActionKind::Curse && self.fighters[o.0].curse.is_none())
                .max_by_key(|o| (self.threat(o.0), std::cmp::Reverse(o.0)));
            if let Some(o) = curse {
                return Some(*o);
            }
        }
        let best = damaging.iter().filter(|(_, d)| *d > 0).min_by_key(|(o, d)| {
            let hp = self.fighters[o.0].hp;
            ((hp + d - 1) / d, -d, hp, o.0)
        });
        if let Some((o, _)) = best {
            return Some(*o);
        }
        if let Some(o) = heals().min_by_key(|o| hp_frac(o.0)) {
            return Some(o);
        }
        opts.iter().filter(|o| o.1 == ActionKind::Bless).max_by_key(|o| (self.threat(o.0), std::cmp::Reverse(o.0))).copied()
    }

    /// How good a cell is for a fighter with nothing to attack: somewhere it can attack from,
    /// in its preferred row, (warriors) with nobody of its own in front, close to an enemy.
    fn approach_score(&self, id: usize, s: Slot) -> (bool, bool, bool, i32) {
        let f = &self.fighters[id];
        let dist = self.living(f.team.other()).filter(|e| e.slot.row == Row::Front).map(|e| e.slot.col.abs_diff(s.col) as i32).min().unwrap_or(0);
        let front_free = self.at(f.team, Slot::new(Row::Front, s.col)).is_none_or(|o| o == id);
        (self.has_hostile_option_from(id, s), s.row == f.base.preferred_row(), front_free, -dist)
    }

    /// Cell the AI would step to, if stepping strictly improves its position. Units in the
    /// reserve stay there.
    fn ai_move(&self) -> Option<Slot> {
        let id = self.active()?;
        let f = &self.fighters[id];
        if f.slot.row == Row::Reserve || self.has_hostile_option_from(id, f.slot) {
            return None;
        }
        let here = self.approach_score(id, f.slot);
        self.moves(id)
            .into_iter()
            .filter(|s| s.row.is_active())
            .map(|s| (self.approach_score(id, s), s))
            .filter(|(score, _)| *score > here)
            .max_by_key(|&(score, s)| (score, std::cmp::Reverse(s.col)))
            .map(|(_, s)| s)
    }

    /// Plays one action of the active fighter automatically.
    pub fn ai_step(&mut self) -> Option<Step> {
        let actor = self.active()?;
        if let Some((t, kind)) = self.ai_choice() {
            let hit = self.act_with(t, kind).ok()?;
            return Some(Step::Act { actor, hit });
        }
        if let Some(to) = self.ai_move() {
            let from = self.fighters[actor].slot;
            self.move_active(to).ok()?;
            return Some(Step::Move { actor, from, to });
        }
        self.skip();
        Some(Step::Wait { actor })
    }

    // ------------------------------------------------------------------------------------
    // After the battle
    // ------------------------------------------------------------------------------------

    /// XP for the surviving units of `team` once the battle is over (mechanics.md 1.4,
    /// simplified): with `ratio` = enemy strength / own strength,
    /// `k = 1 ± |ratio−1|·ExpCorrection/100` clamped to [0.25, 4],
    /// `pool = MainExpCorrection% · k · destroyed enemy strength + enemy strength / 20`.
    /// The pool is split by weight `(4 − row)·10 + damage dealt and HP healed`, at least 1
    /// each, then scaled by `HeroExpirienceModificator` (player) or `AIExpiriencePercent`.
    /// The exe's extra damage-exchange term is left out.
    pub fn xp_awards(&self, team: Team) -> Vec<XpAward> {
        if self.deploying || self.outcome() == Outcome::Ongoing {
            return Vec::new();
        }
        let strength = |tm: Team| self.fighters.iter().filter(|f| f.team == tm).map(|f| f.tactical).sum::<i32>().max(1) as f64;
        let (own, enemy) = (strength(team), strength(team.other()));
        let destroyed: i32 = self.fighters.iter().filter(|f| f.team != team && !f.alive()).map(|f| f.tactical).sum();
        let o = self.opt();
        let ratio = enemy / own;
        let skew = (ratio - 1.0).abs() * o.exp_correction as f64 / 100.0;
        let k = if ratio >= 1.0 { 1.0 + skew } else { 1.0 - skew }.clamp(0.25, 4.0);
        let pool = o.main_exp_correction as f64 / 100.0 * k * destroyed as f64 + enemy / 20.0;
        let survivors: Vec<usize> = (0..self.fighters.len()).filter(|&i| self.fighters[i].team == team && self.fighters[i].alive()).collect();
        let weight = |i: usize| ((4 - self.fighters[i].slot.row.number()) * 10 + self.fighters[i].dealt) as f64;
        let total: f64 = survivors.iter().map(|&i| weight(i)).sum::<f64>().max(1.0);
        let modifier = if team == Team::Player { o.hero_experience_modificator } else { o.ai_experience_percent };
        survivors
            .into_iter()
            .map(|i| {
                let share = (pool * weight(i) / total).round().max(1.0) as i32;
                XpAward { fighter: i, xp: (share * modifier / 100).clamp(1, MAX_XP_GAIN) }
            })
            .collect()
    }

    /// Final state of the player's fighters. The hero cannot die while a unit of his army
    /// survives: he comes back with 1 HP (mechanics.md 2.5). Slots are the deployed ones.
    pub fn player_results(&self) -> Vec<FighterResult> {
        let survivors = self.living(Team::Player).next().is_some();
        self.fighters
            .iter()
            .filter_map(|f| {
                let squad_index = f.squad_index?;
                let hp = if f.is_hero && !f.alive() && survivors { 1 } else { f.hp.max(0) };
                Some(FighterResult { squad_index, hp, slot: if self.deploying { f.slot } else { f.deployed } })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::testkit::*;
    use crate::rules::content::{MagicDirection, UnitDef};
    use Row::*;

    const fn f(col: u8) -> Slot {
        Slot::new(Front, col)
    }
    const fn b(col: u8) -> Slot {
        Slot::new(Back, col)
    }
    const fn r(col: u8) -> Slot {
        Slot::new(Reserve, col)
    }

    /// Test units: 10 warrior 30/5, 11 shooter 20, 12 Elemental enemy mage 40, 13 Life healer 20,
    /// 14 Death mage 30, 15 Life strike mage 30, 16 undead warrior, 17 elemental warrior,
    /// 18 weak warrior 10/0 with 200 HP (a punching bag), 1 = Knight hero class.
    fn units() -> Vec<UnitDef> {
        use MagicDirection::*;
        let bag = UnitDef { hits: 200, ..warrior(18, 10, 0) };
        vec![
            warrior(10, 30, 5),
            shooter(11, 20),
            mage(12, 40, MagicSchool::Elemental, ToEnemy),
            mage(13, 20, MagicSchool::Life, ToAlly),
            mage(14, 30, MagicSchool::Death, ToEnemy),
            mage(15, 30, MagicSchool::Life, ToEnemy),
            UnitDef { nature: Nature::Undead, ..warrior(16, 20, 0) },
            UnitDef { nature: Nature::Elemental, ..warrior(17, 20, 0) },
            bag,
            warrior(1, 30, 5),
        ]
    }

    fn content_with(extra: Vec<UnitDef>, formation: Formation) -> Arc<Content> {
        let mut us = units();
        for u in extra {
            us.retain(|x| x.id != u.id);
            us.push(u);
        }
        let mut c = content(us, vec![]);
        c.formation = formation;
        Arc::new(c)
    }

    fn battle_in(c: &Arc<Content>, player: &[(u32, Slot)], enemies: &[(u32, Slot)]) -> Battle {
        let squad: Vec<Unit> = player.iter().map(|&(id, s)| Unit::new(c, UnitId(id), s)).collect();
        let p: Vec<_> = squad.iter().enumerate().collect();
        let e: Vec<Unit> = enemies.iter().map(|&(id, s)| Unit::new(c, UnitId(id), s)).collect();
        let mut bt = Battle::new(c.clone(), &p, &e, Team::Player);
        bt.begin();
        bt
    }

    fn battle(player: &[(u32, Slot)], enemies: &[(u32, Slot)]) -> Battle {
        battle_in(&content_with(vec![], Formation::WIDE), player, enemies)
    }

    fn with(extra: Vec<UnitDef>, player: &[(u32, Slot)], enemies: &[(u32, Slot)]) -> Battle {
        battle_in(&content_with(extra, Formation::WIDE), player, enemies)
    }

    /// Makes `id` the active fighter by skipping others (fails after a full turn).
    fn turn_of(bt: &mut Battle, id: usize) {
        for _ in 0..50 {
            if bt.active() == Some(id) {
                return;
            }
            bt.skip();
        }
        panic!("fighter {id} never gets a turn");
    }

    fn bonus(id: u32, b: Bonus, base: UnitDef) -> UnitDef {
        UnitDef { id, bonus: Some(b), ..base }
    }

    // --- reach -----------------------------------------------------------------------------

    #[test]
    fn warrior_hits_front_cells_c_minus_1_to_c_plus_1() {
        let bt = battle(&[(10, f(2))], &[(18, f(1)), (18, f(2)), (18, f(3)), (18, f(4)), (18, b(2))]);
        assert_eq!(bt.targets(0), vec![1, 2, 3]);
        assert!(bt.targets(0).iter().all(|&t| bt.options(0, t) == vec![ActionKind::Melee]));
    }

    #[test]
    fn warrior_in_the_back_row_cannot_attack() {
        let bt = battle(&[(10, f(2)), (10, b(2))], &[(18, f(2))]);
        assert!(bt.targets(1).is_empty());
        assert!(bt.helpless(1) && !bt.helpless(0));
    }

    #[test]
    fn long_strike_reaches_the_nearest_on_each_side_through_three_empty_cells() {
        let bt = battle(&[(10, f(2))], &[(18, f(0)), (18, f(5)), (18, f(4)), (18, b(2))]);
        // c=2: 1,2,3 empty; nearest right is col 4 (not 5), nearest left col 0.
        assert_eq!(bt.targets(0), vec![1, 3]);
        assert_eq!(bt.options(0, 3), vec![ActionKind::LongStrike]);
        // One cell of c−1..c+1 occupied: no long strike.
        let bt = battle(&[(10, f(2))], &[(18, f(0)), (18, f(3))]);
        assert_eq!(bt.targets(0), vec![2]);
    }

    #[test]
    fn long_strike_halves_defence_and_flank_strike_doubles_attack() {
        // Attack 30 vs defence 20: normal 10, long strike 30 − 10 = 20, FlankStrike 60 − 10 = 50.
        let tough = UnitDef { hits: 500, ..warrior(19, 1, 20) };
        let flanker = bonus(20, Bonus::FlankStrike, warrior(20, 30, 5));
        let bt = with(vec![tough.clone(), flanker.clone()], &[(10, f(2))], &[(19, f(2))]);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 10);
        let bt = with(vec![tough.clone(), flanker.clone()], &[(10, f(0))], &[(19, f(4))]);
        assert_eq!(bt.options(0, 1), vec![ActionKind::LongStrike]);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::LongStrike), 20);
        let bt = with(vec![tough, flanker], &[(20, f(0))], &[(19, f(4))]);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::LongStrike), 50);
    }

    #[test]
    fn shooter_reach_from_back_and_front_rows() {
        // From the back row: anyone in the enemy front and back rows.
        let bt = battle(&[(11, b(0)), (10, f(3))], &[(18, f(3)), (18, b(5))]);
        assert_eq!(bt.targets(0), vec![2, 3]);
        // From the front row with an enemy in c−1..c+1: only those adjacent front units.
        let bt = battle(&[(11, f(3))], &[(18, f(4)), (18, f(0)), (18, b(3))]);
        assert_eq!(bt.targets(0), vec![1]);
        // From the front row with the three opposite cells empty: anyone.
        let bt = battle(&[(11, f(3))], &[(18, f(0)), (18, b(3))]);
        assert_eq!(bt.targets(0), vec![1, 2]);
    }

    #[test]
    fn hostile_mage_reach() {
        let bt = battle(&[(12, b(0)), (10, f(0))], &[(18, f(3)), (18, b(5))]);
        assert_eq!(bt.targets(0), vec![2, 3]);
        // Front row, enemy opposite: cannot cast at all.
        let bt = battle(&[(12, f(3))], &[(18, f(3)), (18, b(0))]);
        assert!(bt.targets(0).is_empty());
        let bt = battle(&[(12, f(3))], &[(18, f(0)), (18, b(3))]);
        assert_eq!(bt.targets(0), vec![1, 2]);
    }

    #[test]
    fn friendly_mage_heals_the_wounded_and_blesses_the_rest() {
        let mut bt = battle(&[(13, b(2)), (10, f(2)), (10, f(3))], &[(18, f(2))]);
        assert_eq!(bt.options(0, 2), vec![ActionKind::Bless]);
        bt.fighters[1].hp = 20;
        assert_eq!(bt.options(0, 1), vec![ActionKind::Heal, ActionKind::Bless]);
        assert!(bt.options(0, 3).is_empty(), "ToAlly never targets enemies");
        turn_of(&mut bt, 0);
        let hit = bt.act(1).unwrap();
        assert_eq!((hit.kind, hit.amount, bt.fighters[1].hp), (ActionKind::Heal, 20, 40));
        turn_of(&mut bt, 0);
        bt.act(2).unwrap();
        assert!(bt.options(0, 2).is_empty(), "already blessed, unhurt");
    }

    #[test]
    fn reserve_cannot_be_targeted_or_attack() {
        let c = content_with(vec![], Formation::VANILLA);
        let bt = battle_in(&c, &[(11, b(1)), (11, r(1)), (10, f(1))], &[(18, f(1)), (11, r(2))]);
        assert_eq!(bt.targets(0), vec![3], "enemy reserve archer is out of reach");
        assert!(bt.targets(1).is_empty(), "reserve shooter cannot shoot");
        assert!(bt.targets(4).is_empty());
        // From the reserve a unit can step to any empty front or back cell.
        assert!(bt.moves(1).contains(&f(3)) && bt.moves(1).contains(&b(0)) && !bt.moves(1).contains(&r(0)));
        // From the back row into the reserve.
        assert!(bt.moves(0).contains(&r(3)));
        assert!(!bt.moves(2).iter().any(|s| s.row == Reserve), "not from the front row");
    }

    #[test]
    fn vanilla_battle_collapses_back_then_reserve() {
        let c = content_with(vec![], Formation::VANILLA);
        let mut bt = battle_in(&c, &[(10, f(1))], &[(18, f(1)), (11, r(2))]);
        let enemy = bt.at(Team::Enemy, f(1)).unwrap();
        bt.fighters[enemy].hp = 1;
        turn_of(&mut bt, 0);
        bt.act(enemy).unwrap();
        assert_eq!(bt.fighters[2].slot, f(2), "the reserve steps up when nobody else stands");
    }

    // --- movement, order, actions ---------------------------------------------------------------

    #[test]
    fn moves_are_to_columns_c_minus_1_to_c_plus_1_in_own_rows() {
        let bt = battle(&[(10, f(0)), (10, f(1))], &[(18, f(0))]);
        assert_eq!(bt.moves(1), vec![f(2), b(0), b(1), b(2)]);
        assert_eq!(bt.moves(0), vec![b(0), b(1)]);
    }

    #[test]
    fn back_row_collapses_forward_when_the_front_falls() {
        let mut bt = battle(&[(10, f(2))], &[(18, f(2)), (11, b(4))]);
        bt.fighters[1].hp = 1;
        turn_of(&mut bt, 0);
        bt.act(1).unwrap();
        assert_eq!(bt.fighters[2].slot, f(4));
        // A side that deploys only in the back row starts in front.
        let bt = battle(&[(11, b(2))], &[(18, f(2))]);
        assert_eq!(bt.fighters[0].slot, f(2));
    }

    #[test]
    fn initiative_order_with_attacker_bonus() {
        let fast = UnitDef { initiative: 12, ..warrior(21, 10, 0) };
        let same = UnitDef { initiative: 12, ..warrior(22, 10, 0) };
        let slow = UnitDef { initiative: 11, ..warrior(23, 10, 0) };
        // Player attacks: its 11 becomes 12 and beats the defender's 12 on the tie.
        let bt = with(vec![fast, same, slow], &[(23, f(0)), (21, f(1))], &[(22, f(1))]);
        let order: Vec<usize> = bt.queue().collect();
        assert_eq!(order, vec![1, 0, 2]);
    }

    #[test]
    fn artillery_always_acts_first() {
        let gun = bonus(24, Bonus::Artillery, UnitDef { initiative: 1, ..shooter(24, 10) });
        let bt = with(vec![gun], &[(10, f(0))], &[(24, b(0))]);
        assert_eq!(bt.active(), Some(1));
    }

    #[test]
    fn manevres_and_fast_start() {
        let two = UnitDef { manevres: 2, ..shooter(25, 10) };
        let horse = bonus(26, Bonus::HorseAtack, UnitDef { initiative: 30, ..warrior(26, 10, 0) });
        let mut bt = with(vec![two, horse], &[(26, f(0)), (25, b(0))], &[(18, f(0))]);
        assert_eq!((bt.active(), bt.actions_left()), (Some(0), 2), "HorseAtack: +1 on turn 1");
        bt.skip();
        assert_eq!((bt.active(), bt.actions_left()), (Some(1), 2));
        bt.move_active(b(1)).unwrap();
        assert_eq!((bt.active(), bt.actions_left()), (Some(1), 1), "a step costs one action");
        while bt.round < 2 {
            bt.skip();
        }
        assert_eq!((bt.active(), bt.actions_left()), (Some(0), 1), "no bonus on turn 2");
    }

    #[test]
    fn turn_limit_ends_in_a_stalemate() {
        let mut bt = battle(&[(10, f(0))], &[(18, f(5))]);
        while bt.outcome() == Outcome::Ongoing {
            bt.skip();
        }
        assert_eq!((bt.outcome(), bt.round), (Outcome::Stalemate, 25));
    }

    // --- damage ---------------------------------------------------------------------------

    #[test]
    fn damage_is_attack_minus_defence_at_least_one() {
        let bt = battle(&[(10, f(2)), (18, f(3))], &[(10, f(2))]);
        assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 25);
        assert_eq!(bt.physical_damage(1, 2, ActionKind::Melee), 5);
        let armour = UnitDef { hits: 100, ..warrior(27, 1, 50) };
        let bt = with(vec![armour], &[(10, f(2))], &[(27, f(2))]);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 1);
    }

    #[test]
    fn back_row_gets_row2_def_against_shots_only() {
        let mixed = UnitDef { hits: 100, ..warrior(28, 10, 4) };
        let bt = with(vec![mixed], &[(11, b(0)), (10, f(1))], &[(28, f(1)), (28, b(3))]);
        assert_eq!(bt.physical_damage(0, 2, ActionKind::Shot), 16);
        assert_eq!(bt.physical_damage(0, 3, ActionKind::Shot), 11, "+5 in the back row");
        assert_eq!(bt.physical_damage(1, 3, ActionKind::Melee), 26, "melee unaffected");
    }

    #[test]
    fn spear_defense_triples_melee_defence_on_turn_one() {
        let spear = bonus(29, Bonus::SpearDefense, UnitDef { hits: 300, initiative: 1, ..warrior(29, 5, 8) });
        let mut bt = with(vec![spear], &[(10, f(2)), (11, b(2))], &[(29, f(2))]);
        assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 6, "30 − 24");
        assert_eq!(bt.physical_damage(1, 2, ActionKind::Shot), 12, "shots: 20 − 8");
        while bt.round < 2 {
            bt.skip();
        }
        assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 22);
    }

    #[test]
    fn armor_ignore_skips_defence_but_not_the_building() {
        let pierce = bonus(30, Bonus::ArmorIgnore, warrior(30, 30, 0));
        let armour = UnitDef { hits: 100, ..warrior(27, 1, 20) };
        let mut bt = with(vec![pierce, armour], &[(30, f(2))], &[(27, f(2))]);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 30);
        bt.set_building_defence(Team::Enemy, 6);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 24);
    }

    #[test]
    fn unvulnerable_and_ghost_take_one_and_ghost_kills_its_killer() {
        let stone = bonus(31, Bonus::Unvulnerabe, UnitDef { hits: 3, ..warrior(31, 1, 0) });
        let ghost = bonus(32, Bonus::Ghost, UnitDef { hits: 1, initiative: 1, ..warrior(32, 1, 0) });
        let mut bt = with(vec![stone, ghost], &[(10, f(2)), (15, b(2))], &[(31, f(2)), (32, f(3))]);
        assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 1);
        assert_eq!(bt.physical_damage(0, 3, ActionKind::Melee), 1);
        assert_eq!(bt.magic_strike(1, 3), 30, "magic is not physical");
        turn_of(&mut bt, 0);
        let hit = bt.act(3).unwrap();
        assert!(hit.killed && hit.actor_died);
        assert!(!bt.fighters[0].alive());
    }

    #[test]
    fn evasive_dead_vs_shots_god_anger_and_evasion() {
        let evasive = bonus(33, Bonus::Evasive, UnitDef { hits: 100, ..warrior(33, 1, 0) });
        let corpse = bonus(34, Bonus::Dead, UnitDef { hits: 100, ..warrior(34, 1, 0) });
        let angry = bonus(35, Bonus::GodAnger, shooter(35, 20));
        let dodger = UnitDef { evasion: Some(50), hits: 100, ..warrior(36, 1, 0) };
        let bt = with(vec![evasive, corpse, angry, dodger], &[(11, b(0)), (35, b(1))], &[(33, f(0)), (34, f(1)), (36, f(2))]);
        assert_eq!(bt.physical_damage(0, 2, ActionKind::Shot), 13, "20 × 2/3");
        assert_eq!(bt.physical_damage(0, 3, ActionKind::Shot), 6, "20 × 3/10");
        assert_eq!(bt.physical_damage(1, 3, ActionKind::Shot), 16, "6 + 10 GodAnger");
        assert_eq!(bt.physical_damage(0, 4, ActionKind::Shot), 10, "Evasion 50%");
    }

    #[test]
    fn knight_hero_cuts_physical_damage_to_his_army_by_ten_percent() {
        let bt = battle(&[(1, f(0)), (18, f(2))], &[(10, f(2)), (15, b(2))]);
        // 30 − 0 = 30 → 27; the bag is not the hero but is in the knight's army.
        assert_eq!(bt.physical_damage(2, 1, ActionKind::Melee), 27);
        assert_eq!(bt.magic_strike(3, 1), 30, "magic is not reduced");
        let bt = battle(&[(10, f(0)), (18, f(2))], &[(10, f(2))]);
        assert_eq!(bt.physical_damage(2, 1, ActionKind::Melee), 30);
    }

    #[test]
    fn counterblow_and_death_curse() {
        let chief = bonus(37, Bonus::Counterblow, UnitDef { hits: 100, ..warrior(37, 25, 0) });
        let cursed = bonus(38, Bonus::DeathCurse, UnitDef { hits: 1, ..warrior(38, 1, 0) });
        let mut bt = with(vec![chief, cursed], &[(10, f(2)), (10, f(4))], &[(37, f(2)), (38, f(4))]);
        turn_of(&mut bt, 0);
        let hit = bt.act(2).unwrap();
        assert_eq!((hit.amount, hit.counter), (30, Some(20)), "25 − 5 back");
        turn_of(&mut bt, 1);
        let hit = bt.act(3).unwrap();
        assert!(hit.killed && hit.actor_died && !bt.fighters[1].alive());
    }

    // --- magic ------------------------------------------------------------------------------

    #[test]
    fn magic_strike_by_school_and_nature() {
        let bt = battle(&[(15, b(0)), (14, b(1)), (12, b(2))], &[(18, f(0)), (16, f(1)), (17, f(2))]);
        let (life, death, elem) = (0, 1, 2);
        let (normal, undead, elemental) = (3, 4, 5);
        assert_eq!([bt.magic_strike(life, normal), bt.magic_strike(life, undead), bt.magic_strike(life, elemental)], [30, 60, 22]);
        assert_eq!([bt.magic_strike(death, normal), bt.magic_strike(death, undead), bt.magic_strike(death, elemental)], [30, 15, 22]);
        assert_eq!([bt.magic_strike(elem, normal), bt.magic_strike(elem, undead)], [30, 30], "Elemental: 0.75 × 40");
    }

    #[test]
    fn protection_reduces_hostile_magic() {
        let warded = UnitDef { protect_life: 50, protect_elemental: 100, hits: 100, ..warrior(39, 20, 0) };
        let bt = with(vec![warded], &[(15, b(0)), (12, b(1))], &[(39, f(0))]);
        assert_eq!(bt.magic_strike(0, 2), 15);
        assert_eq!(bt.magic_strike(1, 2), 0);
        assert_eq!(bt.options(1, 2), Vec::<ActionKind>::new(), "fully protected: nothing to cast");
    }

    #[test]
    fn blessings_and_curses_by_school() {
        let life_all = mage(40, 36, MagicSchool::Life, MagicDirection::ToAll);
        let death_all = mage(41, 36, MagicSchool::Death, MagicDirection::ToAll);
        let elem_all = mage(42, 36, MagicSchool::Elemental, MagicDirection::ToAll);
        let bt = with(vec![life_all, death_all, elem_all], &[(40, b(0)), (41, b(1)), (42, b(2)), (10, f(0))], &[(10, f(0)), (16, f(1))]);
        let (ally, foe, undead) = (3, 4, 5);
        // Life: def + 3P/12 + 1 = 10, atk + 3P/24 = 4; curse def −(1 + 3P/10) = −11, atk −P/10 = −3.
        assert_eq!(bt.bless_buff(0, ally), Buff { attack: 4, defence: 10, ..Buff::default() });
        assert_eq!(bt.curse_buff(0, foe), Buff { attack: -3, defence: -11, ..Buff::default() });
        assert!(bt.bless_buff(0, undead).is_empty(), "Life does not bless the undead");
        // Death: atk + P/6 + 1 = 7, def + P/12 = 3; curse atk −(1 + P/5) = −8, def −P/10 = −3.
        assert_eq!(bt.bless_buff(1, ally), Buff { attack: 7, defence: 3, ..Buff::default() });
        assert_eq!(bt.curse_buff(1, foe), Buff { attack: -8, defence: -3, ..Buff::default() });
        // Elemental: actions ±f(36) = 1, initiative + (P/7 + 1) = 6.
        assert_eq!(bt.bless_buff(2, ally), Buff { actions: 1, initiative: 6, ..Buff::default() });
        assert_eq!(bt.curse_buff(2, foe), Buff { actions: -1, initiative: -6, ..Buff::default() });
        assert_eq!([actions_of_power(19), actions_of_power(20), actions_of_power(45), actions_of_power(100)], [0, 1, 2, 3]);
    }

    #[test]
    fn heal_by_school_and_nature() {
        let death_healer = mage(43, 20, MagicSchool::Death, MagicDirection::ToAlly);
        let mut bt = with(vec![death_healer], &[(13, b(0)), (43, b(1)), (10, f(0)), (16, f(1))], &[(18, f(0))]);
        bt.fighters[2].hp = 10;
        bt.fighters[3].hp = 10;
        assert_eq!(bt.preview(0, 2, ActionKind::Heal), Preview::Heal(20));
        assert!(!bt.options(0, 3).contains(&ActionKind::Heal), "Life cannot heal the undead");
        assert!(bt.options(1, 3).contains(&ActionKind::Heal), "Death heals the undead");
        assert!(!bt.options(1, 2).contains(&ActionKind::Heal), "…and only them");
    }

    #[test]
    fn curse_lowers_stats_and_wears_off() {
        let mut bt = battle(&[(18, f(5)), (12, b(0))], &[(10, f(0))]);
        turn_of(&mut bt, 1);
        let hit = bt.act(2).unwrap();
        assert_eq!(hit.kind, ActionKind::Curse, "Elemental defaults to the curse");
        // P 40: actions −1, initiative −(1 + 40/7).
        assert_eq!((bt.fighters[2].stats[Stat::Manevres], bt.fighters[2].stats[Stat::Initiative]), (0, 4));
        assert!(bt.options(1, 2).contains(&ActionKind::Strike));
        while bt.round <= EFFECT_TURNS {
            assert_ne!(bt.active(), Some(2), "no actions left while cursed");
            bt.skip();
        }
        assert_eq!(bt.fighters[2].curse(), None);
        assert_eq!(bt.fighters[2].stats[Stat::Manevres], 1);
    }

    #[test]
    fn magic_power_drains_per_turn_down_to_the_floor() {
        // Elemental: −5 per turn, floor 15. Life healer at 20: −2 to 15. Death: −2, floor 0.
        let low = mage(44, 10, MagicSchool::Life, MagicDirection::ToAlly);
        let own = UnitDef { min_magic_power: Some(30), mana_drain: Some(1), ..mage(45, 32, MagicSchool::Elemental, MagicDirection::ToEnemy) };
        let mut bt = with(vec![low, own], &[(12, b(0)), (13, b(1)), (14, b(2)), (44, b(3)), (45, b(4))], &[(18, f(0))]);
        let powers = |bt: &Battle| (0..5).map(|i| bt.fighters[i].stats[Stat::MagicPower]).collect::<Vec<_>>();
        assert_eq!(powers(&bt), vec![40, 20, 30, 10, 32]);
        let mut seen = vec![];
        for turn in 2..=6 {
            while bt.round < turn {
                bt.skip();
            }
            seen.push(powers(&bt));
        }
        assert_eq!(seen[0], vec![35, 18, 28, 10, 31]);
        assert_eq!(seen[1], vec![30, 16, 26, 10, 30]);
        assert_eq!(seen[4], vec![15, 15, 20, 10, 30], "floors: 15, 15, community override 30; 10 is below the floor and stays");
    }

    // --- outcome, hero, XP ---------------------------------------------------------------------

    #[test]
    fn hero_survives_while_his_army_lives() {
        let mut bt = battle(&[(10, f(2)), (10, f(3))], &[(18, f(2))]);
        bt.fighters[0].hp = 0;
        assert_eq!(bt.outcome(), Outcome::Ongoing, "hero down, army fights on");
        bt.fighters[2].hp = 0;
        assert_eq!(bt.outcome(), Outcome::Victory);
        let res = bt.player_results();
        assert_eq!((res[0].hp, res[1].hp), (1, 50), "hero comes back badly wounded");
        let mut bt = battle(&[(10, f(2)), (10, f(3))], &[(18, f(2))]);
        bt.fighters[0].hp = 0;
        bt.fighters[1].hp = 0;
        assert_eq!(bt.outcome(), Outcome::Defeat, "defeat only when the whole army is dead");
        assert_eq!(bt.player_results()[0].hp, 0);
    }

    #[test]
    fn deployment_is_kept_after_the_battle() {
        let c = content_with(vec![], Formation::WIDE);
        let squad = [Unit::new(&c, UnitId(10), f(2)), Unit::new(&c, UnitId(11), b(2))];
        let p: Vec<_> = squad.iter().enumerate().collect();
        let mut bt = Battle::new(c.clone(), &p, &[Unit::new(&c, UnitId(18), f(2))], Team::Player);
        assert_eq!(bt.active(), None);
        bt.move_card(f(2), b(2)).unwrap();
        assert_eq!((bt.fighters[0].slot, bt.fighters[1].slot), (b(2), f(2)));
        bt.move_card(f(2), f(5)).unwrap();
        assert_eq!(bt.move_card(f(0), f(1)), Err(ActionError::InvalidTarget));
        assert_eq!(bt.move_card(f(5), r(0)), Err(ActionError::InvalidTarget), "no reserve in 2×6");
        bt.begin();
        assert_eq!(bt.move_card(f(5), f(4)), Err(ActionError::NotDeploying));
        bt.fighters[0].slot = f(0); // pushed around during the fight
        let res = bt.player_results();
        assert_eq!((res[0].slot, res[1].slot), (b(2), f(5)));
    }

    #[test]
    fn xp_goes_to_survivors_weighted_by_row_and_contribution() {
        let mut bt = battle(&[(10, f(2)), (11, b(2)), (11, b(3))], &[(18, f(2)), (18, f(3))]);
        assert!(bt.xp_awards(Team::Player).is_empty(), "not over yet");
        bt.fighters[2].hp = 0;
        for i in [3, 4] {
            bt.fighters[i].hp = 0;
        }
        let xp = bt.xp_awards(Team::Player);
        assert_eq!(xp.iter().map(|a| a.fighter).collect::<Vec<_>>(), vec![0, 1]);
        assert!(xp[0].xp > xp[1].xp, "front row weighs more: {xp:?}");
        // Strength 150 vs 100: ratio 2/3, k = 1 − 1/3·0.5 = 5/6; pool = 0.3·5/6·100 + 5 = 30.
        // Weights 30 and 20; the vanilla player modifier halves it: 9 and 6.
        assert_eq!(xp.iter().map(|a| a.xp).collect::<Vec<_>>(), vec![9, 6]);
    }

    // --- AI ---------------------------------------------------------------------------------

    #[test]
    fn ai_kills_when_it_can_else_fewest_hits() {
        let mut bt = battle(&[(18, f(1)), (18, f(2)), (18, f(3))], &[(10, f(2))]);
        bt.fighters[1].hp = 150;
        turn_of(&mut bt, 3);
        assert_eq!(bt.ai_choice(), Some((1, ActionKind::Melee)), "fewest hits to kill");
        bt.fighters[2].hp = 5;
        assert_eq!(bt.ai_choice(), Some((2, ActionKind::Melee)), "a kill");
    }

    #[test]
    fn ai_mage_curses_first_then_strikes() {
        let mut bt = battle(&[(10, f(2))], &[(12, b(2)), (18, f(2))]);
        turn_of(&mut bt, 1);
        let Some(Step::Act { hit, .. }) = bt.ai_step() else { panic!() };
        assert_eq!((hit.target, hit.kind), (0, ActionKind::Curse));
        turn_of(&mut bt, 1);
        let Some(Step::Act { hit, .. }) = bt.ai_step() else { panic!() };
        assert_eq!((hit.target, hit.kind), (0, ActionKind::Strike), "already cursed: strike");
    }

    #[test]
    fn ai_warrior_uses_the_long_strike_or_steps_forward() {
        // Three empty cells opposite: the long strike reaches the far unit, no need to walk.
        let mut bt = battle(&[(18, f(1))], &[(10, f(4))]);
        turn_of(&mut bt, 1);
        assert_eq!(bt.ai_choice(), Some((0, ActionKind::LongStrike)));
        // A warrior in the back row steps into a free front cell.
        let mut bt = battle(&[(18, f(2))], &[(18, f(2)), (10, b(3))]);
        turn_of(&mut bt, 2);
        assert_eq!(bt.ai_step(), Some(Step::Move { actor: 2, from: b(3), to: f(3) }));
    }

    #[test]
    fn auto_battles_terminate_on_synthetic_armies() {
        let army = [(10, f(1)), (10, f(2)), (11, b(1)), (12, b(2)), (13, b(3)), (14, b(4))];
        for formation in [Formation::WIDE, Formation::VANILLA] {
            let c = content_with(vec![], formation);
            let mut bt = battle_in(&c, &army, &army);
            let mut steps = 0;
            while bt.outcome() == Outcome::Ongoing {
                bt.ai_step().expect("an active fighter");
                steps += 1;
                assert!(steps < 5000);
            }
        }
    }

    #[test]
    fn real_armies_auto_battle_terminates() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Arc::new(Content::from_dt(&dt));
        let ids: Vec<UnitId> = c.unit_ids().collect();
        let mut outcomes = [0; 4];
        for (n, chunk) in ids.chunks(8).enumerate() {
            let army = |offset: usize| -> Vec<Unit> {
                let mut taken = Vec::new();
                (0..8)
                    .map(|i| {
                        let id = chunk[(i + offset) % chunk.len()];
                        let s = Stats::of_level(&c, id, 1);
                        let slot = c.formation.free_slot(&taken, s.preferred_row()).unwrap();
                        taken.push(slot);
                        Unit::new(&c, id, slot)
                    })
                    .collect()
            };
            let (p, e) = (army(0), army(n % 3 + 1));
            let squad: Vec<_> = p.iter().enumerate().collect();
            let mut bt = Battle::new(c.clone(), &squad, &e, Team::Enemy);
            bt.begin();
            let mut steps = 0;
            while bt.outcome() == Outcome::Ongoing {
                bt.ai_step().expect("an active fighter");
                steps += 1;
                assert!(steps < 20_000, "battle {n} never ends");
            }
            outcomes[bt.outcome() as usize] += 1;
            assert!(bt.round <= 25);
            let _ = bt.xp_awards(Team::Player);
        }
        assert_eq!(outcomes[0], 0);
        assert!(outcomes.iter().sum::<i32>() >= 12);
    }
}
