//! AI armies (mechanics.md 5.6, §8.8; `original-mechanics/world.md` §4–5): what the
//! scenario's armies want and do while the player walks.
//!
//! - **Brain**: every army the AI steers ([`managed`]: the scenario's land armies) picks a goal
//!   on a cadence ([`THINK_MINUTES`]) from its behaviour style (feudal, rogue, peasant: army
//!   byte 59), its target model (byte 85) and the `Min/Max…Target` priorities of `_Global.ini`
//!   ([`Priorities`]): attack the player or a hostile army, take a hostile castle or fort
//!   (rogues: their lost home first), heal at a friendly building, fill a garrison, hire,
//!   shop, collect a village's tribute, talk to a friendly army, wander its patrol, go home.
//!   As the original (world.md §5): every candidate is seeded with its priority into one
//!   flood over the map and the army takes the lowest `priority + path cost` (10 per
//!   orthogonal grass cell; [`choose`]); lower wins. Armies and the player are candidates
//!   only within `AIDistance0..2` cells of the army, by its style ([`target_range`]); a
//!   patrolling army takes targets only inside its patrol box. An army or garrison is worth
//!   attacking only when a simulated battle says it wins ([`battle_seed`]). The per-army flags
//!   (bytes 76–81) remove goals.
//! - **Walking** (world.md §2): an army banks the minutes of every hero step and wait tick
//!   (up to 200) and takes a step when they cover `cost × speed` (×1.5 diagonally).
//! - **Economy** (noon, [`Game::ai_new_day`]): income from owned buildings and the army's
//!   daily income (byte 80 × 10); feudal lords pay wages and keep `NeedUpkeepDay` days of
//!   them in reserve when they hire or shop; rogues pay no wages and hire only rogue units;
//!   peasants do neither. Garrisons heal at midnight ([`Game::ai_midnight`]).
//! - **AI battles**: mutually hostile armies that meet, or an army reaching its target, fight
//!   with the battle engine played by the AI on both sides ([`Game::ai_battle`]); losses, XP,
//!   loot and captures are applied, and the player hears of it when it happens within sight.
//! - **Lords and respawn**: a beaten feudal lord who still owns a building retreats into it
//!   and comes back after [`RECOVER_DAYS`] *(guess)*; other armies with a home building and a
//!   respawn time come back after it at the centre of their home ([`Respawn`], world.md §5).
//!
//! Everything the original leaves open is marked *(guess)* and listed in mechanics.md §8.8.

use std::collections::{BinaryHeap, HashMap};
use std::cmp::Reverse;
use std::sync::Arc;

use crate::dt::dtm::Army as DtArmy;

use super::battle::{Battle, Outcome, Team};
use super::clock::MINUTES_PER_DAY;
use super::content::{Content, GlobalOptions, Nature, UnitId, WageKind};
use super::events::ArmyId;
use super::fog;
use super::game::{troop_unit, Event, Foe, Game};
use super::items;
use super::map::Tile;
use super::rng::Rng;
use super::units::{Stats, Unit};
use super::world::{Army, Location, LocationKind, Owner, Troop, World};

/// An AI army thinks again after this many game minutes *(guess)*.
pub const THINK_MINUTES: f64 = 60.0;
/// Route searches (A*) all AI armies together may start per slice of game time; the rest wait
/// for the next slice. Chasing the player is not counted.
pub const MAX_PATHS_PER_SLICE: usize = 4;
/// Cells a goal route search may expand.
pub const GOAL_PATH_NODES: usize = 12_000;
/// Cells a chase route search may expand.
const CHASE_PATH_NODES: usize = 4_000;
/// Cells the goal flood may expand ([`choose`]).
const FLOOD_NODES: usize = 40_000;
/// Seeds of the goal flood are capped here (the original's 32766).
const MAX_SEED: i64 = 32_766;
/// Random points a wandering army considers (world.md §5).
const WANDER_POINTS: usize = 4;
/// A castle or fort's seed is `AtackCastle × this` (world.md §5).
const CASTLE_FACTOR: i64 = 50;
/// A beaten lord recovers in his building this many days *(guess)*.
pub const RECOVER_DAYS: u64 = 3;
/// Armies heal at a building only below this share of their hit points, in per mille *(guess)*.
pub const HEAL_BELOW: i64 = 750;
/// Items an AI army carries at most *(guess)*.
pub const MAX_ARMY_ITEMS: usize = 12;
/// A goal that could not be reached is left alone this long.
const BLOCK_MINUTES: f64 = MINUTES_PER_DAY as f64;
/// After a stalemate two armies leave each other alone this long *(guess)*.
const TRUCE_MINUTES: f64 = MINUTES_PER_DAY as f64;
/// Between two talks with friendly armies *(guess)*.
const TALK_EVERY: f64 = MINUTES_PER_DAY as f64;
/// Minutes a talk takes.
const TALK_MINUTES: f64 = 60.0;
/// Battle reports kept in [`Game::ai_log`].
pub const LOG_KEPT: usize = 30;
/// Steps after which an auto-played battle is cut off (the engine's turn limit ends it well
/// before).
const MAX_BATTLE_STEPS: usize = 20_000;
/// Armies meet on neighbouring cells, as the hero does.
const CONTACT: i32 = 1;

/// Behaviour style (army byte 59, mechanics.md 5.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Style {
    /// Like the player: income, tribute, wages, hiring, shopping.
    #[default]
    Feudal,
    /// No wages, no tribute, hires rogue units, retakes its forts.
    Rogue,
    /// Wanders.
    Peasant,
}

impl Style {
    /// Byte 59 (0 feudal, 1 rogue, 2 peasant); other values fall back to the map model
    /// (4 feudal, 5 bandits, 6 peasants).
    pub fn of(behaviour: u8, model: u8) -> Style {
        match (behaviour, model) {
            (0, _) => Style::Feudal,
            (1, _) => Style::Rogue,
            (2, _) => Style::Peasant,
            (_, 5) => Style::Rogue,
            (_, 6) => Style::Peasant,
            _ => Style::Feudal,
        }
    }
}

/// Target models (army byte 85): index into the `_Global.ini` priority lists.
pub mod model {
    pub const STANDARD: usize = 0;
    pub const AGGRESSIVE: usize = 1;
    pub const PASSIVE: usize = 2;
    pub const HOARDING: usize = 3;
    pub const TRADING: usize = 4;
}

/// What the scenario says about an army's behaviour.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AiProfile {
    /// Steered by the AI. False for the demo's gangs.
    pub enabled: bool,
    pub style: Style,
    /// Target model, 0..=4 ([`model`]).
    pub model: usize,
    /// Byte 69, about −70..100: how much stronger an enemy it dares to attack.
    pub aggression: i32,
    /// Attitude towards the four factions (byte 65; the one towards the player is
    /// [`Army::attitude`]).
    pub relations: [i8; 4],
    pub respawn_days: u32,
    /// Respawn the whole army, not only the leader.
    pub respawn_all: bool,
    /// Daily gold income: byte 80 × 10 (world.md §5; word 17 is its starting gold).
    pub extra_income: i32,
    /// Byte 82, default 50: the garrison it keeps in its buildings, in percent of its own
    /// strength *(guess)*.
    pub garrison_strength: i32,
    /// Byte 71: experience correction in percent. It scales the XP the player gains by
    /// beating this army (experience.md §3); 0 is read as 100 *(guess: no shipped army has 0)*.
    pub exp_correction: i32,
    /// Byte 14, "add experience like the player": units it hires start with XP taken from
    /// the player's army (experience.md §5).
    #[serde(default)]
    pub exp_like_player: bool,
    /// Byte 19: bonus XP for the units it hires.
    #[serde(default)]
    pub hire_bonus_exp: i32,
    /// Its units carry no money: no gold to take.
    pub no_money: bool,
    /// Flags (bytes 76–81).
    pub ignored: bool,
    pub player_only: bool,
    pub no_random: bool,
    pub no_talk: bool,
    pub no_buildings: bool,
    /// The army as the scenario places it (for respawns).
    pub start_troops: Vec<Troop>,
}

impl AiProfile {
    /// The profile of a scenario army; `troops` as placed.
    pub fn from_dt(a: &DtArmy, troops: &[Troop]) -> AiProfile {
        AiProfile {
            enabled: true,
            style: Style::of(a.behaviour, a.model),
            model: (a.target_model as usize).min(model::TRADING),
            aggression: a.aggression as i32,
            relations: a.relations,
            respawn_days: a.respawn_days as u32,
            respawn_all: a.respawn_all != 0,
            extra_income: a.unknown_80 as i32 * 10,
            garrison_strength: if a.garrison_strength == 0 { 50 } else { a.garrison_strength as i32 },
            exp_correction: if a.exp_correction == 0 { 100 } else { a.exp_correction as i32 },
            exp_like_player: a.exp_like_player != 0,
            hire_bonus_exp: a.hire_bonus_exp as i32,
            no_money: a.no_money != 0,
            ignored: a.ignored_by_ai != 0,
            player_only: a.hunts_player_only != 0,
            no_random: a.no_random_targets != 0,
            no_talk: a.no_socialising != 0,
            no_buildings: a.no_building_interest != 0,
            start_troops: troops.to_vec(),
        }
    }
}

/// What an AI army is up to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Goal {
    #[default]
    Idle,
    /// A patrol leg to this cell.
    Wander(Tile),
    /// Back to its post.
    Home,
    AttackPlayer,
    /// Go to the player to meet him: a friendly army that hunts only the player (a
    /// messenger), whose meeting the scenario's events wait for.
    MeetPlayer,
    /// An army, by [`Army::uid`].
    AttackArmy(u32),
    /// Take a castle or fort (index into `locations`).
    Capture(usize),
    Heal(usize),
    /// Fill the garrison of its own building.
    Garrison(usize),
    Hire(usize),
    Shop(usize),
    Village(usize),
    /// Visit a friendly army.
    Talk(u32),
}

impl Goal {
    pub fn building(self) -> Option<usize> {
        match self {
            Goal::Capture(l) | Goal::Heal(l) | Goal::Garrison(l) | Goal::Hire(l) | Goal::Shop(l) | Goal::Village(l) => Some(l),
            _ => None,
        }
    }

    pub fn army(self) -> Option<u32> {
        match self {
            Goal::AttackArmy(u) | Goal::Talk(u) => Some(u),
            _ => None,
        }
    }

