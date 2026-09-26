//! The world map: terrain, buildings (locations), armies and the hero's start.
//!
//! Built either from an original scenario ([`World::from_scenario`], `docs/reference/dtm-format.md`)
//! or from the built-in demo kingdom ([`World::standard`], `data/kingdom.txt`), through the
//! same types.

use std::collections::HashMap;

use crate::i18n::{n_, tr};

use crate::dt::dtm::{self, Archetype, BuildingType, EventKind, Scenario};

use super::clock::Clock;
use super::content::{Content, HeroClass, ItemId, UnitId};
use super::formation::{Row, Slot};
use super::ai::{AiMind, AiProfile, Respawn};
use super::magic::ActiveSpell;
use super::map::{is_water, Decoration, Grid, Tile, TileMap, BASE_SPEED};
use super::units::{Stats, Unit};

const KINGDOM: &str = include_str!("../../data/kingdom.txt");

/// Attitude value at or below which a side attacks the player (relations run −3..3).
pub const HOSTILE_BELOW: i8 = 0;

/// A unit in an army or a garrison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Troop {
    pub unit: UnitId,
    /// 1 as hired.
    pub level: i32,
    pub slot: Slot,
    /// Hit points lost to world spells (a troop fights with its maximum minus this).
    #[serde(default)]
    pub hurt: i32,
    /// Experience towards the next level (AI armies gain it in their own battles).
    #[serde(default)]
    pub xp: i32,
}

impl Troop {
    /// A troop at full health.
    pub fn new(unit: UnitId, level: i32, slot: Slot) -> Troop {
        Troop { unit, level, slot, hurt: 0, xp: 0 }
    }
}

/// Places `(unit, level, count)` entries into free formation cells, each unit in its preferred
/// row, around the already `occupied` cells. Unknown unit ids and units beyond the formation's
/// capacity are dropped; returns the troops and how many units were dropped.
///
/// The `.DTm` troop triple is (unit, extra level, count): the middle byte is 0 for almost
/// every troop and the last byte is 1–9, so the middle one is levels above the first *(L)*.
pub fn place_troops(content: &Content, occupied: &[Slot], entries: &[(u32, i32, i32)]) -> (Vec<Troop>, usize) {
    let mut taken = occupied.to_vec();
    let mut out = Vec::new();
    let mut dropped = 0;
    for &(unit, level, count) in entries {
        let id = UnitId(unit);
        if content.try_unit(id).is_none() {
            dropped += count.max(0) as usize;
            continue;
        }
        let row = Stats::of_level(content, id, 1).preferred_row();
        for _ in 0..count.max(0) {
            match content.formation.free_slot(&taken, row) {
                Some(slot) if taken.len() < content.formation.capacity() => {
                    taken.push(slot);
                    out.push(Troop::new(id, level.max(1), slot));
                }
                _ => dropped += 1,
            }
        }
    }
    (out, dropped)
}

fn dt_entries(troops: &[dtm::Troop]) -> Vec<(u32, i32, i32)> {
    troops.iter().filter(|t| t.unit != 0 && t.count > 0).map(|t| (t.unit as u32, t.level as i32 + 1, t.count as i32)).collect()
}

/// The 16 building types of the original plus the demo's bandit camp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LocationKind {
    Palace,
    Town,
    Village,
    Castle,
    Fort,
    Tavern,
    Market,
    Church,
    Smithy,
    Shipyard,
    Altar,
    Entrance,
    Ruins,
    StoneBridge,
    WoodenBridge,
    Obelisk,
    /// Demo only: a bandit camp with a garrison and a reward.
    Camp,
}

impl LocationKind {
    pub fn from_building(t: BuildingType) -> LocationKind {
        use LocationKind as K;
        match t {
            BuildingType::Palace => K::Palace,
            BuildingType::Town => K::Town,
            BuildingType::Village => K::Village,
            BuildingType::Castle => K::Castle,
            BuildingType::Fort => K::Fort,
            BuildingType::Tavern => K::Tavern,
            BuildingType::Market => K::Market,
            BuildingType::Church => K::Church,
            BuildingType::Smithy => K::Smithy,
            BuildingType::Shipyard => K::Shipyard,
            BuildingType::Altar => K::Altar,
            BuildingType::DungeonEntrance => K::Entrance,
            BuildingType::Ruins => K::Ruins,
            BuildingType::StoneBridge => K::StoneBridge,
            BuildingType::WoodenBridge => K::WoodenBridge,
            BuildingType::Obelisk => K::Obelisk,
        }
    }

    pub fn is_bridge(self) -> bool {
        matches!(self, LocationKind::StoneBridge | LocationKind::WoodenBridge)
    }

    /// Castles and forts are taken by beating their garrison.
    pub fn capturable(self) -> bool {
        matches!(self, LocationKind::Castle | LocationKind::Fort)
    }

    /// Kinds whose garrison fights a hostile visitor (ruins guard their treasure).
    pub fn defends(self) -> bool {
        matches!(self, LocationKind::Castle | LocationKind::Fort | LocationKind::Ruins | LocationKind::Camp)
    }

    pub fn label(self) -> &'static str {
        use LocationKind::*;
        match self {
            Palace => tr("Palace"),
            Town => tr("Town"),
            Village => tr("Village"),
            Castle => tr("Castle"),
            Fort => tr("Fort"),
            Tavern => tr("Tavern"),
            Market => tr("Market"),
            Church => tr("Church"),
            Smithy => tr("Smithy"),
            Shipyard => tr("Shipyard"),
            Altar => tr("Altar"),
            Entrance => tr("Dungeon entrance"),
            Ruins => tr("Ruins"),
            StoneBridge => tr("Stone bridge"),
            WoodenBridge => tr("Wooden bridge"),
            Obelisk => tr("Obelisk"),
            Camp => tr("Bandit camp"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Owner {
    Player,
    /// An army of the scenario, by its 1-based id.
    Army(u8),
    /// The building's own (neutral) owner.
    Neutral,
}

/// A unit type a barracks offers. `stock: None` = unlimited (the demo).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Recruit {
    pub unit: UnitId,
    pub stock: Option<i32>,
    /// The editor's maximum (at least the start count).
    pub max: i32,
    /// Unused (the regrowth is a daily roll, `rules::economy::regrow`); kept for saves.
    pub progress: i32,
}

impl Recruit {
    pub fn new(unit: UnitId, start: i32, max: i32) -> Recruit {
        Recruit { unit, stock: Some(start.max(0)), max: max.max(start).max(0), progress: 0 }
    }
}

/// A player's unit left in a garrison, and the game minute it was left there.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Stationed {
    pub unit: Unit,
    pub since: u64,
}

/// A scenario event id (1-based, file order), as buildings list them.
pub type EventId = u16;

/// What the building screens show of a scenario event (the event engine is separate).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventInfo {
    pub kind: Option<EventKind>,
    /// The title without its flag script.
    pub title: String,
}

/// A lantern or event point of the scenario (`docs/reference/dtm-format.md` §8). Events
/// refer to it by `id`; the hero standing on `tile` is "at the point".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapPoint {
    pub id: u8,
    pub tile: Tile,
    /// Radius a lit lantern reveals.
    pub radius: i32,
    /// Model 8: a lantern lit from the start.
    pub lit: bool,
}

/// Items for sale.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Shop {
    /// Always stocked.
    pub fixed: Vec<ItemId>,
    /// Random items added at each restock.
    pub random: usize,
    /// Price range of the random items (0, 0 = any).
    pub price: (i32, i32),
    /// On sale now.
    pub stock: Vec<ItemId>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Location {
    /// 1-based building id in the scenario (events refer to it); 0 in the demo.
    pub id: u16,
    /// Texts of the scenario: not saved, restored from it on load.
    #[serde(skip)]
    pub name: String,
    /// The neutral owner's name (a village headman, a lord …).
    #[serde(skip)]
    pub owner_name: String,
    #[serde(skip)]
    pub description: String,
    pub kind: LocationKind,
    /// Centre of the footprint `(x0 + sx/2, y0 + sy/2)`, where AI armies stand (world.md §7).
    /// There is no entry cell: stepping onto any footprint cell enters the building.
    pub tile: Tile,
    /// Bottom-right cell of the footprint.
    pub anchor: Tile,
    /// Footprint size in cells (x, y), at least 1×1.
    pub size: (i32, i32),
    /// Sprite: `Objects.ugs` section-B (picture type, variant). The demo borrows pictures of
    /// the same kinds.
    pub picture: (u8, u8),
    pub owner: Owner,
    /// 1 player, 2 ally, 3 neighbour, 4 enemy.
    pub faction: u8,
    /// Attitude towards the player, −3..3.
    pub attitude: i8,
    /// Daily gold and mana for the owner (castles, forts, towns) or, for villages, the
    /// tribute that accumulates up to the maximum.
    pub gold_income: i32,
    pub gold_max: i32,
    pub mana_income: i32,
    pub mana_max: i32,
    /// Village tribute waiting to be collected.
    pub tribute_gold: i32,
    pub tribute_mana: i32,
    /// The scenario's garrison (the owner's troops).
    pub garrison: Vec<Troop>,
    pub garrison_defence: i32,
    /// The player's units left here (his castles and forts).
    pub stationed: Vec<Stationed>,
    pub recruits: Vec<Recruit>,
    /// Recruiting any listed type is allowed (the "all types" flag).
    pub recruit_all_types: bool,
    pub shop: Option<Shop>,
    /// Ruins: treasure gold and items; the demo's camps: reward and loot rolls.
    pub treasure_gold: i32,
    pub treasure: Vec<ItemId>,
    pub loot_rolls: u32,
    /// Spells taught here (1-based spell index).
    pub spells: Vec<u8>,
    /// Local events (1-based event ids).
    pub events: Vec<u16>,
    /// Linked building (a village's castle), index into `locations`.
    pub linked: Option<usize>,
    /// Garrison beaten / treasure taken.
    pub cleared: bool,
}

