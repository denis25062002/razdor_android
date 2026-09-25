//! Card battles, following the original's rules (original-mechanics/battle.md).
//!
//! Each side stands in a [`Formation`] (front, back and reserve rows). The turn order is the
//! original's descending initiative threshold: at each threshold the player's units are
//! scanned in army order, then the enemy's, and a unit whose initiative reaches the threshold
//! spends all its actions (`Manevres`) in a row. Every action costs one: an attack, a spell, a
//! step or a pass. Damage is deterministic, and so is the AI. Blessings, curses and other
//! modifiers last until the next turn starts. The battle ends when a side is gone, when a side
//! has only units that surrender, or after the first action of turn `BattleEndTurn`, which the
//! player wins if any of his units stand. There is no retreat.

use std::sync::Arc;

use super::content::{Bonus, Content, HeroClass, MagicSchool, Nature, SpellDef, Stat, UnitId};
use super::experience::{self, Role, SideUnit};
use super::formation::{Formation, Row, Slot};
use super::units::{Stats, Unit};

/// Turn 1 starts its initiative scan here (4840ec); later turns start where the first unit of
/// the turn before acted.
const TURN_ONE_THRESHOLD: i32 = 75;
/// The knight's army takes this % of physical damage: the exe sets 80 when `[GlobalOptions]`
/// loads (4e4501), whatever the ini says.
const KNIGHT_PERCENT: i32 = 80;
/// Regeneration a poison sets: `Poison`, `PoisonS`, `PoisonArmorIgnore` (at most).
const POISON_REGEN: i32 = -20;
const STRONG_POISON_REGEN: i32 = -25;
const PIERCING_POISON_REGEN: i32 = -10;
/// `CtrPoison`: regeneration its melee attacker loses per hit (stacks).
const CTR_POISON_STEP: i32 = 20;
/// A `Poison` mage poisons when its power after protection is above this.
const MAGE_POISON_POWER: i32 = 15;
/// `Exhaustion`: points of every magic protection lost per hostile spell.
const EXHAUSTION_POINTS: i32 = 10;
/// `Drying`: extra damage of a hostile spell, % of the target's max HP.
const DRYING_PERCENT: i32 = 8;
/// `Fortify`: defence bonus per turn after the first, % of DefenceBlow, for up to 5 turns.
const FORTIFY_PERCENT: i32 = 25;
const FORTIFY_TURNS: i32 = 5;
/// `Splash`: the first hit and the neighbours' hits, % of attack or power.
const SPLASH_MAIN: i32 = 80;
const SPLASH_SIDE: i32 = 40;
/// `KillingStrike`: a target left at or below this % of max HP dies.
const KILLING_STRIKE_PERCENT: i32 = 25;
/// `Bleed`: the bleeding value a hit sets; each action start costs this % of AB + AS + MP.
const BLEED_PERCENT: i32 = 75;
/// `Stun`: initiative modifier lost per hit, % of the current initiative.
const STUN_PERCENT: i32 = 30;
/// `Berserk`: attack modifier = this % of AB × the share of HP lost.
const BERSERK_PERCENT: i32 = 75;
/// `Flock`: attack modifier ± this % of AB (or AS).
const FLOCK_PERCENT: i32 = 25;
/// `Artillery` and `FirstShot`: initiative on turn 1, twice with building defence ≥ 10.
const FIRST_TURN_INITIATIVE: i32 = 30;
/// `FateGift`: protections +20, regeneration +20, max HP +20%, initiative modifier +5.
const FATE_PROTECTION: i32 = 20;
const FATE_REGEN: i32 = 20;
const FATE_HP_PERCENT: i32 = 20;
const FATE_INITIATIVE: i32 = 5;
/// Undead Death casters' magic power floor is raised by this.
const UNDEAD_DEATH_FLOOR: i32 = 25;

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

    const BOTH: [Team; 2] = [Team::Player, Team::Enemy];
}

/// How the battle went. There is no draw: at the turn limit the player wins if any of his
/// units stand (4c50ec has no other branch).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ongoing,
    Victory,
    Defeat,
}

/// Why a finished battle ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// A side has no units left.
    Wiped,
    /// The first action of turn `BattleEndTurn` was made.
    TurnLimit,
    /// Every remaining unit of this side has `Surrender > 0`: it gave up.
    Surrender(Team),
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
    /// Hostile magic damage: on a target that already has a negative modifier this turn.
    Strike,
    /// Hostile magic debuff: on a target without one.
    Curse,
    /// Friendly magic on a wounded ally.
    Heal,
    /// Friendly magic on anyone else.
    Bless,
}

impl ActionKind {
    pub fn is_physical(self) -> bool {
        matches!(self, ActionKind::Melee | ActionKind::LongStrike | ActionKind::Shot)
    }

    pub fn is_melee(self) -> bool {
        matches!(self, ActionKind::Melee | ActionKind::LongStrike)
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

/// Stat changes: of a blessing (positive) or curse (negative), or a unit's per-turn
/// modifiers (`actions` unused there).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buff {
    /// Added to melee and ranged attack (only those the unit has).
    pub attack: i32,
    /// Added to melee and ranged defence.
    pub defence: i32,
    pub initiative: i32,
    /// Actions left this turn (Elemental magic).
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

    fn negative(&self) -> bool {
        self.attack < 0 || self.defence < 0 || self.initiative < 0
    }
}

/// The expected effect of an action, for hover previews.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preview {
    Damage(i32),
    Heal(i32),
    Buff(Buff),
}

/// The battle AI's view of a unit (4836cc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AiRole {
    Warrior,
    Shooter,
    Mage,
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
    /// Battle stats: the start of the battle (level, items, potions, spells) plus what the
    /// battle changed for good (EternalGift, ArmorBreaker, Bastion, Exhaustion …).
    pub base: Stats,
    /// Current stats: base with this turn's modifiers and the drained magic power.
    pub stats: Stats,
    /// Magic power left after the per-turn drain.
    pub power: i32,
    /// This turn's attack, defence and initiative modifiers (blessings, curses, Stun,
    /// Berserk, Fortify, Flock …). Every turn start sets them to 0.
    pub mods: Buff,
    /// Blessed or cursed this turn.
    pub blessed: bool,
    pub cursed: bool,
    /// Actions left this turn.
    pub actions: i32,
    /// Regeneration % per turn; a poison replaces it with a negative value.
    pub regen: i32,
    /// Community `Bleed`: % of AB + AS + MP lost at each action start (0 = not bleeding).
    pub bleed: i32,
    /// May still move into or out of the reserve this turn.
    reserve_move: bool,
    /// Community `NoHeal`: hit by a crippling weapon; no heal or blessing this battle.
    pub crippled: bool,
    /// `Surrender` of its type; a side left with only such units gives up.
    pub surrender: i32,
    /// Left the field by surrendering.
    pub surrendered: bool,
    surrender_hp: i32,
    /// Tactical cost at the start (experience.md §1), for the sides' strength.
    pub tactical: i32,
    /// Role in the side's strength sum, set at the start.
    pub role: Role,
    /// For the XP share: attacks and spells made, all actions taken (moves and passes too)
    /// and hit points lost.
    pub useful: i32,
    pub taken: i32,
    pub lost: i32,
    /// Cell after deployment; written back to the squad.
    deployed: Slot,
    ai_power: i32,
    ai_role: AiRole,
}

impl Fighter {
    fn new(content: &Content, unit: &Unit, team: Team, squad_index: Option<usize>) -> Fighter {
        let mut base = unit.stats(content);
        // One bonus per unit: each worn item with a bonus overwrites the unit's, the last one
        // wins (4919f0).
        if let Some(b) = base.bonuses.last().cloned() {
            base.bonuses = vec![b];
        }
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
            regen: base[Stat::Regen],
            stats: base.clone(),
            base,
            mods: Buff::default(),
            blessed: false,
            cursed: false,
            actions: 0,
            bleed: 0,
            reserve_move: true,
            crippled: false,
            surrender: content.unit(unit.def).surrender.max(0),
            surrendered: false,
            surrender_hp: 0,
            tactical: 1,
            role: Role::Melee,
            useful: 0,
            taken: 0,
            lost: 0,
            deployed: unit.slot,
            ai_power: 0,
            ai_role: AiRole::Warrior,
        }
    }

    pub fn alive(&self) -> bool {
        self.hp > 0
    }

    pub fn max_hp(&self) -> i32 {
        self.stats.max_hp()
    }

    /// Poisoned (or otherwise losing HP each turn).
    pub fn poisoned(&self) -> bool {
        self.regen < 0
    }

    /// Any negative attack, defence or initiative modifier this turn: a hostile mage strikes
    /// it instead of cursing.
    pub fn weakened(&self) -> bool {
        self.mods.negative()
    }

    fn is_warrior(&self) -> bool {
        self.base[Stat::AttackBlow] > 0
    }

    fn is_shooter(&self) -> bool {
        self.base[Stat::AttackShot] > 0
    }

    fn has_attack(&self) -> bool {
        self.is_warrior() || self.is_shooter()
    }

    fn has(&self, b: Bonus) -> bool {
        self.base.has(&b)
    }

    fn wounded(&self) -> bool {
        self.hp < self.max_hp()
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
    /// The actor died: killed a `DeathCurse`/`Ghost` unit, fell to the counterblow, the
    /// preventive strike or its bleeding, or it is a `Suicide` unit.
    pub actor_died: bool,
    /// Community `Splash`: the neighbours' damage or healing (fighter, amount).
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
    /// One action passed.
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

/// What the AI does with the active unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plan {
    Act(usize, ActionKind),
    Move(Slot),
    Pass,
}