    fn same_kind(self, other: Goal) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }
}

/// An AI army's current goal and bookkeeping.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AiMind {
    pub goal: Goal,
    /// Game minute of the next goal choice.
    pub think_at: f64,
    /// Noons in a row it could not pay its wages.
    pub unpaid_days: u32,
    /// No talk before this minute.
    pub talk_after: f64,
    /// No AI battle before this minute (after a stalemate) with army `truce_with` (uid),
    /// nor with its buildings.
    pub truce_until: f64,
    pub truce_with: u32,
    /// Goals it could not reach, and until when they are left alone.
    pub blocked: Vec<(Goal, f64)>,
}

/// A beaten army waiting to come back.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Respawn {
    /// Game minute it comes back.
    pub due: f64,
    pub army: Army,
    /// A lord recovering in one of his buildings (else an ordinary respawn).
    pub lord: bool,
}

/// A battle between AI armies (or an AI army and a garrison), for the player's log.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AiNews {
    pub text: String,
    pub tile: Tile,
    /// Game minute.
    pub at: u64,
}

/// Counts for the simulation reports and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AiStats {
    pub battles: u32,
    pub captures: u32,
    pub respawns: u32,
    pub retreats: u32,
    pub hired: u32,
    pub bought: u32,
    pub paths: u32,
}

/// The AI steers this army: a scenario army on land. Ships and the demo's gangs keep the
/// simple rules of `Game::move_armies`.
pub fn managed(a: &Army) -> bool {
    a.ai.enabled && !a.sails()
}

/// Priorities of one target model (`_Global.ini`, mechanics.md 5.6): lower is more urgent.
/// Pairs are (Min, Max): the value runs from Max at no need to Min at full need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Priorities {
    pub attack_army: i32,
    pub attack_castle: i32,
    pub random: i32,
    pub talk: i32,
    pub heal: (i32, i32),
    pub garrison: (i32, i32),
    pub purchase: (i32, i32),
    pub gold_purchase: i32,
    pub village: (i32, i32),
    pub gold_village: i32,
}

/// Razdor's own priorities for content without `_Global.ini` (the demo), on the scale of the
/// additive scoring (a path cost of 10 per grass cell): attacks are cheap, wandering dear.
const DEMO_PRIORITIES: Priorities = Priorities {
    attack_army: 2,
    attack_castle: 2,
    random: 450,
    talk: 350,
    heal: (40, 120),
    garrison: (100, 450),
    purchase: (120, 300),
    gold_purchase: 400,
    village: (20, 200),
    gold_village: 100,
};

impl Priorities {
    /// The priorities of target model `m`. A key missing from a file that has the others
    /// reads as 0, as the exe reads a missing key; so the shipped file's misspelt
    /// `MixHealingTarget` leaves the healing minimum at 0 *(guess)*.
    pub fn of(o: &GlobalOptions, m: usize) -> Priorities {
        let t = &o.ai_targets;
        let all = [
            t.min_attack_army,
            t.min_attack_castle,
            t.min_random,
            t.min_talking,
            t.min_healing,
            t.max_healing,
            t.min_garrison,
            t.max_garrison,
            t.min_purchase,
            t.max_purchase,
            t.gold_purchase,
            t.min_village,
            t.max_village,
            t.gold_village,
        ];
        if all.iter().all(Option::is_none) {
            return DEMO_PRIORITIES;
        }
        let m = m.min(model::TRADING);
        let g = |p: Option<[i32; 5]>| p.map_or(0, |v| v[m]);
        Priorities {
            attack_army: g(t.min_attack_army),
            attack_castle: g(t.min_attack_castle),
            random: g(t.min_random),
            talk: g(t.min_talking),
            heal: (g(t.min_healing), g(t.max_healing)),
            garrison: (g(t.min_garrison), g(t.max_garrison)),
            purchase: (g(t.min_purchase), g(t.max_purchase)),
            gold_purchase: g(t.gold_purchase),
            village: (g(t.min_village), g(t.max_village)),
            gold_village: g(t.gold_village),
        }
    }
}

/// From Max at `need` 0 to Min at `need` 1000 (per mille).
fn lerp((min, max): (i32, i32), need: i64) -> i32 {
    let need = need.clamp(0, 1000);
    (max as i64 - (max as i64 - min as i64) * need / 1000) as i32
}

/// How far an army considers other armies and the player as targets, in cells of the
/// original's distance ([`crate::rules::map::Grid::octile`]): `AIDistance0..2` indexed by its
/// behaviour style (world.md §4: feudal 100, rogue 50, peasant 25 with the shipped values).
pub fn target_range(o: &GlobalOptions, style: Style) -> i32 {
    let band = match style {
        Style::Feudal => 0,
        Style::Rogue => 1,
        Style::Peasant => 2,
    };
    o.ai_distance[band].max(0)
}

/// Cell `t` lies inside army `a`'s patrol box (`post ± radius`, world.md §4); an army that
/// does not patrol takes targets anywhere.
pub fn in_patrol(a: &Army, t: Tile) -> bool {
    !a.patrols || ((t.0 - a.post.0).abs() <= a.patrol_radius && (t.1 - a.post.1).abs() <= a.patrol_radius)
}

/// Attitude of army `a` towards faction `f` (1 player, 2 ally, 3 neighbour, 4 enemy).
pub fn relation(a: &Army, f: u8) -> i8 {
    match f {
        1 => a.attitude,
        2..=4 => a.ai.relations[f as usize - 1],
        _ => 0,
    }
}

/// Army `a` would attack army `b` (another faction it is ill-disposed towards).
pub fn hostile_to(a: &Army, b: &Army) -> bool {
    a.faction != b.faction && relation(a, b.faction) < 0
}

/// Army `a` is ill-disposed towards building `l` (it would take it).
pub fn hostile_to_location(a: &Army, l: &Location) -> bool {
    match l.owner {
        Owner::Army(id) if id == a.id => false,
        Owner::Player => a.hostile(),
        _ => l.faction != a.faction && relation(a, l.faction) < 0,
    }
}

/// Army `a` is welcome in building `l`: its own, or of a faction it is not ill-disposed
/// towards (never the player's).
pub fn welcome_at(a: &Army, l: &Location) -> bool {
    match l.owner {
        Owner::Army(id) if id == a.id => true,
        Owner::Player => false,
        _ => l.faction == a.faction || relation(a, l.faction) >= 0,
    }
}

/// Army `a` may not walk through building `l`, as the hero may not through his enemies'
/// ([`Location::bars_hero`]): castles and forts that are not its own or a friend's, ruins
/// not its own and not cleared, and any other building it is ill-disposed towards.
pub fn bars_army(a: &Army, l: &Location) -> bool {
    let own = l.owner == Owner::Army(a.id);
    let friend = own
        || match l.owner {
            Owner::Player => a.attitude > 0,
            _ => l.faction == a.faction || relation(a, l.faction) > 0,
        };
    match l.kind {
        LocationKind::Castle | LocationKind::Fort => !friend,
        LocationKind::Ruins => !own && !l.cleared,
        k if k.is_bridge() => false,
        _ => hostile_to_location(a, l),
    }
}

/// Army `a`'s route from `from` to `to`, around the buildings it may not walk through
/// ([`bars_army`]); the ones it stands in and heads for stay open.
pub fn army_path(world: &World, a: &Army, from: Tile, to: Tile, max_nodes: usize) -> Vec<Tile> {
    let (start, end) = (world.location_covering(from), world.location_covering(to));
    world.map.path_where(from, to, max_nodes, &|t| match world.location_covering(t) {
        Some(l) if Some(l) != start && Some(l) != end => !bars_army(a, &world.locations[l]),
        _ => true,
    })
}

/// Kinds that hire for the AI: those that hire for the player, villages and altars
/// (mechanics.md 5.3).
pub fn hires_for_ai(kind: LocationKind) -> bool {
    use LocationKind::*;
    matches!(kind, Palace | Town | Castle | Fort | Church | Village | Altar)
}

/// Maximum HP of a troop without items.
pub fn troop_max_hp(c: &Content, t: &Troop) -> i32 {
    Stats::of_level(c, t.unit, t.level.max(1)).max_hp().max(1)
}

/// Strength of a troop: its tactical cost times its share of hit points left.
pub fn troop_strength(c: &Content, t: &Troop) -> i64 {
    let max = troop_max_hp(c, t) as i64;
    c.tactical_cost(t.unit, t.level) as i64 * (max - t.hurt as i64).max(0) / max
}

pub fn army_strength(c: &Content, a: &Army) -> i64 {
    a.troops.iter().map(|t| troop_strength(c, t)).sum()
}

/// Hit points left over the maximum, per mille.
pub fn health(c: &Content, a: &Army) -> i64 {
    let max: i64 = a.troops.iter().map(|t| troop_max_hp(c, t) as i64).sum();
    let hurt: i64 = a.troops.iter().map(|t| t.hurt.max(0) as i64).sum();
    if max == 0 {
        1000
    } else {
        (max - hurt).max(0) * 1000 / max
    }
}

/// Strength of a building's defenders (its garrison, the player's units left there),
/// with its extra defence *(guess: +2% per point)*.
pub fn defenders_strength(c: &Content, l: &Location) -> i64 {
    let troops: i64 = l.garrison.iter().map(|t| troop_strength(c, t)).sum();
    let units: i64 = l.stationed.iter().filter(|s| s.unit.alive()).map(|s| c.tactical_cost(s.unit.def, s.unit.level) as i64 * s.unit.hp as i64 / s.unit.max_hp(c).max(1) as i64).sum();
    (troops + units) * (100 + 2 * l.garrison_defence.max(0) as i64) / 100
}

/// Hit points of both sides before and after a simulated battle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimResult {
    pub own: i64,
    pub own_left: i64,
    pub theirs: i64,
    pub theirs_left: i64,
}