impl Location {
    fn new(kind: LocationKind, name: &str, tile: Tile) -> Location {
        Location {
            id: 0,
            name: name.to_string(),
            owner_name: String::new(),
            description: String::new(),
            kind,
            tile,
            anchor: tile,
            size: (1, 1),
            picture: (0, 0),
            owner: Owner::Neutral,
            faction: 3,
            attitude: 1,
            gold_income: 0,
            gold_max: 0,
            mana_income: 0,
            mana_max: 0,
            tribute_gold: 0,
            tribute_mana: 0,
            garrison: Vec::new(),
            garrison_defence: 0,
            stationed: Vec::new(),
            recruits: Vec::new(),
            recruit_all_types: false,
            shop: None,
            treasure_gold: 0,
            treasure: Vec::new(),
            loot_rolls: 0,
            spells: Vec::new(),
            events: Vec::new(),
            linked: None,
            cleared: false,
        }
    }

    /// Footprint cells (world.md §7): `size` cells up and to the left of the anchor, plus
    /// one extra row above when the building is wider than tall.
    pub fn cells(&self) -> impl Iterator<Item = Tile> + '_ {
        let (ax, ay) = self.anchor;
        let rows = self.size.1 + i32::from(self.size.0 > self.size.1);
        (0..rows).flat_map(move |j| (0..self.size.0).map(move |i| (ax - i, ay - j)))
    }

    /// Centre of the footprint `(x0 + sx/2, y0 + sy/2)` (world.md §7).
    pub fn centre(&self) -> Tile {
        let (x0, y0) = (self.anchor.0 - self.size.0 + 1, self.anchor.1 - self.size.1 + 1);
        (x0 + self.size.0 / 2, y0 + self.size.1 / 2)
    }

    /// Bars the hero's route (world.md §1): a castle or fort whose attitude to him is at most
    /// 0, ruins that are not his (not yet cleared), and any other enemy building (attitude
    /// below 0; bridges excepted). The route goes around it unless it is the building clicked.
    /// Other buildings are walked through without being entered.
    pub fn bars_hero(&self) -> bool {
        match self.kind {
            LocationKind::Castle | LocationKind::Fort => !self.owned() && self.attitude <= 0,
            LocationKind::Ruins => !self.owned() && !self.cleared,
            k if k.is_bridge() => false,
            _ => self.hostile(),
        }
    }

    pub fn owned(&self) -> bool {
        self.owner == Owner::Player
    }

    /// Not the player's and ill-disposed towards him.
    pub fn hostile(&self) -> bool {
        !self.owned() && self.attitude < HOSTILE_BELOW
    }

    /// A garrison that fights the player when he steps in.
    pub fn defended(&self) -> bool {
        self.kind.defends() && self.hostile() && !self.cleared && !self.garrison.is_empty()
    }

    /// Paid healing (mechanics.md 1.6): towns, castles, forts and churches.
    pub fn heals(&self) -> bool {
        use LocationKind::*;
        matches!(self.kind, Palace | Town | Castle | Fort | Church)
    }

    /// Resurrection: only towns and churches.
    pub fn resurrects(&self) -> bool {
        use LocationKind::*;
        matches!(self.kind, Palace | Town | Church)
    }

    /// The player may leave troops here: his own castles and forts.
    pub fn takes_garrison(&self) -> bool {
        self.owned() && self.kind.capturable()
    }

    /// Hiring for the player: towns, castles, forts and churches; villages and altars hire
    /// for the AI only (mechanics.md 5.3).
    pub fn hires(&self) -> bool {
        use LocationKind::*;
        matches!(self.kind, Palace | Town | Castle | Fort | Church)
    }

    /// Income the owner receives each day (villages pay tribute instead).
    pub fn pays_income(&self) -> bool {
        !matches!(self.kind, LocationKind::Village)
    }
}

/// An army on the map (an AI lord, a gang, peasants) or, in the demo, a bandit gang.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Army {
    /// 1-based army id in the scenario; 0 for the demo's gangs.
    pub id: u8,
    /// Unique among the world's armies for the whole game (a spell's target is followed by it).
    pub uid: u32,
    /// Texts of the scenario: not saved, restored from it on load.
    #[serde(skip)]
    pub name: String,
    #[serde(skip)]
    pub leader_name: String,
    #[serde(skip)]
    pub description: String,
    /// Map model (`.DTm` army byte 5): 4 feudal, 5 bandits, 6 peasants, …
    pub model: u8,
    /// Position in world units (see `map::center`).
    pub pos: (f32, f32),
    /// Home building, index into `locations`.
    pub home: Option<usize>,
    /// Centre of its patrol.
    pub post: Tile,
    pub patrols: bool,
    pub patrol_radius: i32,
    pub troops: Vec<Troop>,
    /// 1 player, 2 ally, 3 neighbour, 4 enemy.
    pub faction: u8,
    /// Attitude towards the player, −3..3.
    pub attitude: i8,
    /// Gold carried (loot basis).
    pub gold: i32,
    pub items: Vec<ItemId>,
    /// Minutes per cost unit of a step (the hero's knight is 5; [`Army::speed_for`]).
    /// 0 in saves from before speeds: [`World::restore_statics`] takes the scenario's.
    #[serde(default)]
    pub speed: u32,
    /// Game minutes banked towards its next step (world.md §2: the hero's steps and waits
    /// feed it, up to [`AI_BUDGET_CAP`]).
    #[serde(default)]
    pub budget: f32,
    /// Where it stood before its last steps (drawing only).
    #[serde(skip)]
    pub shown_from: Option<(f32, f32)>,
    pub path: Vec<Tile>,
    pub chasing: bool,
    /// Game minute until which it leaves the player alone (after a stalemate).
    pub ignore_until: f64,
    /// Already greeted the player (friendly meeting shown once while nearby).
    pub met: bool,
    /// Game minute until which it stands still between patrol legs.
    pub rest_until: f64,
    /// Named character (1-based, the scenario's list) leading it; 0 none.
    pub named: u8,
    /// World spells cast on it that still last (curses of the player's hero).
    #[serde(default)]
    pub effects: Vec<ActiveSpell>,
    /// Ship type (`.DTm` army byte 72): 0 a land army, else it sails (`rules::ships::kind`).
    #[serde(default)]
    pub ship: u8,
    /// What the scenario says about its behaviour (`rules::ai`); `enabled` is false for the
    /// demo's gangs, which keep the simple chase-and-patrol rules.
    #[serde(default)]
    pub ai: AiProfile,
    /// Its current goal and bookkeeping (`rules::ai`).
    #[serde(default)]
    pub mind: AiMind,
}

impl Army {
    pub fn tile(&self, map: &TileMap) -> Tile {
        map.tile_at(self.pos)
    }

    /// Attacks the player on contact and chases him (its `attitude` is the relation of
    /// [`relation`], worked out at load and when events change its faction or attitudes).
    pub fn hostile(&self) -> bool {
        self.attitude < HOSTILE_BELOW
    }

    /// The troop in the middle of the front row, else the first one.
    pub fn leader(&self) -> Option<UnitId> {
        self.troops.first().map(|t| t.unit)
    }

    /// A ship: it moves on open water only (`rules::ships`).
    pub fn sails(&self) -> bool {
        self.ship != 0
    }

    /// Speed of an army from the editor's speed correction (world.md §2): `max(1, 5 − c)`,
    /// then one less when its leader is the Archmage unit (GlobalIndex 2; the original's
    /// loader does so, probably meaning the Ranger), at least 1. One point is 20% of the time.
    pub fn speed_for(correction: i8, leader: u32) -> u32 {
        let s = (BASE_SPEED as i32 - correction as i32).max(1);
        let s = if leader == ARCHMAGE_UNIT { s - 1 } else { s };
        s.max(1) as u32
    }
}

