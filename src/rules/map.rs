//! Cell grid of the world map and pathfinding.
//!
//! The original stores a plain W×H grid. Its editor draws the grid as plain 32×22 px cells
//! (`Graphics/Editor/Grid*.tga`, no stagger), its path arrows (`Way_Arrows.ugs`) and map
//! sprites have 8 directions, its roads are thin 4-connected strokes with diagonal steps, and
//! its 1×1 bridge pieces run diagonally across rivers one column per row, which no staggered
//! (hex) layout connects. So scenarios use [`Grid::Square8`]: rectangular cells, 8 neighbours,
//! travel time proportional to the distance walked *(guess, from that evidence; the files do
//! not state the topology)*. The built-in demo keeps its hex layout ([`Grid::HexOddR`], odd
//! rows shifted half a cell right), as `data/kingdom.txt` was drawn for it.
//!
//! Tile `(col, row)`; world positions are in units where one column is 1 wide. Each cell has
//! a surface (the original's terrain code, `dt::dtm::Surface`) and a travel cost in game
//! minutes, derived from the surface and the map objects on it.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub use crate::dt::dtm::Surface;

pub type Tile = (i32, i32);

/// Vertical distance between hex rows, in world units.
pub const ROW_HEIGHT: f32 = 0.866_025_4; // √3 / 2
/// Vertical distance between rows of the original's 32×22 px cells, in world units.
pub const SQUARE_ROW_HEIGHT: f32 = 22.0 / 32.0;

/// Centre of a hex (odd-r) in world units.
fn hex_center((c, r): Tile) -> (f32, f32) {
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

/// Every hex within `radius` steps of `t` (in bounds or not), `t` included.
pub fn hex_disk(t: Tile, radius: i32) -> impl Iterator<Item = Tile> {
    let r = radius.max(0);
    (t.1 - r..=t.1 + r)
        .flat_map(move |y| (t.0 - r - 1..=t.0 + r + 1).map(move |x| (x, y)))
        .filter(move |&n| hex_distance(t, n) <= r)
}

/// Hex containing a world position.
fn hex_tile_at(p: (f32, f32)) -> Tile {
    let r = (p.1 / ROW_HEIGHT).round() as i32;
    let c = (p.0 - 0.5 * r.rem_euclid(2) as f32).round() as i32;
    // The rounded guess is right or off by one neighbour: pick the nearest centre.
    std::iter::once((c, r))
        .chain(hex_neighbours((c, r)))
        .min_by(|&a, &b| {
            let d = |t| {
                let (x, y) = hex_center(t);
                (x - p.0).powi(2) + (y - p.1).powi(2)
            };
            d(a).total_cmp(&d(b))
        })
        .unwrap()
}

/// Topology and geometry of the cell grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grid {
    /// Pointy-top hexes, odd rows shifted half a cell right (the built-in demo).
    HexOddR,
    /// The original's rectangular cells (32×22 px), 8 neighbours.
    Square8,
}

impl Grid {
    /// Vertical distance between rows, in world units.
    pub fn row_height(self) -> f32 {
        match self {
            Grid::HexOddR => ROW_HEIGHT,
            Grid::Square8 => SQUARE_ROW_HEIGHT,
        }
    }

    /// Horizontal offset of a row, in world units (hex odd rows are shifted).
    pub fn row_offset(self, r: i32) -> f32 {
        match self {
            Grid::HexOddR => 0.5 * r.rem_euclid(2) as f32,
            Grid::Square8 => 0.0,
        }
    }

    /// Centre of a cell in world units.
    pub fn center(self, t: Tile) -> (f32, f32) {
        match self {
            Grid::HexOddR => hex_center(t),
            Grid::Square8 => (t.0 as f32, t.1 as f32 * SQUARE_ROW_HEIGHT),
        }
    }

    /// Cell containing a world position.
    pub fn tile_at(self, p: (f32, f32)) -> Tile {
        match self {
            Grid::HexOddR => hex_tile_at(p),
            Grid::Square8 => (p.0.round() as i32, (p.1 / SQUARE_ROW_HEIGHT).round() as i32),
        }
    }