/// Plays a battle between `mine` (attacking) and `theirs` (with a building's extra `defence`)
/// with the battle engine on both sides, as the original scores its targets (world.md §5).
pub fn simulate(c: &Arc<Content>, mine: &[Unit], theirs: &[Unit], defence: i32) -> SimResult {
    let side: Vec<(usize, &Unit)> = mine.iter().enumerate().map(|(k, u)| (k + 1, u)).collect();
    let mut b = Battle::new(c.clone(), &side, theirs, Team::Player);
    // The same engine as the off-screen battle (4a0710): AI mode 0, no Splash, both sides
    // auto-arranged. Its pre-simulation only predicts the XP, unused here, so it is not run.
    b.set_simulation();
    b.skip_prediction();
    if defence > 0 {
        b.set_building_defence(Team::Enemy, defence);
    }
    b.auto_arrange(Team::Player);
    b.auto_arrange(Team::Enemy);
    b.begin();
    let mut steps = 0;
    while b.outcome() == Outcome::Ongoing && steps < MAX_BATTLE_STEPS {
        b.ai_step();
        steps += 1;
    }
    let na = mine.len().min(b.fighters.len());
    let left = |fs: &[super::battle::Fighter]| -> i64 { fs.iter().map(|f| f.hp.max(0) as i64).sum() };
    let hp = |us: &[Unit]| -> i64 { us.iter().map(|u| u.hp.max(0) as i64).sum() };
    SimResult { own: hp(mine), own_left: left(&b.fighters[..na]), theirs: hp(theirs), theirs_left: left(&b.fighters[na..]) }
}

/// The seed of a target army, the player or a garrison from a simulated battle (world.md §5):
/// the army's `aggression`% of both sides' hit points moves the result *(the exact use is
/// M: here a win needs `own_left − theirs_left + (own + theirs) × aggression/100 > 0`)*. A
/// win scores `1 + lost share × ZeroDensity·30 × own/theirs` and seeds
/// `(priority + score) × (relation + 4)`; a loss is no target (the original's repulsion field
/// around a danger is not modelled).
pub fn battle_seed(s: SimResult, priority: i32, relation: i8, zero_density: i32, aggression: i32) -> Option<i64> {
    if s.own <= 0 || s.theirs <= 0 {
        return None;
    }
    let margin = (s.own_left - s.theirs_left) * 100 + (s.own + s.theirs) * aggression as i64;
    if margin <= 0 {
        return None;
    }
    let lost = (s.own - s.own_left).max(0) as f64 / s.own as f64;
    let score = 1.0 + lost * (zero_density.max(0) * 30) as f64 * s.own as f64 / s.theirs as f64;
    let factor = (relation as i32 + 4).max(1) as f64;
    Some((((priority.max(0) as f64 + score) * factor).round() as i64).min(MAX_SEED))
}

/// Simulated battles of the day, by (attacker uid, target key), so an army does not replay
/// the same battle at every thought.
#[derive(Clone, Debug, Default)]
pub struct Sims {
    day: u64,
    results: HashMap<(u32, u64), SimResult>,
}

/// Key of the player as a simulated target.
const SIM_PLAYER: u64 = u64::MAX;
/// Keys of garrisons: this plus the building's index.
const SIM_GARRISON: u64 = 1 << 40;

impl Sims {
    fn get(&mut self, day: u64, key: (u32, u64), run: impl FnOnce() -> SimResult) -> SimResult {
        if self.day != day {
            self.day = day;
            self.results.clear();
        }
        *self.results.entry(key).or_insert_with(run)
    }
}

/// Daily wages of an AI army: every troop but the leader, by its cost and hiring kind
/// (mechanics.md 1.5); units paid in mana are not paid.
pub fn army_wages(c: &Content, troops: &[Troop]) -> i32 {
    troops.iter().skip(1).filter(|t| !c.paid_in_mana(t.unit)).map(|t| c.wage_for(t.unit, WageKind::of(c.unit(t.unit)))).sum()
}

/// Gold a feudal lord keeps: `NeedUpkeepDay` days of wages (of `troops`).
pub fn reserve(c: &Content, a: &Army, troops: &[Troop]) -> i32 {
    match a.ai.style {
        Style::Feudal => c.options.need_upkeep_day.max(0) * army_wages(c, troops),
        _ => 0,
    }
}

/// The units an army fights with: its troops, and its items worn by the first unit that
/// can wear each *(guess: the original's use of an AI army's items is not decoded)*.
pub fn army_units(c: &Content, a: &Army) -> Vec<Unit> {
    let mut units: Vec<Unit> = a.troops.iter().map(|t| troop_unit(c, t)).collect();
    for &item in &a.items {
        for u in units.iter_mut() {
            if let Ok(s) = items::slot_for(c, u, item) {
                items::put_on(c, u, s, item);
                break;
            }
        }
    }
    units
}

/// The best recruit an army can hire at `l` now: affordable within its reserve, in stock, a
/// rogue unit for rogues, not paid in mana; the strongest first.
fn best_recruit(c: &Content, a: &Army, l: &Location, troops: &[Troop]) -> Option<(UnitId, i32)> {
    let mut best: Option<(UnitId, i32, i32)> = None;
    for r in &l.recruits {
        if r.stock == Some(0) || c.try_unit(r.unit).is_none() || c.paid_in_mana(r.unit) {
            continue;
        }
        let def = c.unit(r.unit);
        if a.ai.style == Style::Rogue && def.nature != Nature::Rogue {
            continue;
        }
        let cost = def.cost.max(0);
        let mut after = troops.to_vec();
        after.push(Troop::new(r.unit, 1, super::formation::Slot::new(super::formation::Row::Front, 0)));
        if a.gold - cost < reserve(c, a, &after) {
            continue;
        }
        let power = c.tactical_cost(r.unit, 1);
        if best.is_none_or(|(_, _, p)| power > p) {
            best = Some((r.unit, cost, power));
        }
    }
    best.map(|(u, cost, _)| (u, cost))
}

/// Free formation cell for a new unit next to `troops`: the first from the reserve forward,
/// whatever the unit (495ce0).
fn free_slot(c: &Content, troops: &[Troop]) -> Option<super::formation::Slot> {
    if troops.len() >= c.formation.capacity() {
        return None;
    }
    let taken: Vec<_> = troops.iter().map(|t| t.slot).collect();
    c.formation.new_unit_slot(&taken)
}

/// The dearest item of the shop at `l` the army can afford within its reserve and some unit
/// of it can wear. Price: the item's cost.
fn best_buy(c: &Content, a: &Army, l: &Location) -> Option<(usize, i32)> {
    let shop = l.shop.as_ref()?;
    if a.items.len() >= MAX_ARMY_ITEMS {
        return None;
    }
    let budget = a.gold - reserve(c, a, &a.troops);
    let units = army_units(c, a);
    let mut best: Option<(usize, i32)> = None;
    for (k, &item) in shop.stock.iter().enumerate() {
        let Some(def) = c.try_item(item) else { continue };
        let cost = def.cost.max(0);
        if cost > budget || best.is_some_and(|(_, b)| b >= cost) {
            continue;
        }
        if units.iter().any(|u| items::slot_for(c, u, item).is_ok()) {
            best = Some((k, cost));
        }
    }
    best
}

/// Candidate goals of one army, each with its seed and its cells.
struct Seeds<'a> {
    list: Vec<(i64, Goal, Vec<Tile>)>,
    blocked: &'a [(Goal, f64)],
    now: f64,
}

impl Seeds<'_> {
    fn offer(&mut self, seed: i64, goal: Goal, cells: Vec<Tile>) {
        if self.blocked.iter().any(|&(g, until)| g == goal && self.now < until) || cells.is_empty() {
            return;
        }
        self.list.push((seed.clamp(0, MAX_SEED), goal, cells));
    }
}

/// What army `i` wants now, and the way there (world.md §5). `hero`: the player's cell (on
/// land) and his army, if he can be attacked; `wander`: random points of its patrol box to
/// consider. Every candidate is seeded with its priority; one flood from the army over the
/// map (reaching a cell costs `cost × weight` of the cell left, as the original's flood from
/// the targets does) finds the lowest `seed + path cost`, which wins. Returns the goal and the
/// path to the cell of it the flood reached (empty if it stands there, or the goal is idle).
#[allow(clippy::too_many_arguments)]
pub fn choose(w: &World, c: &Arc<Content>, i: usize, hero: Option<(Tile, &[Unit])>, now: f64, day: u64, wander: &[Tile], sims: &mut Sims) -> (Goal, Vec<Tile>) {
    let a = &w.armies[i];
    let p = &a.ai;
    let map = &w.map;
    let g = map.grid;
    let here = a.tile(map);
    let pr = Priorities::of(&c.options, p.model);
    let range = target_range(&c.options, p.style);
    let zd = c.options.zero_density;
    let mut seeds = Seeds { list: Vec::new(), blocked: &a.mind.blocked, now };
    let reach = |t: Tile| w.same_region(here, t);
    let in_view = |t: Tile| g.octile(here, t) <= range && in_patrol(a, t);
    let mut mine: Option<Vec<Unit>> = None;
    let mut units = |a: &Army| mine.get_or_insert_with(|| army_units(c, a)).clone();

    // The player (every style: peasants too hunt him when ill-disposed).
    if let Some((h, squad)) = hero {
        if a.hostile() && now >= a.ignore_until && in_view(h) && reach(h) {
            let own = units(a);
            let s = sims.get(day, (a.uid, SIM_PLAYER), || simulate(c, &own, squad, 0));
            if let Some(seed) = battle_seed(s, pr.attack_army, a.attitude, zd, p.aggression) {
                seeds.offer(seed, Goal::AttackPlayer, vec![h]);
            }
        }
        // A friendly army that hunts only the player has nothing else to go for: it comes to
        // meet him (the scenarios' messengers), wherever its post is.
        if !a.hostile() && p.player_only && g.octile(here, h) <= range && reach(h) {
            seeds.offer(pr.talk as i64, Goal::MeetPlayer, vec![h]);
        }
    }
    if p.style != Style::Peasant {
        // Hostile armies in range.
        if !p.player_only {
            for b in w.armies.iter().filter(|b| b.uid != a.uid && managed(b) && !b.ai.ignored && hostile_to(a, b)) {
                let bt = b.tile(map);
                if !in_view(bt) || !reach(bt) || now < a.mind.truce_until || now < b.mind.truce_until {
                    continue;
                }
                let own = units(a);
                let s = sims.get(day, (a.uid, b.uid as u64), || simulate(c, &own, &army_units(c, b), 0));
                if let Some(seed) = battle_seed(s, pr.attack_army, relation(a, b.faction), zd, p.aggression) {
                    seeds.offer(seed, Goal::AttackArmy(b.uid), vec![bt]);
                }
            }
        }
        // Buildings: inside its patrol box (anywhere when it does not patrol), a lost home
        // anywhere.
        if !p.no_buildings {
            offer_buildings(&mut seeds, w, c, a, &pr, day, sims, &mut units);
        }
        // A friendly army of its own faction in range.
        if !p.no_talk && now >= a.mind.talk_after {
            for b in w.armies.iter().filter(|b| b.uid != a.uid && managed(b) && b.faction == a.faction) {
                let bt = b.tile(map);
                if in_view(bt) && reach(bt) && !hostile_to(b, a) {
                    seeds.offer(pr.talk as i64, Goal::Talk(b.uid), vec![bt]);
                }
            }
        }
    }
    // Wandering its patrol (unless it takes no random targets), or going back to its post
    // when it is outside its patrol box. The original also seeds the wander points of an
    // army that does not patrol (ai.md §7.2); Razdor's goal scoring does not yet.
    let box_radius = if a.patrols { a.patrol_radius } else { 0 };
    let away = g.distance(here, a.post) > box_radius + 1;
    if away && w.same_region(here, a.post) {
        seeds.offer(pr.random as i64, Goal::Home, vec![a.post]);
    } else if a.patrols && a.patrol_radius > 0 && !p.no_random {
        for &t in wander {
            seeds.offer(pr.random as i64, Goal::Wander(t), vec![t]);
        }
    }
    flood(w, here, &seeds.list)
}

