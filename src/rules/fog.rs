//! Fog of war: which cells of the world map the player has explored.
//!
//! The original (`docs/reference/original-mechanics/world.md` §3): unexplored ground is black
//! and **counts as impassable to the hero's planner** until he has seen it (the AI ignores the
//! fog); the hero reveals a disc around himself after every step and at the start; explored
//! ground stays fully visible for good; lanterns reveal the same kind of disc.
//!
//! - Sight radius by class: knight 9, archmage 8, ranger 10 cells ([`sight_radius`]); the disc
//!   is a circle in **cells** (an ellipse on the 32×22 px screen).
//! - A lantern's radius (at most 24) is in cells too.
//! - The original's edge is soft (a brightness per half-cell); a cell counts as explored within
//!   about `r + 0.6` cells of the centre ([`EDGE`], M).
//! - Clicking into the dark plans a route over explored ground only, to the explored cell
//!   nearest the target ([`plan`]); as the walk reveals ground the route is planned again, so
//!   the hero feels his way towards the spot and stops when no explored way gets closer
//!   *(Razdor's handling of such a click)*.
//! - The built-in demo plays without fog ([`Fog::disabled`]).
//!
//! The state is plain data (`w`, `h`, a flag and a `Vec<u64>` bitset) so a save file can store
//! it as it is.

use crate::dt::dtm::Scenario;

use super::content::HeroClass;
use super::map::{Tile, TileMap};

/// Cells beyond the radius that still come into view: the original's soft edge, a cell being
/// explored when the average brightness of its half-cells is high enough (world.md §3, M).
pub const EDGE: f32 = 0.6;
/// Largest lantern radius the editor allows.
pub const MAX_LANTERN_RADIUS: i32 = 24;
/// Point model of an active lantern (`.DTm` point byte 5).
pub const LANTERN_MODEL: u8 = 8;

/// How far the hero of `class` sees, in cells (world.md §3: 18, 16 and 20 half-cells).
pub fn sight_radius(class: HeroClass) -> i32 {
    match class {
        HeroClass::Knight => 9,
        HeroClass::Archmage => 8,
        HeroClass::Ranger => 10,
    }
}

/// Cell `b` lies within the disc of radius `r` cells around `a` (with the soft [`EDGE`]).
pub fn within(a: Tile, b: Tile, r: i32) -> bool {
    let (dx, dy) = ((a.0 - b.0) as f32, (a.1 - b.1) as f32);
    let e = r.max(0) as f32 + EDGE;
    dx * dx + dy * dy <= e * e
}

/// Explored cells of a `w × h` map, one bit per cell (row by row).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fog {
    pub w: i32,
    pub h: i32,
    /// Off: every cell counts as explored (the demo).
    pub enabled: bool,
    /// Bit `y*w + x` is set once cell `(x, y)` has been seen.
    pub bits: Vec<u64>,
}

impl Fog {
    /// All dark.
    pub fn new(w: i32, h: i32) -> Fog {
        let n = (w.max(0) * h.max(0)) as usize;
        Fog { w, h, enabled: true, bits: vec![0; n.div_ceil(64)] }
    }

    /// No fog: everything counts as explored.
    pub fn disabled(w: i32, h: i32) -> Fog {
        Fog { enabled: false, ..Fog::new(w, h) }
    }

