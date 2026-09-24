use super::formation::{Row, Slot};
use super::items::ItemId;
use super::map::{center, tile_at, Tile, TileMap};
use super::units::UnitKind;

const KINGDOM: &str = include_str!("../../data/kingdom.txt");

#[derive(Clone, Debug)]
pub enum LocationKind {
    /// `owned` castles pay `income` every midnight. `market` is the stock for sale.
    Castle { recruits: Vec<UnitKind>, income: i32, owned: bool, market: Vec<ItemId> },
    /// Once per day: tribute, or the priest heals the squad instead.
    Village { tribute: i32, used_on_day: Option<u32> },
    Church,
    /// `loot`: items dropped when cleared.
    Camp { enemies: Vec<(UnitKind, Slot)>, reward: i32, loot: u32 },
}

#[derive(Clone, Debug)]
pub struct Location {
    pub name: &'static str,
    pub tile: Tile,
    pub kind: LocationKind,
    pub cleared: bool,
}

/// A bandit gang roaming the map.
#[derive(Clone, Debug)]
pub struct Party {
    /// Position in world units (see `map::center`).
    pub pos: (f32, f32),
    /// Camp it belongs to (index into `locations`).
    pub home: usize,
    pub enemies: Vec<(UnitKind, Slot)>,
    pub path: Vec<Tile>,
    pub chasing: bool,
    /// Game minute until which it leaves the player alone (after a stalemate).
    pub ignore_until: f64,
}

impl Party {
    pub fn tile(&self) -> Tile {
        tile_at(self.pos)
    }
}

#[derive(Clone, Debug)]
pub struct World {
    pub map: TileMap,
    pub locations: Vec<Location>,
    pub parties: Vec<Party>,
}

pub const GANG_REWARD: i32 = 30;

pub fn gang() -> Vec<(UnitKind, Slot)> {
    use UnitKind::*;
    vec![(Bandit, Slot::new(Row::Front, 2)), (Bandit, Slot::new(Row::Front, 3)), (BanditArcher, Slot::new(Row::Back, 2))]
}

impl World {
    pub fn standard() -> Self {
        use UnitKind::*;
        let map = TileMap::parse(KINGDOM);
        let tile = |c: char| {
            map.markers.iter().find(|(m, _)| *m == c).map(|&(_, t)| t).unwrap_or_else(|| panic!("map has no '{c}'"))
        };
        let f = |col| Slot::new(Row::Front, col);
        let b = |col| Slot::new(Row::Back, col);
        let loc = |name, c, kind| Location { name, tile: tile(c), kind, cleared: false };
        let village = || LocationKind::Village { tribute: 10, used_on_day: None };
        let locations = vec![
            loc(
                "Oakford",
                'C',
                LocationKind::Castle {
                    recruits: vec![Spearman, Archer, Healer],
                    income: 20,
                    owned: true,
                    market: Vec::new(),
                },
            ),
            loc("Millbrook", 'M', village()),
            loc("Ashford", 'A', village()),
            loc("Saltmarsh", 'S', village()),
            loc("St. Beor's church", '+', LocationKind::Church),
            loc(
                "Greywall",
                'G',
                LocationKind::Castle {
                    recruits: vec![Swordsman, Archer, Healer],
                    income: 0,
                    owned: false,
                    market: Vec::new(),
                },
            ),
            loc(
                "Bandit camp",
                'B',
                LocationKind::Camp {
                    enemies: vec![(Bandit, f(1)), (Bandit, f(2)), (Bandit, f(3)), (BanditArcher, b(2)), (BanditArcher, b(3))],
                    reward: 100,
                    loot: 1,
                },
            ),
            loc(
                "Bandit lair",
                'L',
                LocationKind::Camp {
                    enemies: vec![
                        (BanditChief, f(2)),
                        (Bandit, f(1)),
                        (Bandit, f(3)),
                        (BanditArcher, b(1)),
                        (BanditArcher, b(3)),
                    ],
                    reward: 150,
                    loot: 2,
                },
            ),
        ];
        let mut w = World { map, locations, parties: Vec::new() };
        // Two gangs already on the roads, one from each camp.
        let camp = w.index_of("Bandit camp");
        let lair = w.index_of("Bandit lair");
        w.spawn_party(camp, (39, 16));
        w.spawn_party(lair, (14, 24));
        w
    }

    pub fn index_of(&self, name: &str) -> usize {
        self.locations.iter().position(|l| l.name == name).unwrap_or_else(|| panic!("no location {name}"))
    }

    pub fn location_at(&self, t: Tile) -> Option<usize> {
        self.locations.iter().position(|l| l.tile == t)
    }

    pub fn spawn_party(&mut self, home: usize, at: Tile) {
        self.parties.push(Party {
            pos: center(at),
            home,
            enemies: gang(),
            path: Vec::new(),
            chasing: false,
            ignore_until: 0.0,
        });
    }

    pub fn camps(&self) -> impl Iterator<Item = (usize, &Location)> {
        self.locations.iter().enumerate().filter(|(_, l)| matches!(l.kind, LocationKind::Camp { .. }))
    }

    pub fn all_camps_cleared(&self) -> bool {
        self.camps().all(|(_, l)| l.cleared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_world_places_every_location_on_a_passable_tile() {
        let w = World::standard();
        assert_eq!(w.locations.len(), 8);
        for l in &w.locations {
            assert!(w.map.passable(l.tile), "{} is on impassable ground", l.name);
        }
        assert_eq!(w.location_at(w.locations[0].tile), Some(0));
    }

    #[test]
    fn every_location_is_reachable_from_home() {
        let w = World::standard();
        let home = w.locations[0].tile;
        for l in &w.locations[1..] {
            assert!(!w.map.path(home, l.tile).is_empty(), "{} unreachable", l.name);
        }
    }

    #[test]
    fn two_gangs_start_on_the_map() {
        let w = World::standard();
        assert_eq!(w.parties.len(), 2);
        assert!(w.parties.iter().all(|p| w.map.passable(p.tile())));
    }
}
