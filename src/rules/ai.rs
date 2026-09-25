//! AI armies (mechanics.md 5.6, §8.8): what the scenario's armies want and do while the
//! player walks.
//!
//! - **Brain**: every army the AI steers ([`managed`]: the scenario's land armies) picks a goal
//!   on a cadence ([`THINK_MINUTES`]) from its behaviour style (feudal, rogue, peasant: army
//!   byte 59), its target model (byte 85) and the `Min/Max…Target` priorities of `_Global.ini`
//!   ([`Priorities`]): attack the player or a hostile army in view, take a hostile castle or
//!   fort (rogues: their lost home first), heal at a friendly building, fill a garrison, hire,
//!   shop, collect a village's tribute, talk to a friendly army, wander its patrol, go home.
//!   Lower priority values win; a goal's score is its priority times `SCORE_CELLS + distance`.
//!   The per-army flags (bytes 76–81) remove goals. Routes are A* on the map with the army's
//!   speed correction, planned only when the goal's cell changes and at most
//!   [`MAX_PATHS_PER_SLICE`] times per slice of game time.
//! - **Economy** (noon, [`Game::ai_new_day`]): income from owned buildings and the army's
//!   extra income; feudal lords pay wages and keep `NeedUpkeepDay` days of them in reserve
//!   when they hire or shop; rogues pay no wages and hire only rogue units; peasants do
//!   neither.
//! - **AI battles**: mutually hostile armies that meet, or an army reaching its target, fight
//!   with the battle engine played by the AI on both sides ([`Game::ai_battle`]); losses, XP,
//!   loot and captures are applied, and the player hears of it when it happens within sight.
//! - **Lords and respawn**: a beaten feudal lord who still owns a building retreats into it
//!   and comes back after [`RECOVER_DAYS`]; other armies with a respawn time come back after
//!   it, the leader alone or the whole army ([`Respawn`]).
//!
//! Everything the original leaves open is marked *(guess)* and listed in mechanics.md §8.8.

use crate::dt::dtm::Army as DtArmy;

use super::battle::{Battle, Outcome, Team};
use super::clock::MINUTES_PER_DAY;
use super::content::{Content, GlobalOptions, Nature, UnitId, WageKind};
use super::events::ArmyId;
use super::fog;
use super::game::{troop_unit, Event, Foe, Game, CHASE_RADIUS};
use super::items;
use super::map::Tile;
use super::units::{Stats, Unit};
use super::world::{Army, Location, LocationKind, Owner, Troop, World};

/// An AI army thinks again after this many game minutes *(guess)*.
pub const THINK_MINUTES: f64 = 60.0;
/// A goal's score is `priority × (SCORE_CELLS + distance in cells)`: ten cells away doubles
/// the priority *(guess)*.
pub const SCORE_CELLS: i64 = 10;
/// Route searches (A*) all AI armies together may start per slice of game time; the rest wait
/// for the next slice. Chasing the player is not counted.
pub const MAX_PATHS_PER_SLICE: usize = 4;
/// Cells a goal route search may expand.
pub const GOAL_PATH_NODES: usize = 12_000;
/// Cells a chase route search may expand.
const CHASE_PATH_NODES: usize = 4_000;
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
    /// Extra daily gold (byte 17).
    pub extra_income: i32,
    /// Byte 82, default 50: the garrison it keeps in its buildings, in percent of its own
    /// strength *(guess)*.
    pub garrison_strength: i32,
    /// Byte 71: experience correction in percent.
    pub exp_correction: i32,
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
            extra_income: a.gold_income as i32,
            garrison_strength: if a.garrison_strength == 0 { 50 } else { a.garrison_strength as i32 },
            exp_correction: if a.exp_correction == 0 { 100 } else { a.exp_correction as i32 },
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