    /// The neighbours of a cell (in bounds or not): 6 for hexes, 8 for squares.
    pub fn neighbours(self, (c, r): Tile) -> impl Iterator<Item = Tile> {
        let mut out = [(0, 0); 8];
        let n = match self {
            Grid::HexOddR => {
                out[..6].copy_from_slice(&hex_neighbours((c, r)));
                6
            }
            Grid::Square8 => {
                out = [(c - 1, r), (c + 1, r), (c, r - 1), (c, r + 1), (c - 1, r - 1), (c + 1, r - 1), (c - 1, r + 1), (c + 1, r + 1)];
                8
            }
        };
        out.into_iter().take(n)
    }

    /// Steps between two cells.
    pub fn distance(self, a: Tile, b: Tile) -> i32 {
        match self {
            Grid::HexOddR => hex_distance(a, b),
            Grid::Square8 => (a.0 - b.0).abs().max((a.1 - b.1).abs()),
        }
    }

    /// Every cell within `radius` steps of `t` (in bounds or not), `t` included.
    pub fn disk(self, t: Tile, radius: i32) -> Vec<Tile> {
        match self {
            Grid::HexOddR => hex_disk(t, radius).collect(),
            Grid::Square8 => {
                let r = radius.max(0);
                (t.1 - r..=t.1 + r).flat_map(|y| (t.0 - r..=t.0 + r).map(move |x| (x, y))).collect()
            }
        }
    }

    /// Length of the step between two neighbouring cells, in world units.
    pub fn step_length(self, a: Tile, b: Tile) -> f32 {
        let (p, q) = (self.center(a), self.center(b));
        (p.0 - q.0).hypot(p.1 - q.1)
    }
}

/// Minutes to walk onto a cell of this surface on foot, `None` if impassable.
///
/// The original's per-terrain costs are not in its data files or found in the exe; these
/// are *(guess)*: road fastest, grass and fields normal, soils a little slower, sand, marsh,
/// shallows, lava and snow slow; coastal and deep water, the impassable swamp and the
/// impassable snowdrifts block.
pub fn surface_minutes(s: Surface) -> Option<u16> {
    use Surface::*;
    Some(match s {
        Road => 30,
        GrassLowland | GrassPlain | DryPlain => 60,
        ClaySoil | StonySoil | ScorchedLand => 75,
        SandDunes => 90,
        Marsh | ShallowsFords | LavaFields | SnowyGround => 120,
        CoastalWater | DeepSea | ImpassableSwamp | ImpassableSnowdrifts => return None,
    })
}

/// Open water a ship sails on: coastal water and deep sea (shallows and fords are walked).
pub fn is_water(s: Surface) -> bool {
    matches!(s, Surface::CoastalWater | Surface::DeepSea)
}

/// Cheapest cost of any passable cell, for the A* heuristic.
pub const MIN_MINUTES: u16 = 30;

/// What a map object does to the cells it covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectEffect {
    /// Extra travel time in percent.
    Slow(u16),
    Block,
}

/// Map object classes (`.DTm` objects, `Objects.ugs` section A categories).
pub mod object_class {
    pub const HILLS: u8 = 1;
    pub const GREEN_HILL: u8 = 2;
    pub const ROCKY_HILLS: u8 = 3;
    pub const YELLOW_HILL: u8 = 4;
    pub const MOUNTAINS: u8 = 5;
    pub const DARK_MOUNTAINS: u8 = 6;
    pub const ROCKS: u8 = 8;
    pub const TREES: u8 = 9;
    pub const DEAD_TREES: u8 = 10;
    pub const THICKET: u8 = 11;
}

/// Effect of an object class *(guess for the numbers; which classes block and which slow
/// follows the manual)*: mountains, dense thickets and rocks block; hills and trees slow.
pub fn object_effect(class: u8) -> Option<ObjectEffect> {
    use object_class::*;
    match class {
        MOUNTAINS | DARK_MOUNTAINS | THICKET | ROCKS => Some(ObjectEffect::Block),
        HILLS | GREEN_HILL | ROCKY_HILLS | YELLOW_HILL | TREES | DEAD_TREES => Some(ObjectEffect::Slow(50)),
        _ => None,
    }
}

/// How many cells around its base an object covers. Hills, mountains and rocks come in
/// size families by the tens digit of the sprite id (10–19 are 64 px wide, 20–29 96 px, …
/// 60 is 224 px, about `family + 1` cells of 32 px). A massif covers a disk of radius
/// `(family − 1) / 2` whose bottom is the object's cell, i.e. the object stands at the front
/// (bottom) of its sprite *(guess: the smallest cover that keeps the stony massif areas
/// mostly closed without cutting the roads between them)*. Trees cover their own cell.
pub fn object_radius(class: u8, sprite: u8) -> i32 {
    use object_class::*;
    match class {
        HILLS | GREEN_HILL | ROCKY_HILLS | YELLOW_HILL | MOUNTAINS | DARK_MOUNTAINS | ROCKS => ((sprite / 10) as i32 - 1).max(0) / 2,
        _ => 0,
    }
}