/// Seeds the buildings army `a` might go to.
#[allow(clippy::too_many_arguments)]
fn offer_buildings(seeds: &mut Seeds, w: &World, c: &Arc<Content>, a: &Army, pr: &Priorities, day: u64, sims: &mut Sims, units: &mut dyn FnMut(&Army) -> Vec<Unit>) {
    let p = &a.ai;
    let here = a.tile(&w.map);
    let now = seeds.now;
    let hp = health(c, a);
    let free = c.formation.capacity().saturating_sub(a.troops.len()) as i64;
    let cap = c.formation.capacity().max(1) as i64;
    let mine = army_strength(c, a);
    for (l, loc) in w.locations.iter().enumerate() {
        if loc.kind.is_bridge() {
            continue;
        }
        let home_lost = p.style == Style::Rogue && a.home == Some(l) && loc.kind.capturable() && loc.owner != Owner::Army(a.id);
        if (!in_patrol(a, loc.tile) && !home_lost) || !w.same_region(here, loc.tile) {
            continue;
        }
        let cells = || -> Vec<Tile> { loc.cells().filter(|&t| w.location_at(t) == Some(l)).collect() };
        let truce = now < a.mind.truce_until && loc.owner == Owner::Army(a.mind.truce_with.min(u8::MAX as u32) as u8);
        let own = loc.owner == Owner::Army(a.id);
        if loc.kind.capturable() && !p.player_only && (hostile_to_location(a, loc) || home_lost) {
            // A leader alone does not storm walls, except his lost home *(guess)*.
            let force = a.troops.len() > 1 || home_lost;
            if truce || !force {
                continue;
            }
            let defenders: Vec<Unit> = loc.garrison.iter().map(|t| troop_unit(c, t)).chain(loc.stationed.iter().filter(|s| s.unit.alive()).map(|s| s.unit.clone())).collect();
            let wins = defenders.is_empty() || {
                let own_units = units(a);
                let s = sims.get(day, (a.uid, SIM_GARRISON + l as u64), || simulate(c, &own_units, &defenders, loc.garrison_defence));
                battle_seed(s, 0, -1, 0, p.aggression).is_some()
            };
            if wins {
                let seed = pr.attack_castle as i64 * CASTLE_FACTOR;
                seeds.offer(if home_lost { seed / 2 } else { seed }, Goal::Capture(l), cells());
            }
            continue;
        }
        if !welcome_at(a, loc) {
            continue;
        }
        // Healing: in its own castle or fort, or (feudal) any friendly healer.
        let heals_here = (own && loc.kind.capturable()) || (p.style == Style::Feudal && loc.heals());
        if hp < HEAL_BELOW && heals_here {
            seeds.offer(lerp(pr.heal, 1000 - hp) as i64, Goal::Heal(l), cells());
        }
        if own && loc.kind.capturable() && p.style == Style::Feudal && !loc.recruits.is_empty() {
            let target = mine * p.garrison_strength as i64 / 100;
            let have = defenders_strength(c, loc);
            if have < target && loc.garrison.len() < c.formation.capacity() && best_recruit(c, a, loc, &a.troops).is_some() {
                seeds.offer(lerp(pr.garrison, (target - have) * 1000 / target.max(1)) as i64, Goal::Garrison(l), cells());
            }
        }
        if free > 0 && hires_for_ai(loc.kind) && best_recruit(c, a, loc, &a.troops).is_some() {
            seeds.offer(lerp(pr.purchase, free * 1000 / cap) as i64, Goal::Hire(l), cells());
        }
        if p.style == Style::Feudal && best_buy(c, a, loc).is_some() {
            let spare = (a.gold - reserve(c, a, &a.troops)) as i64;
            seeds.offer(lerp(pr.purchase, spare * 1000 / pr.gold_purchase.max(1) as i64) as i64, Goal::Shop(l), cells());
        }
        let tribute_ours = own || loc.faction == a.faction || loc.linked.is_some_and(|k| w.locations[k].owner == Owner::Army(a.id));
        if p.style == Style::Feudal && loc.kind == LocationKind::Village && loc.tribute_gold > 0 && tribute_ours {
            seeds.offer(lerp(pr.village, loc.tribute_gold as i64 * 1000 / pr.gold_village.max(1) as i64) as i64, Goal::Village(l), cells());
        }
    }
}

/// One flood from `from` over the foot map: the seed with the lowest `seed + path cost`
/// wins; the path is the flood's way to the winning cell (world.md §5, 0x482a58).
fn flood(w: &World, from: Tile, seeds: &[(i64, Goal, Vec<Tile>)]) -> (Goal, Vec<Tile>) {
    let map = &w.map;
    let g = map.grid;
    let Some(start) = map.mask_index(from) else { return (Goal::Idle, Vec::new()) };
    let mut at: HashMap<usize, (i64, usize)> = HashMap::new();
    for (k, (s, _, cells)) in seeds.iter().enumerate() {
        for &t in cells {
            let Some(j) = map.mask_index(t) else { continue };
            let e = at.entry(j).or_insert((*s, k));
            if *s < e.0 {
                *e = (*s, k);
            }
        }
    }
    let Some(min_seed) = at.values().map(|v| v.0).min() else { return (Goal::Idle, Vec::new()) };
    let n = (map.w * map.h) as usize;
    let mut dist = vec![u32::MAX; n];
    let mut parent = vec![u32::MAX; n];
    dist[start] = 0;
    let mut open = BinaryHeap::from([Reverse((0u32, start as u32))]);
    let mut best: Option<(i64, usize, usize)> = None;
    let mut expanded = 0;
    let tile = |i: usize| (i as i32 % map.w, i as i32 / map.w);
    while let Some(Reverse((d, i))) = open.pop() {
        let i = i as usize;
        if d > dist[i] {
            continue;
        }
        if best.is_some_and(|(b, _, _)| d as i64 + min_seed >= b) {
            break;
        }
        if let Some(&(s, k)) = at.get(&i) {
            if best.is_none_or(|(b, _, _)| s + (d as i64) < b) {
                best = Some((s + d as i64, k, i));
            }
        }
        expanded += 1;
        if expanded > FLOOD_NODES {
            break;
        }
        let here = tile(i);
        let leave = map.cost(here).unwrap_or(super::map::ROAD) as u32;
        for nb in g.neighbours(here) {
            let Some(j) = map.mask_index(nb) else { continue };
            if !map.passable(nb) {
                continue;
            }
            let nd = d + leave * g.weight(here, nb);
            if nd < dist[j] {
                dist[j] = nd;
                parent[j] = i as u32;
                open.push(Reverse((nd, j as u32)));
            }
        }
    }
    let Some((_, k, cell)) = best else { return (Goal::Idle, Vec::new()) };
    let mut path = Vec::new();
    let mut cur = cell;
    while cur != start {
        path.push(tile(cur));
        cur = parent[cur] as usize;
    }
    path.reverse();
    (seeds[k].1, path)
}

/// Who beat an army.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beaten {
    ByPlayer,
    ByAi,
}

/// The other side of an AI battle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Defender {
    Army(usize),
    Garrison(usize),
}

impl Game {
    /// Anyone (the player or an AI army) beat this scenario army.
    pub fn army_beaten_by_anyone(&self, id: ArmyId) -> bool {
        self.beaten_armies.contains(&id) || self.ai_beaten.contains(&id)
    }

    fn army_by_uid(&self, uid: u32) -> Option<usize> {
        self.world.armies.iter().position(|a| a.uid == uid)
    }

    /// The pending foe is army `i` (a battle with the player is about to start).
    fn is_foe(&self, i: usize) -> bool {
        self.foe == Some(Foe::Army(i))
    }

    /// Removes army `i` from the map, keeping the pending foe pointing at the right army.
    fn remove_army(&mut self, i: usize) -> Army {
        match self.foe {
            Some(Foe::Army(j)) if j == i => self.foe = None,
            Some(Foe::Army(j)) if j > i => self.foe = Some(Foe::Army(j - 1)),
            _ => {}
        }
        self.world.armies.remove(i)
    }

    /// The player's cell, if AI armies on land can go for him (not while he sails).
    fn hero_target(&self) -> Option<Tile> {
        (!self.aboard()).then(|| self.tile())
    }

    // ------------------------------------------------------------------------------------
    // Brain and routes
    // ------------------------------------------------------------------------------------

    /// Before the armies walk a slice: respawns due, then goals and routes of the armies the
    /// AI steers.
    pub(crate) fn ai_plan(&mut self) {
        let now = self.clock.total_minutes();
        self.ai_respawns(now);
        let hero = self.hero_target();
        let mut budget = MAX_PATHS_PER_SLICE;
        for i in 0..self.world.armies.len() {
            if !managed(&self.world.armies[i]) || self.is_foe(i) {
                continue;
            }
            if self.needs_thought(i, hero, now) {
                let (goal, path) = self.ai_choice(i);
                self.set_goal(i, goal, path, now);
            }
            self.route(i, hero, now, &mut budget);
        }
    }

