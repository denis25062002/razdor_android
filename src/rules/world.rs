//! The world map: terrain, buildings (locations), armies and the hero's start.
//!
//! Built either from an original scenario ([`World::from_scenario`], `docs/reference/dtm-format.md`)
//! or from the built-in demo kingdom ([`World::standard`], `data/kingdom.txt`), through the
//! same types.

use std::collections::HashMap;

use crate::dt::dtm::{self, Archetype, BuildingType, Scenario};

use super::clock::Clock;
use super::content::{Content, HeroClass, ItemId, UnitId};
use super::formation::{Row, Slot};
use super::map::{Decoration, Grid, Tile, TileMap, MIN_MINUTES};
use super::units::Stats;

const KINGDOM: &str = include_str!("../../data/kingdom.txt");

/// Attitude value at or below which a side attacks the player (relations run −3..3).
pub const HOSTILE_BELOW: i8 = 0;

/// A unit in an army or a garrison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Troop {
    pub unit: UnitId,
    /// 1 as hired.
    pub level: i32,
    pub slot: Slot,
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
                    out.push(Troop { unit: id, level: level.max(1), slot });
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
            Palace => "Palace",
            Town => "Town",
            Village => "Village",
            Castle => "Castle",
            Fort => "Fort",
            Tavern => "Tavern",
            Market => "Market",
            Church => "Church",
            Smithy => "Smithy",
            Shipyard => "Shipyard",
            Altar => "Altar",
            Entrance => "Dungeon entrance",
            Ruins => "Ruins",
            StoneBridge => "Stone bridge",
            WoodenBridge => "Wooden bridge",
            Obelisk => "Obelisk",
            Camp => "Bandit camp",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Player,
    /// An army of the scenario, by its 1-based id.
    Army(u8),
    /// The building's own (neutral) owner.
    Neutral,
}

/// A unit type a barracks offers. `stock: None` = unlimited (the demo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recruit {
    pub unit: UnitId,
    pub stock: Option<i32>,
    pub max: i32,
}

/// Items for sale.
#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug)]
pub struct Location {
    /// 1-based building id in the scenario (events refer to it); 0 in the demo.
    pub id: u16,
    pub name: String,
    /// The neutral owner's name (a village headman, a lord …).
    pub owner_name: String,
    pub description: String,
    pub kind: LocationKind,
    /// Entry cell: stepping onto it enters the building.
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
    pub garrison: Vec<Troop>,
    pub garrison_defence: i32,
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

    /// Footprint cells: `size` cells up and to the left of the anchor.
    pub fn cells(&self) -> impl Iterator<Item = Tile> + '_ {
        let (ax, ay) = self.anchor;
        (0..self.size.1).flat_map(move |j| (0..self.size.0).map(move |i| (ax - i, ay - j)))
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

    /// Income the owner receives each day (villages pay tribute instead).
    pub fn pays_income(&self) -> bool {
        !matches!(self.kind, LocationKind::Village)
    }

    /// Midnight: the village tribute grows by a day's worth, up to the maximum.
    pub fn refill(&mut self) {
        if self.kind == LocationKind::Village {
            self.tribute_gold = (self.tribute_gold + self.gold_income).min(self.gold_max.max(self.gold_income));
            self.tribute_mana = (self.tribute_mana + self.mana_income).min(self.mana_max.max(self.mana_income));
        }
    }
}

/// An army on the map (an AI lord, a gang, peasants) or, in the demo, a bandit gang.
#[derive(Clone, Debug)]
pub struct Army {
    /// 1-based army id in the scenario; 0 for the demo's gangs.
    pub id: u8,
    pub name: String,
    pub leader_name: String,
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
    /// Multiplier on terrain costs (from the editor's speed correction).
    pub slowness: f32,
    pub path: Vec<Tile>,
    pub chasing: bool,
    /// Game minute until which it leaves the player alone (after a stalemate).
    pub ignore_until: f64,
    /// Already greeted the player (friendly meeting shown once while nearby).
    pub met: bool,
    /// Game minute until which it stands still between patrol legs.
    pub rest_until: f64,
}

impl Army {
    pub fn tile(&self, map: &TileMap) -> Tile {
        map.tile_at(self.pos)
    }

    /// Attacks the player on contact and chases him.
    pub fn hostile(&self) -> bool {
        self.attitude < HOSTILE_BELOW
    }

    /// The troop in the middle of the front row, else the first one.
    pub fn leader(&self) -> Option<UnitId> {
        self.troops.first().map(|t| t.unit)
    }