/// Cells an object covers: [`object_radius`] around a centre `radius` rows above it.
pub fn object_cells(grid: Grid, o: &Decoration) -> Vec<Tile> {
    let r = object_radius(o.class, o.sprite);
    grid.disk((o.tile.0, o.tile.1 - r), r)
}

/// A map object (hill, mountain, tree, rock) standing on a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decoration {
    pub tile: Tile,
    pub class: u8,
    pub sprite: u8,
}

#[derive(Clone, Debug)]
pub struct TileMap {
    pub grid: Grid,
    pub w: i32,
    pub h: i32,
    surface: Vec<u8>,
    /// Minutes to walk onto the cell; 0 = impassable.
    cost: Vec<u16>,
    /// Sorted by (row, col).
    pub objects: Vec<Decoration>,
    /// `objects[row_start[r]..row_start[r + 1]]` stand in row `r`.
    row_start: Vec<usize>,
    /// Non-terrain characters of a text map (location markers) and where they are.
    pub markers: Vec<(char, Tile)>,
}

/// An empty 0×0 map: the placeholder a loaded save holds until the real map is rebuilt.
impl Default for TileMap {
    fn default() -> TileMap {
        TileMap::from_codes(Grid::Square8, 0, 0, &[], Vec::new())
    }
}

impl TileMap {
    /// A map from terrain codes (`w*h`, row by row) and objects. Costs follow
    /// [`surface_minutes`], [`object_effect`] and [`object_radius`].
    pub fn from_codes(grid: Grid, w: i32, h: i32, codes: &[u8], objects: Vec<Decoration>) -> TileMap {
        assert_eq!(codes.len(), (w * h) as usize, "terrain size");
        let mut objects = objects;
        objects.retain(|o| o.tile.0 >= 0 && o.tile.1 >= 0 && o.tile.0 < w && o.tile.1 < h);
        objects.sort_by_key(|o| (o.tile.1, o.tile.0));
        let mut row_start = Vec::with_capacity(h as usize + 1);
        let mut i = 0;
        for r in 0..=h {
            while i < objects.len() && objects[i].tile.1 < r {
                i += 1;
            }
            row_start.push(i);
        }
        let mut m = TileMap {
            grid,
            w,
            h,
            surface: codes.to_vec(),
            cost: vec![0; codes.len()],
            objects,
            row_start,
            markers: Vec::new(),
        };
        let mut slow = vec![0u16; codes.len()];
        let mut blocked = vec![false; codes.len()];
        for o in &m.objects {
            let Some(effect) = object_effect(o.class) else { continue };
            for t in object_cells(grid, o) {
                let Some(i) = m.index(t) else { continue };
                match effect {
                    ObjectEffect::Block => blocked[i] = true,
                    ObjectEffect::Slow(p) => slow[i] = slow[i].max(p),
                }
            }
        }
        for i in 0..codes.len() {
            let base = Surface::from_code(codes[i]).and_then(surface_minutes);
            // Roads stay open under objects: the shipped maps put a few dozen thickets on
            // roads, and no mountain *(guess)*.
            if codes[i] == Surface::Road as u8 {
                blocked[i] = false;
            }
            m.cost[i] = match base {
                Some(b) if !blocked[i] => (b as u32 * (100 + slow[i] as u32) / 100) as u16,
                _ => 0,
            };
        }
        m
    }