/// The Archmage's unit (GlobalIndex 2), whose armies the original's loader makes faster.
const ARCHMAGE_UNIT: u32 = 2;
/// Most game minutes an army banks towards its steps (world.md §2).
pub const AI_BUDGET_CAP: f32 = 200.0;

/// Where and with what the hero starts.
#[derive(Clone, Debug, PartialEq)]
pub struct HeroStart {
    pub class: HeroClass,
    pub tile: Tile,
    pub gold: i32,
    pub mana: i32,
    /// Troops besides the hero; the hero stands in `hero_slot`.
    pub hero_slot: Slot,
    pub troops: Vec<Troop>,
    pub items: Vec<ItemId>,
    pub spells: Vec<u8>,
    /// Location the hero starts in, if any.
    pub location: Option<usize>,
    /// Buildings given to him at the start: the preset's start building and those flagged
    /// for his class.
    pub owned: Vec<usize>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct World {
    /// Statics (the map, texts, indexes) are not saved: they are rebuilt from the scenario
    /// when a game is loaded ([`World::restore_statics`]).
    #[serde(skip)]
    pub title: String,
    #[serde(skip)]
    pub map: TileMap,
    pub locations: Vec<Location>,
    /// Armies on the map.
    pub armies: Vec<Army>,
    /// Armies not on the map yet (inactive at start, events bring them in; and ships).
    pub inactive: Vec<Army>,
    /// Troops of a newly spawned demo gang.
    pub gang: Vec<Troop>,
    /// When the game starts.
    pub start: Clock,
    /// Relations between the four factions (player, ally, neighbour, enemy), −3..3.
    pub relations: [[i8; 4]; 4],
    /// The built-in demo (its own flavour rules: gangs, item tribute, weekly markets).
    pub demo: bool,
    /// Units of the scenario that the content does not know or that did not fit.
    pub dropped_units: usize,
    /// The scenario's events by id − 1 (titles and kinds only).
    #[serde(skip)]
    pub events: Vec<EventInfo>,
    /// Lanterns and event points.
    #[serde(skip)]
    pub points: Vec<MapPoint>,
    /// Names of the scenario's named characters, by id − 1.
    #[serde(skip)]
    pub named_characters: Vec<String>,
    /// Last [`Army::uid`] handed to a spawned demo gang.
    pub next_uid: u32,
    /// Beaten armies waiting to come back: lords recovering in a building, and armies with
    /// a respawn time (`rules::ai`).
    #[serde(default)]
    pub respawns: Vec<Respawn>,
    /// Connected region of every passable cell on foot (`TileMap::regions`), to skip goals
    /// an AI army cannot walk to. Rebuilt from the map.
    #[serde(skip)]
    pub(crate) regions: Vec<u32>,
    #[serde(skip)]
    footprints: HashMap<Tile, usize>,
    /// Water the hero sails on, a `w*h` mask (`rules::ships`): shallows and coastal water
    /// that is not a building's footprint.
    #[serde(skip)]
    pub(crate) sea: Vec<bool>,
}

/// How far an army is moved to find a cell of its kind (land, or water for a ship).
pub const PLACE_RADIUS: i32 = 8;

pub const GANG_REWARD: i32 = 30;
/// What the demo calls its roaming gangs.
const GANG_NAME: &str = n_("Bandit gang");
/// Uids above the scenario's army ids (1..=255) go to the demo's gangs.
const FIRST_GANG_UID: u32 = 256;
/// Demo gangs carry this much gold; the victor takes `VictoryGoldDiv` of it.
const GANG_GOLD: i32 = 2 * GANG_REWARD;
/// A little slower than the hero.
const GANG_SPEED: u32 = 6;
const GANG_PATROL: i32 = 8;

/// A demo unit type by its `Key=`. Panics if the built-in data lacks it.
pub fn demo_unit(content: &Content, key: &str) -> UnitId {
    content.unit_by_key(key).unwrap_or_else(|| panic!("demo unit '{key}' missing"))
}

/// A roaming gang: two bandits in front, an archer behind.
pub fn gang(content: &Content) -> Vec<Troop> {
    let (bandit, archer) = (demo_unit(content, "bandit"), demo_unit(content, "bandit_archer"));
    let t = |unit, row, col| Troop::new(unit, 1, Slot::new(row, col));
    vec![t(bandit, Row::Front, 2), t(bandit, Row::Front, 3), t(archer, Row::Back, 2)]
}

fn artifact_ids(content: &Content, ids: impl Iterator<Item = u32>) -> Vec<ItemId> {
    ids.map(ItemId).filter(|&i| content.try_item(i).is_some()).collect()
}

/// The relation between two sides as the original computes it (0x4a0868, world.md §4): `a`
/// is one side's attitude to the other's faction, `b` the other's to the first. Both ≥ 0 →
/// their mean (rounded down), `a` < 0 → `a`, else −1. Below 0 means hostile.
pub fn relation(a: i8, b: i8) -> i8 {
    if a >= 0 && b >= 0 {
        ((a as i16 + b as i16) / 2) as i8
    } else if a < 0 {
        a
    } else {
        -1
    }
}

/// The player's attitude to `faction` (1–4) from the scenario's relation matrix; an unset
/// faction counts as neutral.
pub fn player_attitude_to(relations: &[[i8; 4]; 4], faction: u8) -> i8 {
    (faction as usize).checked_sub(1).and_then(|f| relations[0].get(f).copied()).unwrap_or(0)
}

impl World {
    fn empty(title: &str, map: TileMap, start: Clock) -> World {
        World {
            title: title.to_string(),
            map,
            locations: Vec::new(),
            armies: Vec::new(),
            inactive: Vec::new(),
            gang: Vec::new(),
            start,
            relations: [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]],
            demo: false,
            dropped_units: 0,
            events: Vec::new(),
            points: Vec::new(),
            named_characters: Vec::new(),
            next_uid: FIRST_GANG_UID,
            respawns: Vec::new(),
            regions: Vec::new(),
            footprints: HashMap::new(),
            sea: Vec::new(),
        }
    }

    /// Lays the buildings over the map (world.md §1 step 6, §7): every footprint cell,
    /// bridges included, becomes road on the foot and the ship map; each location stands at
    /// its footprint's centre. Indexes the footprints and the hero's sea.
    fn place_buildings(&mut self) {
        self.footprints.clear();
        for (i, l) in self.locations.iter_mut().enumerate() {
            let c = l.centre();
            l.tile = (c.0.clamp(0, (self.map.w - 1).max(0)), c.1.clamp(0, (self.map.h - 1).max(0)));
            let cells: Vec<Tile> = l.cells().filter(|&t| self.map.in_bounds(t)).collect();
            for t in cells {
                // Where footprints overlap, the later building's wins.
                self.footprints.insert(t, i);
                self.map.pave(t);
            }
        }
        let map = &self.map;
        self.sea = (0..map.w * map.h)
            .map(|i| {
                let t = (i % map.w, i / map.w);
                is_water(map.surface(t)) && map.water_cost(t).is_some() && !self.footprints.contains_key(&t)
            })
            .collect();
    }