pub struct Battle {
    content: Arc<Content>,
    pub formation: Formation,
    pub fighters: Vec<Fighter>,
    /// Battle turn, 1-based once the fight starts.
    pub round: u32,
    pub log: Vec<String>,
    /// The side that started the fight. The original gives its +1 initiative to the player
    /// whoever attacks, so this is for information only.
    pub attacker: Team,
    building_defence: [i32; 2],
    deploying: bool,
    /// Ended by the turn limit or a surrender (a wiped-out side needs no flag).
    ended: Option<EndReason>,
    /// The initiative scan: threshold, the threshold of the turn's first actor, and the
    /// cursor (side, position among the side's living units).
    threshold: i32,
    first_threshold: i32,
    cursor: (usize, usize),
    current: Option<usize>,
    /// Both sides at the start ([`Battle::begin`]).
    start: [SideStart; 2],
    /// The beaten army's experience correction for the player's XP (100 for a garrison).
    xp_correction: i32,
    /// A battle on screen (Splash works only there, 4ed424); false for AI-vs-AI battles.
    interactive: bool,
    /// The AI's level (battle B+5): 1 normally, 2 with "improved enemy AI" (`OptValue9`), 0
    /// between AI armies. It decides when a target counts as killable.
    ai_level: u8,
    /// Community `Hunger`: the living-unit count it last saw (shared by all Hunger units).
    hunger_seen: usize,
    /// Mana a side's surrender gives the winner.
    surrender_mana: [i32; 2],
}

/// A fighter in its side's strength sum.
fn side_unit(f: &Fighter) -> SideUnit {
    let hp = if f.surrendered { f.surrender_hp } else { f.hp };
    SideUnit { tactical: f.tactical, hp, max_hp: f.max_hp(), row: f.slot.row, role: f.role }
}

/// Bonuses that give +1 action on the first turn (48431a).
const FAST_START: [Bonus; 3] = [Bonus::HorseAtack, Bonus::OldVampirsGist, Bonus::FastDead];
/// Piercing: the unit's own defence counts 0 (485908, hooks c2a27c and c2a3bf). Building
/// defence (and Row2Def against shots) still count.
const PIERCE_MELEE: [Bonus; 4] = [Bonus::ArmorIgnore, Bonus::VampirsGist, Bonus::OldVampirsGist, Bonus::PoisonArmorIgnore];
const PIERCE_SHOT: [Bonus; 3] = [Bonus::ArmorIgnore, Bonus::Artillery, Bonus::PoisonArmorIgnore];

/// A blessing of power `p` in `school` on a unit that has an attack (before the target's
/// nature and attack are taken into account).
pub fn bless_effect(o: &super::content::GlobalOptions, school: MagicSchool, p: i32) -> Buff {
    let (bm, bn, w) = (o.bless_main_spell.max(1), o.bless_next_spell.max(1), o.wizard_main_spell.max(1));
    match school {
        MagicSchool::Life => Buff { defence: 3 * p / (2 * bm) + 1, attack: 3 * p / (2 * bn), ..Buff::default() },
        MagicSchool::Elemental => Buff { actions: actions_of_power(p), initiative: p / w + 1, ..Buff::default() },
        MagicSchool::Death => Buff { attack: p / bm + 1, defence: p / bn, ..Buff::default() },
    }
}

/// A curse of hostile power `p` (already reduced by the target's protection) in `school`,
/// on a unit that has an attack.
pub fn curse_effect(o: &super::content::GlobalOptions, school: MagicSchool, p: i32) -> Buff {
    let (cm, cn, w) = (o.curse_main_spell.max(1), o.curse_next_spell.max(1), o.wizard_main_spell.max(1));
    // Life divides by the integer ⅔ of CurseMainSpell (4ed3a8) and a fixed 10 (4ed3b0).
    let life = (2 * cm / 3).max(1);
    match school {
        MagicSchool::Life => Buff { defence: -(p / life + 1), attack: -(p / 10), ..Buff::default() },
        MagicSchool::Elemental => Buff { actions: -actions_of_power(p), initiative: -(1 + p / w), ..Buff::default() },
        MagicSchool::Death => Buff { attack: -(1 + p / cm), defence: -(p / cn), ..Buff::default() },
    }
}

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

/// `n / d` rounded half to even, as Delphi's `Round`.
fn round_even(n: i64, d: i64) -> i64 {
    let (q, r) = (n.div_euclid(d), n.rem_euclid(d));
    match (2 * r).cmp(&d) {
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal => q + (q & 1),
    }
}

impl Battle {
    /// `player` entries are (squad index, unit); squad index 0 is the hero. Starts in the
    /// deploy phase; call [`Battle::begin`] to fight.
    pub fn new(content: Arc<Content>, player: &[(usize, &Unit)], enemies: &[Unit], attacker: Team) -> Battle {
        let mut fighters: Vec<Fighter> =
            player.iter().map(|&(idx, u)| Fighter::new(&content, u, Team::Player, Some(idx))).collect();
        fighters.extend(enemies.iter().map(|u| Fighter::new(&content, u, Team::Enemy, None)));
        let mut b = Battle {
            formation: content.formation,
            content,
            fighters,
            round: 0,
            log: Vec::new(),
            attacker,
            building_defence: [0; 2],
            deploying: true,
            ended: None,
            threshold: 0,
            first_threshold: 0,
            cursor: (0, 0),
            current: None,
            start: [SideStart::default(); 2],
            xp_correction: 100,
            interactive: true,
            ai_level: 1,
            hunger_seen: 0,
            surrender_mana: [0; 2],
        };
        b.fit_to_formation();
        b
    }

    /// Units standing outside the formation (a blocked cell of the wide row, an old save) or
    /// on a taken cell move to the first free one.
    fn fit_to_formation(&mut self) {
        for team in Team::BOTH {
            let mut taken: Vec<Slot> = Vec::new();
            for i in 0..self.fighters.len() {
                let f = &self.fighters[i];
                if f.team != team {
                    continue;
                }
                let slot = if self.formation.contains(f.slot) && !taken.contains(&f.slot) {
                    Some(f.slot)
                } else {
                    self.formation.free_slot(&taken, f.base.preferred_row())
                };
                if let Some(s) = slot {
                    self.fighters[i].slot = s;
                    self.fighters[i].deployed = s;
                    taken.push(s);
                }
            }
        }
    }

    /// The experience correction (percent) of the army the player fights: it scales the
    /// player's XP (experience.md §3). A garrison's is 100.
    pub fn set_xp_correction(&mut self, percent: i32) {
        self.xp_correction = percent;
    }

    /// A battle between AI armies, played off screen: no `Splash`, and the AI counts a
    /// target as killable only by one hit (B+5 = 0).
    pub fn set_simulation(&mut self) {
        self.interactive = false;
        self.ai_level = 0;
    }

