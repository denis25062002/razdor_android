//! Card battles, following the original's rules (mechanics.md 2 and 3.3).
//!
//! Each side stands in a [`Formation`] (front, back and optionally a reserve row). Units act
//! one at a time by initiative; each has `Manevres` actions per turn, spent on an attack, a
//! spell or a step. Damage is deterministic. The battle ends when a side is wiped out, or
//! undecided after `BattleEndTurn` turns. There is no retreat.
//!
//! Choices where the original is unknown are marked *(guess)*.

use std::sync::Arc;

use super::content::{Bonus, Content, HeroClass, MagicSchool, Nature, SpellDef, Stat, UnitId};
use super::experience::{self, Role, SideUnit};
use super::formation::{Formation, Row, Slot};
use super::units::{Stats, Unit};

/// How many turns a blessing or curse lasts, the turn it is cast included *(guess: the
/// original's duration is unknown)*.
pub const EFFECT_TURNS: u32 = 3;
/// Share of max HP a poisoned unit loses each turn (bonus `Poison`; `CtrPoison` too).
const POISON_PERCENT: i32 = 15;
/// Community `PoisonS` (strong poison) and `PoisonArmorIgnore` poison per turn.
const STRONG_POISON_PERCENT: i32 = 25;
const PIERCING_POISON_PERCENT: i32 = 10;
/// Community `Exhaustion`: magic protection lost per hostile spell, in points.
const EXHAUSTION_POINTS: i32 = 15;
/// Community `Drying`: extra damage of hostile magic, % of the target's max HP.
const DRYING_PERCENT: i32 = 8;
/// Community `Fortify`: physical defence +25% per turn, up to +125%.
const FORTIFY_STEP: i32 = 25;
const FORTIFY_MAX: i32 = 125;
/// Community `Splash`: attack on the target and on its row neighbours, %.
const SPLASH_MAIN: i32 = 80;
const SPLASH_SIDE: i32 = 40;
/// Community `KillingStrike`: a target left below this % of max HP dies.
const KILLING_STRIKE_PERCENT: i32 = 25;
/// Community `Bleed`: share of a wound that bleeds again at the next turn *(guess)*.
const BLEED_PERCENT: i32 = 50;
/// Community `FateGift`: attack and defence gain when the gift saves the unit *(guess)*.
const FATE_GIFT_PERCENT: i32 = 25;

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
    /// XP towards the next level as the battle began (for display).
    pub xp: i32,
    /// Stats at the start of the battle (level, items, potions).
    pub base: Stats,
    /// Current stats: base with magic drain, blessing and curse.
    pub stats: Stats,
    /// Magic power left after the per-turn drain.
    pub power: i32,
    blessing: Option<Timed>,
    curse: Option<Timed>,
    /// Poisoned: `poison` % of max HP lost every turn.
    pub poisoned: bool,
    pub poison: i32,
    /// Community `Bleed`: HP lost at the start of the next turn.
    pub bleeding: i32,
    /// Community `Stun`: initiative already cut.
    pub stunned: bool,
    /// Community `NoHeal`: wounded by a crippling weapon, heals no more this battle.
    pub crippled: bool,
    /// Community `FateGift` used up.
    pub fate_used: bool,
    /// Tactical cost at the start (experience.md §1), for the sides' strength.
    pub tactical: i32,
    /// Role in the side's strength sum, set at the start.
    pub role: Role,
    /// For the XP share: attacks and spells made, all actions taken (moves and waits too),
    /// actions left this turn, and hit points lost.
    pub useful: i32,
    pub taken: i32,
    pub left: i32,
    pub lost: i32,
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
            xp: unit.xp,
            power: base[Stat::MagicPower],
            stats: base.clone(),
            base,
            blessing: None,
            curse: None,
            poisoned: false,
            poison: 0,
            bleeding: 0,
            stunned: false,
            crippled: false,
            fate_used: false,
            tactical: 1,
            role: Role::Melee,
            useful: 0,
            taken: 0,
            left: 0,
            lost: 0,
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
    /// Counterblow (after) or PreventiveStrike (before) damage taken by the actor.
    pub counter: Option<i32>,
    /// The actor died: killed a `DeathCurse`/`Ghost` unit, fell to the counterblow or the
    /// preventive strike, or it is a `Suicide` unit.
    pub actor_died: bool,
    /// Community `Splash`: damage to the target's row neighbours (fighter, damage).
    pub splash: Vec<(usize, i32)>,
}

impl Hit {
    fn new(target: usize, kind: ActionKind) -> Hit {
        Hit { target, kind, amount: 0, buff: Buff::default(), killed: false, counter: None, actor_died: false, splash: Vec::new() }
    }
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

/// A side as the battle began, for the XP pool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SideStart {
    pub strength: i64,
    pub hp: i64,
    pub count: usize,
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
    /// The side fights inside a building (a garrison): `Bastion` works there, `Assault`
    /// against it.
    in_building: [bool; 2],
    order: Vec<usize>,
    turn: usize,
    actions_left: i32,
    deploying: bool,
    stalemate: bool,
    /// Both sides at the start ([`Battle::begin`]).
    start: [SideStart; 2],
    /// The beaten army's experience correction for the player's XP (100 for a garrison).
    xp_correction: i32,
}

/// A fighter in its side's strength sum.
fn side_unit(f: &Fighter) -> SideUnit {
    SideUnit { tactical: f.tactical, hp: f.hp, max_hp: f.max_hp(), row: f.slot.row, role: f.role }
}

