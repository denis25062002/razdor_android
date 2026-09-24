use super::formation::{Row, Slot};
use super::units::UnitKind;

#[derive(Clone, Debug)]
pub enum LocationKind {
    Town { recruits: Vec<UnitKind> },
    Camp { enemies: Vec<(UnitKind, Slot)>, reward: i32 },
}

#[derive(Clone, Debug)]
pub struct Location {
    pub name: &'static str,
    /// Position on the map in 0..1 screen-independent coordinates.
    pub pos: (f32, f32),
    pub kind: LocationKind,
    pub cleared: bool,
}

#[derive(Clone, Debug)]
pub struct World {
    pub locations: Vec<Location>,
    pub roads: Vec<(usize, usize)>,
}

pub const HOME: usize = 0;

impl World {
    pub fn standard() -> Self {
        use UnitKind::*;
        let f = |col| Slot::new(Row::Front, col);
        let b = |col| Slot::new(Row::Back, col);
        let loc = |name, pos, kind| Location { name, pos, kind, cleared: false };
        World {
            locations: vec![
                loc("Oakford", (0.18, 0.70), LocationKind::Town { recruits: vec![Spearman, Archer, Healer] }),
                loc(
                    "Bandit camp",
                    (0.45, 0.30),
                    LocationKind::Camp {
                        enemies: vec![
                            (Bandit, f(1)),
                            (Bandit, f(2)),
                            (Bandit, f(3)),
                            (BanditArcher, b(2)),
                            (BanditArcher, b(3)),
                        ],
                        reward: 100,
                    },
                ),
                loc("Greywall", (0.55, 0.78), LocationKind::Town { recruits: vec![Swordsman, Archer, Healer] }),
                loc(
                    "Bandit lair",
                    (0.84, 0.42),
                    LocationKind::Camp {
                        enemies: vec![
                            (BanditChief, f(2)),
                            (Bandit, f(1)),
                            (Bandit, f(3)),
                            (BanditArcher, b(1)),
                            (BanditArcher, b(3)),
                        ],
                        reward: 150,
                    },
                ),
            ],
            roads: vec![(0, 1), (0, 2), (1, 3), (2, 3)],
        }
    }

    pub fn connected(&self, a: usize, b: usize) -> bool {
        self.roads.iter().any(|&(x, y)| (x, y) == (a, b) || (y, x) == (a, b))
    }

    pub fn all_camps_cleared(&self) -> bool {
        self.locations
            .iter()
            .all(|l| !matches!(l.kind, LocationKind::Camp { .. }) || l.cleared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roads_are_bidirectional() {
        let w = World::standard();
        assert!(w.connected(0, 1) && w.connected(1, 0));
        assert!(!w.connected(0, 3));
    }
}