/// Razdor's own priorities for content without `_Global.ini` (the demo).
const DEMO_PRIORITIES: Priorities = Priorities {
    attack_army: 10,
    attack_castle: 20,
    random: 500,
    talk: 400,
    heal: (20, 200),
    garrison: (150, 600),
    purchase: (150, 400),
    gold_purchase: 400,
    village: (50, 300),
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

/// How far an army sees targets, in cells: `CHASE_RADIUS` scaled by `AIDistance0..2`
/// against `AIDistance1`; aggressive armies use `AIDistance0`, passive ones `AIDistance2`,
/// the rest `AIDistance1` *(guess: the original's use of the three ranges is not decoded;
/// with the shipped 100/50/25 that is 12, 6 and 3 cells)*.
pub fn view_radius(o: &GlobalOptions, m: usize) -> i32 {
    let band = match m {
        model::AGGRESSIVE => 0,
        model::PASSIVE => 2,
        _ => 1,
    };
    let normal = o.ai_distance[1].max(1);
    (CHASE_RADIUS * o.ai_distance[band].max(0) / normal).clamp(2, 24)
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

/// `mine` dares to attack `theirs`: `mine × (100 + aggression) ≥ theirs × 100` *(guess)*.
fn dares(aggression: i32, mine: i64, theirs: i64) -> bool {
    mine * (100 + aggression.clamp(-90, 400)) as i64 >= theirs * 100
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
                let before = u.max_hp(c);
                u.items[s] = Some(item);
                // Hit points lost stay lost; a higher maximum adds to the current HP.
                u.hp += (u.max_hp(c) - before).max(0);
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

/// Free formation cell for a new unit of type `unit` next to `troops`.
fn free_slot(c: &Content, troops: &[Troop], unit: UnitId) -> Option<super::formation::Slot> {
    if troops.len() >= c.formation.capacity() {
        return None;
    }
    let taken: Vec<_> = troops.iter().map(|t| t.slot).collect();
    c.formation.free_slot(&taken, Stats::of_level(c, unit, 1).preferred_row())
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

/// Scores offered goals and keeps the best (lowest).
struct Best<'a> {
    score: i64,
    goal: Goal,
    blocked: &'a [(Goal, f64)],
    now: f64,
}

impl Best<'_> {
    fn offer(&mut self, priority: i32, distance: i32, goal: Goal) {
        if self.blocked.iter().any(|&(g, until)| g == goal && self.now < until) {
            return;
        }
        let score = priority.max(1) as i64 * (SCORE_CELLS + distance.max(0) as i64);
        if score < self.score {
            self.score = score;
            self.goal = goal;
        }
    }
}

/// The goal army `i` picks now. `hero`: the player's cell (on land) if he can be attacked.
/// Pure: the caller routes and acts. A wander goal names the army's post; the caller picks
/// the leg's cell.
pub fn choose_goal(w: &World, c: &Content, i: usize, hero: Option<Tile>, now: f64) -> Goal {
    let a = &w.armies[i];
    let p = &a.ai;
    let map = &w.map;
    let here = a.tile(map);
    let pr = Priorities::of(&c.options, p.model);
    let view = view_radius(&c.options, p.model);
    let mut best = Best { score: i64::MAX, goal: Goal::Idle, blocked: &a.mind.blocked, now };
    let reach = |t: Tile| w.same_region(here, t);

    // The player (every style: peasants too hunt him when ill-disposed).
    if let Some(h) = hero {
        let d = map.distance(here, h);
        if a.hostile() && now >= a.ignore_until && d <= view && reach(h) {
            best.offer(pr.attack_army, d, Goal::AttackPlayer);
        }
    }
    if p.style == Style::Peasant {
        offer_wander(&mut best, w, a, &pr);
        return best.goal;
    }
    let mine = army_strength(c, a);
    // Hostile armies in view.
    if !p.player_only {
        for b in w.armies.iter().filter(|b| b.uid != a.uid && managed(b) && !b.ai.ignored && hostile_to(a, b)) {
            let bt = b.tile(map);
            let d = map.distance(here, bt);
            if d <= view && reach(bt) && now >= a.mind.truce_until && now >= b.mind.truce_until && dares(p.aggression, mine, army_strength(c, b)) {
                best.offer(pr.attack_army, d, Goal::AttackArmy(b.uid));
            }
        }
    }
    // Buildings: within its territory (patrol radius and view), a lost home anywhere.
    if !p.no_buildings {
        let territory = p_radius(a).max(view) + view;
        let hp = health(c, a);
        let free = c.formation.capacity().saturating_sub(a.troops.len()) as i64;
        let cap = c.formation.capacity().max(1) as i64;
        for (l, loc) in w.locations.iter().enumerate() {
            if loc.kind.is_bridge() {
                continue;
            }
            let d = map.distance(here, loc.tile);
            let home_lost = p.style == Style::Rogue && a.home == Some(l) && loc.kind.capturable() && loc.owner != Owner::Army(a.id);
            if (d > territory && !home_lost) || !reach(loc.tile) {
                continue;
            }
            let truce = now < a.mind.truce_until && loc.owner == Owner::Army(a.mind.truce_with.min(u8::MAX as u32) as u8);
            let own = loc.owner == Owner::Army(a.id);
            if loc.kind.capturable() && !p.player_only && (hostile_to_location(a, loc) || home_lost) {
                // A leader alone does not storm walls, except his lost home *(guess)*.
                let force = a.troops.len() > 1 || home_lost;
                if !truce && force && dares(p.aggression, mine, defenders_strength(c, loc)) {
                    let prio = if home_lost { pr.attack_castle / 2 } else { pr.attack_castle };
                    best.offer(prio, d, Goal::Capture(l));
                }
                continue;
            }
            if !welcome_at(a, loc) {
                continue;
            }
            // Healing: in its own castle or fort, or (feudal) any friendly healer.
            let heals_here = (own && loc.kind.capturable()) || (p.style == Style::Feudal && loc.heals());
            if hp < HEAL_BELOW && heals_here {
                best.offer(lerp(pr.heal, 1000 - hp), d, Goal::Heal(l));
            }
            if own && loc.kind.capturable() && p.style == Style::Feudal && !loc.recruits.is_empty() {
                let target = mine * p.garrison_strength as i64 / 100;
                let have = defenders_strength(c, loc);
                if have < target && loc.garrison.len() < c.formation.capacity() && best_recruit(c, a, loc, &a.troops).is_some() {
                    best.offer(lerp(pr.garrison, (target - have) * 1000 / target.max(1)), d, Goal::Garrison(l));
                }
            }
            if free > 0 && hires_for_ai(loc.kind) && best_recruit(c, a, loc, &a.troops).is_some() {
                best.offer(lerp(pr.purchase, free * 1000 / cap), d, Goal::Hire(l));
            }
            if p.style == Style::Feudal && best_buy(c, a, loc).is_some() {
                let spare = (a.gold - reserve(c, a, &a.troops)) as i64;
                best.offer(lerp(pr.purchase, spare * 1000 / pr.gold_purchase.max(1) as i64), d, Goal::Shop(l));
            }
            let tribute_ours = own || loc.faction == a.faction || loc.linked.is_some_and(|k| w.locations[k].owner == Owner::Army(a.id));
            if p.style == Style::Feudal && loc.kind == LocationKind::Village && loc.tribute_gold > 0 && tribute_ours {
                best.offer(lerp(pr.village, loc.tribute_gold as i64 * 1000 / pr.gold_village.max(1) as i64), d, Goal::Village(l));
            }
        }
    }
    // A friendly army of its own faction in view.
    if !p.no_talk && now >= a.mind.talk_after {
        for b in w.armies.iter().filter(|b| b.uid != a.uid && managed(b) && b.faction == a.faction) {
            let bt = b.tile(map);
            let d = map.distance(here, bt);
            if d <= view && reach(bt) && !hostile_to(b, a) {
                best.offer(pr.talk, d, Goal::Talk(b.uid));
            }
        }
    }
    offer_wander(&mut best, w, a, &pr);
    best.goal
}

fn p_radius(a: &Army) -> i32 {
    if a.patrols {
        a.patrol_radius
    } else {
        0
    }
}

/// Wandering its patrol (unless it takes no random targets), or going back to its post when
/// it is outside its patrol area.
fn offer_wander(best: &mut Best, w: &World, a: &Army, pr: &Priorities) {
    let here = a.tile(&w.map);
    let away = w.map.distance(here, a.post) > p_radius(a) + 1;
    if away && w.same_region(here, a.post) {
        best.offer(pr.random, 0, Goal::Home);
    } else if a.patrols && a.patrol_radius > 0 && !a.ai.no_random {
        best.offer(pr.random, 0, Goal::Wander(a.post));
    }
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
                let goal = choose_goal(&self.world, &self.content, i, hero, now);
                self.set_goal(i, goal, now);
            }
            self.route(i, hero, now, &mut budget);
        }
    }

    fn needs_thought(&self, i: usize, hero: Option<Tile>, now: f64) -> bool {
        let a = &self.world.armies[i];
        if now >= a.mind.think_at {
            return true;
        }
        let map = &self.world.map;
        let view = view_radius(&self.content.options, a.ai.model);
        let sees_hero = hero.is_some_and(|h| map.distance(a.tile(map), h) <= view);
        match a.mind.goal {
            // The player went out of view (or a truce began).
            Goal::AttackPlayer => !sees_hero || now < a.ignore_until,
            // A hostile army spots the player.
            _ => a.hostile() && now >= a.ignore_until && sees_hero,
        }
    }

    fn set_goal(&mut self, i: usize, goal: Goal, now: f64) {
        let offset = (self.world.armies[i].uid % 6) as f64 * 5.0;
        let mut goal = goal;
        let same = {
            let a = &self.world.armies[i];
            a.mind.goal == goal || (matches!(goal, Goal::Wander(_)) && goal.same_kind(a.mind.goal) && !a.path.is_empty())
        };
        if same {
            self.world.armies[i].mind.think_at = now + THINK_MINUTES + offset;
            return;
        }
        if let Goal::Wander(_) = goal {
            goal = match self.wander_leg(i) {
                Some(t) => Goal::Wander(t),
                None => Goal::Idle,
            };
        }
        let a = &mut self.world.armies[i];
        a.mind.goal = goal;
        a.path.clear();
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

    /// A random cell of army `i`'s patrol area it can walk to, not a building's entry.
    fn wander_leg(&mut self, i: usize) -> Option<Tile> {
        let (post, r, here) = {
            let a = &self.world.armies[i];
            (a.post, a.patrol_radius, a.tile(&self.world.map))
        };
        for _ in 0..4 {
            let t = (post.0 + self.rng.range(-r, r), post.1 + self.rng.range(-r, r));
            let w = &self.world;
            if w.map.passable(t) && w.location_at(t).is_none() && w.map.distance(t, post) <= r && t != here && w.same_region(here, t) {
                return Some(t);
            }
        }
        None
    }

    /// The cell army `i` heads for: the building's entry, the other army's cell, the leg's
    /// end, its post. `None`: the goal is gone.
    fn goal_cell(&self, i: usize, hero: Option<Tile>) -> Option<Tile> {
        let a = &self.world.armies[i];
        match a.mind.goal {
            Goal::Idle => None,
            Goal::Wander(t) => Some(t),
            Goal::Home => Some(a.post),
            Goal::AttackPlayer => hero,
            Goal::AttackArmy(u) | Goal::Talk(u) => self.army_by_uid(u).map(|j| self.world.armies[j].tile(&self.world.map)),
            g => g.building().map(|l| self.world.locations[l].tile),
        }
    }

    /// Plans army `i`'s route to its goal when the goal's cell is not where the route ends.
    fn route(&mut self, i: usize, hero: Option<Tile>, now: f64, budget: &mut usize) {
        let goal = self.world.armies[i].mind.goal;
        if goal == Goal::Idle {
            return;
        }
        let Some(target) = self.goal_cell(i, hero) else {
            self.drop_goal(i, now, false);
            return;
        };
        let (here, end, empty) = {
            let a = &self.world.armies[i];
            (a.tile(&self.world.map), a.path.last().copied(), a.path.is_empty())
        };
        let map = &self.world.map;
        let chases = goal.army().is_some() || goal == Goal::AttackPlayer;
        let arrived = if chases { map.distance(here, target) <= CONTACT } else { here == target };
        if arrived {
            return;
        }
        let fresh = match end {
            Some(e) if chases => map.distance(e, target) <= 1,
            Some(e) => e == target,
            None => false,
        };
        if fresh && !empty {
            return;
        }
        let nodes = if goal == Goal::AttackPlayer {
            CHASE_PATH_NODES
        } else if *budget == 0 {
            return;
        } else {
            *budget -= 1;
            GOAL_PATH_NODES
        };
        self.ai_stats.paths += 1;
        let path = if self.world.same_region(here, target) { self.world.map.path_limited(here, target, nodes) } else { Vec::new() };
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
        if block && a.mind.goal != Goal::AttackPlayer {
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
        if goal == Goal::Idle || goal == Goal::AttackPlayer {
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
            g if d == 0 => {
                if let Some(l) = g.building() {
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
                    let guard = self.world.armies.iter().position(|b| b.id == oid && b.uid != a.uid && managed(b) && map.distance(b.tile(map), loc.tile) <= CONTACT);
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
                    let Some(slot) = free_slot(&c, &loc.garrison, unit) else { break };
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
            let Some(slot) = free_slot(&c, &a.troops, unit) else { break };
            take_stock(&mut self.world.locations[l], unit);
            let a = &mut self.world.armies[i];
            a.gold -= cost;
            a.troops.push(Troop::new(unit, 1, slot));
            hired += 1;
        }
        self.ai_stats.hired += hired;
        hired
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
        let text = format!("{name} took {}.", building_name(loc));
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
            let Some(slot) = free_slot(&c, &loc.garrison, t.unit) else { return };
            t.slot = slot;
            self.world.armies[i].troops.remove(k);
            self.world.locations[l].garrison.push(t);
        }
    }

    /// Adds a battle report to the log; the player hears of it (an event) when it happened
    /// within his sight, or concerned his own building.
    fn report(&mut self, text: String, tile: Tile, concerns_player: bool, events: &mut Vec<Event>) {
        let news = AiNews { text, tile, at: self.clock.total_minutes() as u64 };
        let hero = self.pos;
        let c = self.world.map.center(tile);
        let seen = (c.0 - hero.0).hypot(c.1 - hero.1) <= fog::SIGHT_RADIUS;
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
        b.apply_spells(Team::Player, &self.spells_on_army(att));
        if let Defender::Army(j) = def {
            b.apply_spells(Team::Enemy, &self.spells_on_army(j));
        }
        if defence > 0 {
            b.set_building_defence(Team::Enemy, defence);
        }
        b.begin();
        let mut steps = 0;
        while b.outcome() == Outcome::Ongoing && steps < MAX_BATTLE_STEPS {
            b.ai_step();
            steps += 1;
        }
        let outcome = b.outcome();
        self.ai_stats.battles += 1;
        self.battles += 1;

        // XP: the engine scales the "player" side by the hero's modifier; AI armies get
        // `AIExpiriencePercent` on both sides.
        let o = &c.options;
        let fix = |xp: i32| if o.hero_experience_modificator > 0 { xp * o.ai_experience_percent / o.hero_experience_modificator } else { xp };
        let mut xp_a = vec![0; na];
        for aw in b.xp_awards(Team::Player) {
            if aw.fighter < na {
                xp_a[aw.fighter] += fix(aw.xp);
            }
        }
        let mut xp_b = vec![0; b_units.len()];
        for aw in b.xp_awards(Team::Enemy) {
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
            let corr = a.ai.exp_correction;
            write_back(&c, &mut a.troops, &hp_a, &xp_a, corr);
        }
        match def {
            Defender::Army(j) => {
                let a = &mut self.world.armies[j];
                let corr = a.ai.exp_correction;
                write_back(&c, &mut a.troops, &hp_b, &xp_b, corr);
            }
            Defender::Garrison(l) => {
                let loc = &mut self.world.locations[l];
                let ng = loc.garrison.len();
                write_back(&c, &mut loc.garrison, &hp_b[..ng.min(hp_b.len())], &xp_b[..ng.min(xp_b.len())], 100);
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
                self.report(format!("{a_name} defeated {b_name}."), a_tile, false, events);
                self.army_beaten(j, Beaten::ByAi);
            }
            (Outcome::Victory, Defender::Garrison(l)) => {
                self.world.locations[l].garrison.clear();
                self.take_building(att, l, events);
            }
            (Outcome::Defeat, Defender::Army(j)) => {
                let b_name = army_name(&self.world.armies[j]);
                self.take_loot(j, att);
                self.report(format!("{b_name} defeated {a_name}."), a_tile, false, events);
                self.army_beaten(att, Beaten::ByAi);
            }
            (Outcome::Defeat, Defender::Garrison(l)) => {
                let loc = &self.world.locations[l];
                let text = format!("{a_name} fell at the walls of {}.", building_name(loc));
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

    /// Beaten armies whose time has come return: a lord from his building (if he still owns
    /// one; else an ordinary respawn, if he has a respawn time), the others at their home
    /// building or post, with the scenario's leader or whole army.
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
            army.gold = army.ai.extra_income;
            army.items.clear();
            army.mind = AiMind { think_at: now, ..AiMind::default() };
            let home = army.home.filter(|&h| !hostile_to_location(&army, &self.world.locations[h]) && !self.world.locations[h].owned());
            let tile = match home {
                Some(h) => Some(self.world.locations[h].tile),
                None => self.world.placement(&army),
            };
            let Some(tile) = tile else { continue };
            army.pos = self.world.map.center(tile);
            self.world.armies.push(army);
            self.ai_stats.respawns += 1;
        }
    }

    // ------------------------------------------------------------------------------------
    // Economy
    // ------------------------------------------------------------------------------------

    /// The AI's noon: income from owned buildings and the army's extra income; feudal lords
    /// pay wages (a unit leaves after `MaxTimeNotUpkeep` unpaid, the last hired first) and
    /// hire where they stand; armies in their own castle or fort, and the AI's garrisons,
    /// heal `GarrisonAutoHeal`%.
    pub(crate) fn ai_new_day(&mut self) {
        let c = self.content.clone();
        let heal = c.options.garrison_auto_heal.max(0);
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
                if self.world.locations[l].owner == Owner::Army(id) && self.world.locations[l].kind.capturable() {
                    for t in self.world.armies[i].troops.iter_mut() {
                        t.hurt = (t.hurt - troop_max_hp(&c, t) * heal / 100).max(0);
                    }
                }
                self.ai_hire_at(i, l);
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
/// (mechanics.md 2.5); `xp` scaled by `correction` percent, with level-ups.
fn write_back(c: &Content, troops: &mut Vec<Troop>, hp: &[(i32, i32)], xp: &[i32], correction: i32) {
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
        t.xp += xp.get(k).copied().unwrap_or(0) * correction.max(0) / 100;
        for _ in 0..50 {
            let need = c.xp_to_next(t.unit, t.level);
            if t.xp < need {
                break;
            }
            t.xp -= need;
            t.level += 1;
        }
        keep.push(true);
    }
    let mut k = 0;
    troops.retain(|_| {
        k += 1;
        keep[k - 1]
    });
}

fn army_name(a: &Army) -> String {
    if a.name.trim().is_empty() {
        "An army".to_string()
    } else {
        a.name.trim().to_string()
    }
}

fn building_name(l: &Location) -> String {
    if l.name.trim().is_empty() {
        format!("a {}", l.kind.label().to_lowercase())
    } else {
        l.name.trim().to_string()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod real_maps;