/// Bonuses whose attacks ignore the target's defence.
const PIERCING: [Bonus; 5] =
    [Bonus::ArmorIgnore, Bonus::VampirsGist, Bonus::OldVampirsGist, Bonus::Artillery, Bonus::PoisonArmorIgnore];
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
            in_building: [false; 2],
            order: Vec::new(),
            turn: 0,
            actions_left: 0,
            deploying: true,
            stalemate: false,
            start: [SideStart::default(); 2],
            xp_correction: 100,
        }
    }

    /// The experience correction (percent) of the army the player fights: it scales the
    /// player's XP (experience.md §3). A garrison's is 100.
    pub fn set_xp_correction(&mut self, percent: i32) {
        self.xp_correction = percent;
    }

    /// Both sides as the battle began.
    pub fn start_of(&self, team: Team) -> SideStart {
        self.start[team.index()]
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    /// Lasting world spells on `team`'s army (`rules::magic`): their modifiers change the
    /// fighters' starting stats. A higher maximum HP raises the HP by the same amount, a
    /// lower one caps it *(guess)*. Call before [`Battle::begin`].
    pub fn apply_spells(&mut self, team: Team, spells: &[&SpellDef]) {
        if spells.is_empty() {
            return;
        }
        for f in self.fighters.iter_mut().filter(|f| f.team == team) {
            let before = f.base.max_hp();
            super::magic::apply(&mut f.base, spells);
            let after = f.base.max_hp();
            f.stats = f.base.clone();
            f.power = f.base[Stat::MagicPower];
            if f.alive() {
                f.hp = (f.hp + (after - before).max(0)).min(after);
            }
        }
    }

    /// Extra defence of the building `team` fights in (garrisons). Set before [`Battle::begin`].
    /// It also puts `team` inside a building ([`Battle::set_in_building`]).
    pub fn set_building_defence(&mut self, team: Team, defence: i32) {
        self.building_defence[team.index()] = defence;
        self.in_building[team.index()] = true;
    }

    /// `team` fights inside a building (a castle or fort garrison), even one without extra
    /// defence. Set before [`Battle::begin`].
    pub fn set_in_building(&mut self, team: Team) {
        self.in_building[team.index()] = true;
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
        const COMBAT: [Stat; 4] = [Stat::AttackBlow, Stat::AttackShot, Stat::DefenceBlow, Stat::DefenceShot];
        // Community Bastion: the whole army inside gets +10 defence (once per army *(guess)*).
        let bastion = |team: Team| {
            self.in_building[team.index()] && self.fighters.iter().any(|f| f.team == team && f.alive() && f.base.has(&Bonus::Bastion))
        };
        let bastions = [bastion(Team::Player), bastion(Team::Enemy)];
        // Strength at the start, from the stats the units bring (items, spells) and the
        // building they stand in.
        for f in &mut self.fighters {
            f.tactical = experience::tactical(&self.content, f.unit, &f.base, self.building_defence[f.team.index()]);
            f.role = experience::role(&f.base);
        }
        for team in [Team::Player, Team::Enemy] {
            let side: Vec<&Fighter> = self.fighters.iter().filter(|f| f.team == team && f.alive()).collect();
            self.start[team.index()] = SideStart {
                strength: experience::side_strength(&side.iter().map(|f| side_unit(f)).collect::<Vec<_>>()),
                hp: side.iter().map(|f| f.hp as i64).sum(),
                count: side.len(),
            };
        }
        for f in &mut self.fighters {
            f.deployed = f.slot;
            let inside = self.in_building[f.team.index()];
            let storming = self.in_building[f.team.other().index()];
            // Garrison: stats ×2 inside a strong building (mechanics.md 1.3).
            let mut factor = 1;
            if f.base.has(&Bonus::Garrison) && self.building_defence[f.team.index()] >= 10 {
                factor *= 2;
            }
            // Community: Bastion ×3 inside a castle or fort, Assault ×2 storming one.
            if f.base.has(&Bonus::Bastion) && inside {
                factor *= 3;
            }
            if f.base.has(&Bonus::Assault) && storming {
                factor *= 2;
            }
            for st in COMBAT {
                f.base[st] *= factor;
            }
            if bastions[f.team.index()] {
                f.base[Stat::DefenceBlow] += 10;
                f.base[Stat::DefenceShot] += 10;
            }
            f.stats = f.base.clone();
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

    /// Turn order key: on turn 1 `FirstShot` first (Community), then `Artillery`, then
    /// initiative (attacker +1), attacker on ties.
    fn order_key(&self, i: usize) -> (bool, bool, i32, bool, usize) {
        let f = &self.fighters[i];
        let ini = f.stats[Stat::Initiative] + i32::from(f.team == self.attacker);
        let first = self.round == 1 && f.stats.has(&Bonus::FirstShot);
        (!first, !f.stats.has(&Bonus::Artillery), -ini, f.team != self.attacker, i)
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
        for &i in &order {
            self.fighters[i].left = self.turn_actions(i).max(0);
        }
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
            if f.base.has(&Bonus::Concentration) {
                // Community Concentration: power grows by a tenth of its base every turn, up
                // to twice the base, instead of draining *(guess: the rate is not documented)*.
                let base = f.base[Stat::MagicPower];
                if f.power < 2 * base {
                    f.power = (f.power + (base / 10).max(1)).min(2 * base);
                }
            } else if f.power > floor {
                // Magic power drains to the floor, never below (or up to) it.
                f.power = (f.power - dec).max(floor);
            }
            let max = f.base.max_hp();
            // Regen: percent of max HP per turn (guess: the original's timing is unverified).
            let regen = f.base[Stat::Regen];
            if regen > 0 && f.hp < max && !f.crippled {
                let healed = (max * regen / 100).max(1).min(max - f.hp);
                f.hp += healed;
                self.log.push(format!("{} regenerates +{healed}", f.name));
            }
            if f.poisoned {
                let loss = (max * f.poison.max(1) / 100).max(1).min(f.hp);
                f.hp -= loss;
                f.lost += loss;
                self.log.push(format!("{} suffers {loss} from poison", f.name));
            }
            if f.bleeding > 0 && f.alive() {
                let loss = f.bleeding.min(f.hp);
                f.hp -= loss;
                f.lost += loss;
                f.bleeding = 0;
                self.log.push(format!("{} bleeds for {loss}", f.name));
            }
            self.refresh(i);
        }
        self.collapse(Team::Player);
        self.collapse(Team::Enemy);
    }

    /// Actions `id` gets this turn: `Manevres`, +1 on turn 1 for the fast-start bonuses,
    /// +1 on turns 1 and 2 for the Community `FasterAttack`.
    fn turn_actions(&self, id: usize) -> i32 {
        let f = &self.fighters[id];
        f.stats[Stat::Manevres]
            + i32::from(self.round == 1 && f.stats.has_any(&FAST_START))
            + i32::from(self.round <= 2 && f.stats.has(&Bonus::FasterAttack))
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
        let f = &mut self.fighters[id];
        f.taken += 1;
        f.left = self.actions_left.max(0);
        if self.actions_left <= 0 || !self.fighters[id].alive() {
            self.end_turn();
        }
    }

    /// End the active fighter's turn, forfeiting any remaining actions.
    pub fn skip(&mut self) {
        if let Some(id) = self.active() {
            self.log.push(format!("{} waits", self.fighters[id].name));
            // Waiting spends what is left, one pass per action as in the original.
            let f = &mut self.fighters[id];
            f.taken += self.actions_left.max(0);
            f.left = 0;
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
            // Community Flying: any enemy (front or back row) from any row, as a normal strike.
            let flying = s.has(&Bonus::Flying);
            if s.is_warrior() && flying {
                v.push(Melee);
            } else if s.is_warrior() && from.row == Row::Front && t.slot.row == Row::Front {
                if adjacent {
                    v.push(Melee);
                } else if !blocked && self.long_strike_targets(t.team, from.col).contains(&target) {
                    v.push(LongStrike);
                }
            }
            if s.is_shooter() && (flying || from.row == Row::Back || !blocked || adjacent) {
                v.push(Shot);
            }
            if s.is_mage() && s.magic_direction().hits_enemies() && (flying || from.row == Row::Back || !blocked) {
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

    /// Living units of `team`.
    fn head_count(&self, team: Team) -> usize {
        self.living(team).count()
    }

    /// Physical damage of `a` on `t`, before capping at the target's HP. Deterministic.
    /// A `Splash` unit's attack counts 80% on its target.
    pub fn physical_damage(&self, a: usize, t: usize, kind: ActionKind) -> i32 {
        let pct = if self.fighters[a].stats.has(&Bonus::Splash) { SPLASH_MAIN } else { 100 };
        self.physical_damage_at(a, t, kind, pct)
    }

    /// Physical damage with `pct`% of the attacker's attack.
    fn physical_damage_at(&self, a: usize, t: usize, kind: ActionKind, pct: i32) -> i32 {
        let (af, tf) = (&self.fighters[a], &self.fighters[t]);
        let (s, ts) = (&af.stats, &tf.stats);
        let shot = kind == ActionKind::Shot;
        let building = self.building_defence[tf.team.index()];
        let mut atk = if shot { s[Stat::AttackShot] } else { s[Stat::AttackBlow] } * pct / 100;
        let mut own = if shot { ts[Stat::DefenceShot] } else { ts[Stat::DefenceBlow] };
        if ts.has(&Bonus::Fortify) {
            // Community Fortify: +25% of its own defence per turn (turn 1 included *(guess)*).
            own = own * (100 + (FORTIFY_STEP * self.round.max(1) as i32).min(FORTIFY_MAX)) / 100;
        }
        let mut def = own + building;
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
        if s.has_any(&PIERCING) {
            def = building;
        }
        let mut dmg = (atk - def).max(1);
        // Community: Berserk up to ×2 as its HP falls *(guess: linear in the HP lost)*.
        if s.has(&Bonus::Berserk) {
            let max = af.max_hp().max(1);
            dmg = dmg * (2 * max - af.hp.clamp(0, max)) / max;
        }
        // Community: Flock ±25% by head count against the other army.
        if s.has(&Bonus::Flock) {
            let (own, other) = (self.head_count(af.team), self.head_count(tf.team));
            if own > other {
                dmg = dmg * 125 / 100;
            } else if own < other {
                dmg = dmg * 75 / 100;
            }
        }
        // Community: Dominate +25% on a target with less max HP *(guess: undocumented)*.
        if s.has(&Bonus::Dominate) && tf.max_hp() < af.max_hp() {
            dmg = dmg * 125 / 100;
        }
        if ts.has_any(&[Bonus::VampirsGist, Bonus::OldVampirsGist, Bonus::Evasive]) {
            dmg = dmg * 2 / 3;
        }
        if ts.has(&Bonus::Garrison) && building >= 10 {
            dmg = dmg * 2 / 3;
        }
        // Community: Bastion takes half inside, Assault 70% while storming.
        if ts.has(&Bonus::Bastion) && self.in_building[tf.team.index()] {
            dmg /= 2;
        }
        if ts.has(&Bonus::Assault) && self.in_building[af.team.index()] {
            dmg = dmg * 7 / 10;
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

    /// Caster power against `t` for hostile magic: reduced by the target's protection %,
    /// except for a Community `Potent` caster.
    fn hostile_power(&self, a: usize, t: usize) -> i32 {
        let p = self.fighters[a].stats[Stat::MagicPower];
        if self.fighters[a].stats.has(&Bonus::Potent) {
            return p;
        }
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
        let dmg = if dmg > 0 { dmg + god_bonus(&self.fighters[a].stats) } else { 0 };
        dmg + self.drying(a, t)
    }

    /// Community `Drying`: 8% of the target's max HP on every hostile spell, ignoring
    /// protection (at least 1).
    fn drying(&self, a: usize, t: usize) -> i32 {
        if self.fighters[a].stats.has(&Bonus::Drying) {
            (self.fighters[t].max_hp() * DRYING_PERCENT / 100).max(1)
        } else {
            0
        }
    }

    /// HP a heal restores before capping: Life P (not on undead or elementals), Elemental
    /// P/2, Death P on undead only.
    fn heal_power(&self, a: usize, t: usize) -> i32 {
        let p = self.fighters[a].stats[Stat::MagicPower];
        let nature = self.fighters[t].stats.nature;
        if self.fighters[t].crippled {
            return 0;
        }
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
        let mut hit = Hit::new(target, kind);
        let (name, tname) = (self.fighters[id].name.clone(), self.fighters[target].name.clone());
        if kind.is_physical() {
            self.preventive_strike(id, target, &mut hit);
        }
        if self.fighters[id].alive() {
            match kind {
                ActionKind::Heal => {
                    let t = &self.fighters[target];
                    let healed = self.heal_power(id, target).min(t.max_hp() - t.hp);
                    self.fighters[target].hp += healed;
                    hit.amount = healed;
                    self.log.push(format!("{name} heals {tname} +{healed}"));
                }
                ActionKind::Bless | ActionKind::Curse => {
                    let buff = if kind == ActionKind::Bless { self.bless_buff(id, target) } else { self.curse_buff(id, target) };
                    // Community EternalGift: the whole battle.
                    let until = if self.fighters[id].stats.has(&Bonus::EternalGift) { u32::MAX } else { self.round + EFFECT_TURNS - 1 };
                    let timed = Some(Timed { buff, until });
                    if kind == ActionKind::Bless {
                        self.fighters[target].blessing = timed;
                    } else {
                        self.fighters[target].curse = timed;
                    }
                    self.refresh(target);
                    hit.buff = buff;
                    let verb = if kind == ActionKind::Bless { "blesses" } else { "curses" };
                    self.log.push(format!("{name} {verb} {tname}: {}", buff.describe()));
                    if kind == ActionKind::Curse {
                        let dry = self.drying(id, target);
                        if dry > 0 {
                            self.deal(id, target, dry, &mut hit);
                        }
                        self.exhaust(id, target);
                    }
                }
                _ => {
                    let raw = if kind == ActionKind::Strike { self.magic_strike(id, target) } else { self.physical_damage(id, target, kind) };
                    self.deal(id, target, raw, &mut hit);
                    self.on_hit(id, target, &mut hit);
                    if kind.is_physical() {
                        self.splash(id, target, &mut hit);
                    }
                    let melee = matches!(kind, ActionKind::Melee | ActionKind::LongStrike);
                    let t = &self.fighters[target];
                    // Counterblow: a warrior struck in melee hits back once.
                    if melee && t.alive() && t.stats.has(&Bonus::Counterblow) && t.stats.is_warrior() && self.fighters[id].alive() {
                        let dmg = self.physical_damage(target, id, ActionKind::Melee).min(self.fighters[id].hp);
                        self.wound(id, dmg);
                        hit.counter = Some(dmg);
                        self.log.push(format!("{tname} hits back for {dmg}"));
                        hit.actor_died |= !self.fighters[id].alive() && self.lethal(id);
                    }
                    // Community CtrPoison: striking it in melee poisons the striker *(guess:
                    // shots and spells do not)*.
                    if melee && self.fighters[target].base.has(&Bonus::CtrPoison) && self.fighters[id].alive() {
                        self.poison(id, POISON_PERCENT);
                    }
                }
            }
            // Community Suicide: dies after its own attack.
            if kind.is_hostile() && self.fighters[id].alive() && self.fighters[id].stats.has(&Bonus::Suicide) {
                let hp = self.fighters[id].hp;
                self.wound(id, hp);
                hit.actor_died = true;
                self.log.push(format!("{name} gives its life"));
            }
        }
        self.fighters[id].useful += 1;
        self.collapse(Team::Player);
        self.collapse(Team::Enemy);
        self.spend_action(id);
        Ok(hit)
    }

    /// `i` loses `amount` hit points (already capped at its HP).
    fn wound(&mut self, i: usize, amount: i32) {
        let f = &mut self.fighters[i];
        f.hp -= amount;
        f.lost += amount;
    }

    /// A fighter at 0 HP: true if it dies, false if the Community `FateGift` saves it (once
    /// a battle: full HP and attack and defence +25% *(guess: the size of the gain)*).
    fn lethal(&mut self, i: usize) -> bool {
        let f = &mut self.fighters[i];
        if f.alive() || f.fate_used || !f.base.has(&Bonus::FateGift) {
            return !f.alive();
        }
        f.fate_used = true;
        for st in [Stat::AttackBlow, Stat::AttackShot, Stat::DefenceBlow, Stat::DefenceShot] {
            f.base[st] = f.base[st] * (100 + FATE_GIFT_PERCENT) / 100;
        }
        f.hp = f.base.max_hp();
        self.log.push(format!("{} is spared by fate", f.name));
        self.refresh(i);
        false
    }

    /// Poisons `i` for `percent`% of max HP a turn (the strongest poison counts).
    fn poison(&mut self, i: usize, percent: i32) {
        let f = &mut self.fighters[i];
        if f.alive() {
            f.poisoned = true;
            f.poison = f.poison.max(percent);
        }
    }

    /// Community `Exhaustion`: the target loses 15 points of every magic protection for the
    /// battle (cumulative; all three schools *(guess)*).
    fn exhaust(&mut self, a: usize, t: usize) {
        if !self.fighters[a].stats.has(&Bonus::Exhaustion) || !self.fighters[t].alive() {
            return;
        }
        let f = &mut self.fighters[t];
        for st in [Stat::ProtectLife, Stat::ProtectDeath, Stat::ProtectElemental] {
            f.base[st] = (f.base[st] - EXHAUSTION_POINTS).max(0);
        }
        self.refresh(t);
    }

    /// Community `PreventiveStrike`: a unit about to be struck by an enemy strikes (or
    /// shoots) the attacker first, whatever the reach *(guess)*.
    fn preventive_strike(&mut self, id: usize, target: usize, hit: &mut Hit) {
        let t = &self.fighters[target];
        if !t.alive() || t.team == self.fighters[id].team || !t.stats.has(&Bonus::PreventiveStrike) {
            return;
        }
        let kind = if t.stats.is_warrior() {
            ActionKind::Melee
        } else if t.stats.is_shooter() {
            ActionKind::Shot
        } else {
            return;
        };
        let dmg = self.physical_damage(target, id, kind).min(self.fighters[id].hp);
        self.wound(id, dmg);
        hit.counter = Some(dmg);
        self.log.push(format!("{} strikes first for {dmg}", self.fighters[target].name));
        hit.actor_died |= !self.fighters[id].alive() && self.lethal(id);
    }

    /// Community `Splash`: 40% of the attack on each living neighbour of the target in its
    /// row.
    fn splash(&mut self, id: usize, target: usize, hit: &mut Hit) {
        if !self.fighters[id].stats.has(&Bonus::Splash) {
            return;
        }
        let (team, slot) = (self.fighters[target].team, self.fighters[target].slot);
        let near: Vec<usize> = (0..self.fighters.len())
            .filter(|&n| {
                let f = &self.fighters[n];
                n != target && f.alive() && f.team == team && f.slot.row == slot.row && f.slot.col.abs_diff(slot.col) == 1
            })
            .collect();
        for n in near {
            if !self.fighters[id].alive() {
                break;
            }
            let raw = self.physical_damage_at(id, n, hit.kind, SPLASH_SIDE);
            let mut side = Hit::new(n, hit.kind);
            self.deal(id, n, raw, &mut side);
            hit.splash.push((n, side.amount));
            hit.actor_died |= side.actor_died;
        }
    }

    /// Community effects of a damaging hit (physical or a magic strike) on its target.
    fn on_hit(&mut self, id: usize, target: usize, hit: &mut Hit) {
        let a = self.fighters[id].stats.clone();
        let physical = hit.kind.is_physical();
        if hit.killed {
            // Hunger: a kill heals to full HP [exe: HP set to max HP].
            if a.has(&Bonus::Hunger) && self.fighters[id].alive() && !self.fighters[id].crippled {
                let f = &mut self.fighters[id];
                f.hp = f.max_hp();
            }
            // BloodThrist: a kill gives the action back.
            if a.has(&Bonus::BloodThrist) && self.fighters[id].alive() && self.order.get(self.turn) == Some(&id) {
                self.actions_left += 1;
                self.fighters[id].left += 1;
            }
            return;
        }
        if !self.fighters[target].alive() {
            return;
        }
        // Poisons: after a solid hit (more than 1). Poison works for mages too (Community).
        if hit.amount > 1 {
            let mut pct = 0;
            if a.has(&Bonus::Poison) {
                pct = pct.max(POISON_PERCENT);
            }
            if a.has(&Bonus::PoisonS) {
                pct = pct.max(STRONG_POISON_PERCENT);
            }
            if physical && a.has(&Bonus::PoisonArmorIgnore) {
                pct = pct.max(PIERCING_POISON_PERCENT);
            }
            if pct > 0 {
                self.poison(target, pct);
            }
        }
        if physical && hit.amount > 0 {
            let t = &mut self.fighters[target];
            if a.has(&Bonus::Bleed) {
                t.bleeding += (hit.amount * BLEED_PERCENT / 100).max(1);
            }
            if a.has(&Bonus::ArmorBreaker) {
                t.base[Stat::DefenceBlow] = t.base[Stat::DefenceBlow] * 7 / 10;
                t.base[Stat::DefenceShot] = t.base[Stat::DefenceShot] * 7 / 10;
            }
            if a.has(&Bonus::NoHeal) {
                t.crippled = true;
            }
        }
        let t = &mut self.fighters[target];
        if a.has(&Bonus::Stun) && !t.stunned {
            t.stunned = true;
            t.base[Stat::Initiative] = t.base[Stat::Initiative] * 3 / 4;
        }
        if a.has(&Bonus::Neutralize) {
            t.base.bonuses.clear();
        }
        self.refresh(target);
        if hit.kind == ActionKind::Strike {
            self.exhaust(id, target);
        }
        let t = &self.fighters[target];
        if a.has(&Bonus::KillingStrike) && t.hp * 100 < t.max_hp() * KILLING_STRIKE_PERCENT {
            let (name, tname, left) = (self.fighters[id].name.clone(), t.name.clone(), t.hp);
            hit.amount += left;
            self.wound(target, left);
            self.log.push(format!("{name} finishes {tname}"));
            if self.lethal(target) {
                hit.killed = true;
                self.death_curse(id, target, hit);
            }
        }
    }

    /// The killer of a `DeathCurse` or `Ghost` unit dies.
    fn death_curse(&mut self, id: usize, target: usize, hit: &mut Hit) {
        if self.fighters[target].stats.has_any(&[Bonus::DeathCurse, Bonus::Ghost]) && self.fighters[id].alive() {
            let hp = self.fighters[id].hp;
            self.wound(id, hp);
            hit.actor_died = true;
            let msg = format!("{} dies by {}'s curse", self.fighters[id].name, self.fighters[target].name);
            self.log.push(msg);
        }
    }

    /// Applies `raw` damage (capped at HP), vampirism and the killer-dies bonuses.
    fn deal(&mut self, id: usize, target: usize, raw: i32, hit: &mut Hit) {
        let dmg = raw.min(self.fighters[target].hp);
        self.wound(target, dmg);
        let (name, tname) = (self.fighters[id].name.clone(), self.fighters[target].name.clone());
        let a = &mut self.fighters[id];
        let vamp = a.stats[Stat::Vampirizm];
        if vamp > 0 && !a.crippled {
            a.hp = (a.hp + dmg * vamp / 100).min(a.max_hp());
        }
        hit.amount += dmg;
        let how = match hit.kind {
            ActionKind::LongStrike => " with a long strike",
            ActionKind::Strike => " with magic",
            _ => "",
        };
        if !self.fighters[target].alive() && self.lethal(target) {
            hit.killed = true;
            self.log.push(format!("{name} kills {tname}{how} ({dmg})"));
            self.death_curse(id, target, hit);
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

    /// `team`'s strength now: its living units with their current HP and rows.
    pub fn strength_now(&self, team: Team) -> i64 {
        let side: Vec<SideUnit> = self.fighters.iter().filter(|f| f.team == team && f.alive()).map(side_unit).collect();
        experience::side_strength(&side)
    }

    /// Each survivor's share of `team`'s XP pool once the battle is over, before any
    /// modifier (experience.md §3): pool = the enemy's starting strength div 20 × the share
    /// of `team`'s starting HP not lost; share = [`experience::share`] by row and activity.
    /// The dead get nothing but count in the divisor.
    pub fn xp_awards(&self, team: Team) -> Vec<XpAward> {
        if self.deploying || self.outcome() == Outcome::Ongoing {
            return Vec::new();
        }
        let own = self.start[team.index()];
        let lost: i64 = self.fighters.iter().filter(|f| f.team == team).map(|f| f.lost as i64).sum();
        let pool = experience::battle_pool(self.start[team.other().index()].strength, own.hp, lost);
        (0..self.fighters.len())
            .filter(|&i| self.fighters[i].team == team && self.fighters[i].alive())
            .map(|i| {
                let f = &self.fighters[i];
                XpAward { fighter: i, xp: experience::share(pool, own.count, f.slot.row, f.useful, f.taken, f.left) }
            })
            .collect()
    }

    /// What the player's survivors gain: only after a victory, each share ×
    /// `HeroExpirienceModificator` × the difficulty factor × the beaten army's correction,
    /// capped by the Community limit ([`experience::player_gain`]).
    pub fn player_xp(&self) -> Vec<XpAward> {
        if self.outcome() != Outcome::Victory {
            return Vec::new();
        }
        let o = self.opt();
        self.xp_awards(Team::Player)
            .into_iter()
            .map(|a| XpAward { xp: experience::player_gain(a.xp, o.hero_experience_modificator, o.difficulty_factor, self.xp_correction), ..a })
            .collect()
    }

    /// What an AI side gains in a battle between AI armies: each share ×
    /// `AIExpiriencePercent` / 100, for a side that still has strength at the end.
    pub fn ai_xp(&self, team: Team) -> Vec<XpAward> {
        if self.strength_now(team) <= 0 {
            return Vec::new();
        }
        let pct = self.opt().ai_experience_percent;
        self.xp_awards(team).into_iter().map(|a| XpAward { xp: experience::ai_gain(a.xp, pct), ..a }).collect()
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

    // --- Community bonuses --------------------------------------------------------------------

    /// A battle where `inside` fights in a building (a castle or fort garrison, no extra
    /// defence).
    fn battle_inside(extra: Vec<UnitDef>, player: &[(u32, Slot)], enemies: &[(u32, Slot)], inside: Team) -> Battle {
        let c = content_with(extra, Formation::WIDE);
        let squad: Vec<Unit> = player.iter().map(|&(id, s)| Unit::new(&c, UnitId(id), s)).collect();
        let p: Vec<_> = squad.iter().enumerate().collect();
        let e: Vec<Unit> = enemies.iter().map(|&(id, s)| Unit::new(&c, UnitId(id), s)).collect();
        let mut bt = Battle::new(c.clone(), &p, &e, inside.other());
        bt.set_in_building(inside);
        bt.begin();
        bt
    }

    fn to_round(bt: &mut Battle, round: u32) {
        while bt.round < round {
            bt.skip();
        }
    }

    /// 69: a heavy hitter, attack 100.
    fn hammer() -> UnitDef {
        warrior(69, 100, 0)
    }

    /// 84: armour 20, 300 HP.
    fn armour() -> UnitDef {
        UnitDef { hits: 300, ..warrior(84, 1, 20) }
    }

    #[test]
    fn hunger_heals_to_full_on_a_kill() {
        let hungry = bonus(60, Bonus::Hunger, warrior(60, 30, 0));
        let mut bt = with(vec![hungry], &[(60, f(2))], &[(10, f(2)), (10, f(3))]);
        bt.fighters[0].hp = 10;
        bt.fighters[1].hp = 1;
        turn_of(&mut bt, 0);
        assert!(bt.act(1).unwrap().killed);
        assert_eq!(bt.fighters[0].hp, 50);
    }

    #[test]
    fn berserk_hits_harder_when_wounded() {
        let berserk = bonus(61, Bonus::Berserk, warrior(61, 30, 0));
        let mut bt = with(vec![berserk], &[(61, f(2))], &[(18, f(2))]);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 30, "full HP: ×1");
        bt.fighters[0].hp = 25;
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 45, "half HP: ×1.5");
        bt.fighters[0].hp = 1;
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 59);
    }

    #[test]
    fn exhaustion_wears_down_magic_protection() {
        let tired = bonus(62, Bonus::Exhaustion, mage(62, 30, MagicSchool::Life, MagicDirection::ToEnemy));
        let warded = UnitDef { protect_life: 50, hits: 300, ..warrior(39, 1, 0) };
        let mut bt = with(vec![tired, warded], &[(62, b(0)), (10, f(5))], &[(39, f(0))]);
        assert_eq!(bt.magic_strike(0, 2), 15);
        turn_of(&mut bt, 0);
        assert_eq!(bt.act(2).unwrap().amount, 15);
        assert_eq!((bt.fighters[2].stats[Stat::ProtectLife], bt.fighters[2].stats[Stat::ProtectDeath]), (35, 0));
        assert_eq!(bt.magic_strike(0, 2), 20, "30 × 65%");
        turn_of(&mut bt, 0);
        bt.act(2).unwrap();
        assert_eq!(bt.fighters[2].stats[Stat::ProtectLife], 20, "cumulative");
    }

    #[test]
    fn drying_takes_eight_percent_of_max_hp_through_any_protection() {
        let dry = bonus(63, Bonus::Drying, mage(63, 30, MagicSchool::Life, MagicDirection::ToEnemy));
        let curser = bonus(64, Bonus::Drying, mage(64, 30, MagicSchool::Death, MagicDirection::ToEnemy));
        let warded = UnitDef { protect_life: 100, hits: 300, ..warrior(39, 1, 0) };
        let target = UnitDef { hits: 100, ..warrior(46, 1, 0) };
        let mut bt = with(vec![dry, curser, warded, target], &[(63, b(0)), (64, b(1)), (10, f(5))], &[(39, f(0)), (46, f(1))]);
        assert_eq!(bt.magic_strike(0, 3), 24, "0 + 8% of 300");
        assert_eq!(bt.options(0, 3), vec![ActionKind::Strike]);
        turn_of(&mut bt, 1);
        let hit = bt.act(4).unwrap();
        assert_eq!((hit.kind, hit.amount, bt.fighters[4].hp), (ActionKind::Curse, 8, 92), "a curse dries too");
    }

    #[test]
    fn ctr_poison_poisons_whoever_strikes_it_in_melee() {
        let toad = bonus(65, Bonus::CtrPoison, UnitDef { hits: 300, ..warrior(65, 1, 0) });
        let mut bt = with(vec![toad], &[(10, f(2)), (11, b(2))], &[(65, f(2))]);
        turn_of(&mut bt, 0);
        bt.act(2).unwrap();
        assert!(bt.fighters[0].poisoned);
        assert_eq!(bt.fighters[0].poison, 15);
        turn_of(&mut bt, 1);
        bt.act(2).unwrap();
        assert!(!bt.fighters[1].poisoned, "a shot does not touch it");
    }

    #[test]
    fn suicide_unit_dies_after_its_attack() {
        let bomber = bonus(66, Bonus::Suicide, warrior(66, 30, 0));
        let mut bt = with(vec![bomber], &[(66, f(2)), (10, f(3))], &[(18, f(2))]);
        turn_of(&mut bt, 0);
        let hit = bt.act(2).unwrap();
        assert_eq!((hit.amount, hit.actor_died, bt.fighters[0].hp), (30, true, 0));
    }

    #[test]
    fn splash_hits_the_target_for_80_and_its_row_neighbours_for_40() {
        let sweep = bonus(67, Bonus::Splash, warrior(67, 50, 0));
        let mut bt = with(vec![sweep], &[(67, f(2))], &[(18, f(1)), (18, f(2)), (18, f(3)), (18, f(5)), (18, b(2))]);
        assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 40);
        turn_of(&mut bt, 0);
        let hit = bt.act(2).unwrap();
        assert_eq!(hit.amount, 40);
        assert_eq!(hit.splash, vec![(1, 20), (3, 20)], "the same row only");
        assert_eq!([bt.fighters[4].hp, bt.fighters[5].hp], [200, 200]);
    }

    #[test]
    fn fortify_adds_a_quarter_of_defence_per_turn_up_to_125_percent() {
        let wall = bonus(68, Bonus::Fortify, UnitDef { hits: 3000, ..warrior(68, 1, 20) });
        let mut bt = with(vec![wall, hammer()], &[(69, f(2))], &[(68, f(2))]);
        let mut seen = Vec::new();
        for round in 1..=6 {
            to_round(&mut bt, round);
            seen.push(bt.physical_damage(0, 1, ActionKind::Melee));
        }
        assert_eq!(seen, vec![75, 70, 65, 60, 55, 55]);
    }

    #[test]
    fn dominate_hits_smaller_units_harder() {
        let lord = bonus(70, Bonus::Dominate, UnitDef { hits: 100, ..warrior(70, 30, 0) });
        let bt = with(vec![lord], &[(70, f(2))], &[(18, f(2)), (10, f(3))]);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 30, "the bag has more HP");
        assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 31, "(30 − 5) × 1.25");
    }

    #[test]
    fn strong_poison_takes_a_quarter_and_poison_works_for_mages() {
        let viper = bonus(71, Bonus::PoisonS, warrior(71, 30, 0));
        let witch = bonus(72, Bonus::Poison, mage(72, 30, MagicSchool::Life, MagicDirection::ToEnemy));
        let mut bt = with(vec![viper, witch], &[(71, f(2)), (72, b(0))], &[(18, f(2)), (18, f(3))]);
        turn_of(&mut bt, 0);
        bt.act(2).unwrap();
        turn_of(&mut bt, 1);
        bt.act(3).unwrap();
        assert_eq!((bt.fighters[2].poison, bt.fighters[3].poison), (25, 15));
        to_round(&mut bt, 2);
        assert_eq!((bt.fighters[2].hp, bt.fighters[3].hp), (200 - 30 - 50, 200 - 30 - 30));
    }

    #[test]
    fn concentration_grows_magic_power_instead_of_draining() {
        let focus = bonus(73, Bonus::Concentration, mage(73, 30, MagicSchool::Elemental, MagicDirection::ToEnemy));
        let mut bt = with(vec![focus], &[(73, b(0)), (12, b(1))], &[(18, f(0))]);
        to_round(&mut bt, 2);
        assert_eq!((bt.fighters[0].stats[Stat::MagicPower], bt.fighters[1].stats[Stat::MagicPower]), (33, 35));
        to_round(&mut bt, 15);
        assert_eq!(bt.fighters[0].stats[Stat::MagicPower], 60, "capped at twice the base");
    }

    #[test]
    fn potent_magic_ignores_protection() {
        let potent = bonus(74, Bonus::Potent, mage(74, 30, MagicSchool::Life, MagicDirection::ToEnemy));
        let warded = UnitDef { protect_life: 100, hits: 300, ..warrior(39, 1, 0) };
        let bt = with(vec![potent, warded], &[(74, b(0)), (15, b(1))], &[(39, f(0))]);
        assert_eq!((bt.magic_strike(0, 2), bt.magic_strike(1, 2)), (30, 0));
    }

    #[test]
    fn stun_cuts_initiative_by_a_quarter_once() {
        let mace = bonus(75, Bonus::Stun, warrior(75, 30, 0));
        let mut bt = with(vec![mace], &[(75, f(2))], &[(18, f(2))]);
        turn_of(&mut bt, 0);
        bt.act(1).unwrap();
        assert_eq!(bt.fighters[1].stats[Stat::Initiative], 7);
        turn_of(&mut bt, 0);
        bt.act(1).unwrap();
        assert_eq!(bt.fighters[1].stats[Stat::Initiative], 7);
    }

    #[test]
    fn first_shot_moves_first_on_turn_one_only() {
        let quick = bonus(76, Bonus::FirstShot, UnitDef { initiative: 1, ..warrior(76, 10, 0) });
        let gun = bonus(24, Bonus::Artillery, UnitDef { initiative: 1, ..shooter(24, 10) });
        let mut bt = with(vec![quick, gun], &[(10, f(0))], &[(76, f(0)), (24, b(0))]);
        assert_eq!(bt.queue().collect::<Vec<_>>(), vec![1, 2, 0]);
        to_round(&mut bt, 2);
        assert_eq!(bt.queue().collect::<Vec<_>>(), vec![2, 0, 1], "then Artillery, then initiative");
    }

    #[test]
    fn bastion_in_a_building() {
        let tower = bonus(77, Bonus::Bastion, UnitDef { hits: 300, ..warrior(77, 10, 5) });
        let bt = battle_inside(vec![tower.clone(), hammer()], &[(69, f(2))], &[(77, f(2)), (18, f(3))], Team::Enemy);
        let t = &bt.fighters[1].stats;
        assert_eq!((t[Stat::AttackBlow], t[Stat::DefenceBlow]), (30, 25), "×3, then +10 for the army");
        assert_eq!(bt.fighters[2].stats[Stat::DefenceBlow], 10, "the whole army +10");
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 37, "(100 − 25) / 2");
        // In the open: nothing.
        let bt = with(vec![tower, hammer()], &[(69, f(2))], &[(77, f(2))]);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 95);
    }

    #[test]
    fn assault_when_storming_a_building() {
        let sapper = bonus(78, Bonus::Assault, UnitDef { hits: 300, ..warrior(78, 20, 5) });
        let bt = battle_inside(vec![sapper.clone(), hammer()], &[(78, f(2))], &[(69, f(2))], Team::Enemy);
        let s = &bt.fighters[0].stats;
        assert_eq!((s[Stat::AttackBlow], s[Stat::DefenceBlow]), (40, 10));
        assert_eq!(bt.physical_damage(1, 0, ActionKind::Melee), 63, "(100 − 10) × 0.7");
        let bt = with(vec![sapper, hammer()], &[(78, f(2))], &[(69, f(2))]);
        assert_eq!(bt.physical_damage(1, 0, ActionKind::Melee), 95);
    }

    #[test]
    fn flying_attacks_any_enemy_from_any_row() {
        let bird = bonus(79, Bonus::Flying, warrior(79, 30, 0));
        let bt = with(vec![bird.clone()], &[(79, b(0)), (10, f(0))], &[(18, f(5)), (18, b(3))]);
        assert_eq!(bt.targets(0), vec![2, 3]);
        assert_eq!(bt.options(0, 3), vec![ActionKind::Melee]);
        let c = content_with(vec![bird], Formation::VANILLA);
        let bt = battle_in(&c, &[(79, r(0)), (10, f(0))], &[(18, f(3))]);
        assert!(bt.targets(0).is_empty(), "not from the reserve");
    }

    #[test]
    fn flock_by_head_count() {
        let wolf = bonus(80, Bonus::Flock, warrior(80, 40, 0));
        let bt = with(vec![wolf.clone()], &[(80, f(2)), (18, f(3))], &[(18, f(2))]);
        assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 50);
        let bt = with(vec![wolf.clone()], &[(80, f(2))], &[(18, f(2)), (18, f(3))]);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 30);
        let bt = with(vec![wolf], &[(80, f(2))], &[(18, f(2))]);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 40);
    }

    #[test]
    fn bleed_takes_half_the_wound_again_next_turn() {
        let knife = bonus(81, Bonus::Bleed, warrior(81, 30, 0));
        let mut bt = with(vec![knife], &[(81, f(2))], &[(18, f(2))]);
        turn_of(&mut bt, 0);
        bt.act(1).unwrap();
        assert_eq!(bt.fighters[1].bleeding, 15);
        to_round(&mut bt, 2);
        assert_eq!((bt.fighters[1].hp, bt.fighters[1].bleeding), (155, 0));
    }

    #[test]
    fn hold_line_does_nothing() {
        let line = bonus(82, Bonus::HoldLine, warrior(82, 30, 5));
        let bt = with(vec![line], &[(82, f(2)), (82, f(3))], &[(10, f(2))]);
        assert_eq!(bt.physical_damage(0, 2, ActionKind::Melee), 25);
        assert_eq!(bt.physical_damage(2, 0, ActionKind::Melee), 25);
    }

    #[test]
    fn armor_breaker_cuts_defence_by_30_percent_per_hit() {
        let breaker = bonus(83, Bonus::ArmorBreaker, warrior(83, 30, 0));
        let mut bt = with(vec![breaker, armour()], &[(83, f(2))], &[(84, f(2))]);
        turn_of(&mut bt, 0);
        assert_eq!(bt.act(1).unwrap().amount, 10);
        assert_eq!(bt.fighters[1].stats[Stat::DefenceBlow], 14);
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 16);
    }

    #[test]
    fn poison_armor_ignore_pierces_and_poisons_ten_percent() {
        let sting = bonus(85, Bonus::PoisonArmorIgnore, warrior(85, 30, 0));
        let c = content_with(vec![sting, armour()], Formation::WIDE);
        let squad = [Unit::new(&c, UnitId(85), f(2))];
        let p: Vec<_> = squad.iter().enumerate().collect();
        let mut bt = Battle::new(c.clone(), &p, &[Unit::new(&c, UnitId(84), f(2))], Team::Player);
        bt.set_building_defence(Team::Enemy, 6);
        bt.begin();
        assert_eq!(bt.physical_damage(0, 1, ActionKind::Melee), 24, "the building still counts");
        bt.act(1).unwrap();
        assert_eq!(bt.fighters[1].poison, 10);
    }

    #[test]
    fn faster_attack_gives_an_extra_action_on_the_first_two_turns() {
        let fast = bonus(86, Bonus::FasterAttack, UnitDef { initiative: 30, ..warrior(86, 10, 0) });
        let mut bt = with(vec![fast], &[(86, f(0))], &[(18, f(5))]);
        let mut seen = Vec::new();
        for round in 1..=3 {
            to_round(&mut bt, round);
            turn_of(&mut bt, 0);
            seen.push(bt.actions_left());
            bt.skip();
        }
        assert_eq!(seen, vec![2, 2, 1]);
    }

    #[test]
    fn no_heal_stops_heals_and_regeneration() {
        let cripple = bonus(87, Bonus::NoHeal, warrior(87, 30, 0));
        let troll = UnitDef { regen: 10, hits: 200, ..warrior(88, 1, 0) };
        let mut bt = with(vec![cripple, troll], &[(87, f(2))], &[(88, f(2)), (13, b(0))]);
        turn_of(&mut bt, 0);
        bt.act(1).unwrap();
        assert!(bt.fighters[1].crippled);
        assert!(!bt.options(2, 1).contains(&ActionKind::Heal));
        to_round(&mut bt, 2);
        assert_eq!(bt.fighters[1].hp, 170, "no regeneration");
    }

    #[test]
    fn preventive_strike_hits_the_attacker_first() {
        let guard = bonus(89, Bonus::PreventiveStrike, UnitDef { hits: 100, ..warrior(89, 60, 0) });
        let archer = bonus(47, Bonus::PreventiveStrike, UnitDef { hits: 100, ..shooter(47, 20) });
        let mut bt = with(vec![guard, archer], &[(10, f(2)), (1, f(3))], &[(89, f(2)), (47, f(3))]);
        turn_of(&mut bt, 0);
        let hit = bt.act(2).unwrap();
        assert_eq!((hit.counter, hit.actor_died, hit.amount, bt.fighters[2].hp), (Some(50), true, 0, 100), "55 kills the 50-HP striker first");
        turn_of(&mut bt, 1);
        let hit = bt.act(3).unwrap();
        assert_eq!((hit.counter, hit.amount), (Some(15), 30), "the archer shoots first: 20 − 5; then 30 − 0");
    }

    #[test]
    fn neutralize_strips_the_targets_bonuses() {
        let null = bonus(90, Bonus::Neutralize, UnitDef { hits: 300, ..warrior(90, 30, 0) });
        let chief = bonus(91, Bonus::Counterblow, UnitDef { hits: 300, ..warrior(91, 25, 0) });
        let mut bt = with(vec![null, chief], &[(90, f(2)), (10, f(3))], &[(91, f(2))]);
        turn_of(&mut bt, 1);
        assert_eq!(bt.act(2).unwrap().counter, Some(20), "a plain strike is answered: 25 − 5");
        turn_of(&mut bt, 0);
        assert_eq!(bt.act(2).unwrap().counter, None, "stripped before it can answer");
        assert!(bt.fighters[2].stats.bonuses.is_empty());
    }

    #[test]
    fn killing_strike_finishes_a_target_below_a_quarter() {
        let axe = bonus(92, Bonus::KillingStrike, warrior(92, 30, 0));
        let mut bt = with(vec![axe], &[(92, f(2))], &[(18, f(2)), (18, f(3))]);
        bt.fighters[1].hp = 70;
        bt.fighters[2].hp = 100;
        turn_of(&mut bt, 0);
        let hit = bt.act(1).unwrap();
        assert_eq!((hit.killed, hit.amount), (true, 70), "40 left of 200 is below 25%");
        turn_of(&mut bt, 0);
        let hit = bt.act(2).unwrap();
        assert_eq!((hit.killed, bt.fighters[2].hp), (false, 70));
    }

    #[test]
    fn blood_thirst_gets_its_action_back_after_a_kill() {
        let fang = bonus(93, Bonus::BloodThrist, warrior(93, 30, 0));
        let mut bt = with(vec![fang], &[(93, f(2))], &[(10, f(2)), (10, f(3))]);
        bt.fighters[1].hp = 1;
        turn_of(&mut bt, 0);
        assert!(bt.act(1).unwrap().killed);
        assert_eq!((bt.active(), bt.actions_left()), (Some(0), 1));
        assert!(!bt.act(2).unwrap().killed);
        assert_ne!(bt.active(), Some(0));
    }

    #[test]
    fn eternal_gift_blessings_last_the_whole_battle() {
        let saint = bonus(94, Bonus::EternalGift, mage(94, 36, MagicSchool::Life, MagicDirection::ToAll));
        let mut bt = with(vec![saint], &[(94, b(0)), (10, f(0))], &[(18, f(5))]);
        turn_of(&mut bt, 0);
        assert_eq!(bt.act_with(1, ActionKind::Bless).unwrap().kind, ActionKind::Bless);
        to_round(&mut bt, 20);
        assert!(bt.fighters[1].blessing().is_some());
    }

    #[test]
    fn fate_gift_saves_once_and_makes_stronger() {
        let lucky = bonus(95, Bonus::FateGift, UnitDef { hits: 40, ..warrior(95, 20, 4) });
        let mut bt = with(vec![lucky, hammer()], &[(69, f(2))], &[(95, f(2))]);
        turn_of(&mut bt, 0);
        let hit = bt.act(1).unwrap();
        assert!(!hit.killed);
        let t = &bt.fighters[1];
        assert_eq!((t.hp, t.stats[Stat::AttackBlow], t.stats[Stat::DefenceBlow]), (40, 25, 5));
        turn_of(&mut bt, 0);
        assert!(bt.act(1).unwrap().killed, "only once");
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

    /// The player: warrior 10 in front, shooter 11 behind; the enemy: four punching bags in
    /// front. A bag: D = round((e⁰/1.17 + e⁰/1.07)·200) = 358, H = 558, A = 11,
    /// T = round(3.2·558·12/200) = 107; the enemy side 428.
    fn xp_battle() -> Battle {
        battle(&[(10, f(2)), (11, b(2))], &[(18, f(1)), (18, f(2)), (18, f(3)), (18, f(4))])
    }

    fn win(bt: &mut Battle) {
        for i in 2..bt.fighters.len() {
            bt.fighters[i].hp = 0;
        }
    }

    #[test]
    fn xp_pool_and_shares_follow_the_original() {
        let mut bt = xp_battle();
        assert!(bt.xp_awards(Team::Player).is_empty(), "not over yet");
        assert_eq!(bt.start_of(Team::Enemy).strength, 428);
        assert_eq!(bt.start_of(Team::Player).count, 2);
        win(&mut bt);
        // Pool 428 div 20 = 21; t = 21/4/2 = 2.625; idle front 3t = 7.9, back 2t = 5.25.
        let xp = bt.xp_awards(Team::Player);
        assert_eq!(xp.iter().map(|a| (a.fighter, a.xp)).collect::<Vec<_>>(), vec![(0, 8), (1, 5)]);
        // The player gets ×HeroExpirienceModificator (50) × F (120) × the correction.
        assert_eq!(bt.player_xp().iter().map(|a| a.xp).collect::<Vec<_>>(), vec![5, 3]);
        bt.set_xp_correction(250);
        assert_eq!(bt.player_xp().iter().map(|a| a.xp).collect::<Vec<_>>(), vec![12, 8]);
    }

    #[test]
    fn xp_counts_activity_and_hit_points_lost() {
        let mut bt = xp_battle();
        turn_of(&mut bt, 0);
        let hp = bt.fighters[3].hp;
        bt.act(3).unwrap();
        let (a, t) = (&bt.fighters[0], &bt.fighters[3]);
        assert_eq!((a.useful, a.taken, a.left), (1, 1, 0));
        assert_eq!(t.lost, hp - t.hp, "the target's loss is counted");
        turn_of(&mut bt, 1);
        bt.skip();
        assert_eq!((bt.fighters[1].useful, bt.fighters[1].taken, bt.fighters[1].left), (0, 1, 0), "a wait spends the actions");
        win(&mut bt);
        // The busy warrior gets 4t = 10.5 → 10; the shooter that waited 2t = 5.25 → 5.
        assert_eq!(bt.xp_awards(Team::Player).iter().map(|a| a.xp).collect::<Vec<_>>(), vec![10, 5]);
        // Hit points lost shrink the pool: half the starting HP lost halves it (21 → 10).
        bt.fighters[0].lost = (bt.start_of(Team::Player).hp / 2) as i32;
        assert_eq!(bt.xp_awards(Team::Player).iter().map(|a| a.xp).collect::<Vec<_>>(), vec![5, 2]);
    }

    #[test]
    fn the_dead_get_nothing_but_count_and_only_victory_pays_the_player() {
        let mut bt = battle(&[(10, f(2)), (11, b(2)), (11, b(3))], &[(18, f(1)), (18, f(2)), (18, f(3)), (18, f(4))]);
        bt.fighters[2].hp = 0;
        win(&mut bt);
        // Now N₀ = 3: t = 1.75, front 5.25 → 5, back 3.5 → 4.
        let xp = bt.xp_awards(Team::Player);
        assert_eq!(xp.iter().map(|a| (a.fighter, a.xp)).collect::<Vec<_>>(), vec![(0, 5), (1, 4)]);
        let mut lost = xp_battle();
        lost.fighters[0].hp = 0;
        lost.fighters[1].hp = 0;
        assert_eq!(lost.outcome(), Outcome::Defeat);
        assert!(lost.player_xp().is_empty(), "no XP without a victory");
        assert!(lost.ai_xp(Team::Player).is_empty(), "a wiped-out side has no strength left");
        assert_eq!(lost.ai_xp(Team::Enemy).len(), 4, "the AI survivors gain");
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
            // XP within bounds: every survivor's share between 1 and the pool, the player's
            // gain only after a victory and at most the Community cap.
            let pool = experience::battle_pool(bt.start_of(Team::Enemy).strength, bt.start_of(Team::Player).hp, 0).max(1);
            for a in bt.xp_awards(Team::Player) {
                assert!(a.xp >= 1 && a.xp as i64 <= pool, "battle {n}: share {} of pool {pool}", a.xp);
            }
            let gains = bt.player_xp();
            assert_eq!(gains.is_empty(), bt.outcome() != Outcome::Victory);
            assert!(gains.iter().all(|a| (0..=experience::MAX_BATTLE_XP).contains(&a.xp)));
        }
        assert_eq!(outcomes[0], 0);
        assert!(outcomes.iter().sum::<i32>() >= 12);
    }
}