    fn index(&self, (x, y): Tile) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.w && y < self.h).then(|| (y * self.w + x) as usize)
    }

    /// The cell has been seen (always true with the fog off; false outside the map).
    pub fn explored(&self, t: Tile) -> bool {
        match self.index(t) {
            Some(i) => !self.enabled || self.bits[i / 64] & (1 << (i % 64)) != 0,
            None => false,
        }
    }

    /// An explored cell lies within `r` cells of `t` (a square around it): the soft edge of
    /// the dark reaches that far, so things there show faded through it.
    pub fn explored_near(&self, t: Tile, r: i32) -> bool {
        (-r..=r).any(|dy| (-r..=r).any(|dx| self.explored((t.0 + dx, t.1 + dy))))
    }

    /// Marks one cell explored. Returns true if it was dark.
    pub fn mark(&mut self, t: Tile) -> bool {
        let Some(i) = self.index(t) else { return false };
        let (word, bit) = (i / 64, 1u64 << (i % 64));
        let new = self.bits[word] & bit == 0;
        self.bits[word] |= bit;
        new && self.enabled
    }

    /// Explores the disc of radius `r` cells (at most [`MAX_LANTERN_RADIUS`]) around cell
    /// `(x, y)`, a circle in cells ([`within`]): the hero's sight or a lantern. Returns true if
    /// anything new was revealed.
    pub fn reveal(&mut self, x: i32, y: i32, r: i32) -> bool {
        let r = r.clamp(0, MAX_LANTERN_RADIUS);
        let mut new = false;
        for cy in (y - r - 1).max(0)..=(y + r + 1).min(self.h - 1) {
            for cx in (x - r - 1).max(0)..=(x + r + 1).min(self.w - 1) {
                if within((x, y), (cx, cy), r) {
                    new |= self.mark((cx, cy));
                }
            }
        }
        new
    }

    /// Number of explored cells (the whole map with the fog off).
    pub fn explored_count(&self) -> usize {
        if !self.enabled {
            return (self.w * self.h) as usize;
        }
        self.bits.iter().map(|b| b.count_ones() as usize).sum()
    }

    /// A cheap fingerprint of the explored set, for caches that redraw when it changes.
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ (self.w as u64) << 32 ^ self.h as u64 ^ self.enabled as u64;
        for &b in &self.bits {
            h = (h ^ b).wrapping_mul(0x0100_0000_01b3).rotate_left(5);
        }
        h
    }
}

/// Active lanterns at the start of a scenario: `(cell, radius)` of every point with the
/// lantern model that is active at start and has a radius.
pub fn start_lanterns(s: &Scenario) -> Vec<(Tile, i32)> {
    s.points
        .iter()
        .filter(|p| p.model == LANTERN_MODEL && p.active != 0 && p.radius > 0)
        .map(|p| ((p.x as i32, p.y as i32), p.radius as i32))
        .collect()
}

/// A lantern an event lights, by point id: `(cell, radius)`. An inactive lantern has the
/// event-point model but keeps its radius; one without a radius gets none *(guess)*.
pub fn lantern(s: &Scenario, point: u16) -> Option<(Tile, i32)> {
    let p = s.points.iter().find(|p| p.id as u16 == point)?;
    (p.radius > 0).then_some(((p.x as i32, p.y as i32), p.radius as i32))
}

/// The fog for a new game on `map`: the scenario's active lanterns lit. Returns a disabled
/// fog when `enabled` is false.
pub fn for_scenario(map: &TileMap, s: Option<&Scenario>, enabled: bool) -> Fog {
    if !enabled {
        return Fog::disabled(map.w, map.h);
    }
    let mut fog = Fog::new(map.w, map.h);
    for ((x, y), r) in s.map(start_lanterns).unwrap_or_default() {
        fog.reveal(x, y, r);
    }
    fog
}

/// Route from `from` towards `to` over explored ground only (unexplored cells are impassable).
///
/// If `to` is explored and reachable that way, the cheapest such path. Otherwise the path to
/// the explored cell reachable from `from` nearest to `to` (on screen, ties to the cheaper
/// path), so walking there reveals more ground and the route can be planned again. Empty when
/// no explored cell gets closer than `from` itself.
pub fn plan(map: &TileMap, fog: &Fog, from: Tile, to: Tile) -> Vec<Tile> {
    plan_by(map, fog, from, to, &|_, n| map.cost(n))
}

