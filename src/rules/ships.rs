//! Ships (`docs/reference/mechanics.md` §5.2, `original-mechanics/world.md` §1–2): a shipyard
//! rents the hero a ship for `ShipCost` gold (`_Global.ini [Costs]`); with it he sails the
//! shallows and coastal water (deep sea blocks ships too), and lands on any walkable cell next
//! to the water by clicking the land. One ship at a time: renting another sends the old one
//! back.
//!
//! As the original: ships have no speed of their own. At sea the hero's route is priced on the
//! original's MIXED map (water at its own cost — coastal 1, shallows 2 — land at 5× its cost,
//! building footprints 6) and he does not pass under bridges; a step at sea takes the water's
//! cost times his speed (coastal 5 minutes, shallows 10; the ranger 4 and 8). On landing his
//! route map returns to land only: the ship is gone (world.md, M).
//!
//! The scenario's own ships (army byte 72: hero, pirate and merchant ships) are armies that
//! move on the SHIP map (water and building footprints); pirates attack like any hostile army,
//! merchants never do.
//!
//! Razdor's choices *(guess)*, listed in mechanics.md §8.6:
//! - A rented ship waits at the water nearest the shipyard on foot ([`World::mooring`]); a few
//!   shipyards of the shipped maps stand well inland. The hero boards it by walking onto it
//!   (the planner handles boarding, sailing and landing as one route: [`Game::step_cost`]).
//! - Ship armies always cruise within their patrol radius ([`SHIP_PATROL`] cells when the
//!   editor gives none) and chase the hero to the water next to him.

use serde::{Deserialize, Serialize};

use super::game::Game;
use super::map::{Tile, ROAD};
use super::world::{Army, LocationKind, World};

/// At sea the original's MIXED map prices land at this many times its cost.
pub const MIXED_LAND_FACTOR: u16 = 5;
/// How many steps on foot from a shipyard its ship may wait *(guess)*.
pub const MOORING_RADIUS: i32 = 24;
/// Patrol radius of a scenario ship whose editor radius is 0 *(guess)*.
pub const SHIP_PATROL: i32 = 8;

/// Ship types (`.DTm` army byte 72).
pub mod kind {
    pub const HERO: u8 = 1;
    pub const PIRATE: u8 = 2;
    pub const MERCHANT: u8 = 3;
}

/// The hero's rented ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ship {
    /// Where it is: under the hero while he is aboard, else where he left it.
    pub tile: Tile,
    pub aboard: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShipError {
    /// The hero is not in a friendly shipyard.
    NoShipyard,
    NotEnoughGold,
    /// The shipyard has no water the hero can board from.
    NoWater,
}

impl World {
    /// Open water a ship can sail on.
    pub fn is_sea(&self, t: Tile) -> bool {
        self.map.mask_index(t).and_then(|i| self.sea.get(i).copied()).unwrap_or(false)
    }

    /// The water cell nearest to `t` within `radius` steps (`t` itself if it is water).
    pub fn nearest_sea(&self, t: Tile, radius: i32) -> Option<Tile> {
        let g = self.map.grid;
        (0..=radius).find_map(|r| {
            let ring = g.disk(t, r).into_iter().filter(|&n| g.distance(t, n) == r && self.is_sea(n));
            ring.min_by(|&a, &b| g.step_length(t, a).total_cmp(&g.step_length(t, b)).then((a.1, a.0).cmp(&(b.1, b.0))))
        })
    }