    /// What army `i` would choose now ([`choose`]), with random wander points of its patrol
    /// box and the day's simulated battles.
    pub(crate) fn ai_choice(&mut self, i: usize) -> (Goal, Vec<Tile>) {
        let now = self.clock.total_minutes();
        let day = self.clock.day_index();
        let wander = self.wander_points(i);
        let hero = self.hero_target();
        // The player defends there: all his living units fight, the unpaid ones too.
        let squad: Vec<Unit> = self.squad.iter().enumerate().filter(|(k, u)| *k == 0 || u.alive()).map(|(_, u)| u.clone()).collect();
        let mut sims = std::mem::take(&mut self.sims);
        let out = choose(&self.world, &self.content, i, hero.map(|h| (h, squad.as_slice())), now, day, &wander, &mut sims);
        self.sims = sims;
        out
    }

    fn needs_thought(&self, i: usize, hero: Option<Tile>, now: f64) -> bool {
        let a = &self.world.armies[i];
        if now >= a.mind.think_at {
            return true;
        }
        let map = &self.world.map;
        let range = target_range(&self.content.options, a.ai.style);
        let sees_hero = hero.is_some_and(|h| map.grid.octile(a.tile(map), h) <= range && in_patrol(a, h));
        let near_hero = hero.is_some_and(|h| map.grid.octile(a.tile(map), h) <= range);
        let messenger = !a.hostile() && a.ai.player_only;
        match a.mind.goal {
            // The player went out of range (or a truce began).
            Goal::AttackPlayer => !sees_hero || now < a.ignore_until,
            Goal::MeetPlayer => !near_hero,
            // A messenger comes in range of the player.
            _ if messenger => near_hero,
            // A hostile army spots the player.
            _ => a.hostile() && now >= a.ignore_until && sees_hero,
        }
    }

    fn set_goal(&mut self, i: usize, goal: Goal, path: Vec<Tile>, now: f64) {
        let offset = (self.world.armies[i].uid % 6) as f64 * 5.0;
        let same = {
            let a = &self.world.armies[i];
            a.mind.goal == goal || (matches!(goal, Goal::Wander(_)) && goal.same_kind(a.mind.goal) && !a.path.is_empty())
        };
        if same {
            self.world.armies[i].mind.think_at = now + THINK_MINUTES + offset;
            return;
        }
        if !path.is_empty() {
            self.ai_stats.paths += 1;
        }
        let a = &mut self.world.armies[i];
        a.mind.goal = goal;
        a.path = path;
        a.chasing = goal == Goal::AttackPlayer;
        a.mind.think_at = now + THINK_MINUTES + offset;
        if goal == Goal::Idle {
            // Nothing to do: rest a while *(guess: ZeroDensity × 6..36 minutes, 30..180
            // with the shipped 5)*.
            let z = self.content.options.zero_density.max(1);
            let rest = self.rng.range(6 * z, 36 * z) as f64;
            self.world.armies[i].mind.think_at = now + rest;
        }
    }

    /// The four wander points of army `i` (ai.md §7.2, engine.md §3.4, 0x4a2550): x then y
    /// for each, inside its patrol box (its post ± radius, clamped to the map) when it
    /// patrols, anywhere on the map when it does not. There is no passability test. A point
    /// on its own cell is dropped, and so is one in column 0: the original's user skips every
    /// point whose x is not above 0 (kept). A stationary guard never plans, so never draws.
    fn wander_points(&mut self, i: usize) -> Vec<Tile> {
        let (w, h) = (self.world.map.w, self.world.map.h);
        let (post, r, here, patrols) = {
            let a = &self.world.armies[i];
            (a.post, a.patrol_radius, a.tile(&self.world.map), a.patrols)
        };
        if patrols && r <= 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for _ in 0..WANDER_POINTS {
            let t = if patrols {
                let (x0, x1) = ((post.0 - r).max(0), (post.0 + r).min(w - 1));
                let (y0, y1) = ((post.1 - r).max(0), (post.1 + r).min(h - 1));
                let x = x0 + self.rng.random(x1 + 1 - x0);
                (x, y0 + self.rng.random(y1 + 1 - y0))
            } else {
                let x = self.rng.random(w);
                (x, self.rng.random(h))
            };
            if t != here && t.0 > 0 {
                out.push(t);
            }
        }
        out
    }

    /// The cell army `i` heads for: the building's centre, the other army's cell, the leg's
    /// end, its post. `None`: the goal is gone.
    fn goal_cell(&self, i: usize, hero: Option<Tile>) -> Option<Tile> {
        let a = &self.world.armies[i];
        match a.mind.goal {
            Goal::Idle => None,
            Goal::Wander(t) => Some(t),
            Goal::Home => Some(a.post),
            Goal::AttackPlayer | Goal::MeetPlayer => hero,
            Goal::AttackArmy(u) | Goal::Talk(u) => self.army_by_uid(u).map(|j| self.world.armies[j].tile(&self.world.map)),
            g => g.building().map(|l| self.world.locations[l].tile),
        }
    }

    /// Army `i` is where its goal is: next to the army it goes for, in the building (any of
    /// its cells), on the cell.
    fn at_goal(&self, i: usize, t: Tile, target: Tile) -> bool {
        let goal = self.world.armies[i].mind.goal;
        let map = &self.world.map;
        if goal.army().is_some() || matches!(goal, Goal::AttackPlayer | Goal::MeetPlayer) {
            map.distance(t, target) <= CONTACT
        } else if let Some(l) = goal.building() {
            self.world.location_at(t) == Some(l)
        } else {
            t == target
        }
    }

    /// Plans army `i`'s route to its goal when its route does not end there.
    fn route(&mut self, i: usize, hero: Option<Tile>, now: f64, budget: &mut usize) {
        let goal = self.world.armies[i].mind.goal;
        if goal == Goal::Idle {
            return;
        }
        let Some(target) = self.goal_cell(i, hero) else {
            self.drop_goal(i, now, false);
            return;
        };
        let (here, end) = {
            let a = &self.world.armies[i];
            (a.tile(&self.world.map), a.path.last().copied())
        };
        if self.at_goal(i, here, target) {
            return;
        }
        let chases = goal.army().is_some() || matches!(goal, Goal::AttackPlayer | Goal::MeetPlayer);
        let fresh = match end {
            Some(e) if chases => self.world.map.distance(e, target) <= 1,
            Some(e) => self.at_goal(i, e, target),
            None => false,
        };
        if fresh {
            return;
        }
        let nodes = if matches!(goal, Goal::AttackPlayer | Goal::MeetPlayer) {
            CHASE_PATH_NODES
        } else if *budget == 0 {
            return;
        } else {
            *budget -= 1;
            GOAL_PATH_NODES
        };
        self.ai_stats.paths += 1;
        let path = if self.world.same_region(here, target) { army_path(&self.world, &self.world.armies[i], here, target, nodes) } else { Vec::new() };
        if path.is_empty() {
            self.drop_goal(i, now, true);
        } else {
            self.world.armies[i].path = path;
        }
    }

    /// Army `i` leaves `goal` alone for a while (it came to nothing).
    fn block(&mut self, i: usize, goal: Goal) {
        let now = self.clock.total_minutes();
        let m = &mut self.world.armies[i].mind;
        m.blocked.retain(|&(b, until)| b != goal && now < until);
        m.blocked.push((goal, now + BLOCK_MINUTES));
    }

    /// Gives up the goal (blocking it for a while when it could not be reached) and thinks
    /// again at the next slice.
    fn drop_goal(&mut self, i: usize, now: f64, block: bool) {
        let a = &mut self.world.armies[i];
        if block && !matches!(a.mind.goal, Goal::AttackPlayer | Goal::MeetPlayer) {
            let g = a.mind.goal;
            a.mind.blocked.retain(|&(b, until)| b != g && now < until);
            a.mind.blocked.push((g, now + BLOCK_MINUTES));
        }
        a.mind.goal = Goal::Idle;
        a.chasing = false;
        a.path.clear();
        a.mind.think_at = now;
    }

    /// After the armies walked a slice: those that reached their goal act on it, then AI
    /// armies that meet fight.
    pub(crate) fn ai_after_walk(&mut self, events: &mut Vec<Event>) {
        let now = self.clock.total_minutes();
        let hero = self.hero_target();
        let uids: Vec<u32> = self.world.armies.iter().filter(|a| managed(a)).map(|a| a.uid).collect();
        for uid in uids {
            let Some(i) = self.army_by_uid(uid) else { continue };
            if !self.is_foe(i) {
                self.arrive_ai(i, hero, now, events);
            }
        }
        self.ai_contacts(now, events);
    }

    fn arrive_ai(&mut self, i: usize, hero: Option<Tile>, now: f64, events: &mut Vec<Event>) {
        let goal = self.world.armies[i].mind.goal;
        if matches!(goal, Goal::Idle | Goal::AttackPlayer | Goal::MeetPlayer) {
            return;
        }
        let Some(target) = self.goal_cell(i, hero) else { return };
        let here = self.world.armies[i].tile(&self.world.map);
        let d = self.world.map.distance(here, target);
        let rest = |g: &mut Game, i: usize, minutes: f64| {
            let a = &mut g.world.armies[i];
            a.mind.goal = Goal::Idle;
            a.path.clear();
            a.mind.think_at = now + minutes;
        };
        match goal {
            Goal::AttackArmy(u) if d <= CONTACT => {
                if let Some(j) = self.army_by_uid(u) {
                    if hostile_to(&self.world.armies[i], &self.world.armies[j]) && !self.is_foe(j) {
                        self.ai_battle(i, Defender::Army(j), events);
                    } else {
                        rest(self, i, 0.0);
                    }
                }
            }
            Goal::Talk(_) if d <= CONTACT => {
                self.world.armies[i].mind.talk_after = now + TALK_EVERY;
                rest(self, i, TALK_MINUTES);
            }
            Goal::Wander(_) | Goal::Home if d == 0 || self.world.armies[i].path.is_empty() => {
                let z = self.content.options.zero_density.max(1);
                let minutes = self.rng.range(6 * z, 36 * z) as f64;
                rest(self, i, minutes);
            }
            g if g.building().is_some_and(|l| self.world.location_at(here) == Some(l)) => {
                if let Some(l) = g.building() {
                    // It stands at the footprint's centre (world.md §7).
                    let centre = self.world.map.center(self.world.locations[l].tile);
                    self.world.armies[i].pos = centre;
                    let uid = self.world.armies[i].uid;
                    let minutes = self.act_at(i, l, g, events);
                    // A lost battle took it off the map.
                    if let Some(i) = self.army_by_uid(uid) {
                        rest(self, i, minutes);
                    }
                }
            }
            _ => {}
        }
    }