/// [`plan`] with the steps a [`TileMap::path_by`] cost function allows (on foot, or with a
/// ship, `rules::ships`).
pub fn plan_by(map: &TileMap, fog: &Fog, from: Tile, to: Tile, step: &dyn Fn(Tile, Tile) -> Option<u16>) -> Vec<Tile> {
    let ok = |a: Tile, n: Tile| step(a, n).filter(|_| fog.explored(n));
    if !fog.enabled || fog.explored(to) {
        let p = map.path_by(from, to, usize::MAX, &ok);
        if !p.is_empty() || !fog.enabled {
            return p;
        }
    }
    let Some(goal) = nearest_explored_by(map, fog, from, to, step) else { return Vec::new() };
    map.path_by(from, goal, usize::MAX, &ok)
}

/// [`plan_by`] towards any cell `goal` accepts (a building's footprint), over explored
/// ground: the cheapest way to the nearest explored goal cell; if none can be reached, the
/// way towards the explored cell nearest `centre`.
pub fn plan_to_any(map: &TileMap, fog: &Fog, from: Tile, goal: &dyn Fn(Tile) -> bool, centre: Tile, step: &dyn Fn(Tile, Tile) -> Option<u16>) -> Vec<Tile> {
    let ok = |a: Tile, n: Tile| step(a, n).filter(|_| fog.explored(n));
    let p = map.path_to_any(from, &|t| goal(t) && fog.explored(t), usize::MAX, &ok);
    if !p.is_empty() || !fog.enabled || goal(from) {
        return p;
    }
    let Some(near) = nearest_explored_by(map, fog, from, centre, step) else { return Vec::new() };
    map.path_by(from, near, usize::MAX, &ok)
}

/// The explored, passable cell reachable from `from` over explored cells whose centre is
/// nearest to `to`'s; `None` if that is `from` itself (or `from` is off the map).
pub fn nearest_explored(map: &TileMap, fog: &Fog, from: Tile, to: Tile) -> Option<Tile> {
    nearest_explored_by(map, fog, from, to, &|_, n| map.cost(n))
}

/// [`nearest_explored`] over the steps `step` allows.
pub fn nearest_explored_by(map: &TileMap, fog: &Fog, from: Tile, to: Tile, step: &dyn Fn(Tile, Tile) -> Option<u16>) -> Option<Tile> {
    let start = map.mask_index(from)?;
    let g = map.grid;
    let target = g.center(to);
    let dist = |t: Tile| {
        let c = g.center(t);
        (c.0 - target.0).hypot(c.1 - target.1)
    };
    let mut seen = vec![false; (map.w * map.h) as usize];
    seen[start] = true;
    let mut stack = vec![from];
    let mut best = (dist(from), 0, from);
    while let Some(t) = stack.pop() {
        for n in g.neighbours(t) {
            let Some(j) = map.mask_index(n) else { continue };
            if seen[j] || step(t, n).is_none() || !fog.explored(n) {
                continue;
            }
            seen[j] = true;
            let d = dist(n);
            let steps = g.distance(from, n);
            if d < best.0 - 1e-4 || ((d - best.0).abs() <= 1e-4 && steps < best.1) {
                best = (d, steps, n);
            }
            stack.push(n);
        }
    }
    (best.2 != from).then_some(best.2)
}

/// Which side a location or army belongs to, for its colour on the minimap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Player,
    Ally,
    Neighbour,
    Enemy,
    Neutral,
}

impl Side {
    /// By the player's ownership and the editor's faction code (1 player, 2 ally,
    /// 3 neighbour, 4 enemy; anything else neutral).
    pub fn of(owned_by_player: bool, faction: u8) -> Side {
        if owned_by_player {
            return Side::Player;
        }
        match faction {
            1 => Side::Player,
            2 => Side::Ally,
            3 => Side::Neighbour,
            4 => Side::Enemy,
            _ => Side::Neutral,
        }
    }

    /// Minimap colour (RGB): player green, ally blue, neighbour yellow, enemy red, neutral
    /// light grey, as the editor colours the factions.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Side::Player => [70, 200, 90],
            Side::Ally => [70, 130, 230],
            Side::Neighbour => [230, 200, 60],
            Side::Enemy => [220, 60, 50],
            Side::Neutral => [215, 215, 215],
        }
    }
}