    /// Where the ship of shipyard `l` waits: the water next to the land nearest the entry on
    /// foot (within [`MOORING_RADIUS`] steps; a few shipyards stand well inland), the water
    /// cell nearest the entry among equals.
    pub fn mooring(&self, l: usize) -> Option<Tile> {
        let map = &self.map;
        let g = map.grid;
        let entry = self.locations.get(l)?.tile;
        let mut seen = std::collections::HashSet::from([entry]);
        let mut layer = vec![entry];
        for _ in 0..=MOORING_RADIUS {
            let best = layer
                .iter()
                .flat_map(|&t| g.neighbours(t))
                .filter(|&n| self.is_sea(n))
                .min_by(|&a, &b| g.step_length(entry, a).total_cmp(&g.step_length(entry, b)).then((a.1, a.0).cmp(&(b.1, b.0))));
            if best.is_some() {
                return best;
            }
            let mut next = Vec::new();
            for &t in &layer {
                for n in g.neighbours(t) {
                    if map.passable(n) && seen.insert(n) {
                        next.push(n);
                    }
                }
            }
            layer = next;
        }
        None
    }

    /// Cost units of a ship army's step onto `to`: the original's SHIP map (shallows 2,
    /// coastal water 1, building footprints road), `None` elsewhere.
    pub fn sea_step(&self, to: Tile) -> Option<u16> {
        self.map.water_cost(to)
    }

    /// Where a waiting army comes onto the map: its post, or the nearest cell of its kind
    /// (water for a ship, walkable land otherwise) within [`super::world::PLACE_RADIUS`].
    pub fn placement(&self, a: &Army) -> Option<Tile> {
        let r = super::world::PLACE_RADIUS;
        if a.sails() {
            self.nearest_sea(a.post, r)
        } else if self.map.passable(a.post) {
            Some(a.post)
        } else {
            self.map.nearest_passable(a.post, r)
        }
    }

    /// Cells the hero can reach from `start` on foot and by ship, ignoring the fog, armies and
    /// money: walking, renting a ship at every shipyard he reaches (it waits at its
    /// [`World::mooring`]), sailing that ship's waters and landing on any coast, as a `w*h`
    /// mask. Scripted events (a teleport, a bridge built) are not considered.
    pub fn reachable_with_ships(&self, start: Tile) -> Vec<bool> {
        let map = &self.map;
        let mut reach = vec![false; (map.w * map.h).max(0) as usize];
        let mut used = vec![false; self.locations.len()];
        let mut seeds = vec![start];
        loop {
            let mut stack = Vec::new();
            for s in seeds.drain(..) {
                if let Some(i) = map.mask_index(s) {
                    if !reach[i] {
                        reach[i] = true;
                        stack.push(s);
                    }
                }
            }
            while let Some(t) = stack.pop() {
                let at_sea = self.is_sea(t);
                for n in map.grid.neighbours(t) {
                    let Some(j) = map.mask_index(n) else { continue };
                    // Land is walked onto from anywhere; water only from water (the ship).
                    if !reach[j] && (map.passable(n) || (at_sea && self.is_sea(n))) {
                        reach[j] = true;
                        stack.push(n);
                    }
                }
            }
            for (l, loc) in self.locations.iter().enumerate() {
                let entry = map.mask_index(loc.tile).is_some_and(|i| reach[i]);
                if loc.kind == LocationKind::Shipyard && entry && !used[l] {
                    used[l] = true;
                    seeds.extend(self.mooring(l));
                }
            }
            if seeds.is_empty() {
                return reach;
            }
        }
    }
}

impl Game {
    /// Gold a shipyard asks for a ship (`ShipCost`).
    pub fn ship_price(&self) -> i32 {
        self.content.options.ship_cost.max(0)
    }

    /// The shipyard the hero stands in, if it serves him (not ill-disposed).
    pub fn shipyard_here(&self) -> Option<usize> {
        let l = self.location?;
        let loc = &self.world.locations[l];
        (loc.kind == LocationKind::Shipyard && !loc.hostile()).then_some(l)
    }

    /// Rents a ship at the shipyard here for [`Game::ship_price`]. It waits at the
    /// shipyard's mooring (returned); an earlier ship goes back.
    pub fn rent_ship(&mut self) -> Result<Tile, ShipError> {
        let l = self.shipyard_here().ok_or(ShipError::NoShipyard)?;
        let at = self.world.mooring(l).ok_or(ShipError::NoWater)?;
        let price = self.ship_price();
        if self.gold < price {
            return Err(ShipError::NotEnoughGold);
        }
        self.gold -= price;
        self.ship = Some(Ship { tile: at, aboard: false });
        Ok(at)
    }

