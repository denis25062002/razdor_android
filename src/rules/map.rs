//! Hex terrain grid and pathfinding for the world map.
//!
//! Pointy-top hexes in "odd-r" offset coordinates: tile `(col, row)`, odd rows shifted
//! half a hex to the right. World positions are in units where neighbouring hex centres
//! are exactly 1 apart.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

pub type Tile = (i32, i32);

/// Vertical distance between hex rows, in world units.
pub const ROW_HEIGHT: f32 = 0.866_025_4; // √3 / 2

/// Centre of a hex in world units.
pub fn center((c, r): Tile) -> (f32, f32) {
    (c as f32 + 0.5 * r.rem_euclid(2) as f32, r as f32 * ROW_HEIGHT)
}

fn to_cube((c, r): Tile) -> (i32, i32, i32) {
    let x = c - (r - r.rem_euclid(2)) / 2;
    (x, -x - r, r)
}

/// Steps between two hexes.
pub fn hex_distance(a: Tile, b: Tile) -> i32 {
    let (a, b) = (to_cube(a), to_cube(b));
    (a.0 - b.0).abs().max((a.1 - b.1).abs()).max((a.2 - b.2).abs())
}

/// The six neighbours of a hex (in bounds or not).
pub fn hex_neighbours((c, r): Tile) -> [Tile; 6] {
    let o = r.rem_euclid(2); // odd rows are shifted right
    [(c - 1, r), (c + 1, r), (c - 1 + o, r - 1), (c + o, r - 1), (c - 1 + o, r + 1), (c + o, r + 1)]
}