/// Side of a location: the player's own, else its faction; a location without an owner is
/// neutral.
pub fn location_side(l: &super::world::Location) -> Side {
    use super::world::Owner;
    match l.owner {
        Owner::Player => Side::Player,
        Owner::Neutral if l.faction == 0 => Side::Neutral,
        _ => Side::of(false, l.faction),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Point, Surface};
    use crate::rules::map::Grid;

    #[test]
    fn near_explored_reaches_into_the_dark_edge() {
        let mut fog = Fog::new(10, 10);
        fog.mark((2, 2));
        assert!(fog.explored_near((4, 4), 2) && fog.explored_near((0, 3), 2));
        assert!(!fog.explored_near((5, 2), 2) && !fog.explored((4, 4)));
    }

    fn open_map(w: i32, h: i32) -> TileMap {
        TileMap::from_codes(Grid::Square8, w, h, &vec![Surface::GrassPlain as u8; (w * h) as usize], vec![])
    }

    #[test]
    fn sight_is_9_8_10_cells_by_class() {
        assert_eq!(sight_radius(HeroClass::Knight), 9);
        assert_eq!(sight_radius(HeroClass::Archmage), 8);
        assert_eq!(sight_radius(HeroClass::Ranger), 10);
    }

    #[test]
    fn reveal_is_a_circle_in_cells_and_stays() {
        let mut f = Fog::new(40, 40);
        assert_eq!(f.explored_count(), 0);
        assert!(f.reveal(20, 20, 9));
        // 9 cells every way, rows and columns alike (an ellipse on the 32×22 px screen).
        assert!(f.explored((29, 20)) && !f.explored((30, 20)));
        assert!(f.explored((11, 20)) && !f.explored((10, 20)));
        assert!(f.explored((20, 29)) && !f.explored((20, 30)));
        assert!(f.explored((20, 11)) && !f.explored((20, 10)));
        assert!(f.explored((26, 27)) && !f.explored((27, 27)), "a circle: the corners stay dark");
        let n = f.explored_count();
        assert!(!f.reveal(20, 20, 9), "nothing new");
        // Walking away keeps the old ground explored.
        f.reveal(5, 5, 9);
        assert!(f.explored((20, 20)) && f.explored((5, 5)));
        assert!(f.explored_count() > n);
        assert!(!f.explored((-1, 0)) && !f.explored((40, 0)));
    }

    #[test]
    fn disabled_fog_explores_everything() {
        let f = Fog::disabled(5, 5);
        assert!(f.explored((4, 4)) && !f.explored((5, 4)));
        assert_eq!(f.explored_count(), 25);
    }

    #[test]
    fn lanterns_reveal_their_radius_in_cells() {
        let m = open_map(60, 60);
        let mut s = Scenario::default();
        let point = |id, x, y, model, active, radius| Point { x, y, id, model, active, radius, ..blank_point() };
        s.points = vec![point(1, 10, 10, 8, 1, 3), point(2, 40, 40, 9, 0, 5), point(3, 50, 10, 8, 0, 4), point(4, 30, 50, 8, 1, 0)];
        assert_eq!(start_lanterns(&s), vec![((10, 10), 3)]);
        assert_eq!(lantern(&s, 2), Some(((40, 40), 5)));
        assert_eq!(lantern(&s, 4), None);
        assert_eq!(lantern(&s, 9), None);
        let mut f = for_scenario(&m, Some(&s), true);
        assert!(f.explored((10, 10)) && f.explored((13, 10)) && !f.explored((14, 10)));
        assert!(f.explored((10, 13)) && !f.explored((10, 14)), "rows count as cells too");
        assert!(f.explored((12, 12)) && !f.explored((13, 12)));
        assert!(!f.explored((40, 40)) && !f.explored((50, 10)));
        // Lighting one later (an event).
        let ((x, y), r) = lantern(&s, 2).unwrap();
        assert!(f.reveal(x, y, r));
        assert!(f.explored((45, 40)) && !f.explored((46, 40)));
        // Radius is capped at 24.
        let mut g = Fog::new(60, 60);
        g.reveal(30, 30, 99);
        assert!(g.explored((54, 30)) && !g.explored((55, 30)));
        assert!(!for_scenario(&m, Some(&s), false).enabled);
    }

    fn blank_point() -> Point {
        Point {
            x: 0,
            y: 0,
            id: 0,
            model: 0,
            serial: 0,
            event_slots: [0; 10],
            priorities: [0; 4],
            active_duration: 0,
            radius: 0,
            event_count: 0,
            active: 0,
            unknown_41: [0; 58],
        }
    }

    #[test]
    fn planning_refuses_unexplored_cells() {
        let m = open_map(30, 10);
        let mut f = Fog::new(30, 10);
        f.reveal(5, 5, 3);
        // Explored target: an ordinary path, every step explored.
        let p = plan(&m, &f, (5, 5), (7, 5));
        assert_eq!(p.last(), Some(&(7, 5)));
        // A target in the dark: walk to the explored cell nearest to it, over explored cells.
        let p = plan(&m, &f, (5, 5), (25, 5));
        assert_eq!(p.last(), Some(&(8, 5)));
        assert!(p.iter().all(|&t| f.explored(t)));
        // Standing on that cell already: nothing to do.
        assert!(plan(&m, &f, (8, 5), (25, 5)).is_empty());
        // An explored target with only dark ground between: walk towards it, not through.
        f.reveal(25, 5, 2);
        let p = plan(&m, &f, (5, 5), (25, 5));
        assert_eq!(p.last(), Some(&(8, 5)));
        // The plain pathfinder would cross the dark.
        assert_eq!(m.path((5, 5), (25, 5)).last(), Some(&(25, 5)));
        // With the fog off, the ordinary path.
        let off = Fog::disabled(30, 10);
        assert_eq!(plan(&m, &off, (5, 5), (25, 5)), m.path((5, 5), (25, 5)));
        // Towards any cell of a footprint: the nearest explored one.
        let p = plan_to_any(&m, &f, (5, 5), &|t| t.0 >= 7 && t.1 == 5, (9, 5), &|_, n| m.cost(n));
        assert_eq!(p.last(), Some(&(7, 5)));
    }

    #[test]
    fn feeling_the_way_reaches_a_dark_target() {
        let m = open_map(60, 12);
        let mut f = Fog::new(60, 12);
        let mut here = (2, 6);
        f.reveal(here.0, here.1, 9);
        for _ in 0..20 {
            let p = plan(&m, &f, here, (57, 6));
            let Some(&next) = p.last() else { break };
            for &t in &p {
                f.reveal(t.0, t.1, 9);
            }
            here = next;
        }
        assert_eq!(here, (57, 6));
    }

    #[test]
    fn sides_and_colours() {
        assert_eq!(Side::of(true, 4), Side::Player);
        assert_eq!(Side::of(false, 1), Side::Player);
        assert_eq!(Side::of(false, 2), Side::Ally);
        assert_eq!(Side::of(false, 3), Side::Neighbour);
        assert_eq!(Side::of(false, 4), Side::Enemy);
        assert_eq!(Side::of(false, 0), Side::Neutral);
        assert_eq!(Side::of(false, 9), Side::Neutral);
        let c = |s: Side| s.rgb();
        assert!(c(Side::Player)[1] > c(Side::Player)[0], "green");
        assert!(c(Side::Ally)[2] > c(Side::Ally)[0], "blue");
        assert!(c(Side::Enemy)[0] > c(Side::Enemy)[1], "red");
        assert!(c(Side::Neighbour)[0] > 200 && c(Side::Neighbour)[1] > 180 && c(Side::Neighbour)[2] < 100, "yellow");
        let n = c(Side::Neutral);
        assert!(n[0] == n[1] && n[1] == n[2], "grey");
        let all = [Side::Player, Side::Ally, Side::Neighbour, Side::Enemy, Side::Neutral].map(c);
        for i in 0..5 {
            for j in i + 1..5 {
                assert_ne!(all[i], all[j]);
            }
        }
    }

    #[test]
    fn fingerprint_changes_with_the_explored_set() {
        let mut f = Fog::new(10, 10);
        let a = f.fingerprint();
        f.reveal(3, 3, 1);
        assert_ne!(a, f.fingerprint());
    }
}