    /// The world of an original scenario.
    ///
    /// - Terrain and objects set the cell costs (`map` module).
    /// - Every building becomes a location of its type with its footprint (bottom-right
    ///   anchor, `size_x × size_y`), walked at road speed and entered from any of its cells.
    /// - Active land armies are placed on the map, the others (and ships) wait in `inactive`.
    ///   Hostility is the army's own attitude towards the player (< 0 attacks). Army word 17
    ///   is its starting gold, byte 80 × 10 its daily income (world.md §5).
    pub fn from_scenario(s: &Scenario, content: &Content) -> World {
        let (w, h) = (s.width() as i32, s.height() as i32);
        let objects = s
            .objects
            .iter()
            .map(|o| Decoration { tile: (o.x as i32, o.y as i32), class: o.class, sprite: o.sprite })
            .collect();
        let map = TileMap::from_codes(Grid::Square8, w, h, &s.terrain, objects);
        let start = if s.header.start_time > 0 { Clock::at_minutes(s.header.start_time as u64) } else { Clock::demo_start() };
        let mut world = World::empty(&s.title, map, start);
        world.relations = s.header.relations;
        world.events = s.events.iter().map(|e| EventInfo { kind: e.kind(), title: e.title_text().trim().to_string() }).collect();
        world.points = s
            .points
            .iter()
            .map(|p| MapPoint { id: p.id, tile: (p.x as i32, p.y as i32), radius: p.radius as i32, lit: p.model == 8 && p.active != 0 })
            .collect();
        world.named_characters = s.named_characters.iter().map(|n| n.name.clone()).collect();

        for (i, b) in s.buildings.iter().enumerate() {
            let kind = b.building_type().map_or(LocationKind::Smithy, LocationKind::from_building);
            let size = (b.size_x.max(1) as i32, b.size_y.max(1) as i32);
            let anchor = (b.x as i32, b.y as i32);
            let mut l = Location::new(kind, &b.name, anchor);
            l.anchor = anchor;
            l.size = size;
            l.id = i as u16 + 1;
            l.owner_name = b.owner_name.clone();
            l.description = b.description.clone();
            l.picture = (b.picture_type, b.picture_variant);
            l.owner = b.owner().map_or(Owner::Neutral, Owner::Army);
            l.faction = b.faction;
            l.attitude = b.relations[0];
            if b.faction == 1 {
                l.owner = Owner::Player;
            }
            l.gold_income = b.gold_per_day as i32;
            l.gold_max = b.gold_max as i32;
            l.mana_income = b.mana_per_day as i32;
            l.mana_max = b.mana_max as i32;
            // A village starts with one day's tribute *(guess)*.
            l.tribute_gold = if kind == LocationKind::Village { l.gold_income } else { 0 };
            l.tribute_mana = if kind == LocationKind::Village { l.mana_income } else { 0 };
            let (garrison, dropped) = place_troops(content, &[], &dt_entries(&b.garrison));
            world.dropped_units += dropped;
            l.garrison = garrison;
            l.garrison_defence = b.garrison_extra_defence as i32;
            if b.has_barracks != 0 || b.barracks.iter().any(|r| r.unit != 0) {
                l.recruits = b
                    .barracks
                    .iter()
                    .filter(|r| r.unit != 0 && content.try_unit(UnitId(r.unit as u32)).is_some())
                    .map(|r| Recruit::new(UnitId(r.unit as u32), r.start_count as i32, r.max_count as i32))
                    .collect();
            }
            l.recruit_all_types = b.recruit_all_types != 0;
            let items = artifact_ids(content, b.artifacts().map(u32::from));
            if kind == LocationKind::Ruins {
                l.treasure = items;
                l.treasure_gold = b.price_max as i32;
            } else if !items.is_empty() || b.random_artifacts_for_sale > 0 {
                l.shop = Some(Shop {
                    fixed: items,
                    random: b.random_artifacts_for_sale as usize,
                    price: (b.price_min as i32, b.price_max as i32),
                    stock: Vec::new(),
                });
            }
            l.spells = b.spells_for_sale.iter().copied().filter(|&x| x != 0).collect();
            l.events = b.events().collect();
            l.linked = (b.linked_building as usize).checked_sub(1).filter(|&j| j < s.buildings.len());
            world.locations.push(l);
        }
        world.place_buildings();

        for a in &s.armies {
            let mut entries = Vec::new();
            if a.leader_unit != 0 {
                entries.push((a.leader_unit as u32, a.leader_level as i32 + 1, 1));
            }
            entries.extend(dt_entries(&a.troops));
            let (troops, dropped) = place_troops(content, &[], &entries);
            world.dropped_units += dropped;
            if troops.is_empty() {
                continue;
            }
            let troops_for_ai = troops.clone();
            let at = (a.x as i32, a.y as i32);
            let placed = if a.ship != 0 {
                world.nearest_sea(at, PLACE_RADIUS)
            } else if world.map.passable(at) {
                Some(at)
            } else {
                world.map.nearest_passable(at, PLACE_RADIUS)
            };
            let tile = placed.unwrap_or(at);
            // Merchant ships trade and never attack (guess; one shipped merchant is marked
            // ill-disposed in its file).
            // The original's relation (0x4a0868): the player's attitude to the army's faction
            // and the army's to the player. An army whose own attitudes were left at 0 (РК1's
            // mage) is hostile through its faction.
            let towards = player_attitude_to(&world.relations, a.faction);
            let attitude = relation(towards, a.relations[0]);
            let attitude = if a.ship == super::ships::kind::MERCHANT { attitude.max(0) } else { attitude };
            let home = (a.home_building as usize).checked_sub(1).filter(|&j| j < world.locations.len());
            let army = Army {
                id: a.id,
                uid: a.id as u32,
                name: a.name.clone(),
                leader_name: a.leader_name.clone(),
                description: a.description.clone(),
                model: a.model,
                pos: world.map.center(tile),
                home,
                post: tile,
                // Ships always cruise their waters *(guess)*.
                patrols: a.patrols != 0 || a.ship != 0,
                patrol_radius: if a.ship != 0 && a.patrol_radius == 0 { super::ships::SHIP_PATROL } else { a.patrol_radius as i32 },
                troops,
                faction: a.faction,
                attitude,
                gold: a.gold_income as i32,
                items: artifact_ids(content, a.artifacts.iter().filter(|&&x| x != 0).map(|&x| x as u32)),
                speed: Army::speed_for(a.speed_correction, a.leader_unit as u32),
                budget: 0.0,
                shown_from: None,
                path: Vec::new(),
                chasing: false,
                ignore_until: 0.0,
                met: false,
                rest_until: 0.0,
                named: a.named_character,
                effects: Vec::new(),
                ship: a.ship,
                ai: AiProfile::from_dt(a, &troops_for_ai),
                mind: AiMind::default(),
            };
            // An army with no cell of its kind nearby (a land army far out on the water)
            // waits with the inactive.
            if a.is_active() && placed.is_some() {
                world.armies.push(army);
            } else {
                world.inactive.push(army);
            }
        }
        world.regions = world.map.regions().0;
        world
    }