/// Hex containing a world position.
pub fn tile_at(p: (f32, f32)) -> Tile {
    let r = (p.1 / ROW_HEIGHT).round() as i32;
    let c = (p.0 - 0.5 * r.rem_euclid(2) as f32).round() as i32;
    // The rounded guess is right or off by one neighbour: pick the nearest centre.
    std::iter::once((c, r))
        .chain(hex_neighbours((c, r)))
        .min_by(|&a, &b| {
            let d = |t| {
                let (x, y) = center(t);
                (x - p.0).powi(2) + (y - p.1).powi(2)
            };
            d(a).total_cmp(&d(b))
        })
        .unwrap()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terrain {
    Road,
    Grass,
    Forest,
    Swamp,
    Water,
    Mountain,
}

impl Terrain {
    fn from_char(c: char) -> Terrain {
        match c {
            '=' => Terrain::Road,
            'T' => Terrain::Forest,
            ',' => Terrain::Swamp,
            '~' => Terrain::Water,
            '^' => Terrain::Mountain,
            // '.' and location letters.
            _ => Terrain::Grass,
        }
    }

    /// Game minutes to walk one tile, `None` if impassable.
    pub fn minutes(self) -> Option<f32> {
        match self {
            Terrain::Road => Some(30.0),
            Terrain::Grass => Some(60.0),
            Terrain::Forest => Some(150.0),
            Terrain::Swamp => Some(180.0),
            Terrain::Water | Terrain::Mountain => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TileMap {
    pub w: i32,
    pub h: i32,
    tiles: Vec<Terrain>,
    /// Non-terrain characters (location markers) and where they are.
    pub markers: Vec<(char, Tile)>,
}

impl TileMap {
    /// Parses rows of terrain characters; location letters stand on road.
    pub fn parse(text: &str) -> TileMap {
        let rows: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        let h = rows.len() as i32;
        let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as i32;
        let mut tiles = vec![Terrain::Grass; (w * h) as usize];
        let mut markers = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                let t = if ".=T,~^".contains(c) {
                    Terrain::from_char(c)
                } else {
                    markers.push((c, (x as i32, y as i32)));
                    Terrain::Road
                };
                tiles[y * w as usize + x] = t;
            }
        }
        TileMap { w, h, tiles, markers }
    }

    pub fn in_bounds(&self, (x, y): Tile) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    pub fn terrain(&self, t: Tile) -> Terrain {
        if self.in_bounds(t) {
            self.tiles[(t.1 * self.w + t.0) as usize]
        } else {
            Terrain::Water
        }
    }

    pub fn passable(&self, t: Tile) -> bool {
        self.terrain(t).minutes().is_some()
    }

    fn neighbours(&self, t: Tile) -> impl Iterator<Item = Tile> + '_ {
        hex_neighbours(t).into_iter().filter(|&n| self.passable(n))
    }

    /// Cheapest path from `from` to `to` (A*), excluding `from`. Empty if unreachable or equal.
    pub fn path(&self, from: Tile, to: Tile) -> Vec<Tile> {
        if from == to || !self.passable(to) {
            return Vec::new();
        }
        // Road is the cheapest terrain, so it gives an admissible heuristic.
        let h = |t: Tile| 30.0 * hex_distance(t, to) as f32;
        // Costs in whole minutes keep the heap ordering exact.
        let mut open = BinaryHeap::from([Reverse(((h(from)) as u32, 0u32, from))]);
        let mut best: HashMap<Tile, u32> = HashMap::from([(from, 0)]);
        let mut parent: HashMap<Tile, Tile> = HashMap::new();
        while let Some(Reverse((_, g, t))) = open.pop() {
            if t == to {
                let mut path = vec![to];
                let mut cur = to;
                while let Some(&p) = parent.get(&cur) {
                    if p == from {
                        break;
                    }
                    path.push(p);
                    cur = p;
                }
                path.reverse();
                return path;
            }
            if g > best[&t] {
                continue;
            }
            for n in self.neighbours(t) {
                let ng = g + self.terrain(n).minutes().unwrap_or(0.0) as u32;
                if best.get(&n).is_none_or(|&old| ng < old) {
                    best.insert(n, ng);
                    parent.insert(n, t);
                    open.push(Reverse((ng + h(n) as u32, ng, n)));
                }
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Odd rows are drawn shifted right:
    //   . . . . .
    //    . ~ ~ ~ .
    //   . ~ X ~ .
    //    . = = = =
    //   T T T T T
    const MAP: &str = "\
.....
.~~~.
.~X~.
.====
TTTTT
";

    #[test]
    fn parses_terrain_and_markers() {
        let m = TileMap::parse(MAP);
        assert_eq!((m.w, m.h), (5, 5));
        assert_eq!(m.terrain((1, 1)), Terrain::Water);
        assert_eq!(m.terrain((1, 3)), Terrain::Road);
        assert_eq!(m.terrain((0, 4)), Terrain::Forest);
        assert_eq!(m.markers, vec![('X', (2, 2))]);
        assert_eq!(m.terrain((2, 2)), Terrain::Road);
    }

    #[test]
    fn hex_neighbours_depend_on_row_parity() {
        // Even row: the row above/below sits half a hex to the left.
        assert_eq!(hex_neighbours((2, 2)), [(1, 2), (3, 2), (1, 1), (2, 1), (1, 3), (2, 3)]);
        // Odd row: shifted right.
        assert_eq!(hex_neighbours((2, 1)), [(1, 1), (3, 1), (2, 0), (3, 0), (2, 2), (3, 2)]);
        for n in hex_neighbours((2, 1)) {
            assert_eq!(hex_distance((2, 1), n), 1);
            let (a, b) = (center((2, 1)), center(n));
            assert!(((a.0 - b.0).hypot(a.1 - b.1) - 1.0).abs() < 1e-4, "neighbour centres are 1 apart");
        }
        assert_eq!(hex_distance((0, 0), (4, 0)), 4);
        assert_eq!(hex_distance((0, 0), (0, 4)), 4);
    }

    #[test]
    fn tile_at_inverts_center() {
        for r in 0..6 {
            for c in 0..6 {
                assert_eq!(tile_at(center((c, r))), (c, r));
                let (x, y) = center((c, r));
                assert_eq!(tile_at((x + 0.3, y - 0.3)), (c, r), "near the centre stays inside");
            }
        }
    }

    #[test]
    fn path_avoids_water_and_prefers_road() {
        let m = TileMap::parse(MAP);
        let p = m.path((0, 3), (4, 3));
        assert_eq!(p, vec![(1, 3), (2, 3), (3, 3), (4, 3)], "straight along the road");
        let p = m.path((0, 0), (4, 4));
        assert!(p.iter().all(|&t| m.passable(t)));
        for w in p.windows(2) {
            assert_eq!(hex_distance(w[0], w[1]), 1, "every step is to a neighbouring hex");
        }
    }

    #[test]
    fn no_path_into_water_or_to_self() {
        let m = TileMap::parse(MAP);
        assert!(m.path((0, 0), (1, 1)).is_empty());
        assert!(m.path((0, 0), (0, 0)).is_empty());
    }
}