    /// Army `i` reached building `l` for `goal`. Returns the minutes it stays.
    fn act_at(&mut self, i: usize, l: usize, goal: Goal, events: &mut Vec<Event>) -> f64 {
        let c = self.content.clone();
        match goal {
            Goal::Capture(_) => {
                let a = &self.world.armies[i];
                let loc = &self.world.locations[l];
                let home_lost = a.ai.style == Style::Rogue && a.home == Some(l) && loc.owner != Owner::Army(a.id);
                if !(hostile_to_location(a, loc) || home_lost) {
                    return 0.0;
                }
                // Its owner standing at the gate defends it first.
                if let Owner::Army(oid) = loc.owner {
                    let map = &self.world.map;
                    let guard = self.world.armies.iter().position(|b| b.id == oid && b.uid != a.uid && managed(b) && (self.world.location_at(b.tile(map)) == Some(l) || map.distance(b.tile(map), loc.tile) <= CONTACT));
                    if let Some(j) = guard {
                        let now = self.clock.total_minutes();
                        let (a, b) = (&self.world.armies[i], &self.world.armies[j]);
                        if !self.is_foe(j) && now >= a.mind.truce_until && now >= b.mind.truce_until {
                            self.ai_battle(i, Defender::Army(j), events);
                        }
                        return THINK_MINUTES;
                    }
                }
                let loc = &self.world.locations[l];
                if loc.garrison.is_empty() && loc.stationed.iter().all(|s| !s.unit.alive()) {
                    self.take_building(i, l, events);
                } else {
                    self.ai_battle(i, Defender::Garrison(l), events);
                }
                THINK_MINUTES
            }
            Goal::Heal(_) => {
                let own = self.world.locations[l].owner == Owner::Army(self.world.armies[i].id);
                let a = &mut self.world.armies[i];
                let mut healed = 0;
                for t in a.troops.iter_mut().filter(|t| t.hurt > 0) {
                    let max = troop_max_hp(&c, t);
                    // Paid as the player pays (mechanics.md 1.6), free in its own building.
                    let price = if own { 0 } else { (c.unit(t.unit).cost.max(0) * c.options.healing_const * t.hurt + 100 * max - 1) / (100 * max) };
                    if price > a.gold {
                        break;
                    }
                    a.gold -= price;
                    t.hurt = 0;
                    healed += 1;
                }
                if healed == 0 {
                    self.block(i, goal);
                }
                (healed * c.options.healing_time.max(0)) as f64
            }
            Goal::Garrison(_) => {
                let mut hired = 0;
                loop {
                    let a = &self.world.armies[i];
                    let loc = &self.world.locations[l];
                    let target = army_strength(&c, a) * a.ai.garrison_strength as i64 / 100;
                    if defenders_strength(&c, loc) >= target {
                        break;
                    }
                    let Some((unit, cost)) = best_recruit(&c, a, loc, &a.troops) else { break };
                    let Some(slot) = free_slot(&c, &loc.garrison) else { break };
                    self.world.armies[i].gold -= cost;
                    let loc = &mut self.world.locations[l];
                    take_stock(loc, unit);
                    loc.garrison.push(Troop::new(unit, 1, slot));
                    hired += 1;
                }
                self.ai_stats.hired += hired;
                if hired == 0 {
                    self.block(i, goal);
                }
                THINK_MINUTES
            }
            Goal::Hire(_) => {
                if self.ai_hire_at(i, l) == 0 {
                    self.block(i, goal);
                }
                THINK_MINUTES
            }
            Goal::Shop(_) => {
                if let Some((k, cost)) = best_buy(&c, &self.world.armies[i], &self.world.locations[l]) {
                    let shop = self.world.locations[l].shop.as_mut().expect("shop");
                    let item = shop.stock.remove(k);
                    if let Some(f) = shop.fixed.iter().position(|&x| x == item) {
                        shop.fixed.remove(f);
                    }
                    let a = &mut self.world.armies[i];
                    a.gold -= cost;
                    a.items.push(item);
                    self.ai_stats.bought += 1;
                } else {
                    self.block(i, goal);
                }
                THINK_MINUTES
            }
            Goal::Village(_) => {
                let loc = &mut self.world.locations[l];
                let gold = std::mem::take(&mut loc.tribute_gold);
                loc.tribute_mana = 0;
                self.world.armies[i].gold += gold;
                THINK_MINUTES
            }
            _ => 0.0,
        }
    }

    /// Hires at building `l` into army `i` while it can (free cell, stock, gold above its
    /// reserve). Returns how many it hired.
    pub(crate) fn ai_hire_at(&mut self, i: usize, l: usize) -> u32 {
        let c = self.content.clone();
        let mut hired = 0;
        loop {
            let a = &self.world.armies[i];
            let loc = &self.world.locations[l];
            if !hires_for_ai(loc.kind) || !welcome_at(a, loc) {
                break;
            }
            let Some((unit, cost)) = best_recruit(&c, a, loc, &a.troops) else { break };
            let Some(slot) = free_slot(&c, &a.troops) else { break };
            take_stock(&mut self.world.locations[l], unit);
            let mut t = Troop::new(unit, 1, slot);
            let xp = self.hire_xp(i, unit);
            if xp > 0 {
                ai_hire_gain(&c, &mut self.rng, &mut t, xp);
            }
            let a = &mut self.world.armies[i];
            a.gold -= cost;
            a.troops.push(t);
            hired += 1;
        }
        self.ai_stats.hired += hired;
        hired
    }

    /// XP a unit of type `unit` that army `i` hires starts with (experience.md §5): with
    /// "add experience like the player", `P − strength/2`, where P is the player's army's
    /// strength and XP per unit (`Σ(tactical + XP) / (units + 2)`) and `strength` the
    /// recruit's own; plus the army's hire bonus; then a random amount from half of it to
    /// one and a half times it.
    fn hire_xp(&mut self, i: usize, unit: UnitId) -> i32 {
        let c = self.content.clone();
        let p = &self.world.armies[i].ai;
        let mut x = 0i64;
        if p.exp_like_player {
            let sum: i64 = self.squad.iter().map(|u| c.tactical_cost(u.def, u.level) as i64 + u.xp as i64).sum();
            let per = sum / (self.squad.len() as i64 + 2);
            if per > 0 {
                x = per - c.tactical_cost(unit, 1) as i64 / 2;
            }
        }
        x += p.hire_bonus_exp as i64;
        if x <= 0 {
            return 0;
        }
        let x = x.min(i32::MAX as i64 / 2) as i32;
        self.rng.random(x) + x / 2
    }

    /// Army `i` walks into building `l`, which has no defenders: it is its.
    fn take_building(&mut self, i: usize, l: usize, events: &mut Vec<Event>) {
        let a = &self.world.armies[i];
        let (id, faction, attitude, name) = (a.id, a.faction, a.attitude, army_name(a));
        let loc = &mut self.world.locations[l];
        let was_players = loc.owned();
        loc.owner = Owner::Army(id);
        loc.faction = faction;
        loc.attitude = attitude;
        loc.cleared = false;
        loc.stationed.clear();
        let text = crate::trf!("{name} took {place}.", name, place = building_name(loc));
        let tile = loc.tile;
        self.ai_stats.captures += 1;
        self.leave_garrison(i, l);
        self.report(text, tile, was_players, events);
    }

    /// Army `i` leaves troops in the castle or fort it took: its weakest ones, until the
    /// garrison has `garrison_strength`% of what the army keeps; the leader stays with
    /// it *(guess: the original's garrison handling is not decoded)*.
    fn leave_garrison(&mut self, i: usize, l: usize) {
        let c = self.content.clone();
        let cap = c.formation.capacity();
        loop {
            let a = &self.world.armies[i];
            let loc = &self.world.locations[l];
            if a.troops.len() <= 1 || loc.garrison.len() >= cap {
                return;
            }
            let rest: i64 = army_strength(&c, a);
            let Some((k, t)) = a.troops.iter().enumerate().skip(1).min_by_key(|(_, t)| troop_strength(&c, t)) else { return };
            let s = troop_strength(&c, t);
            if defenders_strength(&c, loc) * 100 >= (rest - s) * a.ai.garrison_strength as i64 {
                return;
            }
            let mut t = *t;
            let Some(slot) = free_slot(&c, &loc.garrison) else { return };
            t.slot = slot;
            self.world.armies[i].troops.remove(k);
            self.world.locations[l].garrison.push(t);
        }
    }

    /// Adds a battle report to the log; the player hears of it (an event) when it happened
    /// within his sight, or concerned his own building.
    fn report(&mut self, text: String, tile: Tile, concerns_player: bool, events: &mut Vec<Event>) {
        let news = AiNews { text, tile, at: self.clock.total_minutes() as u64 };
        // Within the hero's sight (a circle in cells, world.md §3).
        let seen = fog::within(self.tile(), tile, self.sight_radius());
        if seen || concerns_player {
            events.push(Event::Battle(news.clone()));
            self.ai_log.push(news);
            if self.ai_log.len() > LOG_KEPT {
                self.ai_log.remove(0);
            }
        }
    }

    /// Mutually hostile AI armies on neighbouring cells fight, one battle at a time.
    fn ai_contacts(&mut self, now: f64, events: &mut Vec<Event>) {
        for _ in 0..16 {
            let Some((x, y)) = self.contact_pair(now) else { return };
            self.ai_battle(x, Defender::Army(y), events);
        }
    }

    /// The first pair (attacker, defender) of AI armies next to each other that fight: one
    /// is going for the other, or both are ill-disposed towards each other and neither is
    /// ignored by the AI or hunts only the player.
    fn contact_pair(&self, now: f64) -> Option<(usize, usize)> {
        let w = &self.world;
        let map = &w.map;
        let n = w.armies.len();
        let tiles: Vec<Tile> = w.armies.iter().map(|a| a.tile(map)).collect();
        let fights = |a: &Army, b: &Army| {
            let wants = a.mind.goal == Goal::AttackArmy(b.uid) && hostile_to(a, b);
            let mutual = hostile_to(a, b) && hostile_to(b, a) && !a.ai.ignored && !b.ai.ignored && !a.ai.player_only && !b.ai.player_only && a.ai.style != Style::Peasant && b.ai.style != Style::Peasant;
            wants || mutual
        };
        for x in 0..n {
            let a = &w.armies[x];
            if !managed(a) || self.is_foe(x) || now < a.mind.truce_until {
                continue;
            }
            for y in 0..n {
                let b = &w.armies[y];
                if x == y || !managed(b) || self.is_foe(y) || now < b.mind.truce_until || map.distance(tiles[x], tiles[y]) > CONTACT {
                    continue;
                }
                if fights(a, b) {
                    return Some((x, y));
                }
            }
        }
        None
    }