    /// The buildings given to the hero of `class` at the start (world.md §7): the preset's
    /// start building (byte 16, 1-based) and every building flagged for the class (building
    /// byte 353 + class). Bridges are never given.
    pub fn start_buildings(&self, s: &Scenario, class: HeroClass) -> Vec<usize> {
        let k = match class {
            HeroClass::Knight => 0,
            HeroClass::Archmage => 1,
            HeroClass::Ranger => 2,
        };
        let archetype = [Archetype::Knight, Archetype::Archmage, Archetype::Ranger][k];
        let p = s.header.hero(archetype);
        let ok = |i: usize| self.locations.get(i).is_some_and(|l| !l.kind.is_bridge());
        let preset = (p.start_building as usize).checked_sub(1);
        let flagged = s.buildings.iter().enumerate().filter(|(_, b)| b.start_for[k] != 0).map(|(i, _)| i);
        let mut out: Vec<usize> = preset.into_iter().chain(flagged).filter(|&i| ok(i)).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Where and with what the hero of `class` starts in scenario `s` (its header preset):
    /// exactly on the preset's cell (world.md §7, no relocation); his start buildings
    /// ([`World::start_buildings`]) become his without moving him.
    pub fn hero_start(&self, s: &Scenario, content: &Content, class: HeroClass) -> HeroStart {
        let archetype = match class {
            HeroClass::Knight => Archetype::Knight,
            HeroClass::Archmage => Archetype::Archmage,
            HeroClass::Ranger => Archetype::Ranger,
        };
        let p = s.header.hero(archetype);
        let tile = (p.x as i32, p.y as i32);
        let hero_row = Stats::of_level(content, class.unit(), 1).preferred_row();
        let hero_slot = content.formation.free_slot(&[], hero_row).expect("empty formation");
        let (troops, _) = place_troops(content, &[hero_slot], &dt_entries(&p.troops));
        HeroStart {
            class,
            tile,
            gold: p.gold as u16 as i16 as i32,
            mana: p.mana as u16 as i16 as i32,
            hero_slot,
            troops,
            items: artifact_ids(content, p.artifacts.iter().filter(|&&x| x != 0).map(|&x| x as u32)),
            spells: p.spells.iter().copied().filter(|&x| x != 0).collect(),
            location: self.location_at(tile),
            owned: self.start_buildings(s, class),
        }
    }

    /// Building `l` becomes the player's (a start building, a capture): owner, his faction,
    /// full attitude.
    pub fn give_to_player(&mut self, l: usize) {
        let loc = &mut self.locations[l];
        loc.owner = Owner::Player;
        loc.faction = 1;
        loc.attitude = 3;
    }

    /// The demo kingdom of `data/kingdom.txt`, populated with the built-in demo units.
    pub fn standard(content: &Content) -> Self {
        let u = |key| demo_unit(content, key);
        let (spearman, archer, swordsman, healer) = (u("spearman"), u("archer"), u("swordsman"), u("healer"));
        let (bandit, bandit_archer, bandit_chief) = (u("bandit"), u("bandit_archer"), u("bandit_chief"));
        let map = TileMap::parse(KINGDOM);
        let tile = |c: char| {
            map.markers.iter().find(|(m, _)| *m == c).map(|&(_, t)| t).unwrap_or_else(|| panic!("map has no '{c}'"))
        };
        let t = |unit, row, col| Troop::new(unit, 1, Slot::new(row, col));
        let (f, b) = (Row::Front, Row::Back);
        let recruits = |units: Vec<UnitId>| units.into_iter().map(|unit| Recruit { unit, stock: None, max: 0, progress: 0 }).collect();
        let shop = || Some(Shop { fixed: Vec::new(), random: 6, price: (0, 0), stock: Vec::new() });

        let mut oakford = Location::new(LocationKind::Castle, tr("Oakford"), tile('C'));
        oakford.picture = (3, 0);
        oakford.owner = Owner::Player;
        oakford.faction = 1;
        oakford.attitude = 3;
        oakford.gold_income = 20;
        oakford.recruits = recruits(vec![spearman, archer, healer]);
        oakford.shop = shop();
        let mut greywall = Location::new(LocationKind::Castle, tr("Greywall"), tile('G'));
        greywall.picture = (3, 2);
        greywall.recruits = recruits(vec![swordsman, archer, healer]);
        greywall.shop = shop();
        greywall.spells = vec![3, 5];
        let village = |name, c| {
            let mut v = Location::new(LocationKind::Village, name, tile(c));
            v.gold_income = 10;
            v.gold_max = 10;
            v.tribute_gold = 10;
            // The peasants pray for the hero: mana for the demo's spells.
            v.mana_income = 10;
            v.mana_max = 10;
            v.tribute_mana = 10;
            v.picture = (2, 6);
            v
        };
        let camp = |name, c, garrison, reward, loot| {
            let mut l = Location::new(LocationKind::Camp, name, tile(c));
            l.faction = 4;
            l.attitude = -3;
            l.garrison = garrison;
            l.treasure_gold = reward;
            l.loot_rolls = loot;
            l.picture = (12, 5);
            l
        };
        let locations = vec![
            oakford,
            village(tr("Millbrook"), 'M'),
            village(tr("Ashford"), 'A'),
            village(tr("Saltmarsh"), 'S'),
            Location { picture: (7, 4), spells: vec![1, 2, 4], ..Location::new(LocationKind::Church, tr("St. Beor's church"), tile('+')) },
            greywall,
            camp(
                tr("Bandit camp"),
                'B',
                vec![t(bandit, f, 1), t(bandit, f, 2), t(bandit, f, 3), t(bandit_archer, b, 2), t(bandit_archer, b, 3)],
                100,
                1,
            ),
            camp(
                tr("Bandit lair"),
                'L',
                vec![t(bandit_chief, f, 2), t(bandit, f, 1), t(bandit, f, 3), t(bandit_archer, b, 1), t(bandit_archer, b, 3)],
                150,
                2,
            ),
        ];
        let mut w = World::empty(tr("Demo kingdom"), map, Clock::demo_start());
        w.locations = locations;
        w.demo = true;
        w.gang = gang(content);
        w.place_buildings();
        // Two gangs already on the roads, one from each camp.
        let camp = w.index_of(tr("Bandit camp"));
        let lair = w.index_of(tr("Bandit lair"));
        w.spawn_gang(camp, (39, 16));
        w.spawn_gang(lair, (14, 24));
        w.regions = w.map.regions().0;
        w
    }

    /// Puts back what a save leaves out (the `#[serde(skip)]` fields: the map, the texts and
    /// the indexes) from `fresh`, the same world rebuilt from its scenario or the demo. Fails
    /// when the saved world does not fit it (another map).
    pub fn restore_statics(&mut self, fresh: World) -> Result<(), String> {
        if self.locations.len() != fresh.locations.len() {
            return Err(format!("{} buildings saved, the map has {}", self.locations.len(), fresh.locations.len()));
        }
        for (l, f) in self.locations.iter_mut().zip(&fresh.locations) {
            if l.id != f.id || l.kind != f.kind || l.anchor != f.anchor {
                return Err(format!("building {} does not match the map", l.id));
            }
            l.name.clone_from(&f.name);
            l.owner_name.clone_from(&f.owner_name);
            l.description.clone_from(&f.description);
        }
        let texts: HashMap<u8, &Army> = fresh.armies.iter().chain(fresh.inactive.iter()).filter(|a| a.id != 0).map(|a| (a.id, a)).collect();
        let respawning = self.respawns.iter_mut().map(|r| &mut r.army);
        for a in self.armies.iter_mut().chain(self.inactive.iter_mut()).chain(respawning) {
            match texts.get(&a.id) {
                _ if a.id == 0 => {
                    a.name = tr(GANG_NAME).to_string();
                    if a.speed == 0 {
                        a.speed = GANG_SPEED;
                    }
                }
                Some(f) => {
                    if a.speed == 0 {
                        a.speed = f.speed;
                    }
                    a.name.clone_from(&f.name);
                    a.leader_name.clone_from(&f.leader_name);
                    a.description.clone_from(&f.description);
                    // Saves from before the AI kept no profile: the scenario's.
                    if !a.ai.enabled {
                        a.ai = f.ai.clone();
                    }
                }
                None => return Err(format!("army {} is not on the map", a.id)),
            }
        }
        self.title = fresh.title;
        self.map = fresh.map;
        self.events = fresh.events;
        self.points = fresh.points;
        self.named_characters = fresh.named_characters;
        self.footprints = fresh.footprints;
        self.sea = fresh.sea;
        self.regions = fresh.regions;
        Ok(())
    }

    pub fn index_of(&self, name: &str) -> usize {
        self.locations.iter().position(|l| l.name == name).unwrap_or_else(|| panic!("no location {name}"))
    }

    /// Quests and rumours offered in the main hall of location `l`: its event slots that
    /// hold quest or rumour events. The event engine (`rules::events`) decides which of them
    /// are available; this is only the building's list.
    pub fn local_events(&self, l: usize) -> Vec<EventId> {
        self.locations[l]
            .events
            .iter()
            .copied()
            .filter(|&id| {
                let e = (id as usize).checked_sub(1).and_then(|i| self.events.get(i));
                e.is_some_and(|e| matches!(e.kind, Some(EventKind::Quest | EventKind::Rumour)))
            })
            .collect()
    }

    /// An event's title, if the scenario has it.
    pub fn event_title(&self, id: EventId) -> Option<&str> {
        (id as usize).checked_sub(1).and_then(|i| self.events.get(i)).map(|e| e.title.as_str()).filter(|t| !t.is_empty())
    }

    /// The building (not a bridge) whose footprint covers `t`: standing there is being in it.
    pub fn location_at(&self, t: Tile) -> Option<usize> {
        self.location_covering(t).filter(|&l| !self.locations[l].kind.is_bridge())
    }

    /// Location whose footprint covers `t` (bridges included).
    pub fn location_covering(&self, t: Tile) -> Option<usize> {
        self.footprints.get(&t).copied()
    }

    /// A demo gang from camp `home` at `at`.
    pub fn spawn_gang(&mut self, home: usize, at: Tile) {
        self.next_uid = self.next_uid.max(FIRST_GANG_UID) + 1;
        self.armies.push(Army {
            id: 0,
            uid: self.next_uid,
            name: tr(GANG_NAME).to_string(),
            leader_name: String::new(),
            description: String::new(),
            model: 5,
            pos: self.map.center(at),
            home: Some(home),
            post: self.locations[home].tile,
            patrols: true,
            patrol_radius: GANG_PATROL,
            troops: self.gang.clone(),
            faction: 4,
            attitude: -3,
            gold: GANG_GOLD,
            items: Vec::new(),
            speed: GANG_SPEED,
            budget: 0.0,
            shown_from: None,
            path: Vec::new(),
            chasing: false,
            ignore_until: 0.0,
            met: false,
            rest_until: 0.0,
            named: 0,
            effects: Vec::new(),
            ship: 0,
            ai: AiProfile::default(),
            mind: AiMind::default(),
        });
    }

    /// Both cells lie in the same region on foot (true when regions are not known).
    pub fn same_region(&self, a: Tile, b: Tile) -> bool {
        match (self.map.mask_index(a), self.map.mask_index(b)) {
            (Some(i), Some(j)) if !self.regions.is_empty() => self.regions[i] == self.regions[j],
            (Some(_), Some(_)) => true,
            _ => false,
        }
    }

    pub fn camps(&self) -> impl Iterator<Item = (usize, &Location)> {
        self.locations.iter().enumerate().filter(|(_, l)| l.kind == LocationKind::Camp)
    }

    /// The demo is won when every camp is cleared; worlds without camps are not won this way.
    pub fn all_camps_cleared(&self) -> bool {
        self.camps().next().is_some() && self.camps().all(|(_, l)| l.cleared)
    }

    /// Location closest to `t` on foot among those `pick` accepts, with the path to it.
    pub fn nearest_location(&self, t: Tile, pick: impl Fn(&Location) -> bool) -> Option<(usize, Vec<Tile>)> {
        let mut candidates: Vec<usize> = (0..self.locations.len()).filter(|&i| pick(&self.locations[i])).collect();
        candidates.sort_by_key(|&i| self.map.distance(t, self.locations[i].tile));
        candidates
            .into_iter()
            .take(8)
            .filter_map(|i| {
                let p = self.map.path(t, self.locations[i].tile);
                (!p.is_empty() || self.locations[i].tile == t).then_some((i, p))
            })
            .min_by_key(|(_, p)| self.map.path_minutes(t, p))
    }
}

#[cfg(test)]
pub(crate) mod testkit {
    //! Small synthetic scenarios, built in code.
    use super::*;
    use crate::dt::dtm::{Army as DtArmy, Building, HeroPreset, MapObject, Surface, Troop as DtTroop};

