//! Cell grid of the world map and pathfinding.
//!
//! The original (`docs/reference/original-mechanics/world.md` §1) uses plain squares of 32×22
//! px with 8 neighbours: [`Grid::Square8`]. Its planner weighs an orthogonal step 2 and a
//! diagonal one 3 (×1.5, vertical and horizontal alike), and prices the cell entered; walking
//! charges the cell left, `cost × speed` minutes (×1.5 diagonally). The built-in demo keeps its
//! hex layout ([`Grid::HexOddR`], odd rows shifted half a cell right, every step weight 2), as
//! `data/kingdom.txt` was drawn for it.
//!
//! Tile `(col, row)`; world positions are in units where one column is 1 wide. Each cell has
//! a surface (the original's terrain code, `dt::dtm::Surface`) and costs in the original's
//! units on foot (its LAND map) and by ship (its SHIP map), from the surface and the map
//! objects on it ([`TileMap::from_codes`]).

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

    /// Length of the step between two neighbouring cells, in world units (drawing only).
    pub fn step_length(self, a: Tile, b: Tile) -> f32 {
        let (p, q) = (self.center(a), self.center(b));
        (p.0 - q.0).hypot(p.1 - q.1)
    }

    /// Planner weight of a step between neighbours (world.md §1): 2 orthogonal, 3 diagonal
    /// on the original's squares (a diagonal is ×1.5, vertical and horizontal alike); every
    /// hex step is 2.
    pub fn weight(self, a: Tile, b: Tile) -> u32 {
        match self {
            Grid::Square8 if a.0 != b.0 && a.1 != b.1 => DIAGONAL_WEIGHT,
            _ => ORTHOGONAL_WEIGHT,
        }
    }

    /// The original's distance (world.md §4): `max(|dx|,|dy|) + min(|dx|,|dy|)/2` cells on
    /// squares; hex steps on the demo's grid.
    pub fn octile(self, a: Tile, b: Tile) -> i32 {
        match self {
            Grid::HexOddR => hex_distance(a, b),
            Grid::Square8 => {
                let (dx, dy) = ((a.0 - b.0).abs(), (a.1 - b.1).abs());
                dx.max(dy) + dx.min(dy) / 2
            }
        }
    }

    /// Lower bound of the planner weights between two cells (for A*).
    fn weight_bound(self, a: Tile, b: Tile) -> u32 {
        match self {
            Grid::HexOddR => ORTHOGONAL_WEIGHT * hex_distance(a, b) as u32,
            Grid::Square8 => {
                let (dx, dy) = ((a.0 - b.0).unsigned_abs(), (a.1 - b.1).unsigned_abs());
                ORTHOGONAL_WEIGHT * dx.max(dy) + (DIAGONAL_WEIGHT - ORTHOGONAL_WEIGHT) * dx.min(dy)
            }
        }
    }
}

/// Planner weight of an orthogonal step; a diagonal one is [`DIAGONAL_WEIGHT`].
pub const ORTHOGONAL_WEIGHT: u32 = 2;
pub const DIAGONAL_WEIGHT: u32 = 3;

/// The original's terrain value of a surface (world.md §1, cost units): shallows 2, coastal
/// water 1, road 3, stony soil 4, grass, dry plain, sand, clay and scorched land 5, snowy
/// ground 6, marsh 8; deep sea, lava, the impassable swamp and snowdrifts −32 (blocked).
pub fn surface_value(s: Surface) -> i32 {
    use Surface::*;
    match s {
        ShallowsFords => 2,
        CoastalWater => 1,
        Road => 3,
        StonySoil => 4,
        GrassLowland | GrassPlain | DryPlain | SandDunes | ClaySoil | ScorchedLand => 5,
        SnowyGround => 6,
        Marsh => 8,
        DeepSea | LavaFields | ImpassableSwamp | ImpassableSnowdrifts => BLOCKED,
    }
}

