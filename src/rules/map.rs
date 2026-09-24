//! Terrain grid and pathfinding for the world map.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

pub type Tile = (i32, i32);

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

    /// Minutes to step from `a` onto neighbouring tile `b` (diagonals cost √2 more).
    pub fn step_minutes(&self, a: Tile, b: Tile) -> Option<f32> {
        let base = self.terrain(b).minutes()?;
        let diagonal = a.0 != b.0 && a.1 != b.1;
        Some(if diagonal { base * std::f32::consts::SQRT_2 } else { base })
    }

    fn neighbours(&self, t: Tile) -> impl Iterator<Item = Tile> + '_ {
        (-1..=1).flat_map(move |dy| (-1..=1).map(move |dx| (dx, dy))).filter_map(move |(dx, dy)| {
            let n = (t.0 + dx, t.1 + dy);
            // No corner cutting past impassable tiles.
            let ok = (dx, dy) != (0, 0)
                && self.passable(n)
                && (dx == 0 || dy == 0 || (self.passable((t.0 + dx, t.1)) && self.passable((t.0, t.1 + dy))));
            ok.then_some(n)
        })
    }

    /// Cheapest path from `from` to `to` (A*), excluding `from`. Empty if unreachable or equal.
    pub fn path(&self, from: Tile, to: Tile) -> Vec<Tile> {
        if from == to || !self.passable(to) {
            return Vec::new();
        }
        // Road is the cheapest terrain, so it gives an admissible heuristic.
        let h = |t: Tile| {
            let (dx, dy) = ((t.0 - to.0).abs() as f32, (t.1 - to.1).abs() as f32);
            30.0 * (dx.max(dy) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dy))
        };
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
                let ng = g + self.step_minutes(t, n).unwrap_or(0.0).round() as u32;
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
    fn path_avoids_water_and_prefers_road() {
        let m = TileMap::parse(MAP);
        let p = m.path((0, 3), (4, 3));
        assert_eq!(p.last(), Some(&(4, 3)));
        assert!(p.iter().all(|&t| m.passable(t)));
        // Along the road (4 steps), not through the forest below.
        assert_eq!(p, vec![(1, 3), (2, 3), (3, 3), (4, 3)]);
    }

    #[test]
    fn no_path_into_water_or_to_self() {
        let m = TileMap::parse(MAP);
        assert!(m.path((0, 0), (1, 1)).is_empty());
        assert!(m.path((0, 0), (0, 0)).is_empty());
    }

    #[test]
    fn no_corner_cutting_past_water() {
        let m = TileMap::parse(MAP);
        // (4,2) -> (3,3) is diagonal; the corner (3,2) is water, so the step is not allowed.
        assert!(m.neighbours((4, 2)).all(|n| n != (3, 3)));
        assert!(m.neighbours((4, 2)).any(|n| n == (4, 3)));
    }
}