    /// Units 1–3 are the hero classes, 4 a warrior, 5 a shooter; items 7 (ring) and 9 (potion).
    pub fn content() -> Content {
        use crate::rules::content::testkit as ck;
        use crate::rules::content::{ArtefactType, MagicDirection, MagicSchool};
        let units = vec![
            ck::warrior(1, 20, 5),
            ck::mage(2, 10, MagicSchool::Elemental, MagicDirection::ToEnemy),
            ck::shooter(3, 15),
            ck::warrior(4, 10, 2),
            ck::shooter(5, 8),
        ];
        ck::content(units, vec![ck::item(7, ArtefactType::Ring), ck::item(9, ArtefactType::Potion)])
    }

    pub fn troop(unit: u8, level: u8, count: u8) -> DtTroop {
        DtTroop { unit, level, count }
    }

    /// A `w × h` grass scenario starting 1204-05-19 09:00.
    pub fn scenario(w: u32, h: u32) -> Scenario {
        let mut s = Scenario::default();
        s.header.width = w;
        s.header.height = h;
        s.header.start_time = 624_354_300;
        s.header.relations = [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]];
        s.terrain = vec![Surface::GrassPlain as u8; (w * h) as usize];
        s.title = "Test".into();
        s
    }

    pub fn set(s: &mut Scenario, x: u32, y: u32, surface: Surface) {
        let w = s.width();
        s.terrain[(y * w + x) as usize] = surface as u8;
    }

    pub fn object(s: &mut Scenario, x: u16, y: u16, class: u8, sprite: u8) {
        s.objects.push(MapObject { x, y, sprite, class });
    }

    pub fn building(kind: BuildingType, x: u16, y: u16, size: (u8, u8)) -> Building {
        Building { kind: kind as u8, picture_type: kind as u8, x, y, size_x: size.0, size_y: size.1, name: format!("{kind:?}"), ..Building::default() }
    }

    pub fn army(id: u8, x: u16, y: u16, attitude: i8, troops: &[DtTroop]) -> DtArmy {
        let mut t = [DtTroop::default(); 6];
        t[..troops.len()].copy_from_slice(troops);
        DtArmy { id, x, y, model: 4, faction: if attitude < 0 { 4 } else { 3 }, relations: [attitude, 0, 0, 0], troops: t, name: format!("Army {id}"), ..DtArmy::default() }
    }