    pub fn aboard(&self) -> bool {
        self.ship.is_some_and(|s| s.aboard)
    }

    /// Planner cost units of the hero's step from `from` onto its neighbour `to`, `None` if he
    /// cannot take it (world.md §1): land is walked at its cost; water needs the ship, boarded
    /// by stepping onto it, and costs its own value. A route planned at sea (`at_sea`) is
    /// priced on the MIXED map: land at [`MIXED_LAND_FACTOR`]× its cost, building footprints
    /// twice a road.
    pub fn step_cost(&self, from: Tile, to: Tile, at_sea: bool) -> Option<u16> {
        let w = &self.world;
        if w.is_sea(to) {
            let ship = self.ship?;
            return if w.is_sea(from) || (!ship.aboard && to == ship.tile) { w.map.water_cost(to) } else { None };
        }
        let c = w.map.cost(to)?;
        Some(match (at_sea, w.location_covering(to).is_some()) {
            (false, _) => c,
            (true, true) => 2 * ROAD,
            (true, false) => MIXED_LAND_FACTOR * c,
        })
    }

    /// A click on `t` can be sailed to: water the hero can sail, and he has a ship.
    pub fn can_sail_to(&self, t: Tile) -> bool {
        self.ship.is_some() && self.world.is_sea(t)
    }

    /// After a step: on water the hero is aboard and the ship under him; stepping ashore
    /// from it he lands and the ship is gone (the original's route map returns to land only;
    /// world.md, M). A ship waiting at its mooring stays until he boards it.
    pub(crate) fn update_ship(&mut self) {
        let t = self.tile();
        let sea = self.world.is_sea(t);
        match self.ship {
            Some(_) if sea => self.ship = Some(Ship { tile: t, aboard: true }),
            Some(s) if s.aboard => self.ship = None,
            _ => {}
        }
    }

    /// Where a hostile ship army at `from` heads to reach the hero: his cell at sea, else the
    /// water next to him nearest to it.
    pub(crate) fn sea_chase_goal(world: &World, from: Tile, hero: Tile) -> Option<Tile> {
        if world.is_sea(hero) {
            return Some(hero);
        }
        let g = world.map.grid;
        g.neighbours(hero).filter(|&n| world.is_sea(n)).min_by_key(|&n| g.distance(from, n))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::dt::dtm::{BuildingType, Scenario, Surface};
    use crate::rules::content::HeroClass;
    use crate::rules::game::{Event, Foe};
    use crate::rules::world::testkit::{self as tk, army, building, hero, scenario, troop};

    /// A 30×12 map: land x 0..=9, a strait of coastal water x 10..=19 (shallows in the
    /// middle), land x 20..=29. A shipyard at (8, 5); the knight starts at (2, 5) with 600
    /// gold.
    fn strait() -> Scenario {
        let mut s = scenario(30, 12);
        for y in 0..12 {
            for x in 10..20 {
                tk::set(&mut s, x, y, if (13..17).contains(&x) { Surface::ShallowsFords } else { Surface::CoastalWater });
            }
        }
        let mut yard = building(BuildingType::Shipyard, 8, 5, (1, 1));
        yard.relations = [1, 0, 0, 0];
        s.buildings = vec![yard];
        s.header.heroes[0] = hero(2, 5, 600, &[troop(4, 0, 1)]);
        s
    }

    fn start(s: &Scenario) -> Game {
        let mut g = Game::from_scenario(Arc::new(tk::content()), s, HeroClass::Knight, 5);
        // No fog: these tests are about the ship.
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        g
    }

    fn walk_until_stopped(g: &mut Game) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..20_000 {
            if !g.moving() {
                break;
            }
            events.extend(g.tick(0.05));
        }
        events
    }