    /// Army `att` fights `def` off-screen, the battle engine playing both sides. The loser
    /// is beaten ([`Game::army_beaten`]); a beaten garrison gives up its castle or fort.
    /// Losses, hit points, XP (`AIExpiriencePercent`, the army's experience correction) and
    /// the loot (`VictoryGoldDiv` of the loser's gold and its items) are applied.
    pub fn ai_battle(&mut self, att: usize, def: Defender, events: &mut Vec<Event>) -> Outcome {
        let c = self.content.clone();
        let a_units = army_units(&c, &self.world.armies[att]);
        let (b_units, defence) = match def {
            Defender::Army(j) => (army_units(&c, &self.world.armies[j]), 0),
            Defender::Garrison(l) => {
                let loc = &self.world.locations[l];
                let mut u: Vec<Unit> = loc.garrison.iter().map(|t| troop_unit(&c, t)).collect();
                u.extend(loc.stationed.iter().filter(|s| s.unit.alive()).map(|s| s.unit.clone()));
                (u, loc.garrison_defence)
            }
        };
        let na = a_units.len();
        // Squad indices from 1: no fighter counts as the player's hero.
        let side_a: Vec<(usize, &Unit)> = a_units.iter().enumerate().map(|(k, u)| (k + 1, u)).collect();
        let mut b = Battle::new(c.clone(), &side_a, &b_units, Team::Player);
        b.set_simulation();
        b.apply_spells(Team::Player, &self.spells_on_army(att));
        if let Defender::Army(j) = def {
            b.apply_spells(Team::Enemy, &self.spells_on_army(j));
        }
        if defence > 0 {
            b.set_building_defence(Team::Enemy, defence);
        }
        // Both sides are auto-arranged (4a0710).
        b.auto_arrange(Team::Player);
        b.auto_arrange(Team::Enemy);
        b.begin();
        let mut steps = 0;
        while b.outcome() == Outcome::Ongoing && steps < MAX_BATTLE_STEPS {
            b.ai_step();
            steps += 1;
        }
        let outcome = b.outcome();
        self.ai_stats.battles += 1;
        self.battles += 1;

        // XP: each side that still has strength gains its shares × AIExpiriencePercent;
        // no army correction and no difficulty factor apply between AI armies.
        let mut xp_a = vec![0; na];
        for aw in b.ai_xp(Team::Player) {
            if aw.fighter < na {
                xp_a[aw.fighter] += aw.xp;
            }
        }
        let mut xp_b = vec![0; b_units.len()];
        for aw in b.ai_xp(Team::Enemy) {
            if let Some(k) = aw.fighter.checked_sub(na) {
                if k < xp_b.len() {
                    xp_b[k] += aw.xp;
                }
            }
        }
        let hp_a: Vec<(i32, i32)> = b.fighters[..na].iter().map(|f| (f.hp.max(0), f.max_hp())).collect();
        let hp_b: Vec<(i32, i32)> = b.fighters[na..].iter().map(|f| (f.hp.max(0), f.max_hp())).collect();
        {
            let a = &mut self.world.armies[att];
            write_back(&c, &mut self.rng, &mut a.troops, &hp_a, &xp_a);
        }
        match def {
            Defender::Army(j) => {
                let a = &mut self.world.armies[j];
                write_back(&c, &mut self.rng, &mut a.troops, &hp_b, &xp_b);
            }
            Defender::Garrison(l) => {
                let loc = &mut self.world.locations[l];
                let ng = loc.garrison.len();
                write_back(&c, &mut self.rng, &mut loc.garrison, &hp_b[..ng.min(hp_b.len())], &xp_b[..ng.min(xp_b.len())]);
                let mut k = ng;
                for s in loc.stationed.iter_mut().filter(|s| s.unit.alive()) {
                    if let Some(&(hp, _)) = hp_b.get(k) {
                        s.unit.hp = hp;
                        s.unit.gain_xp(&c, xp_b[k]);
                    }
                    k += 1;
                }
                loc.stationed.retain(|s| s.unit.alive());
            }
        }

        let a_name = army_name(&self.world.armies[att]);
        let a_tile = self.world.armies[att].tile(&self.world.map);
        match (outcome, def) {
            (Outcome::Victory, Defender::Army(j)) => {
                let b_name = army_name(&self.world.armies[j]);
                self.take_loot(att, j);
                self.report(crate::trf!("{winner} defeated {loser}.", winner = a_name, loser = b_name), a_tile, false, events);
                self.army_beaten(j, Beaten::ByAi);
            }
            (Outcome::Victory, Defender::Garrison(l)) => {
                self.world.locations[l].garrison.clear();
                self.take_building(att, l, events);
            }
            (Outcome::Defeat, Defender::Army(j)) => {
                let b_name = army_name(&self.world.armies[j]);
                self.take_loot(j, att);
                self.report(crate::trf!("{winner} defeated {loser}.", winner = b_name, loser = a_name), a_tile, false, events);
                self.army_beaten(att, Beaten::ByAi);
            }
            (Outcome::Defeat, Defender::Garrison(l)) => {
                let loc = &self.world.locations[l];
                let text = crate::trf!("{name} fell at the walls of {place}.", name = a_name, place = building_name(loc));
                let (tile, mine) = (loc.tile, loc.owned());
                self.report(text, tile, mine, events);
                self.army_beaten(att, Beaten::ByAi);
            }
            _ => {
                // A stalemate: both leave each other alone for a while.
                let now = self.clock.total_minutes();
                let other = match def {
                    Defender::Army(j) => self.world.armies[j].uid,
                    Defender::Garrison(l) => match self.world.locations[l].owner {
                        Owner::Army(id) => id as u32,
                        _ => u32::MAX,
                    },
                };
                let me = self.world.armies[att].uid;
                let a = &mut self.world.armies[att];
                a.mind.truce_until = now + TRUCE_MINUTES;
                a.mind.truce_with = other;
                let g = a.mind.goal;
                a.mind.blocked.push((g, now + TRUCE_MINUTES));
                a.mind.goal = Goal::Idle;
                a.path.clear();
                if let Defender::Army(j) = def {
                    self.world.armies[j].mind.truce_until = now + TRUCE_MINUTES;
                    self.world.armies[j].mind.truce_with = me;
                }
            }
        }
        outcome
    }

    /// Army `winner` takes `loser`'s loot: `VictoryGoldDiv` of its gold (none if its units
    /// carry no money) and its items, up to [`MAX_ARMY_ITEMS`].
    fn take_loot(&mut self, winner: usize, loser: usize) {
        let l = &self.world.armies[loser];
        let gold = if l.ai.no_money { 0 } else { self.victory_gold(l.gold) };
        let l = &mut self.world.armies[loser];
        l.gold -= gold;
        let items = std::mem::take(&mut l.items);
        let w = &mut self.world.armies[winner];
        w.gold += gold;
        let room = MAX_ARMY_ITEMS.saturating_sub(w.items.len());
        w.items.extend(items.into_iter().take(room));
    }

    // ------------------------------------------------------------------------------------
    // Beaten armies: lords' retreat, respawn
    // ------------------------------------------------------------------------------------

    /// The building a beaten lord retreats to: his home if he owns it, else his building
    /// nearest to him.
    fn refuge(&self, a: &Army) -> Option<usize> {
        let own = |l: &Location| l.owner == Owner::Army(a.id) && a.id != 0;
        if let Some(h) = a.home.filter(|&h| own(&self.world.locations[h])) {
            return Some(h);
        }
        let here = a.tile(&self.world.map);
        (0..self.world.locations.len()).filter(|&l| own(&self.world.locations[l])).min_by_key(|&l| self.world.map.distance(here, self.world.locations[l].tile))
    }

    /// Army `i` lost a battle (or a spell destroyed it): it leaves the map and is recorded as
    /// beaten. A feudal lord who still owns a building retreats into it with his leader and
    /// comes back after [`RECOVER_DAYS`] (mechanics.md 2.5); an army with a respawn time
    /// comes back after it (the leader alone, or the whole army when flagged).
    pub(crate) fn army_beaten(&mut self, i: usize, by: Beaten) {
        let now = self.clock.total_minutes();
        let mut a = self.remove_army(i);
        if a.id != 0 {
            match by {
                Beaten::ByPlayer => self.beaten_armies.insert(a.id),
                Beaten::ByAi => self.ai_beaten.insert(a.id),
            };
        }
        if !a.ai.enabled {
            return;
        }
        a.path.clear();
        a.chasing = false;
        a.mind = AiMind::default();
        a.budget = 0.0;
        a.effects.clear();
        let c = self.content.clone();
        if a.ai.style == Style::Feudal && self.refuge(&a).is_some() {
            let leader = a.troops.first().copied().or_else(|| a.ai.start_troops.first().copied());
            a.troops = leader.into_iter().map(|mut t| {
                t.hurt = troop_max_hp(&c, &t) - 1;
                t
            }).collect();
            if !a.troops.is_empty() {
                self.world.respawns.push(Respawn { due: now + (RECOVER_DAYS * MINUTES_PER_DAY) as f64, army: a, lord: true });
                self.ai_stats.retreats += 1;
                return;
            }
        }
        if a.ai.respawn_days > 0 {
            let due = now + (a.ai.respawn_days as u64 * MINUTES_PER_DAY) as f64;
            self.world.respawns.push(Respawn { due, army: a, lord: false });
        }
    }

    /// Where a respawning army comes back (world.md §5): the centre of its home building. A
    /// feudal army that no longer owns its home uses a town it owns, else a castle, else a
    /// fort; owning none, or having no home, it does not come back.
    fn respawn_home(&self, a: &Army) -> Option<usize> {
        let home = a.home?;
        if a.ai.style != Style::Feudal || self.world.locations[home].owner == Owner::Army(a.id) {
            return Some(home);
        }
        let owned = |kinds: &[LocationKind]| self.world.locations.iter().position(|l| l.owner == Owner::Army(a.id) && kinds.contains(&l.kind));
        owned(&[LocationKind::Town, LocationKind::Palace]).or_else(|| owned(&[LocationKind::Castle])).or_else(|| owned(&[LocationKind::Fort]))
    }