    /// Parses rows of terrain characters (the built-in demo's `data/kingdom.txt`):
    /// `.` grass, `=` road, `T` forest (grass with a tree), `,` marsh, `~` deep water,
    /// `^` mountain (stony soil with a mountain). Other characters are location markers
    /// standing on road.
    pub fn parse(text: &str) -> TileMap {
        let rows: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        let h = rows.len() as i32;
        let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as i32;
        let mut codes = vec![Surface::GrassPlain as u8; (w * h) as usize];
        let mut objects = Vec::new();
        let mut markers = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                let tile = (x as i32, y as i32);
                let (surface, object) = match c {
                    '.' => (Surface::GrassPlain, None),
                    '=' => (Surface::Road, None),
                    'T' => (Surface::GrassPlain, Some((object_class::TREES, (x * 7 + y * 3) as u8 % 9))),
                    ',' => (Surface::Marsh, None),
                    '~' => (Surface::DeepSea, None),
                    '^' => (Surface::StonySoil, Some((object_class::MOUNTAINS, 10 + (x + y) as u8 % 4))),
                    _ => {
                        markers.push((c, tile));
                        (Surface::Road, None)
                    }
                };
                codes[y * w as usize + x] = surface as u8;
                if let Some((class, sprite)) = object {
                    objects.push(Decoration { tile, class, sprite });
                }
            }
        }
        let mut m = TileMap::from_codes(Grid::HexOddR, w, h, &codes, objects);
        m.markers = markers;
        m
    }

    pub fn in_bounds(&self, (x, y): Tile) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }

    fn index(&self, t: Tile) -> Option<usize> {
        self.in_bounds(t).then(|| (t.1 * self.w + t.0) as usize)
    }

    fn tile_of(&self, i: usize) -> Tile {
        (i as i32 % self.w, i as i32 / self.w)
    }

    /// Terrain code of a cell (deep sea outside the map).
    pub fn surface_code(&self, t: Tile) -> u8 {
        self.index(t).map_or(Surface::DeepSea as u8, |i| self.surface[i])
    }

    pub fn surface(&self, t: Tile) -> Surface {
        Surface::from_code(self.surface_code(t)).unwrap_or(Surface::DeepSea)
    }

    /// Minutes to walk onto the cell, `None` if impassable.
    pub fn minutes(&self, t: Tile) -> Option<u16> {
        self.index(t).map(|i| self.cost[i]).filter(|&c| c > 0)
    }

    pub fn passable(&self, t: Tile) -> bool {
        self.minutes(t).is_some()
    }

    /// Makes a cell impassable (a building's walls).
    pub fn block(&mut self, t: Tile) {
        if let Some(i) = self.index(t) {
            self.cost[i] = 0;
        }
    }

    /// Makes a cell passable at the given cost (a bridge, a building's entry).
    pub fn open(&mut self, t: Tile, minutes: u16) {
        if let Some(i) = self.index(t) {
            self.cost[i] = minutes.max(1);
        }
    }

    /// Objects standing in rows `r0..r1`.
    pub fn objects_in_rows(&self, r0: i32, r1: i32) -> &[Decoration] {
        let r0 = r0.clamp(0, self.h) as usize;
        let r1 = r1.clamp(0, self.h) as usize;
        if r0 >= r1 {
            return &[];
        }
        &self.objects[self.row_start[r0]..self.row_start[r1]]
    }

    /// The passable cell nearest to `t` within `radius` steps (`t` itself if passable).
    pub fn nearest_passable(&self, t: Tile, radius: i32) -> Option<Tile> {
        let g = self.grid;
        (0..=radius).find_map(|r| g.disk(t, r).into_iter().filter(|&n| g.distance(t, n) == r).find(|&n| self.passable(n)))
    }

    /// Cheapest path from `from` to `to` (A*), excluding `from`. Empty if unreachable or equal.
    pub fn path(&self, from: Tile, to: Tile) -> Vec<Tile> {
        self.path_limited(from, to, usize::MAX)
    }

    /// [`TileMap::path`] giving up after expanding `max_nodes` cells (for AI armies).
    pub fn path_limited(&self, from: Tile, to: Tile, max_nodes: usize) -> Vec<Tile> {
        self.path_where(from, to, max_nodes, &|_| true)
    }

    /// [`TileMap::path_limited`] stepping only onto cells `allowed` accepts (the fog of war:
    /// explored cells, `rules::fog`).
    pub fn path_where(&self, from: Tile, to: Tile, max_nodes: usize, allowed: &dyn Fn(Tile) -> bool) -> Vec<Tile> {
        if !self.passable(to) || !allowed(to) {
            return Vec::new();
        }
        self.path_by(from, to, max_nodes, &|_, n| self.minutes(n).filter(|_| allowed(n)))
    }

    /// Cheapest path (A*) where `step(from, onto)` gives the minutes per cell of a step
    /// between neighbours, `None` if it is not allowed (ships: `rules::ships`). Costs must be
    /// at least [`MIN_MINUTES`]. Excludes `from`; empty if unreachable or equal.
    pub fn path_by(&self, from: Tile, to: Tile, max_nodes: usize, step: &dyn Fn(Tile, Tile) -> Option<u16>) -> Vec<Tile> {
        let (Some(start), Some(goal)) = (self.index(from), self.index(to)) else { return Vec::new() };
        if from == to {
            return Vec::new();
        }
        let g = self.grid;
        // Every step costs at least the cheapest cell times its length: admissible.
        let h = |t: Tile| (MIN_MINUTES as f32 * g.step_length(t, to)) as u32;
        let n = self.cost.len();
        let mut best = vec![u32::MAX; n];
        let mut parent = vec![u32::MAX; n];
        best[start] = 0;
        let mut open = BinaryHeap::from([Reverse((h(from), 0u32, start as u32))]);
        let mut expanded = 0;
        while let Some(Reverse((_, gc, i))) = open.pop() {
            let i = i as usize;
            if i == goal {
                let mut path = Vec::new();
                let mut cur = goal;
                while cur != start {
                    path.push(self.tile_of(cur));
                    cur = parent[cur] as usize;
                }
                path.reverse();
                return path;
            }
            if gc > best[i] {
                continue;
            }
            expanded += 1;
            if expanded > max_nodes {
                break;
            }
            let here = self.tile_of(i);
            for nb in self.grid.neighbours(here) {
                let Some(j) = self.index(nb) else { continue };
                let Some(c) = step(here, nb).filter(|&c| c > 0) else { continue };
                let ng = gc + (c as f32 * self.grid.step_length(here, nb)).round() as u32;
                if ng < best[j] {
                    best[j] = ng;
                    parent[j] = i as u32;
                    open.push(Reverse((ng + h(nb), ng, j as u32)));
                }
            }
        }
        Vec::new()
    }

    /// Travel time (minutes) of `path` from `from`, as [`TileMap::path`] returns it: each
    /// cell's cost times the length of the step onto it.
    pub fn path_minutes(&self, from: Tile, path: &[Tile]) -> u32 {
        self.path_minutes_by(from, path, &|t| self.minutes(t).unwrap_or(0) as f32)
    }

    /// [`TileMap::path_minutes`] with the minutes per cell given by `cost`.
    pub fn path_minutes_by(&self, from: Tile, path: &[Tile], cost: &dyn Fn(Tile) -> f32) -> u32 {
        let mut prev = from;
        let mut total = 0.0;
        for &t in path {
            total += cost(t) * self.grid.step_length(prev, t);
            prev = t;
        }
        total.round() as u32
    }

    /// Centre of a cell in world units.
    pub fn center(&self, t: Tile) -> (f32, f32) {
        self.grid.center(t)
    }

    /// Cell containing a world position.
    pub fn tile_at(&self, p: (f32, f32)) -> Tile {
        self.grid.tile_at(p)
    }

    /// Steps between two cells.
    pub fn distance(&self, a: Tile, b: Tile) -> i32 {
        self.grid.distance(a, b)
    }

    /// Cells reachable on foot from `from` (a flood fill), as a `w*h` mask.
    pub fn reachable(&self, from: Tile) -> Vec<bool> {
        let mut seen = vec![false; self.cost.len()];
        let Some(start) = self.index(from) else { return seen };
        seen[start] = true;
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            for nb in self.grid.neighbours(self.tile_of(i)) {
                if let Some(j) = self.index(nb) {
                    if !seen[j] && self.cost[j] > 0 {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        seen
    }

    /// Connected regions of passable cells: a label per cell (`u32::MAX` for impassable ones)
    /// and the size of each region.
    pub fn regions(&self) -> (Vec<u32>, Vec<usize>) {
        let mut label = vec![u32::MAX; self.cost.len()];
        let mut sizes = Vec::new();
        for s in 0..self.cost.len() {
            if self.cost[s] == 0 || label[s] != u32::MAX {
                continue;
            }
            let id = sizes.len() as u32;
            let mut n = 1;
            label[s] = id;
            let mut stack = vec![s];
            while let Some(i) = stack.pop() {
                for nb in self.grid.neighbours(self.tile_of(i)) {
                    if let Some(j) = self.index(nb) {
                        if label[j] == u32::MAX && self.cost[j] > 0 {
                            label[j] = id;
                            n += 1;
                            stack.push(j);
                        }
                    }
                }
            }
            sizes.push(n);
        }
        (label, sizes)
    }

    /// Index of a tile into [`TileMap::reachable`]'s mask.
    pub fn mask_index(&self, t: Tile) -> Option<usize> {
        self.index(t)
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
        assert_eq!(m.surface((1, 1)), Surface::DeepSea);
        assert_eq!(m.surface((1, 3)), Surface::Road);
        assert_eq!(m.surface((0, 4)), Surface::GrassPlain);
        assert_eq!(m.objects.len(), 5, "a tree on every forest cell");
        assert_eq!(m.markers, vec![('X', (2, 2))]);
        assert_eq!(m.surface((2, 2)), Surface::Road);
        assert_eq!(m.minutes((1, 3)), Some(30));
        assert_eq!(m.minutes((0, 0)), Some(60));
        assert_eq!(m.minutes((0, 4)), Some(90), "trees slow by half");
        assert_eq!(m.minutes((1, 1)), None);
        assert_eq!(m.objects_in_rows(4, 5).len(), 5);
        assert!(m.objects_in_rows(0, 4).is_empty());
    }

    #[test]
    fn hex_neighbours_depend_on_row_parity() {
        // Even row: the row above/below sits half a hex to the left.
        assert_eq!(hex_neighbours((2, 2)), [(1, 2), (3, 2), (1, 1), (2, 1), (1, 3), (2, 3)]);
        // Odd row: shifted right.
        assert_eq!(hex_neighbours((2, 1)), [(1, 1), (3, 1), (2, 0), (3, 0), (2, 2), (3, 2)]);
        for n in hex_neighbours((2, 1)) {
            assert_eq!(hex_distance((2, 1), n), 1);
            let (a, b) = (Grid::HexOddR.center((2, 1)), Grid::HexOddR.center(n));
            assert!(((a.0 - b.0).hypot(a.1 - b.1) - 1.0).abs() < 1e-4, "neighbour centres are 1 apart");
        }
        assert_eq!(hex_distance((0, 0), (4, 0)), 4);
        assert_eq!(hex_distance((0, 0), (0, 4)), 4);
        assert_eq!(hex_disk((5, 5), 0).count(), 1);
        assert_eq!(hex_disk((5, 5), 1).count(), 7);
        assert_eq!(hex_disk((5, 6), 2).count(), 19);
    }

    #[test]
    fn tile_at_inverts_center() {
        for r in 0..6 {
            for c in 0..6 {
                for g in [Grid::HexOddR, Grid::Square8] {
                    assert_eq!(g.tile_at(g.center((c, r))), (c, r));
                    let (x, y) = g.center((c, r));
                    assert_eq!(g.tile_at((x + 0.3, y - 0.2)), (c, r), "near the centre stays inside");
                }
            }
        }
    }

    #[test]
    fn surfaces_and_objects_set_the_cost() {
        use Surface::*;
        let codes = [Road, GrassPlain, DeepSea, ImpassableSwamp, ImpassableSnowdrifts, ShallowsFords, CoastalWater, Marsh];
        let codes: Vec<u8> = codes.iter().map(|s| *s as u8).collect();
        let m = TileMap::from_codes(Grid::HexOddR, 8, 1, &codes, vec![]);
        let costs: Vec<Option<u16>> = (0..8).map(|x| m.minutes((x, 0))).collect();
        assert_eq!(costs, [Some(30), Some(60), None, None, None, Some(120), None, Some(120)]);
        // Objects on a 9×5 grass field.
        let obj = |x, y, class, sprite| Decoration { tile: (x, y), class, sprite };
        let objects = vec![
            obj(1, 1, object_class::MOUNTAINS, 12), // family 1: its own cell
            obj(6, 4, object_class::MOUNTAINS, 50), // family 5: radius 2, above its cell
            obj(3, 1, object_class::HILLS, 10),
            obj(3, 3, object_class::TREES, 5),
            obj(4, 3, object_class::THICKET, 5),
            obj(0, 4, object_class::ROCKS, 12),
            obj(1, 4, 7, 0), // unknown class: no effect
        ];
        let m = TileMap::from_codes(Grid::HexOddR, 9, 7, &[GrassPlain as u8; 63], objects);
        assert!(!m.passable((1, 1)) && m.passable((2, 1)) && m.passable((1, 2)));
        assert!(hex_disk((6, 2), 2).filter(|t| m.in_bounds(*t)).all(|t| !m.passable(t)));
        assert!(m.passable((6, 5)), "nothing below the mountain's cell");
        assert!(m.passable((8, 0)) && m.passable((3, 0)));
        assert_eq!(m.minutes((3, 1)), Some(90), "hills slow");
        assert_eq!(m.minutes((3, 3)), Some(90), "trees slow");
        assert_eq!(m.minutes((4, 3)), None, "thickets block");
        assert_eq!(m.minutes((0, 4)), None, "rocks block");
        assert_eq!(m.minutes((1, 4)), Some(60));
        // Roads stay open under objects.
        let mut codes = [GrassPlain as u8; 3];
        codes[1] = Road as u8;
        let objects = vec![obj(0, 0, object_class::THICKET, 1), obj(1, 0, object_class::THICKET, 1)];
        let m = TileMap::from_codes(Grid::Square8, 3, 1, &codes, objects);
        assert_eq!((m.minutes((0, 0)), m.minutes((1, 0))), (None, Some(30)));
    }

    #[test]
    fn path_avoids_water_and_prefers_road() {
        let m = TileMap::parse(MAP);
        let p = m.path((0, 3), (4, 3));
        assert_eq!(p, vec![(1, 3), (2, 3), (3, 3), (4, 3)], "straight along the road");
        assert_eq!(m.path_minutes((0, 3), &p), 120);
        let p = m.path((0, 0), (4, 4));
        assert!(p.iter().all(|&t| m.passable(t)));
        for w in p.windows(2) {
            assert_eq!(hex_distance(w[0], w[1]), 1, "every step is to a neighbouring hex");
        }
        assert!(m.path_limited((0, 0), (4, 4), 3).is_empty(), "gives up");
    }

    #[test]
    fn no_path_into_water_or_to_self() {
        let m = TileMap::parse(MAP);
        assert!(m.path((0, 0), (1, 1)).is_empty());
        assert!(m.path((0, 0), (0, 0)).is_empty());
        assert!(m.path((0, 0), (99, 0)).is_empty());
        let near = m.nearest_passable((1, 1), 2).unwrap();
        assert!(m.passable(near) && hex_distance(near, (1, 1)) == 1);
        assert_eq!(m.nearest_passable((0, 0), 2), Some((0, 0)));
    }

    #[test]
    fn square_grid_has_eight_neighbours_and_diagonal_steps() {
        let g = Grid::Square8;
        let n: Vec<Tile> = g.neighbours((5, 5)).collect();
        assert_eq!(n.len(), 8);
        assert!(n.contains(&(4, 6)) && n.contains(&(6, 4)));
        assert_eq!(g.distance((0, 0), (3, 7)), 7);
        assert_eq!(g.disk((5, 5), 2).len(), 25);
        assert!((g.step_length((0, 0), (1, 0)) - 1.0).abs() < 1e-6);
        assert!((g.step_length((0, 0), (0, 1)) - 22.0 / 32.0).abs() < 1e-6, "cells are 32×22");
        // A diagonal causeway across water, one column per row (like the original's bridges).
        let mut codes = vec![Surface::DeepSea as u8; 25];
        for i in 0..5 {
            codes[i * 5 + (4 - i)] = Surface::Road as u8;
        }
        let m = TileMap::from_codes(g, 5, 5, &codes, vec![]);
        let p = m.path((4, 0), (0, 4));
        assert_eq!(p, vec![(3, 1), (2, 2), (1, 3), (0, 4)]);
        assert_eq!(m.path_minutes((4, 0), &p), (4.0 * 30.0 * g.step_length((0, 0), (1, 1))).round() as u32);
        // The same causeway is broken on a hex grid.
        let hex = TileMap::from_codes(Grid::HexOddR, 5, 5, &codes, vec![]);
        assert!(hex.path((4, 0), (0, 4)).is_empty());
    }

    #[test]
    fn block_and_open_cells_and_flood_fill() {
        let mut m = TileMap::parse(MAP);
        m.open((1, 1), 30);
        assert_eq!(m.minutes((1, 1)), Some(30));
        m.block((0, 3));
        assert!(!m.passable((0, 3)));
        let reach = m.reachable((0, 0));
        assert!(reach[m.mask_index((4, 4)).unwrap()]);
        assert!(!reach[m.mask_index((0, 3)).unwrap()]);
    }
}