    fn at_yard(s: &Scenario) -> Game {
        let mut g = start(s);
        assert!(g.set_destination((8, 5)));
        assert_eq!(walk_until_stopped(&mut g).last(), Some(&Event::Arrived(0)));
        g
    }

    #[test]
    fn shallows_and_coastal_water_are_sea_and_block_on_foot() {
        let g = start(&strait());
        assert!(g.world.is_sea((10, 5)) && g.world.is_sea((15, 0)) && !g.world.is_sea((9, 5)) && !g.world.is_sea((20, 5)));
        assert!(!g.world.map.passable((10, 5)) && !g.world.map.passable((15, 5)), "no wading through the shallows");
        assert!(!g.world.is_sea((8, 5)), "the shipyard is land");
        assert!(!g.can_sail_to((15, 5)), "no ship yet");
        let mut g = g;
        assert!(!g.set_destination((25, 5)), "the far shore is out of reach on foot");
    }

    #[test]
    fn deep_sea_blocks_ships_too() {
        let mut s = strait();
        for y in 0..12 {
            tk::set(&mut s, 15, y, Surface::DeepSea);
        }
        let mut g = at_yard(&s);
        assert!(!g.world.is_sea((15, 5)));
        g.rent_ship().unwrap();
        assert!(!g.set_destination((25, 5)), "no crossing over deep sea");
        assert!(g.set_destination((14, 5)), "the near waters are sailed");
    }

    #[test]
    fn a_shipyard_rents_a_ship_for_ship_cost() {
        let s = strait();
        let mut g = start(&s);
        assert_eq!(g.rent_ship(), Err(ShipError::NoShipyard), "not in a shipyard");
        let mut g2 = at_yard(&s);
        use crate::rules::town::{first_tab, Tab};
        assert_eq!(first_tab(&g2.world.locations[0]), Some(Tab::Shipyard));
        assert_eq!(g2.ship_price(), 250);
        g2.gold = 249;
        assert_eq!(g2.rent_ship(), Err(ShipError::NotEnoughGold));
        g2.gold = 600;
        assert_eq!(g2.rent_ship(), Ok((10, 5)), "it waits on the water next to the yard");
        assert_eq!((g2.gold, g2.ship), (350, Some(Ship { tile: (10, 5), aboard: false })));
        // One ship at a time: renting again replaces it.
        assert_eq!(g2.rent_ship(), Ok((10, 5)));
        assert_eq!(g2.gold, 100);
        g.gold = 0;
        assert_eq!(g.ship, None);
    }