    pub fn hero(x: u16, y: u16, gold: u32, troops: &[DtTroop]) -> HeroPreset {
        let mut t = [DtTroop::default(); 6];
        t[..troops.len()].copy_from_slice(troops);
        HeroPreset { x, y, gold, troops: t, ..HeroPreset::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;
    use crate::dt::dtm::Surface;
    use crate::rules::content::Content;
    use crate::rules::game::Game;
    use crate::rules::map::object_class;


    #[test]
    fn an_armys_hostility_follows_the_originals_relation() {
        // 0x4a0868 (world.md §4): a = the player's attitude to the army's faction (header
        // matrix), b = the army's to the player; both ≥ 0 → (a + b) / 2, a < 0 → a, else −1.
        assert_eq!([relation(-2, 0), relation(2, 0), relation(1, -1), relation(3, 1)], [-2, 1, -1, 2]);
        let mut s = scenario(20, 6);
        // РК1's "Маг Эктор": faction 4 (enemy), its own attitudes left at 0.
        let mut mage = army(1, 5, 2, 0, &[troop(4, 0, 1)]);
        mage.faction = 4;
        mage.relations = [0, 0, 0, 0];
        let mut friend = army(2, 12, 2, 0, &[troop(4, 0, 1)]);
        friend.faction = 2;
        friend.relations = [0, 0, 0, 0];
        s.armies = vec![mage, friend];
        let w = World::from_scenario(&s, &content());
        assert!(w.armies[0].hostile(), "the player's −2 towards the enemy faction makes it hostile");
        assert!(!w.armies[1].hostile(), "an ally with no attitudes of its own stays friendly");
    }

    #[test]
    fn standard_world_places_every_location_on_a_passable_tile() {
        let w = World::standard(&Content::builtin());
        assert_eq!(w.locations.len(), 8);
        for l in &w.locations {
            assert!(w.map.passable(l.tile), "{} is on impassable ground", l.name);
        }
        assert_eq!(w.location_at(w.locations[0].tile), Some(0));
        assert!(w.locations[0].owned() && !w.locations[0].hostile());
        assert!(w.camps().all(|(_, l)| l.defended()));
    }

    #[test]
    fn every_location_is_reachable_from_home() {
        let w = World::standard(&Content::builtin());
        let home = w.locations[0].tile;
        for l in &w.locations[1..] {
            assert!(!w.map.path(home, l.tile).is_empty(), "{} unreachable", l.name);
        }
    }

    #[test]
    fn two_gangs_start_on_the_map() {
        let w = World::standard(&Content::builtin());
        assert_eq!(w.armies.len(), 2);
        assert!(w.armies.iter().all(|p| w.map.passable(p.tile(&w.map)) && p.hostile()));
    }

    #[test]
    fn scenario_terrain_and_objects_set_passability() {
        let mut s = scenario(8, 6);
        set(&mut s, 1, 0, Surface::DeepSea);
        set(&mut s, 2, 0, Surface::ImpassableSwamp);
        set(&mut s, 3, 0, Surface::ImpassableSnowdrifts);
        set(&mut s, 4, 0, Surface::Road);
        set(&mut s, 5, 0, Surface::Marsh);
        object(&mut s, 0, 2, object_class::MOUNTAINS, 11);
        object(&mut s, 1, 2, object_class::THICKET, 3);
        object(&mut s, 2, 2, object_class::HILLS, 12);
        object(&mut s, 3, 2, object_class::TREES, 3);
        object(&mut s, 4, 2, object_class::DEAD_TREES, 110);
        object(&mut s, 5, 2, object_class::ROCKS, 12);
        let w = World::from_scenario(&s, &content());
        let m = &w.map;
        assert_eq!((m.w, m.h), (8, 6));
        assert!(m.passable((0, 0)));
        assert!(!m.passable((1, 0)) && !m.passable((2, 0)) && !m.passable((3, 0)));
        assert!(m.minutes((4, 0)) < m.minutes((0, 0)), "road is fastest");
        assert!(m.minutes((5, 0)) > m.minutes((0, 0)), "marsh is slow");
        assert!(!m.passable((0, 2)) && !m.passable((1, 2)) && !m.passable((5, 2)));
        for x in 2..5 {
            assert!(m.minutes((x, 2)) > m.minutes((0, 0)), "hills and trees slow ({x})");
        }
        assert_eq!(w.start.label(), "1204, month 5, day 19, 9 h");
    }

    #[test]
    fn building_footprints_are_walked_at_road_speed() {
        let mut s = scenario(10, 10);
        for x in 0..10 {
            set(&mut s, x, 7, Surface::DeepSea);
        }
        let mut castle = building(BuildingType::Castle, 5, 4, (4, 3));
        castle.gold_per_day = 55;
        castle.faction = 4;
        castle.relations = [-2, 0, 0, 0];
        castle.owner_army = 2;
        castle.garrison[0] = troop(4, 0, 2);
        castle.garrison[1] = troop(5, 1, 1);
        castle.garrison_extra_defence = 11;
        castle.barracks[0] = crate::dt::dtm::RecruitSlot { unit: 4, start_count: 3, max_count: 9 };
        castle.has_barracks = 1;
        castle.artifact_slots[0] = 7;
        castle.random_artifacts_for_sale = 2;
        let mut ruins = building(BuildingType::Ruins, 8, 2, (1, 1));
        ruins.artifact_slots[0] = 9;
        ruins.price_max = 300;
        ruins.relations = [-3, 0, 0, 0];
        ruins.garrison[0] = troop(4, 0, 1);
        let bridge = building(BuildingType::StoneBridge, 3, 7, (1, 1));
        let mut village = building(BuildingType::Village, 1, 9, (2, 2));
        village.gold_per_day = 20;
        village.gold_max = 50;
        village.linked_building = 1;
        s.buildings = vec![castle, ruins, bridge, village];
        let w = World::from_scenario(&s, &content());

        let c = &w.locations[0];
        assert_eq!((c.kind, c.id, c.anchor, c.size), (LocationKind::Castle, 1, (5, 4), (4, 3)));
        let cells: Vec<Tile> = c.cells().collect();
        // 4 × 3 up and left of the anchor, plus a row above: wider than tall.
        assert_eq!(cells.len(), 16);
        assert!(cells.contains(&(2, 2)) && cells.contains(&(5, 4)) && cells.contains(&(2, 1)) && !cells.contains(&(1, 4)) && !cells.contains(&(2, 0)));
        assert_eq!(c.tile, (4, 3), "the footprint's centre (x0 + sx/2, y0 + sy/2)");
        assert!(cells.iter().all(|&t| w.map.cost(t) == Some(crate::rules::map::ROAD)), "every cell is road");
        assert!(cells.iter().all(|&t| w.location_at(t) == Some(0)), "any cell is the building");
        assert_eq!(w.location_at((1, 3)), None);
        assert_eq!((c.owner, c.faction, c.gold_income, c.garrison_defence), (Owner::Army(2), 4, 55, 11));
        assert!(c.hostile() && c.defended() && c.bars_hero());
        assert_eq!(c.garrison.iter().map(|t| (t.unit.0, t.level)).collect::<Vec<_>>(), [(4, 1), (4, 1), (5, 2)]);
        assert_eq!(c.garrison[2].slot.row, Row::Back, "the shooter stands behind");
        assert_eq!(c.recruits, vec![Recruit { unit: UnitId(4), stock: Some(3), max: 9, progress: 0 }]);
        assert_eq!(c.shop.as_ref().map(|s| (s.fixed.clone(), s.random)), Some((vec![ItemId(7)], 2)));

        let r = &w.locations[1];
        assert_eq!((r.kind, r.tile, r.treasure.clone(), r.treasure_gold), (LocationKind::Ruins, (8, 2), vec![ItemId(9)], 300));
        assert!(r.shop.is_none() && r.defended() && r.bars_hero());

        let b = &w.locations[2];
        assert!(b.kind.is_bridge() && !b.bars_hero());
        assert!(w.map.passable((3, 7)) && !w.map.passable((2, 7)), "the bridge crosses the water");
        assert_eq!(w.location_at((3, 7)), None, "bridges are not entered");
        assert_eq!(w.location_covering((3, 7)), Some(2));

        let v = &w.locations[3];
        assert_eq!((v.kind, v.tile, v.linked, v.tribute_gold), (LocationKind::Village, (1, 9), Some(0), 20));
        assert!(!v.hostile() && !v.defended() && !v.bars_hero(), "villages are walked through");
        // Every building type maps to its kind.
        for t in BuildingType::ALL {
            assert_eq!(format!("{t:?}").replace("DungeonEntrance", "Entrance"), format!("{:?}", LocationKind::from_building(t)));
        }
    }

    #[test]
    fn ill_disposed_castles_forts_unowned_ruins_and_enemy_buildings_bar_the_way() {
        let mut l = Location::new(LocationKind::Fort, "Fort", (0, 0));
        for (attitude, bars) in [(-2, true), (0, true), (1, false)] {
            l.attitude = attitude;
            assert_eq!(l.bars_hero(), bars, "attitude {attitude}");
        }
        l.attitude = -3;
        l.owner = Owner::Player;
        assert!(!l.bars_hero(), "his own");
        let mut r = Location::new(LocationKind::Ruins, "Ruins", (0, 0));
        r.attitude = 3;
        assert!(r.bars_hero(), "ruins not his, whatever their attitude");
        r.cleared = true;
        assert!(!r.bars_hero());
        for k in [LocationKind::Town, LocationKind::Village, LocationKind::Church, LocationKind::Tavern, LocationKind::Palace] {
            let mut t = Location::new(k, "", (0, 0));
            t.attitude = -3;
            assert!(t.bars_hero(), "an enemy {k:?}: the route goes around it");
            t.attitude = 0;
            assert!(!t.bars_hero(), "a neutral {k:?} is walked through");
        }
        let mut b = Location::new(LocationKind::StoneBridge, "", (0, 0));
        b.attitude = -3;
        assert!(!b.bars_hero(), "bridges never bar the way");
    }

    #[test]
    fn a_fort_at_the_foot_of_a_bridge_is_walked_through() {
        // Water on rows 4 and 5, a bridge across at x = 5; the fort (4..=5, 6..=7) closes
        // the gap between impassable bogs on the south bank.
        let mut s = scenario(10, 10);
        for x in 0..10 {
            set(&mut s, x, 4, Surface::DeepSea);
            set(&mut s, x, 5, Surface::DeepSea);
        }
        for x in [0, 1, 2, 3, 6, 7, 8, 9] {
            set(&mut s, x, 6, Surface::ImpassableSwamp);
            set(&mut s, x, 7, Surface::ImpassableSwamp);
        }
        let fort = building(BuildingType::Fort, 5, 7, (2, 2));
        let bridges = [building(BuildingType::StoneBridge, 5, 4, (1, 1)), building(BuildingType::StoneBridge, 5, 5, (1, 1))];
        s.buildings = vec![fort, building(BuildingType::Village, 2, 9, (1, 1)), bridges[0].clone(), bridges[1].clone()];
        let w = World::from_scenario(&s, &content());
        let path = w.map.path((2, 9), (2, 1));
        assert!(!path.is_empty(), "north over the bridge, through the fort's footprint");
        assert!(path.iter().any(|&t| w.location_at(t) == Some(0)), "{path:?}");
    }

    #[test]
    fn main_halls_list_their_quests_and_rumours() {
        use crate::dt::dtm::Event as DtEvent;
        let mut s = scenario(6, 6);
        let ev = |kind, title: &str| DtEvent { kind, title: title.into(), ..DtEvent::default() };
        s.events = vec![ev(3, "A quest%+flag"), ev(2, "Local"), ev(4, "Rumour"), ev(1, "Global")];
        let mut t = building(BuildingType::Town, 2, 2, (1, 1));
        t.event_slots[..4].copy_from_slice(&[1, 2, 3, 9]);
        t.event_count = 4;
        s.buildings = vec![t];
        let w = World::from_scenario(&s, &content());
        assert_eq!(w.local_events(0), vec![1, 3], "quests and rumours; local events fire by themselves");
        assert_eq!(w.event_title(1), Some("A quest"), "without the flag script");
        assert_eq!(w.event_title(9), None);
    }

    #[test]
    fn only_active_armies_start_on_the_map_and_hostility_follows_attitude() {
        let mut s = scenario(12, 12);
        let mut foe = army(1, 2, 2, -2, &[troop(4, 0, 3), troop(5, 2, 2)]);
        foe.leader_unit = 1;
        foe.patrols = 1;
        foe.patrol_radius = 6;
        foe.gold_income = 80;
        foe.unknown_80 = 3;
        foe.artifacts = [7, 0, 0];
        let friend = army(2, 8, 8, 1, &[troop(4, 0, 1)]);
        let mut sleeper = army(3, 5, 5, -2, &[troop(4, 0, 1)]);
        sleeper.model = 7;
        sleeper.inactive = 1;
        let mut unknown = army(4, 6, 6, -1, &[troop(99, 0, 2)]);
        unknown.leader_unit = 0;
        s.armies = vec![foe, friend, sleeper, unknown];
        let w = World::from_scenario(&s, &content());
        assert_eq!(w.armies.iter().map(|a| a.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(w.inactive.iter().map(|a| a.id).collect::<Vec<_>>(), [3]);
        assert_eq!(w.dropped_units, 2, "the unknown unit type is dropped");
        let a = &w.armies[0];
        assert!(a.hostile() && !w.armies[1].hostile());
        assert_eq!(a.tile(&w.map), (2, 2));
        assert_eq!(a.troops.len(), 6, "leader + 3 + 2");
        assert_eq!(a.leader(), Some(UnitId(1)));
        assert_eq!(a.troops.iter().filter(|t| t.unit == UnitId(5)).map(|t| t.level).collect::<Vec<_>>(), [3, 3]);
        assert!(a.troops.iter().all(|t| t.slot.row == if t.unit == UnitId(5) { Row::Back } else { Row::Front }));
        assert_eq!((a.patrols, a.patrol_radius, a.gold, a.items.clone()), (true, 6, 80, vec![ItemId(7)]), "word 17 is its starting gold");
        assert_eq!(a.ai.extra_income, 30, "byte 80 × 10 is its daily income");
        assert_eq!(a.speed, 5);
    }

    #[test]
    fn army_speed_is_five_minus_the_correction() {
        assert_eq!(Army::speed_for(0, 4), 5);
        assert_eq!(Army::speed_for(1, 4), 4, "one point is 20% of the time");
        assert_eq!(Army::speed_for(-3, 4), 8);
        assert_eq!(Army::speed_for(7, 4), 1, "at least 1");
        assert_eq!(Army::speed_for(0, 2), 4, "led by the Archmage unit: one less");
        assert_eq!(Army::speed_for(4, 2), 1);
    }

    #[test]
    fn hero_starts_exactly_on_the_preset_cell() {
        let mut s = scenario(12, 12);
        let mut fort = building(BuildingType::Fort, 6, 6, (2, 2));
        fort.faction = 3;
        fort.relations = [-1, 0, 0, 0];
        s.buildings = vec![fort];
        s.header.heroes[0] = hero(3, 3, 150, &[troop(4, 0, 2), troop(5, 0, 1)]);
        s.header.heroes[0].artifacts = [7, 0, 0];
        s.header.heroes[0].mana = 100;
        // The archmage starts on a cell of the fort, his start building: he stays there and
        // the fort is his.
        s.header.heroes[1] = hero(6, 5, 500, &[troop(4, 0, 1)]);
        s.header.heroes[1].start_building = 1;
        let c = content();
        let w = World::from_scenario(&s, &c);
        let k = w.hero_start(&s, &c, HeroClass::Knight);
        assert_eq!((k.tile, k.gold, k.mana, k.items.clone(), k.location, k.owned.clone()), ((3, 3), 150, 100, vec![ItemId(7)], None, vec![]));
        assert_eq!(k.troops.len(), 3);
        assert!(k.troops.iter().all(|t| t.slot != k.hero_slot));
        let m = w.hero_start(&s, &c, HeroClass::Archmage);
        assert_eq!((m.tile, m.gold, m.location, m.owned.clone()), ((6, 5), 500, Some(0), vec![0]));
        let g = Game::from_scenario(std::sync::Arc::new(c), &s, HeroClass::Archmage, 1);
        let f = &g.world.locations[0];
        assert_eq!((g.tile(), g.location, f.owner, f.faction, f.attitude), ((6, 5), Some(0), Owner::Player, 1, 3));
    }
}

#[cfg(test)]
mod real_maps {
    //! Checks against the player's install; skipped without `RAZDOR_DT_DIR`.
    use super::*;
    use crate::dt::install::DtInstall;
    use crate::rules::game::{Event, Game};
    use std::sync::Arc;

    fn install() -> Option<(DtInstall, Arc<Content>)> {
        let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Arc::new(Content::from_dt(&dt));
        Some((dt, c))
    }

    /// Non-bridge buildings whose entry the hero of `class` can walk to from his start.
    fn reachable_entries(w: &World, start: Tile) -> (usize, usize) {
        let reach = w.map.reachable(start);
        let entries: Vec<&Location> = w.locations.iter().filter(|l| !l.kind.is_bridge()).collect();
        let ok = entries.iter().filter(|l| w.map.mask_index(l.tile).is_some_and(|i| reach[i])).count();
        (ok, entries.len())
    }

    #[test]
    fn every_shipped_map_loads_into_a_world() {
        let Some((dt, c)) = install() else { return };
        let mut totals = (0, 0);
        for m in &dt.maps {
            let s = m.load().unwrap();
            let w = World::from_scenario(&s, &c);
            assert_eq!((w.map.w, w.map.h), (s.width() as i32, s.height() as i32), "{}", m.name);
            assert_eq!(w.locations.len(), s.buildings.len(), "{}", m.name);
            let manned = s.armies.iter().filter(|a| a.leader_unit != 0 || a.troops().next().is_some()).count();
            assert_eq!(w.armies.len() + w.inactive.len(), manned, "{}: every army with troops", m.name);
            // Every unit type is known; one РК6 garrison lists 13 units, one more than a formation holds.
            assert!(w.dropped_units <= 1, "{}: {} units dropped", m.name, w.dropped_units);
            assert!(w.armies.iter().all(|a| if a.sails() { w.is_sea(a.tile(&w.map)) } else { w.map.passable(a.tile(&w.map)) }), "{}", m.name);
            assert!(w.locations.iter().all(|l| w.map.passable(l.tile)), "{}", m.name);
            for class in HeroClass::ALL {
                let h = w.hero_start(&s, &c, class);
                // Exactly the preset: on land, or at sea ("Тихая пристань" starts on the shallows).
                assert!(w.map.passable(h.tile) || w.is_sea(h.tile), "{} {class:?}", m.name);
                let (ok, n) = reachable_entries(&w, h.tile);
                totals.0 += ok;
                totals.1 += n;
                let g = Game::from_scenario(c.clone(), &s, class, 1);
                // Opening events may change the preset (a companion joins, gold is given).
                if g.script().is_some_and(|e| e.total_fired() == 0) {
                    assert_eq!(g.squad.len(), 1 + h.troops.len());
                    assert_eq!(g.gold, h.gold);
                }
            }
        }
        // On foot alone; with ships every building is reachable (see below).
        assert!(totals.0 * 100 / totals.1 >= 80, "{totals:?}");
    }

    /// Buildings (not bridges) whose entry the hero can reach by land and sea from `start`
    /// ([`World::reachable_with_ships`]): (reached, all, ids of the others).
    fn reachable_by_ship(w: &World, start: Tile) -> (usize, usize, Vec<u16>) {
        let reach = w.reachable_with_ships(start);
        let entries: Vec<&Location> = w.locations.iter().filter(|l| !l.kind.is_bridge()).collect();
        let missed: Vec<u16> = entries.iter().filter(|l| !w.map.mask_index(l.tile).is_some_and(|i| reach[i])).map(|l| l.id).collect();
        (entries.len() - missed.len(), entries.len(), missed)
    }

    #[test]
    fn every_building_is_reachable_by_land_and_sea() {
        let Some((dt, c)) = install() else { return };
        let mut report = Vec::new();
        for m in &dt.maps {
            let s = m.load().unwrap();
            let w = World::from_scenario(&s, &c);
            for class in HeroClass::ALL {
                let h = w.hero_start(&s, &c, class);
                let (ok, n, missed) = reachable_by_ship(&w, h.tile);
                report.push((m.name.clone(), class, ok, n, missed));
            }
        }
        for (name, class, ok, n, missed) in &report {
            eprintln!("{name} {class:?}: {ok}/{n} buildings; unreached ids {missed:?}");
        }
        // Every map, except (with the original's movement rules, world.md §1):
        // - the second tutorial's church (building 15), in a ring of dense thickets and bog;
        // - the first tutorial's three villages, across a river crossed by fords: shallows
        //   are water, sailed but not walked, and the tutorial has no shipyard;
        // - РК3/РК5's altar (100/99), shut in by massifs (a square of `sprite div 10` cells
        //   each) and a lake with no shipyard;
        // - РК7's eastern island (21, 24–31), ringed by deep sea, which ships cannot sail.
        // Scripted events (teleports) are not considered.
        for (name, class, ok, n, missed) in &report {
            let expected: &[u16] = if name.starts_with("Обучающий2") {
                &[15]
            } else if name.starts_with("Обучающий1") {
                &[5, 6, 7]
            } else if name.starts_with("РК3") {
                &[100]
            } else if name.starts_with("РК5") {
                &[99]
            } else if name.starts_with("РК7") {
                &[21, 24, 25, 26, 27, 28, 29, 30, 31]
            } else {
                &[]
            };
            assert_eq!(missed.as_slice(), expected, "{name} {class:?}: {ok}/{n}");
        }
    }

    #[test]
    fn rk1_and_rk3_are_walkable_from_every_start() {
        let Some((dt, c)) = install() else { return };
        for prefix in ["РК1", "РК3"] {
            let s = dt.maps.iter().find(|m| m.name.starts_with(prefix)).unwrap().load().unwrap();
            let w = World::from_scenario(&s, &c);
            for class in HeroClass::ALL {
                let h = w.hero_start(&s, &c, class);
                assert!(w.map.passable(h.tile), "{prefix} {class:?}");
                let (ok, n) = reachable_entries(&w, h.tile);
                // РК3's altar is shut in (see above).
                let shut = usize::from(prefix == "РК3");
                assert_eq!(ok, n - shut, "{prefix} {class:?}: every building is reachable on foot or over bridges");
            }
        }
    }

    #[test]
    fn auto_walk_to_the_nearest_village_terminates() {
        let Some((dt, c)) = install() else { return };
        for prefix in ["РК1", "РК3"] {
            let s = dt.maps.iter().find(|m| m.name.starts_with(prefix)).unwrap().load().unwrap();
            let mut g = Game::from_scenario(c.clone(), &s, HeroClass::Knight, 7);
            let (village, _) = g.world.nearest_location(g.tile(), |l| l.kind == LocationKind::Village).expect("a village");
            let target = g.world.locations[village].tile;
            assert!(g.set_destination(target), "{prefix}");
            let start = g.clock.total_minutes();
            let mut events = Vec::new();
            for _ in 0..20_000 {
                if !g.moving() || g.foe.is_some() {
                    break;
                }
                events.extend(g.tick(0.05));
            }
            assert!(!g.moving() || g.foe.is_some(), "{prefix}: the walk ends");
            assert!(g.clock.total_minutes() > start);
            let arrived = events.contains(&Event::Arrived(village)) && g.location == Some(village);
            let met = events.iter().any(|e| matches!(e, Event::Encounter(_) | Event::Met(_)));
            assert!(arrived || met, "{prefix}: arrives, or is stopped by an army: {events:?}");
        }
    }
}