    /// "Improved enemy AI in battle" (`OptValue9`): the enemy also counts a target as
    /// killable when its actions left can do it.
    pub fn set_improved_ai(&mut self, on: bool) {
        self.ai_level = if on { 2 } else { 1 };
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
            f.regen = f.base[Stat::Regen];
            if f.alive() {
                f.hp = (f.hp + (after - before).max(0)).min(after);
            }
        }
    }

    /// The defence bonus `team` has from standing in its own building (0 in the open).
    pub fn building_defence(&self, team: Team) -> i32 {
        self.building_defence[team.index()]
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
        // Strength at the start, from the stats the units bring (items, spells) and the
        // building they stand in.
        for f in &mut self.fighters {
            f.tactical = experience::tactical(&self.content, f.unit, &f.base, self.building_defence[f.team.index()]);
            f.role = experience::role(&f.base);
            let s = &f.base;
            let (ab, sh, mp) = (s[Stat::AttackBlow], s[Stat::AttackShot], s[Stat::MagicPower]);
            let top = ab.max(sh).max(mp);
            let mut power = top + (ab + sh + mp - top) / 3;
            power += [(Bonus::GodAnger, 10), (Bonus::ArmorIgnore, 15), (Bonus::GodStrike, 20), (Bonus::Counterblow, ab), (Bonus::FlankStrike, 10)]
                .iter()
                .filter(|(b, _)| s.has(b))
                .map(|(_, v)| v)
                .sum::<i32>();
            f.ai_power = power;
            f.ai_role = if 3 * mp >= 2 * power && s.is_mage() {
                AiRole::Mage
            } else if 3 * sh >= 2 * power && sh > 0 {
                AiRole::Shooter
            } else {
                AiRole::Warrior
            };
        }
        for team in Team::BOTH {
            let side: Vec<&Fighter> = self.fighters.iter().filter(|f| f.team == team && f.alive()).collect();
            self.start[team.index()] = SideStart {
                strength: experience::side_strength(&side.iter().map(|f| side_unit(f)).collect::<Vec<_>>()),
                hp: side.iter().map(|f| f.hp as i64).sum(),
                count: side.len(),
            };
        }
        for i in 0..self.fighters.len() {
            let building = self.building_defence[self.fighters[i].team.index()];
            let f = &mut self.fighters[i];
            f.deployed = f.slot;
            // Garrison in a strong building: AB, DB and DS ×2; not AS (49861d).
            if f.has(Bonus::Garrison) && building >= 10 {
                for st in [Stat::AttackBlow, Stat::DefenceBlow, Stat::DefenceShot] {
                    f.base[st] *= 2;
                }
            }
            // Side 1, the player, gets +1 initiative whoever attacks (48b917).
            if f.team == Team::Player {
                f.base[Stat::Initiative] += 1;
            }
        }
        self.hunger_seen = self.fighters.iter().filter(|f| f.alive()).count();
        self.start_turn();
        self.advance();
    }

    fn opt(&self) -> &super::content::GlobalOptions {
        &self.content.options
    }

    fn living(&self, team: Team) -> impl Iterator<Item = &Fighter> {
        self.fighters.iter().filter(move |f| f.alive() && f.team == team)
    }

    fn living_ids(&self, team: Team) -> Vec<usize> {
        (0..self.fighters.len()).filter(|&i| self.fighters[i].alive() && self.fighters[i].team == team).collect()
    }

    fn turn_limit(&self) -> u32 {
        self.opt().battle_end_turn.max(1) as u32
    }

    pub fn outcome(&self) -> Outcome {
        let player = self.living(Team::Player).next().is_some();
        let enemy = self.living(Team::Enemy).next().is_some();
        match (player, enemy) {
            (false, _) => Outcome::Defeat,
            (true, false) => Outcome::Victory,
            (true, true) if self.ended.is_some() => Outcome::Victory,
            _ => Outcome::Ongoing,
        }
    }

    /// Why the battle ended, once it has.
    pub fn end_reason(&self) -> Option<EndReason> {
        match self.outcome() {
            Outcome::Ongoing => None,
            _ => Some(self.ended.unwrap_or(EndReason::Wiped)),
        }
    }

    /// Mana `team` gets from the other side's surrender: the sum of the surrendered units'
    /// `Surrender` (units killed before give none).
    pub fn surrender_mana(&self, team: Team) -> i32 {
        self.surrender_mana[team.index()]
    }

    /// Fighter whose turn it is; `None` while deploying or once the battle is over.
    pub fn active(&self) -> Option<usize> {
        if self.deploying || self.outcome() != Outcome::Ongoing {
            return None;
        }
        self.current.filter(|&i| self.fighters[i].alive())
    }

    /// Actions the active fighter has left.
    pub fn actions_left(&self) -> i32 {
        self.active().map_or(0, |i| self.fighters[i].actions)
    }

    /// Current initiative: base with this turn's modifier.
    fn initiative(&self, i: usize) -> i32 {
        self.fighters[i].stats[Stat::Initiative]
    }

    /// The expected order of the fighters still to act this turn, the active one first.
    pub fn queue(&self) -> impl Iterator<Item = usize> + '_ {
        let (t, (side, pos)) = (self.threshold, self.cursor);
        let mut rest: Vec<(i32, usize, usize, usize)> = Vec::new();
        for (s, team) in Team::BOTH.into_iter().enumerate() {
            for (p, i) in self.living_ids(team).into_iter().enumerate() {
                let (ini, f) = (self.initiative(i), &self.fighters[i]);
                if Some(i) == self.active() || f.actions <= 0 || ini <= 0 {
                    continue;
                }
                let this_pass = (s, p) > (side, pos) && ini >= t;
                rest.push((if this_pass { t } else { ini.min(t - 1) }, s, p, i));
            }
        }
        rest.sort_by_key(|&(at, s, p, _)| (-at, s, p));
        self.active().into_iter().chain(rest.into_iter().map(|r| r.3))
    }

    // ------------------------------------------------------------------------------------
    // Turns (4840ec, 489ca0)
    // ------------------------------------------------------------------------------------

    /// Starts the next battle turn: modifiers and flags reset, actions refilled, then from
    /// turn 2 magic drain, regeneration and poison, then the Community turn-start bonuses.
    fn start_turn(&mut self) {
        self.round += 1;
        let round = self.round;
        self.threshold = if round == 1 { TURN_ONE_THRESHOLD } else { self.first_threshold };
        self.first_threshold = 0;
        self.cursor = (0, 0);
        self.log.push(format!("-- Turn {round} --"));
        for f in self.fighters.iter_mut().filter(|f| f.alive()) {
            f.mods = Buff::default();
            f.blessed = false;
            f.cursed = false;
            f.reserve_move = true;
            f.actions = f.base[Stat::Manevres]
                + i32::from(round == 1 && f.base.has_any(&FAST_START))
                + i32::from(round <= 2 && f.has(Bonus::FasterAttack));
        }
        if round >= 2 {
            for i in 0..self.fighters.len() {
                if self.fighters[i].alive() {
                    self.drain(i);
                    self.regenerate(i);
                }
            }
        }
        self.turn_bonuses();
        for i in 0..self.fighters.len() {
            self.refresh(i);
        }
    }

    /// Magic power drain from turn 2 (Community c2851a): `max(MP − drain, floor)`, at least
    /// 0, for units with power; the floor also raises weak casters. Concentration adds the
    /// drain instead.
    fn drain(&mut self, i: usize) {
        let f = &self.fighters[i];
        let Some(school) = f.base.magic else { return };
        if f.power <= 0 {
            return;
        }
        let o = self.opt();
        let dec = f.base.mana_drain.filter(|&v| v != 0).unwrap_or(o.dec_spell(school));
        let mut floor = f.base.min_magic_power.filter(|&v| v != 0).unwrap_or(o.min_spell(school));
        if school == MagicSchool::Death && f.base.nature == Nature::Undead {
            floor += UNDEAD_DEATH_FLOOR;
        }
        let f = &mut self.fighters[i];
        f.power = if f.has(Bonus::Concentration) { f.power + dec } else { f.power - dec };
        f.power = f.power.max(floor).max(0);
    }

    /// Regeneration and poison from turn 2: `HP += round(maxHP × regen / 100)`, capped at
    /// max HP; a unit at 0 or less dies (4846c1).
    fn regenerate(&mut self, i: usize) {
        let f = &mut self.fighters[i];
        let max = f.base.max_hp();
        let delta = round_even(max as i64 * f.regen as i64, 100) as i32;
        if delta == 0 || (delta > 0 && f.hp >= max) {
            return;
        }
        let new = (f.hp + delta).min(max);
        let change = new - f.hp;
        f.hp = new;
        if change < 0 {
            f.lost -= change;
            let msg = format!("{} loses {} to poison", f.name, -change);
            self.log.push(msg);
            if !self.fighters[i].alive() {
                self.died(i);
            }
        } else {
            let msg = format!("{} regenerates +{change}", f.name);
            self.log.push(msg);
        }
    }

    /// Community turn-start bonuses (the hook chain in 4840ec).
    fn turn_bonuses(&mut self) {
        let round = self.round as i32;
        let living = self.fighters.iter().filter(|f| f.alive()).count();
        let starts = [self.start[0].count, self.start[1].count];
        for i in 0..self.fighters.len() {
            if !self.fighters[i].alive() {
                continue;
            }
            let team = self.fighters[i].team;
            let own_building = self.building_defence[team.index()];
            let their_building = self.building_defence[team.other().index()];
            let f = &mut self.fighters[i];
            // Hunger: the unit count changed since the last look: healed to full.
            if round >= 2 && f.has(Bonus::Hunger) && living != self.hunger_seen {
                self.hunger_seen = living;
                f.hp = f.base.max_hp();
            }
            if f.has(Bonus::Berserk) {
                f.mods.attack = berserk(f);
            }
            if round >= 2 && f.has(Bonus::Fortify) {
                f.mods.defence += (f.base[Stat::DefenceBlow] * FORTIFY_PERCENT / 100).max(1) * (round - 1).min(FORTIFY_TURNS);
            }
            // The Community Garrison fix: +AttackShot to the attack modifier.
            if f.has(Bonus::Garrison) && own_building == 10 {
                f.mods.attack += f.base[Stat::AttackShot];
            }
            if round == 1 && (f.has(Bonus::Artillery) || f.has(Bonus::FirstShot)) {
                f.mods.initiative += FIRST_TURN_INITIATIVE * if own_building >= 10 { 2 } else { 1 };
            }
            // Bastion doubles its attacks and defences every turn, with no building check.
            if f.has(Bonus::Bastion) {
                for st in [Stat::AttackBlow, Stat::AttackShot, Stat::DefenceBlow, Stat::DefenceShot] {
                    f.base[st] *= 2;
                }
            }
            if round == 1 && f.has(Bonus::Assault) && their_building >= 10 {
                for st in [Stat::AttackBlow, Stat::AttackShot, Stat::DefenceBlow, Stat::DefenceShot] {
                    f.base[st] *= 2;
                }
            }
            if f.has(Bonus::Flock) {
                let (own, other) = (starts[team.index()], starts[team.other().index()]);
                let of = if f.base[Stat::AttackBlow] > 0 { f.base[Stat::AttackBlow] } else { f.base[Stat::AttackShot] };
                let step = of * FLOCK_PERCENT / 100;
                f.mods.attack += match own.cmp(&other) {
                    std::cmp::Ordering::Greater => step,
                    std::cmp::Ordering::Less => -step,
                    std::cmp::Ordering::Equal => 0,
                };
            }
        }
    }

    /// Picks the next actor with the threshold scan (489ca0), starting new turns as needed.
    fn advance(&mut self) {
        self.current = None;
        if self.outcome() != Outcome::Ongoing {
            return;
        }
        let mut lists = [self.living_ids(Team::Player), self.living_ids(Team::Enemy)];
        loop {
            if self.cursor == (0, 0) && self.threshold > 0 {
                // A pass that finds nobody only lowers the threshold: jump over those.
                let top = lists.iter().flatten().filter(|&&i| self.fighters[i].actions > 0).map(|&i| self.initiative(i)).max().unwrap_or(0);
                self.threshold = self.threshold.min(top.max(0));
            }
            if self.threshold <= 0 {
                if self.round >= self.turn_limit() {
                    // Nobody can act any more: the limit ends it.
                    self.ended = Some(EndReason::TurnLimit);
                    return;
                }
                self.start_turn();
                if self.outcome() != Outcome::Ongoing {
                    return;
                }
                lists = [self.living_ids(Team::Player), self.living_ids(Team::Enemy)];
                continue;
            }
            let (side, pos) = self.cursor;
            let list = &lists[side];
            if let Some(&i) = list.get(pos) {
                if self.initiative(i) >= self.threshold && self.fighters[i].actions > 0 {
                    if self.first_threshold == 0 {
                        self.first_threshold = self.threshold;
                    }
                    self.current = Some(i);
                    return;
                }
            }
            if pos + 1 < list.len() {
                self.cursor.1 += 1;
            } else if side == 0 {
                self.cursor = (1, 0);
            } else {
                self.cursor = (0, 0);
                self.threshold -= 1;
            }
        }
    }

    /// Recomputes current stats from base, drain and this turn's modifiers.
    fn refresh(&mut self, i: usize) {
        let f = &mut self.fighters[i];
        let mut s = f.base.clone();
        s[Stat::MagicPower] = f.power;
        s[Stat::Regen] = f.regen;
        for st in [Stat::AttackBlow, Stat::AttackShot] {
            if s[st] > 0 {
                s[st] += f.mods.attack;
            }
        }
        s[Stat::DefenceBlow] += f.mods.defence;
        s[Stat::DefenceShot] += f.mods.defence;
        s[Stat::Initiative] += f.mods.initiative;
        s.clamp();
        s[Stat::Regen] = f.regen;
        f.stats = s;
    }

    fn row_occupied(&self, team: Team, row: Row) -> bool {
        self.living(team).any(|f| f.slot.row == row)
    }

    /// Row collapse (48a170): with rows 1 and 2 empty the reserve moves to row 1 (same
    /// column) and loses its remaining actions; with only row 1 empty row 2 moves up and
    /// keeps them.
    fn collapse(&mut self, team: Team) {
        if self.row_occupied(team, Row::Front) {
            return;
        }
        let back = self.row_occupied(team, Row::Back);
        let from = if back { Row::Back } else { Row::Reserve };
        let mut moved = false;
        for f in self.fighters.iter_mut().filter(|f| f.alive() && f.team == team && f.slot.row == from) {
            f.slot.row = Row::Front;
            if from == Row::Reserve {
                f.actions = 0;
            }
            moved = true;
        }
        if moved {
            let side = if team == Team::Player { "Your" } else { "The enemy" };
            let what = if back { "rear" } else { "reserve" };
            self.log.push(format!("{side} {what} steps forward"));
        }
    }

    /// A unit has just died: it leaves the field, and its side's rows may collapse.
    fn died(&mut self, i: usize) {
        let team = self.fighters[i].team;
        self.collapse(team);
    }

    /// The end check after every action (48b67b): a side gone, the turn limit, or a side
    /// whose every unit has `Surrender > 0`, which then gives up.
    fn end_check(&mut self) {
        if self.living(Team::Player).next().is_none() || self.living(Team::Enemy).next().is_none() {
            return;
        }
        let limit = self.round >= self.turn_limit();
        let giving_up: Vec<Team> =
            Team::BOTH.into_iter().filter(|&t| self.living(t).all(|f| f.surrender > 0)).collect();
        for &team in &giving_up {
            let mut mana = 0;
            for f in self.fighters.iter_mut().filter(|f| f.alive() && f.team == team) {
                mana += f.surrender;
                f.surrendered = true;
                f.surrender_hp = f.hp;
                f.hp = 0;
            }
            self.surrender_mana[team.other().index()] += mana;
            let side = if team == Team::Player { "Your army" } else { "The enemy" };
            self.log.push(format!("{side} surrenders"));
        }
        if let Some(&team) = giving_up.first() {
            self.ended = Some(EndReason::Surrender(team));
        } else if limit {
            self.ended = Some(EndReason::TurnLimit);
            self.log.push(format!("Turn {} ends the battle", self.round));
        }
    }

    // ------------------------------------------------------------------------------------
    // Reach (484c4c)
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

    /// What `id`, standing on `from`, would do to `target`: one action per cell, as in the
    /// original's cell map. Where several fit, later ones win as there: melee, then a shot,
    /// then hostile magic; Flying's melee only where nothing else fits.
    fn option_at(&self, id: usize, from: Slot, target: usize) -> Option<ActionKind> {
        use ActionKind::*;
        let (f, t) = (&self.fighters[id], &self.fighters[target]);
        if !f.alive() || !t.alive() {
            return None;
        }
        let s = &f.stats;
        if t.team != f.team {
            if !t.slot.row.is_active() {
                return None;
            }
            let near = self.front_near(t.team, from.col);
            let engaged = !near.is_empty();
            let adjacent = t.slot.row == Row::Front && near.contains(&t.slot.col);
            let mut kind = None;
            if f.has(Bonus::Flying) && from.row.is_active() && adjacent {
                kind = Some(Melee);
            }
            if f.is_warrior() && from.row == Row::Front {
                if adjacent {
                    kind = Some(Melee);
                } else if !engaged && self.long_strike_targets(t.team, from.col).contains(&target) {
                    kind = Some(LongStrike);
                }
            }
            if f.is_shooter() && (from.row == Row::Back || (from.row == Row::Front && (!engaged || adjacent))) {
                kind = Some(Shot);
            }
            if s.is_mage() && s.magic_direction().hits_enemies() {
                let reach = from.row == Row::Back || (from.row == Row::Front && !engaged);
                // Ghost casters also reach the three front cells opposite, from any row.
                if reach || (f.has(Bonus::Ghost) && adjacent) {
                    kind = Some(if t.weakened() { Strike } else { Curse });
                }
            }
            kind
        } else {
            if !(s.is_mage() && s.magic_direction().helps_allies()) || t.crippled {
                return None;
            }
            if from.row == Row::Reserve {
                // A caster in the reserve tends the reserve, and nothing else.
                if t.slot.row != Row::Reserve {
                    return None;
                }
            } else {
                if !t.slot.row.is_active() || (t.blessed && !t.wounded()) {
                    return None;
                }
                if self.school(id) == MagicSchool::Elemental && t.base.magic == Some(MagicSchool::Elemental) && !t.wounded() {
                    return None;
                }
            }
            Some(if t.wounded() && self.heal_amount(id, target, f.power) > 0 { Heal } else { Bless })
        }
    }

    /// What the fighter `id` can do to `target` from where it stands (at most one action).
    pub fn options(&self, id: usize, target: usize) -> Vec<ActionKind> {
        self.option_at(id, self.fighters[id].slot, target).into_iter().collect()
    }

    pub fn can_target(&self, id: usize, target: usize) -> bool {
        !self.options(id, target).is_empty()
    }

    pub fn targets(&self, id: usize) -> Vec<usize> {
        (0..self.fighters.len()).filter(|&t| self.can_target(id, t)).collect()
    }

    fn all_options(&self, id: usize) -> Vec<(usize, ActionKind)> {
        (0..self.fighters.len()).filter_map(|t| self.option_at(id, self.fighters[id].slot, t).map(|k| (t, k))).collect()
    }

    /// A unit that could attack but cannot from where it stands (e.g. a warrior in the back
    /// row).
    pub fn helpless(&self, id: usize) -> bool {
        let f = &self.fighters[id];
        let s = &f.stats;
        let could = f.is_warrior() || f.is_shooter() || (s.is_mage() && s.magic_direction().hits_enemies());
        could && !self.all_options(id).iter().any(|o| o.1.is_hostile())
    }

    /// Empty own cells the fighter could step to (484c4c): from row 1 or 2, columns c−1..c+1
    /// of rows 1 and 2; any reserve cell while it may still use the reserve this turn. From
    /// the reserve (with the same permission): any cell of rows 1 and 2. No swaps.
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
                (Row::Reserve, Row::Reserve) => false,
                (Row::Reserve, _) | (_, Row::Reserve) => f.reserve_move,
                _ => s.col.abs_diff(from.col) <= 1,
            })
            .collect()
    }

    // ------------------------------------------------------------------------------------
    // Damage and magic (485908, 485b3c)
    // ------------------------------------------------------------------------------------

    fn has_knight(&self, team: Team) -> bool {
        self.fighters.iter().any(|f| f.team == team && f.is_hero && HeroClass::of_unit(f.unit) == Some(HeroClass::Knight))
    }

    fn splash_pct(&self, a: usize) -> i32 {
        if self.interactive && self.fighters[a].has(Bonus::Splash) {
            SPLASH_MAIN
        } else {
            100
        }
    }

    /// Physical damage of `a` on `t` (before capping at the target's HP). A `Splash` unit's
    /// first hit uses 80% of its attack.
    pub fn physical_damage(&self, a: usize, t: usize, kind: ActionKind) -> i32 {
        self.physical_damage_at(a, t, kind, self.splash_pct(a))
    }

    /// Physical damage with `pct`% of the attacker's attack (485908).
    fn physical_damage_at(&self, a: usize, t: usize, kind: ActionKind, pct: i32) -> i32 {
        let (af, tf) = (&self.fighters[a], &self.fighters[t]);
        let (s, ts) = (&af.stats, &tf.stats);
        let shot = kind == ActionKind::Shot;
        let building = self.building_defence[tf.team.index()];
        let mut atk = if shot { s[Stat::AttackShot] } else { s[Stat::AttackBlow] } * pct / 100;
        let mut def = if shot { ts[Stat::DefenceShot] } else { ts[Stat::DefenceBlow] };
        if shot {
            if s.has_any(&PIERCE_SHOT) {
                def = 0;
            }
            if tf.slot.row == Row::Back {
                def += self.opt().row2_def;
            }
        } else {
            if self.round == 1 && ts.has(&Bonus::SpearDefense) {
                def *= 3;
            }
            if s.has_any(&PIERCE_MELEE) {
                def = 0;
            }
            if kind == ActionKind::LongStrike {
                def /= 2;
                if s.has(&Bonus::FlankStrike) {
                    atk *= 2;
                }
            }
        }
        def += building;
        let mut dmg = if atk > def { atk - def } else { 1 };
        // Assault takes ×2/3 from a garrison (the misaligned test at c2a403, medium).
        let assaulted = ts.has(&Bonus::Assault) && self.building_defence[af.team.index()] > 0 && af.mods.initiative >= 0;
        if ts.has_any(&[Bonus::Evasive, Bonus::VampirsGist, Bonus::OldVampirsGist]) || assaulted {
            dmg = dmg * 2 / 3;
        }
        if ts.has(&Bonus::Garrison) && building >= 10 {
            dmg = dmg * 2 / 3;
        }
        if shot && ts.has_any(&[Bonus::Dead, Bonus::FastDead]) {
            dmg = dmg * 3 / 10;
        }
        if self.has_knight(tf.team) {
            dmg = dmg * KNIGHT_PERCENT / 100;
        }
        if ts.has_any(&[Bonus::Unvulnerabe, Bonus::Ghost]) {
            dmg = 1;
        }
        dmg += god_bonus(s);
        if dmg == 0 {
            dmg = 1;
        }
        (dmg * (100 - ts.evasion.clamp(0, 100)) / 100).max(1)
    }

    fn school(&self, a: usize) -> MagicSchool {
        self.fighters[a].stats.magic.unwrap_or(MagicSchool::Elemental)
    }

    /// Caster power `p` against `t` for hostile magic: reduced by the target's protection %,
    /// except for a Community `Potent` caster.
    fn hostile_power_of(&self, a: usize, t: usize, p: i32) -> i32 {
        if self.fighters[a].has(Bonus::Potent) {
            return p;
        }
        let prot = self.fighters[t].stats.protection(self.school(a)).clamp(0, 100);
        (p * (100 - prot) + 50) / 100
    }

    /// The power `a` casts with at `pct`% (Splash).
    fn power_at(&self, a: usize, pct: i32) -> i32 {
        self.fighters[a].power * pct / 100
    }

    /// Magic strike damage of hostile power `p`, before capping at HP: Life ×2 on undead,
    /// Death ×½ on undead, Elemental ¾; ¾ on elementals for Life and Death (not for a
    /// `Potent` caster); plus GodAnger/GodStrike.
    fn strike_damage(&self, a: usize, t: usize, p: i32) -> i32 {
        let nature = self.fighters[t].stats.nature;
        let dmg = match (self.school(a), nature) {
            _ if self.fighters[a].has(Bonus::Potent) => p,
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

    /// A magic strike's damage on `t` as it would be cast now, `Drying` included.
    pub fn magic_strike(&self, a: usize, t: usize) -> i32 {
        let p = self.hostile_power_of(a, t, self.power_at(a, self.splash_pct(a)));
        self.strike_damage(a, t, p) + self.drying(a, t)
    }

    /// Community `Drying`: 8% of the target's max HP on every hostile spell, ignoring
    /// protection (at least 1).
    fn drying(&self, a: usize, t: usize) -> i32 {
        if self.fighters[a].has(Bonus::Drying) {
            (self.fighters[t].max_hp() * DRYING_PERCENT / 100).max(1)
        } else {
            0
        }
    }

    /// HP a heal of power `p` restores before capping: Life P (not on undead or elementals),
    /// Elemental P/2, Death P on undead only.
    fn heal_amount(&self, a: usize, t: usize, p: i32) -> i32 {
        let nature = self.fighters[t].stats.nature;
        match self.school(a) {
            MagicSchool::Life if matches!(nature, Nature::Undead | Nature::Elemental) => 0,
            MagicSchool::Life => p,
            MagicSchool::Elemental => p / 2,
            MagicSchool::Death if nature == Nature::Undead => p,
            MagicSchool::Death => 0,
        }
    }

    /// Blessing of power `p` by school (friendly: power not reduced).
    fn bless_of(&self, a: usize, t: usize, p: i32) -> Buff {
        let target = &self.fighters[t];
        let mut b = match self.school(a) {
            MagicSchool::Life if matches!(target.stats.nature, Nature::Undead | Nature::Elemental) => Buff::default(),
            school => bless_effect(self.opt(), school, p),
        };
        if !target.has_attack() {
            b.attack = 0;
        }
        b
    }

    /// Curse of hostile power `p` by school.
    fn curse_of(&self, a: usize, t: usize, p: i32) -> Buff {
        let mut b = curse_effect(self.opt(), self.school(a), p);
        if !self.fighters[t].has_attack() {
            b.attack = 0;
        }
        b
    }

    /// The blessing `a` would give `t` now.
    pub fn bless_buff(&self, a: usize, t: usize) -> Buff {
        self.bless_of(a, t, self.power_at(a, self.splash_pct(a)))
    }

    /// The curse `a` would put on `t` now.
    pub fn curse_buff(&self, a: usize, t: usize) -> Buff {
        self.curse_of(a, t, self.hostile_power_of(a, t, self.power_at(a, self.splash_pct(a))))
    }

    /// Expected effect of `kind` by `a` on `t`, for hover previews.
    pub fn preview(&self, a: usize, t: usize, kind: ActionKind) -> Preview {
        let target = &self.fighters[t];
        let pct = self.splash_pct(a);
        match kind {
            ActionKind::Strike => Preview::Damage(self.magic_strike(a, t).min(target.hp)),
            ActionKind::Curse => Preview::Buff(self.curse_buff(a, t)),
            ActionKind::Heal => Preview::Heal(self.heal_amount(a, t, self.power_at(a, pct)).min(target.max_hp() - target.hp)),
            ActionKind::Bless => Preview::Buff(self.bless_buff(a, t)),
            k => Preview::Damage(self.physical_damage(a, t, k).min(target.hp)),
        }
    }

    // ------------------------------------------------------------------------------------
    // Actions (48a5c4)
    // ------------------------------------------------------------------------------------

    /// Every action starts by spending one action; a bleeding unit then bleeds, and dies
    /// before acting if that kills it. False if the actor died.
    fn start_action(&mut self, id: usize) -> bool {
        let f = &mut self.fighters[id];
        f.actions -= 1;
        f.taken += 1;
        if f.bleed > 0 {
            let loss = ((f.base[Stat::AttackBlow] + f.base[Stat::AttackShot] + f.power) * f.bleed / 100).clamp(0, f.hp);
            if loss > 0 {
                f.hp -= loss;
                f.lost += loss;
                let msg = format!("{} bleeds for {loss}", f.name);
                self.log.push(msg);
                if !self.fighters[id].alive() {
                    self.died(id);
                    return false;
                }
            }
        }
        true
    }

    /// After every action: the actor's side collapses once it has used its last action
    /// (48b5ac), the end check runs and the next actor is picked.
    fn finish_action(&mut self, id: usize) {
        if self.fighters[id].alive() && self.fighters[id].actions <= 0 {
            let team = self.fighters[id].team;
            self.collapse(team);
        }
        self.end_check();
        self.advance();
    }

    /// Step the active fighter to an empty own cell; costs one action. Stepping into or out
    /// of the reserve uses up that unit's reserve move for the turn.
    pub fn move_active(&mut self, to: Slot) -> Result<(), ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        if !self.moves(id).contains(&to) {
            return Err(ActionError::InvalidTarget);
        }
        if self.start_action(id) {
            let f = &mut self.fighters[id];
            if (f.slot.row == Row::Reserve) != (to.row == Row::Reserve) {
                f.reserve_move = false;
            }
            f.slot = to;
            let msg = format!("{} moves", f.name);
            self.log.push(msg);
        }
        self.finish_action(id);
        Ok(())
    }

    /// The active fighter passes one action (a click on its own cell).
    pub fn pass(&mut self) {
        if let Some(id) = self.active() {
            self.start_action(id);
            self.finish_action(id);
        }
    }

    /// The active fighter passes all its remaining actions.
    pub fn skip(&mut self) {
        if let Some(id) = self.active() {
            self.log.push(format!("{} waits", self.fighters[id].name));
            for _ in 0..self.fighters[id].actions {
                if self.active() != Some(id) {
                    break;
                }
                self.pass();
            }
        }
    }

    /// The active fighter's action on `target`; costs one action.
    pub fn act(&mut self, target: usize) -> Result<Hit, ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        let kind = *self.options(id, target).first().ok_or(ActionError::InvalidTarget)?;
        self.act_with(target, kind)
    }

    /// The active fighter does `kind` to `target` (it must be the cell's action); costs one
    /// action.
    pub fn act_with(&mut self, target: usize, kind: ActionKind) -> Result<Hit, ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        if !self.options(id, target).contains(&kind) {
            return Err(ActionError::InvalidTarget);
        }
        let mut hit = Hit::new(target, kind);
        if self.start_action(id) {
            self.fighters[id].useful += 1;
            if kind.is_physical() {
                self.physical_action(id, target, &mut hit);
            } else if kind.is_hostile() {
                self.hostile_action(id, target, &mut hit);
            } else {
                self.friendly_action(id, target, &mut hit);
            }
            // Suicide: gone after any hostile action of its own.
            if kind.is_hostile() && self.fighters[id].alive() && self.fighters[id].has(Bonus::Suicide) {
                let f = &mut self.fighters[id];
                f.lost += f.hp;
                f.hp = 0;
                let msg = format!("{} gives its life", f.name);
                self.log.push(msg);
                self.died(id);
            }
        }
        hit.actor_died = !self.fighters[id].alive();
        self.finish_action(id);
        Ok(hit)
    }

    /// Community `PreventiveStrike`: before a melee on it, it hits first (melee with an
    /// AttackBlow, else a shot); before a shot or a hostile spell, it shoots first if it has
    /// an AttackShot. False if the attacker died.
    fn preventive_strike(&mut self, id: usize, target: usize, kind: ActionKind, hit: &mut Hit) -> bool {
        let t = &self.fighters[target];
        if !t.alive() || !t.has(Bonus::PreventiveStrike) {
            return true;
        }
        let answer = if kind.is_melee() {
            Some(if t.is_warrior() { ActionKind::Melee } else { ActionKind::Shot })
        } else {
            t.is_shooter().then_some(ActionKind::Shot)
        };
        let Some(answer) = answer else { return true };
        let dmg = self.physical_damage_at(target, id, answer, 100).min(self.fighters[id].hp);
        self.wound(id, dmg);
        hit.counter = Some(dmg);
        self.log.push(format!("{} strikes first for {dmg}", self.fighters[target].name));
        !self.check_death(id, Some(target), false)
    }

    fn physical_action(&mut self, id: usize, target: usize, hit: &mut Hit) {
        let kind = hit.kind;
        if !self.preventive_strike(id, target, kind, hit) {
            return;
        }
        let pct = self.splash_pct(id);
        let (dealt, killed) = self.physical_hit(id, target, kind, pct);
        hit.amount = dealt;
        hit.killed = killed;
        if pct != 100 {
            for n in self.splash_neighbours(id, target, kind) {
                if self.fighters[id].alive() {
                    let (d, _) = self.physical_hit(id, n, kind, SPLASH_SIDE);
                    hit.splash.push((n, d));
                }
            }
        }
        self.after_kill(id, killed, kind);
        // Counterblow: a surviving target answers a melee or long strike with melee damage.
        let t = &self.fighters[target];
        if kind.is_melee() && t.alive() && t.has(Bonus::Counterblow) && self.fighters[id].alive() && !self.fighters[id].has(Bonus::Suicide) {
            let dmg = self.physical_damage_at(target, id, ActionKind::Melee, 100).min(self.fighters[id].hp);
            self.wound(id, dmg);
            hit.counter = Some(dmg);
            self.log.push(format!("{} hits back for {dmg}", self.fighters[target].name));
            self.check_death(id, Some(target), false);
        }
    }

    /// BloodThrist (+1 action) and, for melee, Hunger (full HP) after a kill.
    fn after_kill(&mut self, id: usize, killed: bool, kind: ActionKind) {
        let f = &mut self.fighters[id];
        if !killed || !f.alive() {
            return;
        }
        if f.has(Bonus::BloodThrist) {
            f.actions += 1;
        }
        if kind.is_melee() && f.has(Bonus::Hunger) {
            f.hp = f.max_hp();
        }
    }

    /// Living units beside `target` in its row (c ± 1); for melee also within one column of
    /// the attacker.
    fn splash_neighbours(&self, id: usize, target: usize, kind: ActionKind) -> Vec<usize> {
        let (team, slot) = (self.fighters[target].team, self.fighters[target].slot);
        let col = self.fighters[id].slot.col;
        (0..self.fighters.len())
            .filter(|&n| {
                let f = &self.fighters[n];
                n != target
                    && f.alive()
                    && f.team == team
                    && f.slot.row == slot.row
                    && f.slot.col.abs_diff(slot.col) == 1
                    && (!kind.is_melee() || f.slot.col.abs_diff(col) <= 1)
            })
            .collect()
    }

    /// One physical hit and its effects on the target. Returns (damage dealt, killed).
    fn physical_hit(&mut self, id: usize, target: usize, kind: ActionKind, pct: i32) -> (i32, bool) {
        let raw = self.physical_damage_at(id, target, kind, pct);
        let dealt = raw.min(self.fighters[target].hp);
        self.wound(target, dealt);
        let a = self.fighters[id].base.clone();
        // Vanilla Poison and PoisonS: a hit of more than 1 sets the regeneration.
        if raw > 1 {
            let t = &mut self.fighters[target];
            if a.has(&Bonus::Poison) {
                t.regen = POISON_REGEN;
            }
            if a.has(&Bonus::PoisonS) {
                t.regen = STRONG_POISON_REGEN;
            }
        }
        // Vampirism on the uncapped damage, not from undead or elementals.
        let vamp = self.fighters[id].stats[Stat::Vampirizm];
        if vamp > 0 && !matches!(self.fighters[target].stats.nature, Nature::Undead | Nature::Elemental) {
            let f = &mut self.fighters[id];
            f.hp = (f.hp + raw * vamp / 100).min(f.max_hp());
        }
        if kind.is_melee() && self.fighters[target].has(Bonus::CtrPoison) {
            self.fighters[id].regen -= CTR_POISON_STEP;
            self.refresh(id);
        }
        self.after_hit(id, target, raw);
        let how = if kind == ActionKind::LongStrike { " with a long strike" } else { "" };
        let killed = self.check_death(target, Some(id), true);
        let (name, tname) = (&self.fighters[id].name, &self.fighters[target].name);
        let msg = if killed { format!("{name} kills {tname}{how} ({dealt})") } else { format!("{name} hits {tname}{how} for {dealt}") };
        self.log.push(msg);
        (dealt, killed)
    }

    fn hostile_action(&mut self, id: usize, target: usize, hit: &mut Hit) {
        if !self.preventive_strike(id, target, hit.kind, hit) {
            return;
        }
        let pct = self.splash_pct(id);
        let (amount, buff, killed) = self.hostile_spell(id, target, pct);
        (hit.amount, hit.buff, hit.killed) = (amount, buff, killed);
        if pct != 100 {
            for n in self.splash_neighbours(id, target, hit.kind) {
                if self.fighters[id].alive() {
                    let (d, _, _) = self.hostile_spell(id, n, SPLASH_SIDE);
                    hit.splash.push((n, d));
                }
            }
        }
        self.after_kill(id, killed, hit.kind);
    }

    /// A hostile spell at `pct`% power: a strike on a weakened target, else a curse, then the
    /// spell's side effects. Returns (damage dealt, curse, killed).
    fn hostile_spell(&mut self, id: usize, target: usize, pct: i32) -> (i32, Buff, bool) {
        let p = self.hostile_power_of(id, target, self.power_at(id, pct));
        let school = self.school(id);
        let (name, tname) = (self.fighters[id].name.clone(), self.fighters[target].name.clone());
        let mut dealt = 0;
        let mut buff = Buff::default();
        if self.fighters[target].weakened() {
            let raw = self.strike_damage(id, target, p);
            dealt = raw.min(self.fighters[target].hp);
            self.wound(target, dealt);
            self.log.push(format!("{name} hits {tname} with magic for {dealt}"));
            // Vampirism on magic: Death strikes only, not from undead or elementals.
            let vamp = self.fighters[id].stats[Stat::Vampirizm];
            if school == MagicSchool::Death && vamp > 0 && !matches!(self.fighters[target].stats.nature, Nature::Undead | Nature::Elemental) {
                let f = &mut self.fighters[id];
                f.hp = (f.hp + raw * vamp / 100).min(f.max_hp());
            }
        } else {
            buff = self.curse_of(id, target, p);
            self.apply_buff(id, target, buff, false);
            self.fighters[target].cursed = true;
            self.log.push(format!("{name} curses {tname}: {}", buff.describe()));
            // An undead caster's Elemental or Death curse drains life to it.
            if self.fighters[id].base.nature == Nature::Undead && school != MagicSchool::Life {
                let drain = (p / self.opt().curse_main_spell.max(1) / 2 + 1).min(self.fighters[target].hp);
                self.wound(target, drain);
                dealt += drain;
                let f = &mut self.fighters[id];
                f.hp = (f.hp + drain).min(f.max_hp());
            }
        }
        let a = self.fighters[id].base.clone();
        let dry = self.drying(id, target).min(self.fighters[target].hp);
        if dry > 0 {
            self.wound(target, dry);
            dealt += dry;
        }
        // Poison works for mages whose power after protection is above 15.
        if p > MAGE_POISON_POWER {
            let t = &mut self.fighters[target];
            if a.has(&Bonus::Poison) {
                t.regen = POISON_REGEN;
            }
            if a.has(&Bonus::PoisonS) {
                t.regen = STRONG_POISON_REGEN;
            }
        }
        if a.has(&Bonus::Exhaustion) {
            let t = &mut self.fighters[target];
            for st in [Stat::ProtectLife, Stat::ProtectDeath, Stat::ProtectElemental] {
                t.base[st] = (t.base[st] - EXHAUSTION_POINTS).max(0);
            }
        }
        // In this path the "damage" the hooks test is the spell's power.
        self.after_hit(id, target, p);
        let killed = self.check_death(target, Some(id), true);
        if killed {
            self.log.push(format!("{name} kills {tname} with magic"));
        }
        (dealt, buff, killed)
    }

    fn friendly_action(&mut self, id: usize, target: usize, hit: &mut Hit) {
        let pct = self.splash_pct(id);
        let (amount, buff) = self.friendly_spell(id, target, pct);
        (hit.amount, hit.buff) = (amount, buff);
        if pct != 100 {
            for n in self.splash_neighbours(id, target, hit.kind) {
                if !self.fighters[n].crippled {
                    let (h, _) = self.friendly_spell(id, n, SPLASH_SIDE);
                    hit.splash.push((n, h));
                }
            }
        }
    }

    /// Heal a wounded ally (if the heal does anything), else bless it, at `pct`% power.
    fn friendly_spell(&mut self, id: usize, target: usize, pct: i32) -> (i32, Buff) {
        let p = self.power_at(id, pct);
        let (name, tname) = (self.fighters[id].name.clone(), self.fighters[target].name.clone());
        let t = &self.fighters[target];
        let heal = self.heal_amount(id, target, p);
        if t.wounded() && heal > 0 {
            let healed = heal.min(t.max_hp() - t.hp);
            self.fighters[target].hp += healed;
            self.log.push(format!("{name} heals {tname} +{healed}"));
            return (healed, Buff::default());
        }
        let buff = self.bless_of(id, target, p);
        self.apply_buff(id, target, buff, true);
        self.fighters[target].blessed = true;
        self.log.push(format!("{name} blesses {tname}: {}", buff.describe()));
        (0, buff)
    }

    /// A blessing or curse: added to this turn's modifiers, or with `EternalGift` to the
    /// battle stats (which lasts and stacks; its Life blessing lowers the defences, a bug of
    /// the original). The change of actions left counts for this turn only; a curse cannot
    /// take them below 0.
    fn apply_buff(&mut self, caster: usize, target: usize, b: Buff, bless: bool) {
        let eternal = self.fighters[caster].has(Bonus::EternalGift);
        let life = self.school(caster) == MagicSchool::Life;
        let t = &mut self.fighters[target];
        if eternal {
            let attack = if t.base[Stat::AttackBlow] > 0 { Stat::AttackBlow } else { Stat::AttackShot };
            t.base[attack] += b.attack;
            let defence = if bless && life { -b.defence } else { b.defence };
            t.base[Stat::DefenceBlow] += defence;
            t.base[Stat::DefenceShot] += defence;
            t.base[Stat::Initiative] += b.initiative;
        } else {
            t.mods.attack += b.attack;
            t.mods.defence += b.defence;
            t.mods.initiative += b.initiative;
        }
        t.actions = (t.actions + b.actions).max(0);
        self.refresh(target);
    }

    /// Per-hit effects on the target (Berserk-on-target, Stun and the Community on-hit
    /// block). `v` is the damage, or the spell's power for magic.
    fn after_hit(&mut self, id: usize, target: usize, v: i32) {
        let a = self.fighters[id].base.clone();
        let finish = format!("{} finishes {}", self.fighters[id].name, self.fighters[target].name);
        let t = &mut self.fighters[target];
        if t.has(Bonus::Berserk) && t.alive() {
            t.mods.attack = berserk(t);
        }
        if a.has(&Bonus::Stun) {
            t.mods.initiative -= t.stats[Stat::Initiative] * STUN_PERCENT / 100;
        }
        if v > 1 {
            if a.has(&Bonus::PoisonArmorIgnore) {
                t.regen = t.regen.min(PIERCING_POISON_REGEN);
            }
            if a.has(&Bonus::Bleed) {
                t.bleed = BLEED_PERCENT;
            }
            if a.has(&Bonus::ArmorBreaker) {
                t.base[Stat::DefenceBlow] = t.base[Stat::DefenceBlow] * 3 / 4;
                t.base[Stat::DefenceShot] = t.base[Stat::DefenceShot] * 3 / 4;
            }
            if a.has(&Bonus::KillingStrike) && t.alive() && t.hp * 100 <= t.base.max_hp() * KILLING_STRIKE_PERCENT {
                t.lost += t.hp;
                t.hp = 0;
                self.log.push(finish);
            }
        }
        let t = &mut self.fighters[target];
        if a.has(&Bonus::Neutralize) {
            t.base.bonuses.clear();
        }
        if a.has(&Bonus::NoHeal) {
            t.crippled = true;
            t.regen = t.regen.min(0);
        }
        self.refresh(target);
    }

    /// `i` loses `amount` hit points (already capped at its HP).
    fn wound(&mut self, i: usize, amount: i32) {
        let f = &mut self.fighters[i];
        f.hp -= amount;
        f.lost += amount;
    }

    /// After a hit: true if `i` died. `FateGift` saves a unit once from a hit (`savable`):
    /// actions refilled, protections and regeneration +20, max HP +20% and full, initiative
    /// +5 this turn, and the gift is gone. The killer of a `DeathCurse` unit dies; the killer
    /// of a `Ghost` dies if its Death protection is below 30 × the ghost's actions (48a3f0).
    fn check_death(&mut self, i: usize, killer: Option<usize>, savable: bool) -> bool {
        if self.fighters[i].alive() {
            return false;
        }
        let f = &mut self.fighters[i];
        if savable && f.has(Bonus::FateGift) {
            f.base.bonuses.clear();
            f.actions = f.base[Stat::Manevres];
            for st in [Stat::ProtectLife, Stat::ProtectDeath, Stat::ProtectElemental] {
                f.base[st] += FATE_PROTECTION;
            }
            f.regen += FATE_REGEN;
            f.base[Stat::Hits] += f.base[Stat::Hits] * FATE_HP_PERCENT / 100;
            f.hp = f.base.max_hp();
            f.mods.initiative += FATE_INITIATIVE;
            let msg = format!("{} is spared by fate", f.name);
            self.log.push(msg);
            self.refresh(i);
            return false;
        }
        if let Some(k) = killer.filter(|&k| self.fighters[k].alive()) {
            let dead = &self.fighters[i];
            let curse = dead.has(Bonus::DeathCurse)
                || (dead.has(Bonus::Ghost) && self.fighters[k].stats[Stat::ProtectDeath] < 30 * dead.base[Stat::Manevres]);
            if curse {
                let kf = &mut self.fighters[k];
                kf.lost += kf.hp;
                kf.hp = 0;
                let msg = format!("{} dies by {}'s curse", self.fighters[k].name, self.fighters[i].name);
                self.log.push(msg);
                self.died(k);
            }
        }
        self.died(i);
        true
    }

    // ------------------------------------------------------------------------------------
    // AI (4864e0; notes in battle.md §4)
    // ------------------------------------------------------------------------------------

    /// A target is killable by one hit, or (improved AI, or the player's side at the normal
    /// level) by the actor's actions left.
    fn killable(&self, id: usize, t: usize, dmg: i32) -> bool {
        let smart = self.ai_level == 2 || (self.ai_level == 1 && self.fighters[id].team == Team::Player);
        let hits = if smart { self.fighters[id].actions.max(1) } else { 1 };
        self.fighters[t].hp <= hits * dmg
    }

    /// The best cell by the original's picker (4860cc): rows front to back, columns in the
    /// preferred order, the first strictly higher score wins, and 0 or less never does.
    fn pick<T: Copy>(&self, cands: impl IntoIterator<Item = (Slot, f64, T)>) -> Option<(f64, T)> {
        let order = self.formation.col_order();
        let mut v: Vec<(Slot, f64, T)> = cands.into_iter().collect();
        v.sort_by_key(|(s, _, _)| (s.row, order.iter().position(|&c| c == s.col).unwrap_or(usize::MAX)));
        let mut best: Option<(f64, T)> = None;
        for (_, score, x) in v {
            if score > 0.0 && best.is_none_or(|(b, _)| score > b) {
                best = Some((score, x));
            }
        }
        best
    }

    fn pick_target(&self, cands: impl IntoIterator<Item = (usize, f64)>) -> Option<(f64, usize)> {
        self.pick(cands.into_iter().map(|(t, s)| (self.fighters[t].slot, s, t)))
    }

    /// The target's answer in the AI's melee score: its melee (warrior) or shot (shooter)
    /// damage on the actor, else its power.
    fn return_threat(&self, id: usize, t: usize) -> i32 {
        match self.fighters[t].ai_role {
            AiRole::Warrior => self.physical_damage_at(t, id, ActionKind::Melee, 100),
            AiRole::Shooter => self.physical_damage_at(t, id, ActionKind::Shot, 100),
            AiRole::Mage => self.fighters[t].ai_power,
        }
    }

    fn poisons(&self, id: usize, t: usize, dmg: i32) -> bool {
        let a = &self.fighters[id];
        (a.has(Bonus::Poison) || a.has(Bonus::PoisonS)) && self.fighters[t].regen >= 0 && dmg > 1
    }

    fn ai_plan(&self) -> Option<Plan> {
        let id = self.active()?;
        let opts = self.all_options(id);
        if let Some(to) = self.ai_retreat(id) {
            return Some(Plan::Move(to));
        }
        let pick = |kinds: &[ActionKind], score: &dyn Fn(usize, ActionKind) -> f64| {
            self.pick(opts.iter().filter(|o| kinds.contains(&o.1)).map(|&(t, k)| (self.fighters[t].slot, score(t, k), (t, k))))
        };
        // Melee on the enemy front row.
        let melee = |t: usize, k: ActionKind| {
            let dmg = self.physical_damage(id, t, k);
            let m = self.fighters[t].base[Stat::Manevres].max(1);
            let r = (self.return_threat(id, t) + 1) * m;
            let mut s = if self.killable(id, t, dmg) { r as f64 * 100.0 } else { dmg as f64 * r as f64 };
            if self.poisons(id, t, dmg) {
                s *= 2.0;
            }
            s
        };
        if let Some((_, (t, k))) = pick(&[ActionKind::Melee, ActionKind::LongStrike], &melee) {
            return Some(Plan::Act(t, k));
        }
        let shot = |t: usize, _| {
            let dmg = self.physical_damage(id, t, ActionKind::Shot);
            let tf = &self.fighters[t];
            let m = tf.base[Stat::Manevres] as f64 + if tf.actions > 0 { (tf.actions as f64).sqrt() } else { 0.0 };
            let mut s = ((tf.ai_power + 1) as f64 * dmg as f64 * m).round();
            if self.poisons(id, t, dmg) {
                s *= 2.0;
            }
            if self.killable(id, t, dmg) {
                s *= 4.0;
            }
            if tf.slot.row == Row::Back {
                s *= match tf.ai_role {
                    AiRole::Warrior => 1.0 / 3.0,
                    AiRole::Shooter => 1.5,
                    AiRole::Mage => 1.75,
                };
                if tf.ai_role == AiRole::Mage
                    && tf.stats.magic_direction() == super::content::MagicDirection::ToEnemy
                    && tf.base.nature == self.fighters[id].base.nature
                {
                    s /= 2.0;
                }
            }
            if tf.base[Stat::Manevres] == 1 {
                s /= 2.0;
            }
            s
        };
        if let Some((_, (t, k))) = pick(&[ActionKind::Shot], &shot) {
            return Some(Plan::Act(t, k));
        }
        if let Some(plan) = self.ai_magic(id, &opts) {
            return Some(plan);
        }
        Some(self.ai_move(id, &opts).unwrap_or(Plan::Pass))
    }

    /// A non-warrior in the front row with more than one action steps back behind the
    /// healthiest own front unit.
    fn ai_retreat(&self, id: usize) -> Option<Slot> {
        let f = &self.fighters[id];
        let s = &f.stats;
        let warrior = s[Stat::AttackBlow] > s[Stat::MagicPower] && s[Stat::AttackBlow] > s[Stat::AttackShot];
        if f.slot.row != Row::Front || f.has(Bonus::Ghost) || f.actions <= 1 || warrior {
            return None;
        }
        let others = self.living(f.team).filter(|o| o.slot.row == Row::Front).count() > 1;
        let alone_mage = self.living(f.team).count() == 1 && s.is_mage();
        if !others && !alone_mage {
            return None;
        }
        let cands = self.moves(id).into_iter().filter(|m| m.row == Row::Back).map(|m| {
            let front = self.at(f.team, Slot::new(Row::Front, m.col)).filter(|&o| o != id);
            (m, 1000.0 + front.map_or(0, |o| self.fighters[o].hp) as f64, m)
        });
        self.pick(cands).map(|(_, m)| m)
    }

    /// Magic by the caster's school (487244, 487c93, 488928).
    fn ai_magic(&self, id: usize, opts: &[(usize, ActionKind)]) -> Option<Plan> {
        let f = &self.fighters[id];
        if !f.stats.is_mage() {
            return None;
        }
        let team = f.team;
        let hostile: Vec<usize> = opts.iter().filter(|o| matches!(o.1, ActionKind::Strike | ActionKind::Curse)).map(|o| o.0).collect();
        let friendly: Vec<usize> = opts.iter().filter(|o| matches!(o.1, ActionKind::Heal | ActionKind::Bless)).map(|o| o.0).collect();
        let act = |t: usize| {
            let k = opts.iter().find(|o| o.0 == t).map(|o| o.1).expect("an option");
            Plan::Act(t, k)
        };
        let mp = f.power;
        let cms = self.opt().curse_main_spell.max(1);
        let missing = |t: usize| (self.fighters[t].max_hp() - self.fighters[t].hp) as f64;
        match self.school(id) {
            MagicSchool::Life => {
                let ghostly = hostile.iter().any(|&t| self.fighters[t].base.has_any(&[Bonus::Unvulnerabe, Bonus::Ghost]));
                let shooters = hostile.iter().any(|&t| self.fighters[t].base[Stat::AttackShot] > 0);
                if !ghostly {
                    let heal = self.pick_target(friendly.iter().map(|&t| {
                        let m = missing(t);
                        let small = mp as f64 > 4.0 * m || self.fighters[t].stats.nature == Nature::Undead;
                        (t, if small { 0.0 } else { m })
                    }));
                    if let Some((_, t)) = heal {
                        return Some(act(t));
                    }
                    let bless = self.pick_target(friendly.iter().map(|&t| {
                        let tf = &self.fighters[t];
                        if tf.mods.defence > 0 || tf.actions <= 0 {
                            return (t, 0.0);
                        }
                        let mut s = tf.ai_power as f64 * 100.0 * tf.base[Stat::Manevres] as f64
                            / (tf.stats[Stat::DefenceBlow] + tf.stats[Stat::DefenceShot] + 20) as f64;
                        if shooters {
                            if tf.ai_role == AiRole::Mage {
                                s *= 2.0;
                            }
                        } else if tf.slot.row == Row::Front {
                            s *= 3.0;
                        } else {
                            s /= 5.0;
                        }
                        (t, s)
                    }));
                    if let Some((_, t)) = bless {
                        return Some(act(t));
                    }
                }
                let life = (2 * cms / 3).max(1);
                let strike = self.pick_target(hostile.iter().map(|&t| {
                    let tf = &self.fighters[t];
                    let p = self.hostile_power_of(id, t, mp);
                    if p <= 0 {
                        return (t, 0.0);
                    }
                    let (db, ds) = (tf.stats[Stat::DefenceBlow], tf.stats[Stat::DefenceShot]);
                    let v = if tf.weakened() {
                        (f.actions * p) as f64
                    } else {
                        // The second term compares with DefenceShot but adds DefenceBlow.
                        let q = p / life;
                        (3 * (q.min(db) + if q < ds { q } else { db }) + 1) as f64
                    };
                    let base = if tf.base.has_any(&[Bonus::Unvulnerabe, Bonus::Ghost]) {
                        3 * tf.ai_power
                    } else {
                        db + ds + tf.ai_power * tf.actions
                    } as f64;
                    let mut s = base * v;
                    if tf.weakened() && tf.hp as f64 <= v && !tf.base.has_any(&[Bonus::DeathCurse, Bonus::Ghost]) {
                        s *= 3.0;
                    }
                    s *= match tf.stats.nature {
                        Nature::Undead => 1.0,
                        Nature::Elemental => 2.0 / 3.0,
                        _ => 1.0 / 3.0,
                    };
                    (t, s)
                }));
                strike.map(|(_, t)| act(t))
            }
            MagicSchool::Elemental => {
                let enemies: Vec<&Fighter> = self.living(team.other()).collect();
                let avg_ini = (enemies.iter().map(|e| e.stats[Stat::Initiative]).sum::<i32>() as f64 / enemies.len().max(1) as f64).max(1.0);
                let ghosts = enemies.iter().filter(|e| e.base.has_any(&[Bonus::Unvulnerabe, Bonus::Ghost])).count();
                let damp = if ghosts > 0 && enemies.len() / 2 <= ghosts { 0.1 } else { 1.0 };
                let own = self.pick_target(friendly.iter().map(|&t| {
                    let tf = &self.fighters[t];
                    let haste = if t != id && tf.mods.initiative <= 0 && tf.base[Stat::Manevres] > 0 && tf.actions > 0 {
                        (actions_of_power(mp) * tf.ai_power * tf.stats[Stat::Initiative]) as f64 / avg_ini
                    } else {
                        0.0
                    };
                    let heal = if tf.wounded() { (2 * mp / 3).min(tf.max_hp() - tf.hp) as f64 } else { 0.0 };
                    (t, haste.max(heal) * damp)
                }));
                let foe = self.pick_target(hostile.iter().map(|&t| {
                    let tf = &self.fighters[t];
                    let p = self.hostile_power_of(id, t, mp);
                    let slow = if tf.mods.initiative >= 0 && tf.actions > 0 { (actions_of_power(p) * tf.ai_power * tf.actions) as f64 } else { 0.0 };
                    let strike = if f.actions > 1 { self.strike_damage(id, t, p) as f64 } else { 0.0 };
                    (t, slow.max(strike))
                }));
                match (own, foe) {
                    (Some((a, t)), Some((b, _))) if a > b => Some(act(t)),
                    (_, Some((_, t))) => Some(act(t)),
                    (Some((_, t)), None) => Some(act(t)),
                    (None, None) => None,
                }
            }
            MagicSchool::Death => {
                let best_strike = hostile.iter().map(|&t| self.strike_damage(id, t, self.hostile_power_of(id, t, mp))).max().unwrap_or(0);
                if best_strike <= mp / cms && f.hp * 4 <= f.max_hp() && f.max_hp() - mp >= f.hp && friendly.contains(&id) {
                    return Some(act(id));
                }
                let strike = self.pick_target(hostile.iter().map(|&t| {
                    let tf = &self.fighters[t];
                    let p = self.hostile_power_of(id, t, mp);
                    if p <= 0 {
                        return (t, 0.0);
                    }
                    let n = f.actions;
                    let v = if tf.weakened() { n * p } else { p / cms / 2 + 1 + (n - 1).max(0) * p };
                    let u = if tf.hp <= v { 20 - 2 * (tf.hp / p) } else { 0 };
                    let m = tf.base[Stat::Manevres].max(1);
                    let threat = |dmg: i32, top: i32, low: i32| (top - f.hp / (dmg * m).max(1)).max(low);
                    let th = match tf.ai_role {
                        AiRole::Shooter => threat(self.physical_damage_at(t, id, ActionKind::Shot, 100), 16, 3),
                        AiRole::Mage => threat(self.strike_damage(t, id, self.hostile_power_of(t, id, tf.power)), 12, 2),
                        AiRole::Warrior => threat(self.physical_damage_at(t, id, ActionKind::Melee, 100), 8, 1),
                    };
                    (t, tf.ai_power as f64 * (u + th) as f64 * v as f64)
                }));
                if let Some((_, t)) = strike {
                    return Some(act(t));
                }
                let heal = self.pick_target(friendly.iter().map(|&t| {
                    let tf = &self.fighters[t];
                    (t, if matches!(tf.stats.nature, Nature::Undead | Nature::Elemental) { missing(t) } else { 0.0 })
                }));
                if let Some((_, t)) = heal {
                    return Some(act(t));
                }
                let bless = self.pick_target(friendly.iter().map(|&t| {
                    let tf = &self.fighters[t];
                    let ok = !tf.blessed && tf.actions > 0 && tf.ai_role != AiRole::Mage;
                    (t, if ok { (f.actions * tf.ai_power * tf.hp) as f64 } else { 0.0 })
                }));
                bless.map(|(_, t)| act(t))
            }
        }
    }

    /// Moves when nothing else scored (489549). The AI never moves into the reserve.
    fn ai_move(&self, id: usize, opts: &[(usize, ActionKind)]) -> Option<Plan> {
        let f = &self.fighters[id];
        let moves = self.moves(id);
        let enemy_front: Vec<u8> = self.living(f.team.other()).filter(|e| e.slot.row == Row::Front).map(|e| e.slot.col).collect();
        let enemy_back: Vec<u8> = self.living(f.team.other()).filter(|e| e.slot.row == Row::Back).map(|e| e.slot.col).collect();
        let best = match f.slot.row {
            Row::Back if f.base[Stat::AttackShot] == 0 && f.power == 0 => self.pick(moves.iter().filter(|m| m.row == Row::Front).map(|&m| {
                let behind = self.at(f.team, Slot::new(Row::Back, m.col)).filter(|&o| o != id);
                let support = behind.map_or(0, |o| self.fighters[o].power.abs() + self.fighters[o].base[Stat::AttackShot]);
                let facing = if enemy_front.contains(&m.col) { 2 } else { 0 };
                (m, (1 + support + facing) as f64, m)
            })),
            Row::Front => self.pick(moves.iter().filter(|m| m.row == Row::Front).map(|&m| {
                let pull = |cols: &[u8], k: i32| cols.iter().map(|&c| (k * (4 - c.abs_diff(m.col) as i32)).max(0)).sum::<i32>();
                (m, (pull(&enemy_front, 2) + pull(&enemy_back, 1)) as f64, m)
            })),
            Row::Reserve => {
                let heal = self.pick_target(opts.iter().filter(|o| o.1 == ActionKind::Heal).map(|o| (o.0, (self.fighters[o.0].max_hp() - self.fighters[o.0].hp) as f64)));
                if let Some((_, t)) = heal {
                    return Some(Plan::Act(t, ActionKind::Heal));
                }
                let skip_edge = self.formation.cols == 6;
                self.pick(moves.iter().filter(|m| m.row.is_active() && !(skip_edge && m.col == 0)).map(|&m| {
                    let s = if f.is_warrior() {
                        if m.row == Row::Front { 2 } else { 1 }
                    } else if m.row == Row::Back {
                        3 - m.col.abs_diff(f.slot.col) as i32
                    } else {
                        0
                    };
                    (m, s as f64, m)
                }))
            }
            Row::Back => None,
        };
        best.map(|(_, m)| Plan::Move(m))
    }

    /// The action the AI would take with the active fighter, if it attacks or casts.
    pub fn ai_choice(&self) -> Option<(usize, ActionKind)> {
        match self.ai_plan()? {
            Plan::Act(t, k) => Some((t, k)),
            _ => None,
        }
    }

    /// Plays one action of the active fighter automatically.
    pub fn ai_step(&mut self) -> Option<Step> {
        let actor = self.active()?;
        match self.ai_plan()? {
            Plan::Act(t, kind) => {
                let hit = self.act_with(t, kind).ok()?;
                Some(Step::Act { actor, hit })
            }
            Plan::Move(to) => {
                let from = self.fighters[actor].slot;
                self.move_active(to).ok()?;
                Some(Step::Move { actor, from, to })
            }
            Plan::Pass => {
                self.pass();
                Some(Step::Wait { actor })
            }
        }
    }

    // ------------------------------------------------------------------------------------
    // After the battle
    // ------------------------------------------------------------------------------------

    /// Standing at the end: alive, or surrendered (a surrendering side is paid its XP
    /// before it leaves).
    fn present(&self, i: usize) -> bool {
        self.fighters[i].alive() || self.fighters[i].surrendered
    }

    /// `team`'s strength now: its units still standing with their current HP and rows.
    pub fn strength_now(&self, team: Team) -> i64 {
        let side: Vec<SideUnit> = (0..self.fighters.len())
            .filter(|&i| self.fighters[i].team == team && self.present(i))
            .map(|i| side_unit(&self.fighters[i]))
            .collect();
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
            .filter(|&i| self.fighters[i].team == team && self.present(i))
            .map(|i| {
                let f = &self.fighters[i];
                XpAward { fighter: i, xp: experience::share(pool, own.count, f.slot.row, f.useful, f.taken, f.actions.max(0)) }
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
    /// survives: he comes back with 1 HP (4906a0). Slots are the deployed ones.
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

/// Community `Berserk`: the attack modifier is `AB × 75% × (maxHP − HP) / maxHP`.
fn berserk(f: &Fighter) -> i32 {
    let max = f.base.max_hp().max(1);
    f.base[Stat::AttackBlow] * BERSERK_PERCENT * (max - f.hp.clamp(0, max)) / max / 100
}

#[cfg(test)]
mod tests;