    #[test]
    fn board_sail_and_land_on_the_far_shore() {
        let mut g = at_yard(&strait());
        g.rent_ship().unwrap();
        assert!(g.set_destination((25, 5)), "a route over the water now");
        let path = g.path.clone();
        let boards = path.iter().position(|&t| g.world.is_sea(t)).unwrap();
        assert_eq!(path[boards], (10, 5), "boards the ship where it waits");
        let lands = path.iter().rposition(|&t| g.world.is_sea(t)).unwrap();
        assert!(path[boards..=lands].iter().all(|&t| g.world.is_sea(t)), "then stays at sea until it lands");
        assert_eq!(path.last(), Some(&(25, 5)));
        let before = g.clock.total_minutes();
        let expected = g.travel_minutes(&g.path.clone());
        walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (25, 5));
        let spent = g.clock.total_minutes() - before;
        assert!((spent - expected as f64).abs() < 1.0, "{spent} vs {expected}");
        assert_eq!(g.ship, None, "landed: the route map is land only again, the ship is gone");
        assert!(!g.can_sail_to((15, 5)));
    }

    #[test]
    fn ships_have_no_speed_of_their_own() {
        let mut g = at_yard(&strait());
        g.rent_ship().unwrap();
        // Planner: boarding and sailing cost the water's value; planned at sea, land is 5×.
        assert_eq!(g.step_cost((9, 5), (10, 5), false), Some(1), "boarding onto coastal water");
        assert_eq!(g.step_cost((9, 4), (10, 4), false), None, "only onto the ship");
        g.pos = g.world.map.center((10, 5));
        g.update_ship();
        assert!(g.aboard());
        assert_eq!(g.step_cost((12, 5), (13, 5), true), Some(2), "shallows");
        assert_eq!(g.step_cost((19, 5), (20, 5), true), Some(25), "landing, priced on the MIXED map");
        assert_eq!(g.step_cost((19, 5), (20, 5), false), Some(5));
        // Time: the cell left, times the hero's speed: coastal 5 min, shallows 10.
        assert_eq!(g.step_time((12, 5), (11, 5)), 5.0);
        assert_eq!(g.step_time((14, 5), (15, 5)), 10.0);
        assert_eq!(g.step_time((14, 5), (15, 6)), 15.0, "diagonal ×1.5");
        assert_eq!(g.step_time((5, 5), (6, 5)), 25.0, "grass on foot");
    }

    #[test]
    fn at_sea_the_hero_does_not_pass_under_bridges() {
        let mut s = strait();
        // A bridge across the strait on row 5 and a pier on row 8.
        for x in 10..20 {
            s.buildings.push(building(BuildingType::WoodenBridge, x, 5, (1, 1)));
        }
        let mut g = at_yard(&s);
        g.rent_ship().unwrap_or_else(|e| panic!("{e:?}"));
        let ship = g.ship.unwrap().tile;
        assert_ne!(ship.1, 5);
        g.pos = g.world.map.center(ship);
        g.update_ship();
        // From the northern waters to the southern ones: the bridge cuts the strait.
        let target = if ship.1 < 5 { (15, 9) } else { (15, 1) };
        assert!(g.plan(target).is_empty(), "under the bridge is closed at sea");
        // AI ships sail the SHIP map, where bridges are footprints paved as road.
        assert_eq!(g.world.sea_step((15, 5)), Some(crate::rules::map::ROAD));
    }

    #[test]
    fn a_waiting_ship_stays_until_boarded() {
        let mut g = at_yard(&strait());
        g.rent_ship().unwrap();
        g.set_destination((2, 2));
        walk_until_stopped(&mut g);
        assert_eq!(g.ship, Some(Ship { tile: (10, 5), aboard: false }), "it waits at its mooring");
        // Back through the shipyard (walking onto it enters it and ends the walk).
        assert!(g.set_destination((8, 5)));
        walk_until_stopped(&mut g);
        assert!(g.can_sail_to((15, 8)));
        assert!(g.set_destination((15, 8)));
        let boards = g.path.iter().position(|&t| g.world.is_sea(t)).unwrap();
        assert_eq!(g.path[boards], (10, 5));
        walk_until_stopped(&mut g);
        assert_eq!(g.ship, Some(Ship { tile: (15, 8), aboard: true }));
        // Clicking land from the sea sails there and lands.
        assert!(g.set_destination((4, 10)));
        let lands = g.path.iter().position(|&t| !g.world.is_sea(t)).unwrap();
        assert!(g.path[lands..].iter().all(|&t| !g.world.is_sea(t)), "sails, lands, then walks");
        walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (4, 10));
        assert_eq!(g.ship, None);
    }

    #[test]
    fn pirates_sail_and_attack_the_hero_at_sea() {
        let mut s = strait();
        let mut pirate = army(1, 15, 0, -2, &[troop(4, 0, 1)]);
        pirate.ship = kind::PIRATE;
        let mut merchant = army(2, 11, 11, -2, &[troop(4, 0, 1)]);
        merchant.ship = kind::MERCHANT;
        s.armies = vec![pirate, merchant];
        let mut g = at_yard(&s);
        let ids: Vec<u8> = g.world.armies.iter().map(|a| a.id).collect();
        assert_eq!(ids, [1, 2]);
        assert!(g.world.armies[0].hostile() && g.world.armies[0].sails());
        assert!(!g.world.armies[1].hostile(), "merchants never attack");
        assert!(g.world.armies[0].patrols && g.world.armies[0].patrol_radius == SHIP_PATROL);
        // The ships cruise, on water only.
        let before: Vec<_> = g.world.armies.iter().map(|a| a.pos).collect();
        g.wait(24);
        for a in &g.world.armies {
            assert!(g.world.is_sea(a.tile(&g.world.map)), "army {} left the water", a.id);
        }
        assert_ne!(before, g.world.armies.iter().map(|a| a.pos).collect::<Vec<_>>(), "they moved");
        // Out at sea, the pirates come for the hero.
        g.world.armies.retain(|a| a.id == 1);
        g.world.armies[0].pos = g.world.map.center((15, 1));
        g.world.armies[0].path.clear();
        g.rent_ship().unwrap();
        g.set_destination((14, 5));
        let events = walk_until_stopped(&mut g);
        let events = if g.foe.is_none() { g.wait(4) } else { events };
        assert!(matches!(events.last(), Some(Event::Encounter(0))), "{events:?}");
        assert_eq!(g.foe, Some(Foe::Army(0)));
        assert!(g.world.is_sea(g.world.armies[0].tile(&g.world.map)));
    }

    #[test]
    fn ship_state_survives_a_save() {
        let s = strait();
        let mut g = at_yard(&s);
        g.rent_ship().unwrap();
        g.set_destination((14, 5));
        walk_until_stopped(&mut g);
        assert!(g.aboard());
        let json = serde_json::to_string(&g).unwrap();
        let mut h: Game = serde_json::from_str(&json).unwrap();
        h.content = g.content.clone();
        h.world.restore_statics(World::from_scenario(&s, &g.content)).unwrap();
        assert_eq!(h.ship, Some(Ship { tile: (14, 5), aboard: true }));
        assert!(h.world.is_sea((14, 5)));
        assert!(h.set_destination((25, 5)));
        walk_until_stopped(&mut h);
        assert_eq!(h.tile(), (25, 5));
        // Older saves have no ship.
        let old = json.replace(&format!(",\"ship\":{}", serde_json::to_string(&g.ship).unwrap()), "");
        assert_ne!(old, json);
        let o: Game = serde_json::from_str(&old).unwrap();
        assert_eq!(o.ship, None);
    }

    #[test]
    fn reachability_counts_rented_ships() {
        let s = strait();
        let g = start(&s);
        let w = &g.world;
        let on_foot = w.map.reachable((2, 5));
        let with_ships = w.reachable_with_ships((2, 5));
        let i = w.map.mask_index((25, 5)).unwrap();
        assert!(!on_foot[i] && with_ships[i]);
        // Without the shipyard, the far shore stays out of reach.
        let mut s = strait();
        s.buildings.clear();
        let w = World::from_scenario(&s, &tk::content());
        assert!(!w.reachable_with_ships((2, 5))[i]);
    }

    #[test]
    fn hero_starts_at_his_preset_and_gets_his_start_buildings() {
        let mut s = scenario(20, 20);
        let a = building(BuildingType::Castle, 5, 5, (2, 2));
        let mut b = building(BuildingType::Town, 15, 15, (2, 2));
        b.start_for = [0, 0, 1];
        let mut c = building(BuildingType::Tavern, 15, 4, (1, 1));
        c.start_for = [0, 1, 0];
        s.buildings = vec![a, b, c];
        // The knight's preset names building 1, far from his x/y: he stays at his x/y.
        s.header.heroes[0] = hero(18, 1, 100, &[]);
        s.header.heroes[0].start_building = 1;
        // The ranger stands in his flagged town.
        s.header.heroes[2] = hero(15, 15, 100, &[]);
        s.header.heroes[1] = hero(2, 17, 100, &[]);
        let ct = tk::content();
        let w = World::from_scenario(&s, &ct);
        let k = w.hero_start(&s, &ct, HeroClass::Knight);
        assert_eq!((k.tile, k.location, k.owned.clone()), ((18, 1), None, vec![0]));
        let r = w.hero_start(&s, &ct, HeroClass::Ranger);
        assert_eq!((r.tile, r.location, r.owned.clone()), ((15, 15), Some(1), vec![1]));
        let m = w.hero_start(&s, &ct, HeroClass::Archmage);
        assert_eq!((m.tile, m.owned.clone()), ((2, 17), vec![2]), "a flagged building anywhere is his");
        let g = Game::from_scenario(Arc::new(ct), &s, HeroClass::Knight, 1);
        assert_eq!((g.tile(), g.location), ((18, 1), None));
        let castle = &g.world.locations[0];
        assert!(castle.owned() && castle.faction == 1 && castle.attitude == 3, "his start building is his");
        assert!(!g.world.locations[1].owned() && !g.world.locations[2].owned(), "other classes' buildings are not");
    }
}