#[cfg(test)]
mod real_maps {
    //! Checks against the player's install; skipped without `RAZDOR_DT_DIR`.
    use super::*;
    use crate::dt::install::DtInstall;
    use crate::rules::content::{Content, HeroClass};
    use crate::rules::game::Game;
    use std::sync::Arc;

    fn install() -> Option<(DtInstall, Arc<Content>)> {
        let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Arc::new(Content::from_dt(&dt));
        Some((dt, c))
    }

    #[test]
    fn lanterns_of_the_shipped_maps() {
        let Some((dt, _)) = install() else { return };
        let (mut lit, mut points) = (0, 0);
        for m in &dt.maps {
            let s = m.load().unwrap();
            points += s.points.len();
            for ((x, y), r) in start_lanterns(&s) {
                lit += 1;
                assert!((x as u32) < s.width() && (y as u32) < s.height(), "{}", m.name);
                assert!(r <= MAX_LANTERN_RADIUS, "{}: radius {r}", m.name);
            }
        }
        eprintln!("{} maps: {points} points, {lit} lanterns lit at start", dt.maps.len());
    }

    #[test]
    fn rk1_and_rk3_start_in_a_revealed_region() {
        let Some((dt, c)) = install() else { return };
        for prefix in ["РК1", "РК3"] {
            let s = dt.maps.iter().find(|m| m.name.starts_with(prefix)).unwrap().load().unwrap();
            for class in HeroClass::ALL {
                let g = Game::from_scenario(c.clone(), &s, class, 1);
                let (w, h) = (g.world.map.w, g.world.map.h);
                let here = g.tile();
                let n = g.fog.explored_count();
                assert!(g.fog.enabled && g.fog.explored(here), "{prefix} {class:?}");
                // At least the hero's own circle (8–10 cells each way), far from the whole map.
                assert!(n >= 150 && n < (w * h / 4) as usize, "{prefix} {class:?}: {n} of {}", w * h);
                let r = g.sight_radius();
                for (dx, dy) in [(r, 0), (-r, 0), (0, r), (0, -r)] {
                    let t = (here.0 + dx, here.1 + dy);
                    assert!(!g.world.map.in_bounds(t) || g.fog.explored(t), "{prefix} {class:?}: {t:?}");
                }
                eprintln!("{prefix} {class:?}: {n} of {} cells explored at start", w * h);
            }
        }
    }