/// The value that blocks a cell whatever else is on it.
pub const BLOCKED: i32 = -32;
/// Cost units of a road, and of every building footprint.
pub const ROAD: u16 = 3;
/// The knight's speed: game minutes per cost unit of an orthogonal step (the hero's step
/// time is `cost × speed` minutes, ×1.5 diagonally; world.md §2).
pub const BASE_SPEED: u16 = 5;

/// Water (terrain codes 0–2): never walked; ships sail the shallows and coastal water.
pub fn is_water(s: Surface) -> bool {
    matches!(s, Surface::ShallowsFords | Surface::CoastalWater | Surface::DeepSea)
}

/// Minutes per orthogonal step on foot at the knight's speed 5, `None` for water and
/// blocking surfaces (road 15, grass 25, marsh 40 …).
pub fn surface_minutes(s: Surface) -> Option<u16> {
    let v = surface_value(s);
    (!is_water(s) && v > 0).then(|| v as u16 * BASE_SPEED)
}

/// What a map object does to the cells it covers (world.md §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectEffect {
    /// Hills: the cell's cost starts from this value instead of 0 (the terrain adds to it).
    Base(i32),
    /// Added to the cell's cost ([`BLOCKED`] blocks it).
    Add(i32),
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

/// Effect of an object class (world.md §1): hills 1–3 are a base of 2, class 4 of 3;
/// mountains 5–7 and rocks 8 block; trees 9 add 4, dead trees 10 add 6, thickets 11 block,
/// class 12 adds 4.
pub fn object_effect(class: u8) -> Option<ObjectEffect> {
    match class {
        1..=3 => Some(ObjectEffect::Base(2)),
        4 => Some(ObjectEffect::Base(3)),
        5..=8 | 11 => Some(ObjectEffect::Add(BLOCKED)),
        9 | 12 => Some(ObjectEffect::Add(4)),
        10 => Some(ObjectEffect::Add(6)),
        _ => None,
    }
}

/// A massif (classes 1–8: hills, mountains, rocks) covers a square of `sprite div 10` cells a
/// side whose bottom-right cell is the object's own; plants (9–12) cover their own cell.
pub fn object_side(class: u8, sprite: u8) -> i32 {
    match class {
        1..=8 => (sprite / 10) as i32,
        _ => 1,
    }
}