    /// Beaten armies whose time has come return: a lord from his building (if he still owns
    /// one; else an ordinary respawn, if he has a respawn time), the others at the centre of
    /// their home ([`Game::respawn_home`]) with full hit points, the scenario's leader or
    /// whole army, and the days' income in gold.
    fn ai_respawns(&mut self, now: f64) {
        if !self.world.respawns.iter().any(|r| r.due <= now) {
            return;
        }
        let mut k = 0;
        while k < self.world.respawns.len() {
            if self.world.respawns[k].due > now {
                k += 1;
                continue;
            }
            let Respawn { mut army, lord, .. } = self.world.respawns.remove(k);
            if lord {
                if let Some(l) = self.refuge(&army) {
                    army.troops.iter_mut().for_each(|t| t.hurt = 0);
                    army.pos = self.world.map.center(self.world.locations[l].tile);
                    army.mind.think_at = now;
                    self.world.armies.push(army);
                    continue;
                }
                if army.ai.respawn_days == 0 {
                    continue;
                }
            }
            let start = &army.ai.start_troops;
            army.troops = if army.ai.respawn_all { start.clone() } else { start.iter().take(1).copied().collect() };
            army.troops.iter_mut().for_each(|t| t.hurt = 0);
            if army.troops.is_empty() {
                continue;
            }
            army.gold += army.ai.respawn_days as i32 * army.ai.extra_income;
            army.items.clear();
            army.mind = AiMind { think_at: now, ..AiMind::default() };
            let Some(home) = self.respawn_home(&army) else { continue };
            if army.ai.style != Style::Feudal {
                use LocationKind::*;
                let loc = &mut self.world.locations[home];
                if matches!(loc.kind, Village | Shipyard | Altar | Entrance) {
                    loc.owner = Owner::Army(army.id);
                    loc.faction = army.faction;
                    loc.attitude = army.attitude;
                }
            }
            army.pos = self.world.map.center(self.world.locations[home].tile);
            self.world.armies.push(army);
            self.ai_stats.respawns += 1;
        }
    }

    // ------------------------------------------------------------------------------------
    // Economy
    // ------------------------------------------------------------------------------------

    /// The AI's noon: income from owned buildings and the army's daily income; feudal lords
    /// pay wages (a unit leaves after `MaxTimeNotUpkeep` unpaid, the last hired first) and
    /// hire where they stand.
    pub(crate) fn ai_new_day(&mut self) {
        let c = self.content.clone();
        let limit = (c.options.max_time_not_upkeep.max(1) as u64).div_ceil(MINUTES_PER_DAY) as u32;
        for i in 0..self.world.armies.len() {
            if !managed(&self.world.armies[i]) || self.world.armies[i].ai.style == Style::Peasant {
                continue;
            }
            let id = self.world.armies[i].id;
            let owned: i32 = self.world.locations.iter().filter(|l| l.owner == Owner::Army(id) && l.pays_income()).map(|l| l.gold_income).sum();
            let here = self.world.armies[i].tile(&self.world.map);
            let at = self.world.location_at(here);
            let a = &mut self.world.armies[i];
            a.gold += owned + a.ai.extra_income;
            if a.ai.style == Style::Feudal {
                let wages = army_wages(&c, &a.troops);
                if a.gold >= wages {
                    a.gold -= wages;
                    a.mind.unpaid_days = 0;
                } else {
                    a.mind.unpaid_days += 1;
                    if a.mind.unpaid_days >= limit && a.troops.len() > 1 {
                        a.troops.pop();
                        a.mind.unpaid_days = 0;
                    }
                }
            }
            if let Some(l) = at {
                self.ai_hire_at(i, l);
            }
        }
    }

    /// The AI's midnight (world.md §6): the garrisons of AI and neutral buildings heal
    /// `GarrisonAutoHeal`% of their maximum, and so do armies standing in their own castle or
    /// fort *(guess)*.
    pub(crate) fn ai_midnight(&mut self) {
        let c = self.content.clone();
        let heal = c.options.garrison_auto_heal.max(0);
        for i in 0..self.world.armies.len() {
            let a = &self.world.armies[i];
            let at = self.world.location_at(a.tile(&self.world.map));
            let home = at.is_some_and(|l| self.world.locations[l].owner == Owner::Army(a.id) && self.world.locations[l].kind.capturable());
            if managed(a) && home {
                for t in self.world.armies[i].troops.iter_mut() {
                    t.hurt = (t.hurt - troop_max_hp(&c, t) * heal / 100).max(0);
                }
            }
        }
        for l in self.world.locations.iter_mut().filter(|l| matches!(l.owner, Owner::Army(_) | Owner::Neutral)) {
            for t in l.garrison.iter_mut() {
                t.hurt = (t.hurt - troop_max_hp(&c, t) * heal / 100).max(0);
            }
        }
    }
}

/// Stock of `unit` at `l` goes down by one.
fn take_stock(l: &mut Location, unit: UnitId) {
    if let Some(n) = l.recruits.iter_mut().find(|r| r.unit == unit).and_then(|r| r.stock.as_mut()) {
        *n = (*n - 1).max(0);
    }
}

/// Writes a battle back into `troops`: `hp` (current, maximum in battle) per troop, the dead
/// leave; the leader (troop 0) survives with 1 HP while any of his troops does
/// (mechanics.md 2.5); `xp` is gained with level-ups and a chance to take the upgrade tree
/// ([`ai_unit_gain`]).
fn write_back(c: &Content, rng: &mut Rng, troops: &mut Vec<Troop>, hp: &[(i32, i32)], xp: &[i32]) {
    let survivors = hp.iter().any(|&(h, _)| h > 0);
    let mut keep = Vec::with_capacity(troops.len());
    for (k, t) in troops.iter_mut().enumerate() {
        let Some(&(mut h, max_battle)) = hp.get(k) else {
            keep.push(true);
            continue;
        };
        if k == 0 && h <= 0 && survivors {
            h = 1;
        }
        if h <= 0 {
            // A beaten army keeps its dead: the caller removes it.
            keep.push(!survivors);
            continue;
        }
        let max = troop_max_hp(c, t);
        t.hurt = (max_battle - h).clamp(0, max - 1);
        if let Some(&x) = xp.get(k).filter(|&&x| x > 0) {
            ai_unit_gain(c, rng, t, x);
        }
        keep.push(true);
    }
    let mut k = 0;
    troops.retain(|_| {
        k += 1;
        keep[k - 1]
    });
}

/// A troop banks `xp` and rises the levels it pays for, keeping the rest; AI units keep XP
/// like the player's. Returns the levels gained.
pub fn troop_gain_xp(c: &Content, t: &mut Troop, xp: i32) -> i32 {
    t.xp = t.xp.saturating_add(xp.max(0));
    let mut gained = 0;
    while gained < MAX_LEVELS_AT_ONCE {
        let need = c.xp_to_next(t.unit, t.level);
        if t.xp < need {
            break;
        }
        t.xp -= need;
        t.level += 1;
        gained += 1;
    }
    gained
}

/// How many levels one gain may add (a bound for absurd amounts).
const MAX_LEVELS_AT_ONCE: i32 = 200;

/// An AI unit's gain after a battle (the original's 0x4a4a7c): the XP, then one try at the
/// upgrade tree ([`ai_promote`]).
pub fn ai_unit_gain(c: &Content, rng: &mut Rng, t: &mut Troop, xp: i32) {
    troop_gain_xp(c, t, xp);
    ai_promote(c, rng, t);
}

/// XP a newly hired AI unit starts with, level by level, with a try at the upgrade tree at
/// every level (the original's 0x4a4c04).
fn ai_hire_gain(c: &Content, rng: &mut Rng, t: &mut Troop, xp: i32) {
    let mut left = xp.max(0).saturating_add(t.xp);
    t.xp = 0;
    for _ in 0..MAX_LEVELS_AT_ONCE {
        let need = c.xp_to_next(t.unit, t.level);
        if left < need {
            break;
        }
        left -= need;
        t.level += 1;
        ai_promote(c, rng, t);
    }
    t.xp = left;
}

/// The AI's pick in the upgrade tree (experience.md §4): Militia (unit 4) tries option 1 one
/// time in three and option 3 otherwise, Infantry (unit 8) option 3 one time in three and
/// option 1 otherwise, every other class a random filled option. The pick is taken when
/// its `NextUnitNLevel` is at most the unit's 0-based level: the unit starts the new class
/// at level 1 with no XP. An empty pick promotes nobody *(guess: the original would read an
/// empty slot)*.
fn ai_promote(c: &Content, rng: &mut Rng, t: &mut Troop) -> bool {
    let def = c.unit(t.unit);
    let slots: [Option<&super::content::Upgrade>; 3] = [1u8, 2, 3].map(|n| def.upgrades.iter().find(|u| u.slot == n));
    if slots.iter().all(Option::is_none) {
        return false;
    }
    let pick = match t.unit.0 {
        4 => {
            if rng.random(3) == 0 {
                1
            } else {
                3
            }
        }
        8 => {
            if rng.random(3) == 0 {
                3
            } else {
                1
            }
        }
        _ => loop {
            let n = rng.random(3) as usize + 1;
            if slots[n - 1].is_some() {
                break n;
            }
        },
    };
    let Some(up) = slots[pick - 1] else { return false };
    let Some(target) = up.target.map(UnitId).filter(|&id| c.try_unit(id).is_some()) else { return false };
    if up.level > t.level - 1 {
        return false;
    }
    t.unit = target;
    t.level = 1;
    t.xp = 0;
    true
}

fn army_name(a: &Army) -> String {
    if a.name.trim().is_empty() {
        crate::i18n::tr("An army").to_string()
    } else {
        a.name.trim().to_string()
    }
}

fn building_name(l: &Location) -> String {
    if l.name.trim().is_empty() {
        match crate::i18n::lang() {
            crate::i18n::Lang::En => format!("a {}", l.kind.label().to_lowercase()),
            crate::i18n::Lang::Ru => l.kind.label().to_lowercase(),
        }
    } else {
        l.name.trim().to_string()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod real_maps;