#[cfg(test)]
mod real_maps {
    //! Ships on the player's maps; skipped without `RAZDOR_DT_DIR`. Numbers only.
    use std::sync::Arc;

    use super::*;
    use crate::dt::install::DtInstall;
    use crate::rules::content::{Content, HeroClass};
    use crate::rules::game::Event;

    #[test]
    fn every_shipyard_has_a_mooring() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let c = Content::from_dt(&dt);
        for m in &dt.maps {
            let s = m.load().unwrap();
            let w = World::from_scenario(&s, &c);
            for (i, l) in w.locations.iter().enumerate().filter(|(_, l)| l.kind == LocationKind::Shipyard) {
                // A shipyard with no sailable water near it (shallows count as water now) is reported.
                let water_near = w.nearest_sea(l.tile, MOORING_RADIUS).is_some();
                assert_eq!(w.mooring(i).is_some(), water_near, "{} shipyard {}", m.name, l.id);
                if !water_near {
                    eprintln!("{} shipyard {} has no sailable water near it", m.name, l.id);
                }
            }
            for a in w.armies.iter().filter(|a| a.sails()) {
                assert!(w.is_sea(a.tile(&w.map)), "{} ship {}", m.name, a.id);
            }
        }
    }

    #[test]
    fn ds1_sails_from_a_shipyard_to_a_building_out_of_reach_on_foot() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let c = Arc::new(Content::from_dt(&dt));
        let s = dt.maps.iter().find(|m| m.name.starts_with("ДС1")).unwrap().load().unwrap();
        let mut g = Game::from_scenario(c, &s, HeroClass::Knight, 3);
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        g.world.armies.clear();
        let foot = g.world.map.reachable(g.tile());
        let reached = |t: Tile| g.world.map.mask_index(t).is_some_and(|i| foot[i]);
        let yard = (0..g.world.locations.len()).find(|&i| g.world.locations[i].kind == LocationKind::Shipyard && reached(g.world.locations[i].tile)).expect("a shipyard on foot");
        let far: Vec<usize> = (0..g.world.locations.len()).filter(|&i| !g.world.locations[i].kind.is_bridge() && !reached(g.world.locations[i].tile)).collect();
        assert!(!far.is_empty(), "ДС1 has buildings beyond the water");
        // Stand in the shipyard and rent.
        g.pos = g.world.map.center(g.world.locations[yard].tile);
        g.location = Some(yard);
        g.gold = 1000;
        g.rent_ship().unwrap();
        let target = far.iter().copied().find(|&i| !g.plan(g.world.locations[i].tile).is_empty()).expect("a building across the water");
        let entry = g.world.locations[target].tile;
        assert!(g.set_destination(entry));
        let mut events = Vec::new();
        for _ in 0..100_000 {
            if !g.moving() {
                break;
            }
            events.extend(g.tick(0.05));
            // Scripted messages, and buildings on the way, stop the walk; keep going.
            if !g.moving() && g.location != Some(target) && g.foe.is_none() {
                g.set_destination(entry);
            }
        }
        assert_eq!(g.location, Some(target));
        assert!(events.contains(&Event::Arrived(target)) || g.foe.is_some());
        assert!(g.ship.is_none(), "landed: the ship is gone");
    }
}