/// Cells an object covers (in bounds or not): rows `y−f+1..=y`, columns `x−f+1..=x`.
pub fn object_cells(o: &Decoration) -> impl Iterator<Item = Tile> {
    let f = object_side(o.class, o.sprite);
    let (x, y) = o.tile;
    (0..f).flat_map(move |j| (0..f).map(move |i| (x - i, y - j)))
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
    /// The original's LAND map: cost units of a step onto the cell on foot; 0 = impassable.
    land: Vec<u16>,
    /// Its SHIP map: cost units on water (shallows 2, coastal 1; building footprints road);
    /// 0 = not sailable.
    water: Vec<u16>,
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
    /// A map from terrain codes (`w*h`, row by row) and objects, costed as the original
    /// builds its maps at load (world.md §1): hills set a base over their square, every cell
    /// adds its terrain value (below 0 blocks), plants add theirs on their cell and mountains,
    /// rocks and thickets block; water goes to the ship map, the rest to the foot map.
    /// Buildings are laid over it by `rules::world` ([`TileMap::pave`]).
    pub fn from_codes(grid: Grid, w: i32, h: i32, codes: &[u8], objects: Vec<Decoration>) -> TileMap {
        assert_eq!(codes.len(), (w * h) as usize, "terrain size");
        let mut objects = objects;
        objects.retain(|o| o.tile.0 >= 0 && o.tile.1 >= 0 && o.tile.0 < w && o.tile.1 < h);
        let file_order = objects.clone();
        objects.sort_by_key(|o| (o.tile.1, o.tile.0));
        let mut row_start = Vec::with_capacity(h as usize + 1);
        let mut i = 0;
        for r in 0..=h {
            while i < objects.len() && objects[i].tile.1 < r {
                i += 1;
            }
            row_start.push(i);
        }
        let n = codes.len();
        let mut m = TileMap { grid, w, h, surface: codes.to_vec(), land: vec![0; n], water: vec![0; n], objects, row_start, markers: Vec::new() };
        let index = |t: Tile| (t.0 >= 0 && t.1 >= 0 && t.0 < w && t.1 < h).then(|| (t.1 * w + t.0) as usize);
        // Hills set the base of their square (later objects over earlier ones).
        let mut v = vec![0i32; n];
        for o in &file_order {
            if let Some(ObjectEffect::Base(b)) = object_effect(o.class) {
                for i in object_cells(o).filter_map(index) {
                    v[i] = b;
                }
            }
        }
        // The terrain adds its value; below 0 blocks.
        for i in 0..n {
            v[i] = match Surface::from_code(codes[i]) {
                Some(s) => (v[i] + surface_value(s)).max(0),
                None => 0,
            };
        }
        // Plants add theirs on their own cell (if it is still open), massifs over their
        // square.
        for o in &file_order {
            let Some(ObjectEffect::Add(a)) = object_effect(o.class) else { continue };
            let cells: Vec<usize> = if o.class >= object_class::TREES { index(o.tile).into_iter().collect() } else { object_cells(o).filter_map(index).collect() };
            for i in cells {
                if v[i] > 0 {
                    v[i] = (v[i] + a).max(0);
                }
            }
        }
        // Water on the ship map, the rest on the foot map.
        for i in 0..n {
            let c = v[i].min(u16::MAX as i32) as u16;
            if Surface::from_code(codes[i]).is_some_and(is_water) {
                m.water[i] = c;
            } else {
                m.land[i] = c;
            }
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

    /// Every cell's surface code, row by row.
    pub fn surface_codes(&self) -> &[u8] {
        &self.surface
    }

    pub fn surface(&self, t: Tile) -> Surface {
        Surface::from_code(self.surface_code(t)).unwrap_or(Surface::DeepSea)
    }

    /// Cost units of a step onto the cell on foot (the LAND map), `None` if impassable.
    pub fn cost(&self, t: Tile) -> Option<u16> {
        self.index(t).map(|i| self.land[i]).filter(|&c| c > 0)
    }

    /// Cost units of sailing onto the cell (the SHIP map), `None` if a ship cannot.
    pub fn water_cost(&self, t: Tile) -> Option<u16> {
        self.index(t).map(|i| self.water[i]).filter(|&c| c > 0)
    }

    /// Minutes of an orthogonal step on foot on the cell at the knight's speed, `None` if
    /// impassable.
    pub fn minutes(&self, t: Tile) -> Option<u16> {
        self.cost(t).map(|c| c * BASE_SPEED)
    }

    pub fn passable(&self, t: Tile) -> bool {
        self.cost(t).is_some()
    }

    /// Makes a cell impassable on foot and by ship.
    pub fn block(&mut self, t: Tile) {
        if let Some(i) = self.index(t) {
            self.land[i] = 0;
            self.water[i] = 0;
        }
    }

    /// A building's footprint: road on the foot and on the ship map (world.md §1 step 6).
    pub fn pave(&mut self, t: Tile) {
        if let Some(i) = self.index(t) {
            self.land[i] = ROAD;
            self.water[i] = ROAD;
        }
    }

    /// Makes a cell passable on foot at `cost` units.
    pub fn open(&mut self, t: Tile, cost: u16) {
        if let Some(i) = self.index(t) {
            self.land[i] = cost.max(1);
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

    /// Cheapest path on foot from `from` to `to`, excluding `from`. Empty if unreachable or
    /// equal.
    pub fn path(&self, from: Tile, to: Tile) -> Vec<Tile> {
        self.path_limited(from, to, usize::MAX)
    }

    /// [`TileMap::path`] giving up after expanding `max_nodes` cells (for AI armies).
    pub fn path_limited(&self, from: Tile, to: Tile, max_nodes: usize) -> Vec<Tile> {
        self.path_where(from, to, max_nodes, &|_| true)
    }

    /// [`TileMap::path_limited`] stepping only onto cells `allowed` accepts.
    pub fn path_where(&self, from: Tile, to: Tile, max_nodes: usize, allowed: &dyn Fn(Tile) -> bool) -> Vec<Tile> {
        if !self.passable(to) || !allowed(to) {
            return Vec::new();
        }
        self.search(from, &|t| t == to, Some(to), ROAD as u32, max_nodes, &|_, n| self.cost(n).filter(|_| allowed(n)))
    }

    /// Cheapest path (A*) where `step(from, onto)` gives the cost units of a step between
    /// neighbours (the planner prices the cell entered, times [`Grid::weight`]), `None` if it
    /// is not allowed. Excludes `from`; empty if unreachable or equal.
    pub fn path_by(&self, from: Tile, to: Tile, max_nodes: usize, step: &dyn Fn(Tile, Tile) -> Option<u16>) -> Vec<Tile> {
        self.search(from, &|t| t == to, Some(to), 1, max_nodes, step)
    }

    /// Cheapest path to the nearest cell `goal` accepts (Dijkstra), as [`TileMap::path_by`].
    pub fn path_to_any(&self, from: Tile, goal: &dyn Fn(Tile) -> bool, max_nodes: usize, step: &dyn Fn(Tile, Tile) -> Option<u16>) -> Vec<Tile> {
        self.search(from, goal, None, 1, max_nodes, step)
    }

    fn search(&self, from: Tile, goal: &dyn Fn(Tile) -> bool, target: Option<Tile>, min_cost: u32, max_nodes: usize, step: &dyn Fn(Tile, Tile) -> Option<u16>) -> Vec<Tile> {
        let Some(start) = self.index(from) else { return Vec::new() };
        if goal(from) || target.is_some_and(|t| self.index(t).is_none()) {
            return Vec::new();
        }
        let g = self.grid;
        let h = |t: Tile| target.map_or(0, |to| min_cost * g.weight_bound(t, to));
        let n = self.land.len();
        let mut best = vec![u32::MAX; n];
        let mut parent = vec![u32::MAX; n];
        best[start] = 0;
        let mut open = BinaryHeap::from([Reverse((h(from), 0u32, start as u32))]);
        let mut expanded = 0;
        while let Some(Reverse((_, gc, i))) = open.pop() {
            let i = i as usize;
            if gc > best[i] {
                continue;
            }
            let here = self.tile_of(i);
            if i != start && goal(here) {
                let mut path = Vec::new();
                let mut cur = i;
                while cur != start {
                    path.push(self.tile_of(cur));
                    cur = parent[cur] as usize;
                }
                path.reverse();
                return path;
            }
            expanded += 1;
            if expanded > max_nodes {
                break;
            }
            for nb in g.neighbours(here) {
                let Some(j) = self.index(nb) else { continue };
                let Some(c) = step(here, nb).filter(|&c| c > 0) else { continue };
                let ng = gc + c as u32 * g.weight(here, nb);
                if ng < best[j] {
                    best[j] = ng;
                    parent[j] = i as u32;
                    open.push(Reverse((ng + h(nb), ng, j as u32)));
                }
            }
        }
        Vec::new()
    }

    /// Planner cost of `path` from `from` on foot: every cell entered times its step weight.
    pub fn path_cost(&self, from: Tile, path: &[Tile]) -> u32 {
        let mut prev = from;
        let mut total = 0;
        for &t in path {
            total += self.cost(t).unwrap_or(0) as u32 * self.grid.weight(prev, t);
            prev = t;
        }
        total
    }

    /// Walking time (minutes) of `path` from `from` at the knight's speed: each step costs
    /// the cell being left, ×1.5 diagonally (world.md §1).
    pub fn path_minutes(&self, from: Tile, path: &[Tile]) -> u32 {
        self.path_minutes_by(from, path, &|a, b| step_minutes(self.grid, a, b, self.cost(a).unwrap_or(ROAD), BASE_SPEED as u32))
    }

    /// Minutes of `path` from `from`, `step(from, to)` minutes per step.
    pub fn path_minutes_by(&self, from: Tile, path: &[Tile], step: &dyn Fn(Tile, Tile) -> f32) -> u32 {
        let mut prev = from;
        let mut total = 0.0;
        for &t in path {
            total += step(prev, t);
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
        let mut seen = vec![false; self.land.len()];
        let Some(start) = self.index(from) else { return seen };
        seen[start] = true;
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            for nb in self.grid.neighbours(self.tile_of(i)) {
                if let Some(j) = self.index(nb) {
                    if !seen[j] && self.land[j] > 0 {
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
        let mut label = vec![u32::MAX; self.land.len()];
        let mut sizes = Vec::new();
        for s in 0..self.land.len() {
            if self.land[s] == 0 || label[s] != u32::MAX {
                continue;
            }
            let id = sizes.len() as u32;
            let mut n = 1;
            label[s] = id;
            let mut stack = vec![s];
            while let Some(i) = stack.pop() {
                for nb in self.grid.neighbours(self.tile_of(i)) {
                    if let Some(j) = self.index(nb) {
                        if label[j] == u32::MAX && self.land[j] > 0 {
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

/// Game minutes of a step from `a` to its neighbour `b` for an army of `speed` when the step
/// is charged `cost` units: `cost × speed`, ×1.5 diagonally (world.md §2).
pub fn step_minutes(grid: Grid, a: Tile, b: Tile, cost: u16, speed: u32) -> f32 {
    cost as f32 * speed as f32 * grid.weight(a, b) as f32 / ORTHOGONAL_WEIGHT as f32
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
        assert_eq!(m.minutes((1, 3)), Some(15));
        assert_eq!(m.minutes((0, 0)), Some(25));
        assert_eq!(m.minutes((0, 4)), Some(45), "a tree adds 4 units to grass's 5");
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
            assert_eq!(Grid::HexOddR.weight((2, 1), n), 2);
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
                    assert!(g.tile_at((x + 0.3, y - 0.2)) == (c, r), "near the centre stays inside");
                }
            }
        }
    }

    fn row(surfaces: &[Surface]) -> TileMap {
        let codes: Vec<u8> = surfaces.iter().map(|s| *s as u8).collect();
        TileMap::from_codes(Grid::Square8, codes.len() as i32, 1, &codes, vec![])
    }

    #[test]
    fn terrain_values_are_the_originals() {
        use Surface::*;
        let all = Surface::ALL;
        let m = row(&all);
        let foot: Vec<Option<u16>> = (0..16).map(|x| m.minutes((x, 0))).collect();
        // Road 15, stony 20, grass/dry/sand/clay/scorched 25, snow 30, marsh 40 minutes.
        let expect = [None, None, None, None, Some(15), Some(25), Some(25), Some(25), Some(40), None, Some(25), Some(25), Some(20), Some(25), Some(30), None];
        assert_eq!(foot, expect);
        let ship: Vec<Option<u16>> = (0..16).map(|x| m.water_cost((x, 0))).collect();
        assert_eq!(&ship[..3], &[Some(2), Some(1), None], "shallows 2, coastal 1, deep sea blocks ships too");
        assert!(ship[3..].iter().all(Option::is_none), "no ship on land");
        for s in [ShallowsFords, CoastalWater, DeepSea] {
            assert!(is_water(s) && surface_minutes(s).is_none());
        }
        assert!(!is_water(LavaFields) && surface_minutes(LavaFields).is_none(), "lava blocks");
        assert_eq!(surface_minutes(Marsh), Some(40));
    }

    fn obj(x: i32, y: i32, class: u8, sprite: u8) -> Decoration {
        Decoration { tile: (x, y), class, sprite }
    }

    #[test]
    fn objects_add_fixed_costs() {
        use object_class::*;
        let objects = vec![
            obj(0, 0, HILLS, 12),       // hill: base 2 over 1×1, + grass 5
            obj(1, 0, YELLOW_HILL, 10), // class 4: base 3
            obj(2, 0, TREES, 3),        // +4
            obj(3, 0, DEAD_TREES, 110), // +6, dead trees are slower than trees
            obj(4, 0, 12, 0),           // class 12: +4
            obj(5, 0, THICKET, 3),      // blocks
            obj(6, 0, ROCKS, 12),       // blocks
            obj(7, 0, MOUNTAINS, 15),   // blocks
            obj(8, 0, 7, 10),           // class 7 blocks too
            obj(9, 0, 13, 0),           // unknown: nothing
            obj(10, 0, TREES, 1),       // a tree on a hill: 2 + 5 + 4
            obj(10, 0, HILLS, 10),
        ];
        let m = TileMap::from_codes(Grid::Square8, 12, 1, &[Surface::GrassPlain as u8; 12], objects);
        let c: Vec<Option<u16>> = (0..12).map(|x| m.cost((x, 0))).collect();
        assert_eq!(c, [Some(7), Some(8), Some(9), Some(11), Some(9), None, None, None, None, Some(5), Some(11), Some(5)]);
        // Thickets on a road block it (the original keeps no road open).
        let objects = vec![obj(0, 0, THICKET, 1)];
        let m = TileMap::from_codes(Grid::Square8, 1, 1, &[Surface::Road as u8], objects);
        assert!(!m.passable((0, 0)));
        // A hill in the water stays water: the ship map gets its cost.
        let m = TileMap::from_codes(Grid::Square8, 1, 1, &[Surface::CoastalWater as u8], vec![obj(0, 0, HILLS, 10)]);
        assert_eq!((m.cost((0, 0)), m.water_cost((0, 0))), (None, Some(3)));
    }

    #[test]
    fn massifs_cover_a_square_with_the_object_at_the_bottom_right() {
        let m = TileMap::from_codes(Grid::Square8, 8, 8, &[Surface::GrassPlain as u8; 64], vec![obj(5, 6, object_class::MOUNTAINS, 34)]);
        for y in 0..8 {
            for x in 0..8 {
                let covered = (3..=5).contains(&x) && (4..=6).contains(&y);
                assert_eq!(!m.passable((x, y)), covered, "({x}, {y})");
            }
        }
        // Side = the sprite's tens digit; 0–9 cover no cell; clipped at the map's edge.
        assert_eq!(object_side(object_class::HILLS, 9), 0);
        assert_eq!(object_side(object_class::ROCKS, 20), 2);
        assert_eq!(object_side(object_class::TREES, 55), 1, "plants cover their own cell");
        let m = TileMap::from_codes(Grid::Square8, 3, 3, &[Surface::GrassPlain as u8; 9], vec![obj(1, 1, object_class::HILLS, 40), obj(2, 2, object_class::ROCKS, 5)]);
        assert_eq!(m.cost((0, 0)), Some(7));
        assert_eq!(m.cost((1, 1)), Some(7));
        assert_eq!(m.cost((2, 1)), Some(5), "right of the object's cell: not covered");
        assert!(m.passable((2, 2)), "sprite 5: side 0");
    }

    #[test]
    fn path_avoids_water_and_prefers_road() {
        let m = TileMap::parse(MAP);
        let p = m.path((0, 3), (4, 3));
        assert_eq!(p, vec![(1, 3), (2, 3), (3, 3), (4, 3)], "straight along the road");
        // Charged the cell left: grass 25, then three road cells 15.
        assert_eq!(m.path_minutes((0, 3), &p), 25 + 3 * 15);
        assert_eq!(m.path_cost((0, 3), &p), 4 * 3 * 2, "planner: the cells entered × weight 2");
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
    fn square_grid_has_eight_neighbours_and_diagonals_weigh_one_and_a_half() {
        let g = Grid::Square8;
        let n: Vec<Tile> = g.neighbours((5, 5)).collect();
        assert_eq!(n.len(), 8);
        assert!(n.contains(&(4, 6)) && n.contains(&(6, 4)));
        assert_eq!(g.distance((0, 0), (3, 7)), 7);
        assert_eq!(g.octile((0, 0), (3, 7)), 7 + 1, "max + min/2");
        assert_eq!(g.disk((5, 5), 2).len(), 25);
        assert_eq!((g.weight((0, 0), (1, 0)), g.weight((0, 0), (0, 1)), g.weight((0, 0), (1, 1))), (2, 2, 3), "vertical = horizontal");
        assert_eq!(step_minutes(g, (0, 0), (1, 1), 5, 5), 37.5);
        assert_eq!(step_minutes(g, (0, 0), (0, 1), 5, 4), 20.0, "ranger speed 4");
        // A diagonal causeway across water, one column per row (like the original's bridges).
        let mut codes = vec![Surface::DeepSea as u8; 25];
        for i in 0..5 {
            codes[i * 5 + (4 - i)] = Surface::Road as u8;
        }
        let m = TileMap::from_codes(g, 5, 5, &codes, vec![]);
        let p = m.path((4, 0), (0, 4));
        assert_eq!(p, vec![(3, 1), (2, 2), (1, 3), (0, 4)]);
        assert_eq!(m.path_minutes((4, 0), &p), 4 * 15 * 3 / 2);
        // The same causeway is broken on a hex grid.
        let hex = TileMap::from_codes(Grid::HexOddR, 5, 5, &codes, vec![]);
        assert!(hex.path((4, 0), (0, 4)).is_empty());
        // On open grass two orthogonal steps (weight 4) beat nothing: a diagonal (3) is
        // taken where it saves, e.g. to (2, 1) one diagonal and one straight step.
        let m = TileMap::from_codes(g, 5, 5, &[Surface::GrassPlain as u8; 25], vec![]);
        assert_eq!(m.path((0, 0), (2, 1)).len(), 2);
        assert_eq!(m.path_cost((0, 0), &m.path((0, 0), (2, 1))), 5 * 3 + 5 * 2);
    }

    #[test]
    fn path_to_any_stops_at_the_nearest_goal_cell() {
        let m = TileMap::from_codes(Grid::Square8, 10, 1, &[Surface::GrassPlain as u8; 10], vec![]);
        let p = m.path_to_any((0, 0), &|t| t.0 >= 6, usize::MAX, &|_, n| m.cost(n));
        assert_eq!(p.last(), Some(&(6, 0)));
        assert!(m.path_to_any((7, 0), &|t| t.0 >= 6, usize::MAX, &|_, n| m.cost(n)).is_empty(), "already there");
    }

    #[test]
    fn block_pave_and_open_cells_and_flood_fill() {
        let mut m = TileMap::parse(MAP);
        m.open((1, 1), 3);
        assert_eq!(m.minutes((1, 1)), Some(15));
        m.block((0, 3));
        assert!(!m.passable((0, 3)));
        m.pave((2, 1));
        assert_eq!((m.cost((2, 1)), m.water_cost((2, 1))), (Some(ROAD), Some(ROAD)), "a footprint is road on both maps");
        let reach = m.reachable((0, 0));
        assert!(reach[m.mask_index((4, 4)).unwrap()]);
        assert!(!reach[m.mask_index((0, 3)).unwrap()]);
    }
}