    #[test]
    fn rk1_and_rk3_walk_to_the_nearest_village_through_the_fog() {
        use crate::rules::world::LocationKind;
        let Some((dt, c)) = install() else { return };
        for prefix in ["РК1", "РК3"] {
            let s = dt.maps.iter().find(|m| m.name.starts_with(prefix)).unwrap().load().unwrap();
            let mut g = Game::from_scenario(c.clone(), &s, HeroClass::Knight, 7);
            g.world.armies.clear();
            let (village, _) = g.world.nearest_location(g.tile(), |l| l.kind == LocationKind::Village).expect("a village");
            let target = g.world.locations[village].tile;
            let dark = !g.fog.explored(target);
            assert!(g.set_destination(target), "{prefix}");
            let mut replans = 0;
            for _ in 0..40_000 {
                if !g.moving() {
                    break;
                }
                assert!(g.path.iter().take(1).all(|&t| g.fog.explored(t)), "{prefix}: steps only onto explored cells");
                if g.path.last() != Some(&target) {
                    replans += 1;
                }
                g.tick(0.05);
            }
            assert_eq!(g.location, Some(village), "{prefix}: arrives (village in the dark at start: {dark}, {replans} ticks short of it)");
            eprintln!("{prefix}: village in the dark at start: {dark}; {replans} ticks heading for the frontier");
        }
    }
}