    /// Terrain-cost multiplier for the editor's speed correction (about −3..+5): 10% per
    /// point *(guess)*.
    pub fn slowness_for(correction: i8) -> f32 {
        (1.0 / (1.0 + 0.1 * correction as f32)).clamp(0.5, 2.0)
    }
}

/// Where and with what the hero starts.
#[derive(Clone, Debug, PartialEq)]
pub struct HeroStart {
    pub class: HeroClass,
    pub tile: Tile,
    pub gold: i32,
    pub experience: i32,
    /// Troops besides the hero; the hero stands in `hero_slot`.
    pub hero_slot: Slot,
    pub troops: Vec<Troop>,
    pub items: Vec<ItemId>,
    pub spells: Vec<u8>,
    /// Location the hero starts in, if any.
    pub location: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct World {
    pub title: String,
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
    entries: HashMap<Tile, usize>,
    footprints: HashMap<Tile, usize>,
}

pub const GANG_REWARD: i32 = 30;
/// Demo gangs carry this much gold; the victor takes `VictoryGoldDiv` of it.
const GANG_GOLD: i32 = 2 * GANG_REWARD;
const GANG_SLOWNESS: f32 = 1.25;
const GANG_PATROL: i32 = 8;

/// A demo unit type by its `Key=`. Panics if the built-in data lacks it.
pub fn demo_unit(content: &Content, key: &str) -> UnitId {
    content.unit_by_key(key).unwrap_or_else(|| panic!("demo unit '{key}' missing"))
}

/// A roaming gang: two bandits in front, an archer behind.
pub fn gang(content: &Content) -> Vec<Troop> {
    let (bandit, archer) = (demo_unit(content, "bandit"), demo_unit(content, "bandit_archer"));
    let t = |unit, row, col| Troop { unit, level: 1, slot: Slot::new(row, col) };
    vec![t(bandit, Row::Front, 2), t(bandit, Row::Front, 3), t(archer, Row::Back, 2)]
}

fn artifact_ids(content: &Content, ids: impl Iterator<Item = u32>) -> Vec<ItemId> {
    ids.map(ItemId).filter(|&i| content.try_item(i).is_some()).collect()
}

/// The entry cell of a building *(guess)*: a footprint cell next to open ground outside, the
/// one opening onto the largest connected region winning (so a building between a pocket and
/// the open land opens onto the land), then one where a road arrives, then the one nearest to
/// the middle of the bottom row; with no open side, that middle cell. The shipped maps lead
/// roads to their buildings from below or from a side. `regions` labels the map with every
/// building's walls in place ([`TileMap::regions`]).
pub fn choose_entry(map: &TileMap, regions: &(Vec<u32>, Vec<usize>), l: &Location) -> Tile {
    let cells: Vec<Tile> = l.cells().collect();
    let default = (l.anchor.0 - (l.size.0 - 1) / 2, l.anchor.1);
    let (label, sizes) = regions;
    let region_size = |n: Tile| map.mask_index(n).map(|i| label[i]).filter(|&r| r != u32::MAX).map_or(0, |r| sizes[r as usize]);
    cells
        .iter()
        .filter_map(|&t| {
            let outside: Vec<Tile> = map.grid.neighbours(t).filter(|n| map.in_bounds(*n) && !cells.contains(n)).collect();
            let open = outside.iter().map(|&n| region_size(n)).max().unwrap_or(0);
            let road = outside.iter().any(|&n| map.surface(n) == super::map::Surface::Road && map.passable(n));
            (open > 0).then_some((t, (open, road, -map.distance(t, default), t.1, -(t.0 - default.0).abs())))
        })
        .max_by_key(|(_, k)| *k)
        .map_or(default, |(t, _)| t)
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
            entries: HashMap::new(),
            footprints: HashMap::new(),
        }
    }

    /// Indexes footprints and entries, and carves the buildings into the map: bridges become
    /// road, a building's walls block, its entry cell stays open.
    fn place_buildings(&mut self) {
        self.entries.clear();
        self.footprints.clear();
        for (i, l) in self.locations.iter().enumerate() {
            let cells: Vec<Tile> = l.cells().filter(|&t| self.map.in_bounds(t)).collect();
            for &t in &cells {
                self.footprints.insert(t, i);
                if l.kind.is_bridge() {
                    self.map.open(t, MIN_MINUTES);
                } else if t != l.tile {
                    self.map.block(t);
                }
            }
            if !l.kind.is_bridge() {
                self.entries.insert(l.tile, i);
                if !self.map.passable(l.tile) {
                    // An entry on impassable ground (an island fort) stays reachable where it
                    // can be; the cost is ordinary ground *(guess)*.
                    self.map.open(l.tile, 60);
                }
            }
        }
    }

    /// The world of an original scenario.
    ///
    /// - Terrain and objects set the cell costs (`map` module).
    /// - Every building becomes a location of its type with its footprint (bottom-right
    ///   anchor, `size_x × size_y`) and an entry cell (see [`choose_entry`]).
    /// - Active land armies are placed on the map, the others (and ships) wait in `inactive`.
    ///   Hostility is the army's own attitude towards the player (< 0 attacks).
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
                    .map(|r| Recruit { unit: UnitId(r.unit as u32), stock: Some(r.start_count as i32), max: r.max_count as i32 })
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
        // Entries: judged on the map with every building's walls up.
        let mut walled = world.map.clone();
        for l in &world.locations {
            for t in l.cells() {
                if l.kind.is_bridge() {
                    walled.open(t, MIN_MINUTES);
                } else {
                    walled.block(t);
                }
            }
        }
        let regions = walled.regions();
        for l in world.locations.iter_mut().filter(|l| !l.kind.is_bridge()) {
            l.tile = choose_entry(&walled, &regions, l);
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
            let mut tile = (a.x as i32, a.y as i32);
            if !world.map.passable(tile) {
                tile = world.map.nearest_passable(tile, 8).unwrap_or(tile);
            }
            let home = (a.home_building as usize).checked_sub(1).filter(|&j| j < world.locations.len());
            let army = Army {
                id: a.id,
                name: a.name.clone(),
                leader_name: a.leader_name.clone(),
                description: a.description.clone(),
                model: a.model,
                pos: world.map.center(tile),
                home,
                post: tile,
                patrols: a.patrols != 0,
                patrol_radius: a.patrol_radius as i32,
                troops,
                faction: a.faction,
                attitude: a.relations[0],
                gold: a.gold_income as i32,
                items: artifact_ids(content, a.artifacts.iter().filter(|&&x| x != 0).map(|&x| x as u32)),
                slowness: Army::slowness_for(a.speed_correction),
                path: Vec::new(),
                chasing: false,
                ignore_until: 0.0,
                met: false,
                rest_until: 0.0,
            };
            // Ships (pirates, merchants) are not simulated yet: they wait with the inactive, as
            // does an army placed far out on the water.
            if a.is_active() && a.ship == 0 && world.map.passable(tile) {
                world.armies.push(army);
            } else {
                world.inactive.push(army);
            }
        }
        world
    }

    /// Where and with what the hero of `class` starts in scenario `s` (its header preset).
    /// A start inside a building's walls moves to the building's entry, else to the nearest
    /// open cell.
    pub fn hero_start(&self, s: &Scenario, content: &Content, class: HeroClass) -> HeroStart {
        let archetype = match class {
            HeroClass::Knight => Archetype::Knight,
            HeroClass::Archmage => Archetype::Archmage,
            HeroClass::Ranger => Archetype::Ranger,
        };
        let p = s.header.hero(archetype);
        let mut tile = (p.x as i32, p.y as i32);
        if let Some(&l) = self.footprints.get(&tile) {
            if !self.map.passable(tile) {
                tile = self.locations[l].tile;
            }
        }
        if !self.map.passable(tile) {
            tile = self.map.nearest_passable(tile, 6).unwrap_or(tile);
        }
        let hero_row = Stats::of_level(content, class.unit(), 1).preferred_row();
        let hero_slot = content.formation.free_slot(&[], hero_row).expect("empty formation");
        let (troops, _) = place_troops(content, &[hero_slot], &dt_entries(&p.troops));
        HeroStart {
            class,
            tile,
            gold: p.gold as i32,
            experience: p.experience as i32,
            hero_slot,
            troops,
            items: artifact_ids(content, p.artifacts.iter().filter(|&&x| x != 0).map(|&x| x as u32)),
            spells: p.spells.iter().copied().filter(|&x| x != 0).collect(),
            location: self.location_at(tile),
        }
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
        let t = |unit, row, col| Troop { unit, level: 1, slot: Slot::new(row, col) };
        let (f, b) = (Row::Front, Row::Back);
        let recruits = |units: Vec<UnitId>| units.into_iter().map(|unit| Recruit { unit, stock: None, max: 0 }).collect();
        let shop = || Some(Shop { fixed: Vec::new(), random: 6, price: (0, 0), stock: Vec::new() });

        let mut oakford = Location::new(LocationKind::Castle, "Oakford", tile('C'));
        oakford.picture = (3, 0);
        oakford.owner = Owner::Player;
        oakford.faction = 1;
        oakford.attitude = 3;
        oakford.gold_income = 20;
        oakford.recruits = recruits(vec![spearman, archer, healer]);
        oakford.shop = shop();
        let mut greywall = Location::new(LocationKind::Castle, "Greywall", tile('G'));
        greywall.picture = (3, 2);
        greywall.recruits = recruits(vec![swordsman, archer, healer]);
        greywall.shop = shop();
        let village = |name, c| {
            let mut v = Location::new(LocationKind::Village, name, tile(c));
            v.gold_income = 10;
            v.gold_max = 10;
            v.tribute_gold = 10;
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
            village("Millbrook", 'M'),
            village("Ashford", 'A'),
            village("Saltmarsh", 'S'),
            Location { picture: (7, 4), ..Location::new(LocationKind::Church, "St. Beor's church", tile('+')) },
            greywall,
            camp(
                "Bandit camp",
                'B',
                vec![t(bandit, f, 1), t(bandit, f, 2), t(bandit, f, 3), t(bandit_archer, b, 2), t(bandit_archer, b, 3)],
                100,
                1,
            ),
            camp(
                "Bandit lair",
                'L',
                vec![t(bandit_chief, f, 2), t(bandit, f, 1), t(bandit, f, 3), t(bandit_archer, b, 1), t(bandit_archer, b, 3)],
                150,
                2,
            ),
        ];
        let mut w = World::empty("Demo kingdom", map, Clock::demo_start());
        w.locations = locations;
        w.demo = true;
        w.gang = gang(content);
        w.place_buildings();
        // Two gangs already on the roads, one from each camp.
        let camp = w.index_of("Bandit camp");
        let lair = w.index_of("Bandit lair");
        w.spawn_gang(camp, (39, 16));
        w.spawn_gang(lair, (14, 24));
        w
    }

    pub fn index_of(&self, name: &str) -> usize {
        self.locations.iter().position(|l| l.name == name).unwrap_or_else(|| panic!("no location {name}"))
    }

    /// Location whose entry is `t`.
    pub fn location_at(&self, t: Tile) -> Option<usize> {
        self.entries.get(&t).copied()
    }

    /// Location whose footprint covers `t` (bridges included).
    pub fn location_covering(&self, t: Tile) -> Option<usize> {
        self.footprints.get(&t).copied()
    }

    /// A demo gang from camp `home` at `at`.
    pub fn spawn_gang(&mut self, home: usize, at: Tile) {
        self.armies.push(Army {
            id: 0,
            name: "Bandit gang".to_string(),
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
            slowness: GANG_SLOWNESS,
            path: Vec::new(),
            chasing: false,
            ignore_until: 0.0,
            met: false,
            rest_until: 0.0,
        });
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
    use crate::rules::map::object_class;


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
    fn building_footprints_and_entries() {
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
        assert_eq!(cells.len(), 12);
        assert!(cells.contains(&(2, 2)) && cells.contains(&(5, 4)) && !cells.contains(&(1, 4)));
        assert_eq!(c.tile, (4, 4), "entry: middle of the bottom row");
        assert!(w.map.passable(c.tile));
        assert!(cells.iter().filter(|&&t| t != c.tile).all(|&t| !w.map.passable(t)), "walls block");
        assert_eq!(w.location_at((4, 4)), Some(0));
        assert_eq!(w.location_at((3, 3)), None);
        assert_eq!(w.location_covering((3, 3)), Some(0));
        assert_eq!((c.owner, c.faction, c.gold_income, c.garrison_defence), (Owner::Army(2), 4, 55, 11));
        assert!(c.hostile() && c.defended());
        assert_eq!(c.garrison.iter().map(|t| (t.unit.0, t.level)).collect::<Vec<_>>(), [(4, 1), (4, 1), (5, 2)]);
        assert_eq!(c.garrison[2].slot.row, Row::Back, "the shooter stands behind");
        assert_eq!(c.recruits, vec![Recruit { unit: UnitId(4), stock: Some(3), max: 9 }]);
        assert_eq!(c.shop.as_ref().map(|s| (s.fixed.clone(), s.random)), Some((vec![ItemId(7)], 2)));

        let r = &w.locations[1];
        assert_eq!((r.kind, r.tile, r.treasure.clone(), r.treasure_gold), (LocationKind::Ruins, (8, 2), vec![ItemId(9)], 300));
        assert!(r.shop.is_none() && r.defended());

        let b = &w.locations[2];
        assert!(b.kind.is_bridge());
        assert!(w.map.passable((3, 7)) && !w.map.passable((2, 7)), "the bridge crosses the water");
        assert_eq!(w.location_at((3, 7)), None, "bridges are not entered");

        let v = &w.locations[3];
        assert_eq!((v.kind, v.tile, v.linked, v.tribute_gold), (LocationKind::Village, (1, 9), Some(0), 20));
        assert!(!v.hostile() && !v.defended());
        // Every building type maps to its kind.
        for t in BuildingType::ALL {
            assert_eq!(format!("{t:?}").replace("DungeonEntrance", "Entrance"), format!("{:?}", LocationKind::from_building(t)));
        }
    }

    #[test]
    fn village_tribute_refills_up_to_the_maximum() {
        let mut s = scenario(4, 4);
        let mut v = building(BuildingType::Village, 1, 1, (1, 1));
        v.gold_per_day = 20;
        v.gold_max = 50;
        v.mana_per_day = 5;
        v.mana_max = 8;
        s.buildings = vec![v];
        let mut w = World::from_scenario(&s, &content());
        let v = &mut w.locations[0];
        v.refill();
        v.refill();
        assert_eq!((v.tribute_gold, v.tribute_mana), (50, 8));
    }

    #[test]
    fn only_active_armies_start_on_the_map_and_hostility_follows_attitude() {
        let mut s = scenario(12, 12);
        let mut foe = army(1, 2, 2, -2, &[troop(4, 0, 3), troop(5, 2, 2)]);
        foe.leader_unit = 1;
        foe.patrols = 1;
        foe.patrol_radius = 6;
        foe.gold_income = 80;
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
        assert_eq!((a.patrols, a.patrol_radius, a.gold, a.items.clone()), (true, 6, 80, vec![ItemId(7)]));
    }

    #[test]
    fn hero_starts_from_the_class_preset() {
        let mut s = scenario(12, 12);
        let mut fort = building(BuildingType::Fort, 6, 6, (2, 2));
        fort.faction = 1;
        fort.relations = [3, 0, 0, 0];
        s.buildings = vec![fort];
        s.header.heroes[0] = hero(3, 3, 150, &[troop(4, 0, 2), troop(5, 0, 1)]);
        s.header.heroes[0].artifacts = [7, 0, 0];
        s.header.heroes[0].experience = 100;
        // The archmage starts inside the fort's walls: moved to its entry.
        s.header.heroes[1] = hero(6, 5, 500, &[troop(4, 0, 1)]);
        s.header.heroes[1].start_building = 1;
        let c = content();
        let w = World::from_scenario(&s, &c);
        let k = w.hero_start(&s, &c, HeroClass::Knight);
        assert_eq!((k.tile, k.gold, k.experience, k.items.clone(), k.location), ((3, 3), 150, 100, vec![ItemId(7)], None));
        assert_eq!(k.troops.len(), 3);
        assert!(k.troops.iter().all(|t| t.slot != k.hero_slot));
        let m = w.hero_start(&s, &c, HeroClass::Archmage);
        assert_eq!((m.tile, m.gold, m.location), (w.locations[0].tile, 500, Some(0)));
        assert!(w.locations[0].owned());
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
            assert!(w.armies.iter().all(|a| w.map.passable(a.tile(&w.map))), "{}", m.name);
            assert!(w.locations.iter().all(|l| w.map.passable(l.tile)), "{}", m.name);
            for class in HeroClass::ALL {
                let h = w.hero_start(&s, &c, class);
                assert!(w.map.passable(h.tile), "{} {class:?}", m.name);
                let (ok, n) = reachable_entries(&w, h.tile);
                totals.0 += ok;
                totals.1 += n;
                let g = Game::from_scenario(c.clone(), &s, class, 1);
                assert_eq!(g.squad.len(), 1 + h.troops.len());
                assert_eq!(g.gold, h.gold);
            }
        }
        // Maps with islands need ships (not in yet); the rest is walkable.
        assert!(totals.0 * 100 / totals.1 >= 80, "{totals:?}");
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
                assert_eq!(ok, n, "{prefix} {class:?}: every building entry is reachable on foot or over bridges");
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
