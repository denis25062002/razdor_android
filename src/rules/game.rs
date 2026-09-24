use std::sync::Arc;

use crate::dt::dtm::Scenario;

use super::battle::{Battle, Outcome, Team};
use super::clock::{Clock, Tick};
use super::content::{Bonus, Content, HeroClass, ItemId, Source, UnitId};
use super::formation::Slot;
use super::items::{self, EquipError};
use super::map::{Tile, TileMap};
use super::rng::Rng;
use super::units::{PromoteError, Stats, Unit};
use super::world::{LocationKind, Owner, Troop, World};

/// Game minutes that pass per real second while the party walks (1 h ≈ 0.2 s).
pub const MINUTES_PER_SECOND: f32 = 300.0;
/// Hostile armies chase the player inside this many cells *(guess)*.
pub const CHASE_RADIUS: i32 = 6;
/// Armies meet (and hostile ones attack) on neighbouring cells (mechanics.md 5.2).
const CONTACT: i32 = 1;
/// Largest slice of game time simulated at once, so chases stay smooth.
const STEP_MINUTES: f32 = 5.0;
/// Cells an AI army's pathfinder may expand per search.
const AI_PATH_NODES: usize = 4000;
/// A friendly army greets the player again only after he has gone this far away.
const MEET_AGAIN_DISTANCE: i32 = 4;
const SPAWN_EVERY_DAYS: u64 = 3;
const MAX_GANGS_PER_CAMP: usize = 2;
/// Demo markets restock every 7 days.
const RESTOCK_EVERY_DAYS: u64 = 7;
/// Unworn items the party can carry.
pub const PACK_SIZE: usize = 16;
/// Items on sale in each demo market after a restock.
pub const MARKET_STOCK: usize = 6;
/// Percent chance that a beaten demo gang drops an item.
const GANG_LOOT_CHANCE: i32 = 30;
/// Percent chance that a demo village pays tribute with an item instead of gold.
const TRIBUTE_ITEM_CHANCE: i32 = 25;
/// The Ranger hero moves 20% faster on the map (mechanics.md 7).
const RANGER_SPEED: f32 = 1.2;

#[derive(Debug, PartialEq, Eq)]
pub enum HireError {
    NotOffered,
    NotEnoughGold,
    SquadFull,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TradeError {
    NoMarket,
    NotEnoughGold,
    PackFull,
    NoSuchItem,
    /// Personal items cannot be sold.
    NotForSale,
}

/// What a village paid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tribute {
    Gold(i32),
    Item(ItemId),
}

#[derive(Debug, PartialEq, Eq)]
pub enum BattleResult {
    /// `loot` went into the pack; `left_behind` items did not fit.
    /// `level_ups`: (squad index, new level). `captured`: the castle or fort now the player's.
    Victory {
        reward: i32,
        lost: usize,
        loot: Vec<ItemId>,
        left_behind: usize,
        level_ups: Vec<(usize, i32)>,
        captured: Option<usize>,
    },
    Withdrew { lost: usize, level_ups: Vec<(usize, i32)> },
    Defeat,
}

/// The noon report (video notes: the daily report comes at 12:00).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayReport {
    /// Absolute day index (see [`Clock::day_index`]).
    pub day: u64,
    pub income: i32,
    pub mana: i32,
    pub wages: i32,
    pub unpaid: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The party stopped on a location (index into `world.locations`).
    Arrived(usize),
    /// A hostile army caught the party (index into `world.armies`).
    Encounter(usize),
    /// A friendly army met the party on the road; no battle.
    Met(usize),
    NewDay(DayReport),
}

/// Who the next battle is against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Foe {
    /// The garrison of a location (a castle, a fort, ruins, a demo camp).
    Garrison(usize),
    Army(usize),
}

pub struct Game {
    pub content: Arc<Content>,
    /// Squad member 0 is always the hero.
    pub squad: Vec<Unit>,
    pub gold: i32,
    pub mana: i32,
    pub clock: Clock,
    /// Party position in world units (see `map::center`).
    pub pos: (f32, f32),
    /// Remaining route, next tile first.
    pub path: Vec<Tile>,
    pub world: World,
    /// Location the party is standing on, if any.
    pub location: Option<usize>,
    pub foe: Option<Foe>,
    /// Shared bag of unworn items.
    pub pack: Vec<ItemId>,
    /// Spells known (1-based spell index; used from Stage 7).
    pub spells: Vec<u8>,
    start_day: u64,
    rng: Rng,
    battles: u64,
}

/// Moves `pos` along `path` for up to `minutes` of game time; cell costs are multiplied by
/// `slowness`. Stops early on a cell `stop` accepts (the rest of the path is dropped).
/// Returns the minutes used.
fn walk(map: &TileMap, pos: &mut (f32, f32), path: &mut Vec<Tile>, minutes: f32, slowness: f32, stop: &dyn Fn(Tile) -> bool) -> f32 {
    let mut left = minutes;
    while left > 0.0 {
        let Some(&next) = path.first() else { break };
        let per_tile = map.minutes(next).unwrap_or(60) as f32 * slowness;
        let goal = map.center(next);
        let (dx, dy) = (goal.0 - pos.0, goal.1 - pos.1);
        let need = (dx * dx + dy * dy).sqrt() * per_tile;
        if need <= left {
            *pos = goal;
            path.remove(0);
            left -= need;
            if stop(next) {
                path.clear();
                break;
            }
        } else {
            let k = left / need;
            pos.0 += dx * k;
            pos.1 += dy * k;
            left = 0.0;
        }
    }
    minutes - left
}

impl Game {
    fn with_world(content: Arc<Content>, world: World, squad: Vec<Unit>, tile: Tile, seed: u64) -> Game {
        let clock = world.start;
        let mut g = Game {
            squad,
            gold: 0,
            mana: 0,
            content,
            clock,
            pos: world.map.center(tile),
            path: Vec::new(),
            location: world.location_at(tile),
            world,
            foe: None,
            pack: Vec::new(),
            spells: Vec::new(),
            start_day: clock.day_index(),
            rng: Rng::new(seed ^ 0x9e37_79b9),
            battles: 0,
        };
        g.restock_markets();
        g
    }

    /// A new demo game. `content` must hold the demo units (see [`World::standard`]).
    pub fn new(content: Arc<Content>, hero: HeroClass, seed: u64) -> Self {
        let world = World::standard(&content);
        let home = world.locations[0].tile;
        let id = hero.unit();
        let slot = content.formation.free_slot(&[], Stats::of_level(&content, id, 1).preferred_row()).expect("empty formation");
        let squad = vec![Unit::new(&content, id, slot)];
        let gold = content.start_gold(hero);
        let mut g = Game::with_world(content, world, squad, home, seed);
        g.gold = gold;
        g
    }

    /// A new game on an original scenario, with the hero preset of `hero`.
    pub fn from_scenario(content: Arc<Content>, scenario: &Scenario, hero: HeroClass, seed: u64) -> Self {
        let world = World::from_scenario(scenario, &content);
        let start = world.hero_start(scenario, &content, hero);
        let mut leader = Unit::new(&content, hero.unit(), start.hero_slot);
        leader.gain_xp(&content, start.experience);
        leader.heal_full(&content);
        let mut squad = vec![leader];
        squad.extend(start.troops.iter().map(|t| troop_unit(&content, t)));
        let mut g = Game::with_world(content, world, squad, start.tile, seed);
        g.gold = start.gold;
        g.pack = start.items;
        g.spells = start.spells;
        g
    }

    pub fn hero(&self) -> &Unit {
        &self.squad[0]
    }

    pub fn hero_class(&self) -> Option<HeroClass> {
        HeroClass::of_unit(self.hero().def)
    }

    /// Army cap: the formation's size.
    pub fn max_squad(&self) -> usize {
        self.content.formation.capacity()
    }

    fn squad_has(&self, b: &Bonus) -> bool {
        self.squad.iter().any(|u| u.alive() && u.stats(&self.content).has(b))
    }

    /// Daily wage of squad member `i`: from its cost (mechanics.md 1.5); the hero is free,
    /// `AddPayment` in the army cuts every wage by 30%.
    pub fn wage(&self, i: usize) -> i32 {
        if i == 0 {
            return 0;
        }
        let w = self.content.wage(self.squad[i].def);
        if self.squad_has(&Bonus::AddPayment) {
            w * 70 / 100
        } else {
            w
        }
    }

    /// Price to buy `item`: `Merchant` in the army takes 30% off.
    pub fn buy_price(&self, item: ItemId) -> i32 {
        let p = self.content.item(item).cost.max(0);
        if self.squad_has(&Bonus::Merchant) {
            p * 70 / 100
        } else {
            p
        }
    }

    /// Price a market pays for `item`: `ItemSaleCost`%, +50% with a `Merchant`.
    pub fn sell_price(&self, item: ItemId) -> i32 {
        let p = items::sell_price(&self.content, item);
        if self.squad_has(&Bonus::Merchant) {
            p * 150 / 100
        } else {
            p
        }
    }

    pub fn tile(&self) -> Tile {
        self.world.map.tile_at(self.pos)
    }

    pub fn moving(&self) -> bool {
        !self.path.is_empty()
    }

    /// Terrain-cost multiplier of the hero's army.
    fn slowness(&self) -> f32 {
        if self.hero_class() == Some(HeroClass::Ranger) {
            1.0 / RANGER_SPEED
        } else {
            1.0
        }
    }

    /// Minutes the hero needs to walk `path`.
    pub fn travel_minutes(&self, path: &[Tile]) -> f32 {
        self.world.map.path_minutes(self.tile(), path) as f32 * self.slowness()
    }

    /// Minutes left on the current route.
    pub fn minutes_left(&self) -> f32 {
        self.travel_minutes(&self.path)
    }

    /// Unit types the barracks here offers (the player's or a friendly building).
    pub fn recruits_here(&self) -> Vec<UnitId> {
        match self.location.map(|l| &self.world.locations[l]) {
            Some(l) if !l.hostile() => l.recruits.iter().filter(|r| r.stock != Some(0)).map(|r| r.unit).collect(),
            _ => Vec::new(),
        }
    }

    /// Total wages due at the next report.
    pub fn daily_wages(&self) -> i32 {
        (0..self.squad.len()).map(|i| self.wage(i)).sum()
    }

    /// Gold the player's buildings pay each day.
    pub fn daily_income(&self) -> i32 {
        self.world.locations.iter().filter(|l| l.owned() && l.pays_income()).map(|l| l.gold_income).sum()
    }

    /// Mana the player's buildings give each day.
    pub fn daily_mana(&self) -> i32 {
        self.world.locations.iter().filter(|l| l.owned() && l.pays_income()).map(|l| l.mana_income).sum()
    }

    /// Walk to `to` along the cheapest path. Clicking a building's walls means its entry.
    /// Returns false if it can't be reached.
    pub fn set_destination(&mut self, to: Tile) -> bool {
        let to = self.world.location_covering(to).map(|l| &self.world.locations[l]).filter(|l| !l.kind.is_bridge()).map_or(to, |l| l.tile);
        let path = self.world.map.path(self.tile(), to);
        if path.is_empty() {
            return false;
        }
        self.path = path;
        self.location = None;
        true
    }

    pub fn stop(&mut self) {
        self.path.clear();
    }

    /// Advance the world by `real_dt` seconds. Time only flows while the party walks.
    pub fn tick(&mut self, real_dt: f32) -> Vec<Event> {
        let mut events = Vec::new();
        if !self.moving() || self.foe.is_some() {
            return events;
        }
        let mut budget = real_dt * MINUTES_PER_SECOND;
        let slowness = self.slowness();
        while budget > 0.0 && self.moving() {
            let slice = budget.min(STEP_MINUTES);
            let Game { world, pos, path, .. } = self;
            // A hostile garrison stops the party at its gate.
            let at_gate = |t: Tile| world.location_at(t).is_some_and(|l| world.locations[l].defended());
            let used = walk(&world.map, pos, path, slice, slowness, &at_gate);
            budget -= slice;
            self.pass_time(used, &mut events);
            if let Some(e) = self.contact() {
                events.push(e);
                return events;
            }
            if !self.moving() {
                if let Some(l) = self.world.location_at(self.tile()) {
                    self.arrive(l);
                    events.push(Event::Arrived(l));
                }
            }
        }
        events
    }

    /// Stand still for `hours` (the original's 1 h and 4 h waits): time passes, armies move,
    /// the daily moments happen. A hostile army reaching the party ends the wait.
    pub fn wait(&mut self, hours: u32) -> Vec<Event> {
        let mut events = Vec::new();
        if self.foe.is_some() {
            return events;
        }
        self.stop();
        let mut left = hours as f32 * 60.0;
        while left > 0.0 {
            let slice = left.min(STEP_MINUTES);
            left -= slice;
            self.pass_time(slice, &mut events);
            if let Some(e) = self.contact() {
                events.push(e);
                break;
            }
        }
        events
    }

    /// An army on a neighbouring cell: a hostile one attacks, a friendly one greets once.
    fn contact(&mut self) -> Option<Event> {
        let now = self.clock.total_minutes();
        let here = self.tile();
        let mut found = None;
        let map = &self.world.map;
        for (i, a) in self.world.armies.iter_mut().enumerate() {
            let d = map.distance(a.tile(map), here);
            if d > MEET_AGAIN_DISTANCE {
                a.met = false;
            }
            if found.is_some() || d > CONTACT || now < a.ignore_until {
                continue;
            }
            if a.hostile() {
                found = Some(Event::Encounter(i));
            } else if !a.met {
                a.met = true;
                found = Some(Event::Met(i));
            }
        }
        if let Some(e) = &found {
            self.path.clear();
            if let Event::Encounter(i) = e {
                self.foe = Some(Foe::Army(*i));
            }
        }
        found
    }

    fn arrive(&mut self, l: usize) {
        self.location = Some(l);
        let loc = &self.world.locations[l];
        if loc.defended() {
            self.foe = Some(Foe::Garrison(l));
            return;
        }
        let heals = matches!(loc.kind, LocationKind::Castle | LocationKind::Fort | LocationKind::Town | LocationKind::Church);
        if heals && !loc.hostile() {
            self.heal_all();
        }
    }

    fn pass_time(&mut self, minutes: f32, events: &mut Vec<Event>) {
        for tick in self.clock.advance(minutes as f64) {
            match tick {
                Tick::Midnight(_) => self.world.locations.iter_mut().for_each(|l| l.refill()),
                Tick::Noon(day) => {
                    let report = self.new_day(day);
                    events.push(Event::NewDay(report));
                }
            }
        }
        self.move_armies(minutes);
    }

    fn heal_all(&mut self) {
        let c = self.content.clone();
        self.squad.iter_mut().for_each(|u| u.heal_full(&c));
    }

    /// Percent of max HP the army heals each day: 15 with an `ArmyMedic`, 20 with a Ranger
    /// hero (mechanics.md 1.2, 1.3). They do not add up *(guess)*.
    pub fn daily_heal_percent(&self) -> i32 {
        let medic = if self.squad_has(&Bonus::ArmyMedic) { 15 } else { 0 };
        let ranger = if self.hero_class() == Some(HeroClass::Ranger) { 20 } else { 0 };
        medic.max(ranger)
    }

    /// The noon report: income and mana from the player's buildings, wages (units that
    /// cannot be paid are marked unpaid), daily healing.
    fn new_day(&mut self, day: u64) -> DayReport {
        let income = self.daily_income();
        let mana = self.daily_mana();
        self.gold += income;
        self.mana += mana;
        let (mut wages, mut unpaid) = (0, 0);
        for i in 1..self.squad.len() {
            let w = self.wage(i);
            let u = &mut self.squad[i];
            if self.gold >= w {
                self.gold -= w;
                wages += w;
                u.unpaid = false;
            } else {
                u.unpaid = true;
                unpaid += 1;
            }
        }
        let heal = self.daily_heal_percent();
        if heal > 0 {
            let c = self.content.clone();
            for u in self.squad.iter_mut().filter(|u| u.alive()) {
                let max = u.max_hp(&c);
                u.hp = (u.hp + max * heal / 100).min(max);
            }
        }
        let n = day.saturating_sub(self.start_day) + 1; // the game's first noon is day 1
        if self.world.demo {
            if (n - 1).is_multiple_of(RESTOCK_EVERY_DAYS) && n > 1 {
                self.restock_markets();
            }
            if n.is_multiple_of(SPAWN_EVERY_DAYS) {
                let camps: Vec<_> = self.world.camps().filter(|(_, l)| !l.cleared).map(|(i, l)| (i, l.tile)).collect();
                for (camp, tile) in camps {
                    if self.world.armies.iter().filter(|p| p.home == Some(camp)).count() < MAX_GANGS_PER_CAMP {
                        self.world.spawn_gang(camp, tile);
                    }
                }
            }
        }
        DayReport { day, income, mana, wages, unpaid }
    }

    fn move_armies(&mut self, minutes: f32) {
        let now = self.clock.total_minutes();
        let hero_tile = self.tile();
        let entries: Vec<Tile> = self.world.locations.iter().map(|l| l.tile).collect();
        let World { map, armies, .. } = &mut self.world;
        for a in armies.iter_mut() {
            let here = a.tile(map);
            let near = a.hostile() && now >= a.ignore_until && map.distance(here, hero_tile) <= CHASE_RADIUS;
            if near {
                if !a.chasing || a.path.last() != Some(&hero_tile) {
                    a.path = map.path_limited(here, hero_tile, AI_PATH_NODES);
                    a.chasing = true;
                }
            } else if a.chasing {
                a.chasing = false;
                a.path.clear();
            }
            if a.path.is_empty() && !a.chasing && a.patrols && a.patrol_radius > 0 && now >= a.rest_until {
                let r = a.patrol_radius;
                for _ in 0..3 {
                    let t = (a.post.0 + self.rng.range(-r, r), a.post.1 + self.rng.range(-r, r));
                    if map.passable(t) && !entries.contains(&t) && map.distance(t, a.post) <= r {
                        a.path = map.path_limited(here, t, AI_PATH_NODES);
                        if !a.path.is_empty() {
                            break;
                        }
                    }
                }
                // Rest between patrol legs, or after failing to find one *(guess)*.
                a.rest_until = now + self.rng.range(30, 180) as f64;
            }
            walk(map, &mut a.pos, &mut a.path, minutes, a.slowness, &|_| false);
        }
    }

    /// Hire a unit type offered here, at its `Cost`, into the first free cell.
    pub fn hire(&mut self, kind: UnitId) -> Result<(), HireError> {
        if !self.recruits_here().contains(&kind) {
            return Err(HireError::NotOffered);
        }
        let taken: Vec<Slot> = self.squad.iter().map(|u| u.slot).collect();
        let row = Stats::of_level(&self.content, kind, 1).preferred_row();
        let slot = match self.content.formation.free_slot(&taken, row) {
            Some(slot) if self.squad.len() < self.max_squad() => slot,
            _ => return Err(HireError::SquadFull),
        };
        let cost = self.content.unit(kind).cost.max(0);
        if self.gold < cost {
            return Err(HireError::NotEnoughGold);
        }
        self.gold -= cost;
        if let Some(l) = self.location {
            if let Some(r) = self.world.locations[l].recruits.iter_mut().find(|r| r.unit == kind) {
                if let Some(n) = r.stock.as_mut() {
                    *n -= 1;
                }
            }
        }
        self.squad.push(Unit::new(&self.content, kind, slot));
        Ok(())
    }

    /// Promote squad member `unit` to class `to` of its upgrade tree. Items the new class
    /// cannot wear go to the pack.
    pub fn promote(&mut self, unit: usize, to: UnitId) -> Result<(), PromoteError> {
        let c = self.content.clone();
        let u = self.squad.get_mut(unit).ok_or(PromoteError::NotAvailable)?;
        let removed = u.promote(&c, to)?;
        self.pack.extend(removed);
        Ok(())
    }

    /// Village here with tribute waiting.
    fn village_ready(&self) -> Option<usize> {
        let l = self.location?;
        let v = &self.world.locations[l];
        (v.kind == LocationKind::Village && (v.tribute_gold > 0 || v.tribute_mana > 0) && !v.hostile()).then_some(l)
    }

    /// Tribute the village here would pay now, if any is waiting.
    pub fn tribute_available(&self) -> Option<i32> {
        self.village_ready().map(|l| self.world.locations[l].tribute_gold)
    }

    /// Takes the village's waiting tribute: (gold, mana).
    fn use_village(&mut self) -> Option<(i32, i32)> {
        let l = self.village_ready()?;
        let v = &mut self.world.locations[l];
        let got = (v.tribute_gold, v.tribute_mana);
        v.tribute_gold = 0;
        v.tribute_mana = 0;
        Some(got)
    }

    /// Collects the waiting tribute: gold and mana; in the demo sometimes an item instead
    /// (gold if the pack is full).
    pub fn collect_tribute(&mut self) -> Option<Tribute> {
        let (gold, mana) = self.use_village()?;
        self.mana += mana;
        if self.world.demo && self.pack.len() < PACK_SIZE && self.rng.range(1, 100) <= TRIBUTE_ITEM_CHANCE {
            if let Some(item) = self.roll_item(Source::Tribute) {
                self.pack.push(item);
                return Some(Tribute::Item(item));
            }
        }
        self.gold += gold;
        Some(Tribute::Gold(gold))
    }

    /// The village priest heals the squad instead of tribute being collected.
    pub fn priest_heal(&mut self) -> bool {
        let used = self.use_village().is_some();
        if used {
            self.heal_all();
        }
        used
    }

    /// Battle against the pending foe. Unpaid units refuse to fight. Walking into a garrison
    /// makes the player the attacker (the building's extra defence helps the garrison); an
    /// army that catches the player attacks.
    pub fn start_battle(&mut self) -> Battle {
        let (enemies, attacker, defence) = match self.foe {
            Some(Foe::Garrison(l)) => {
                let loc = &self.world.locations[l];
                (loc.garrison.clone(), Team::Player, loc.garrison_defence)
            }
            Some(Foe::Army(i)) => (self.world.armies[i].troops.clone(), Team::Enemy, 0),
            None => (Vec::new(), Team::Player, 0),
        };
        let enemies: Vec<Unit> = enemies.iter().map(|t| troop_unit(&self.content, t)).collect();
        let player: Vec<_> = self.squad.iter().enumerate().filter(|(i, u)| *i == 0 || !u.unpaid).collect();
        self.battles += 1;
        let mut b = Battle::new(self.content.clone(), &player, &enemies, attacker);
        if defence > 0 {
            b.set_building_defence(Team::Enemy, defence);
        }
        b
    }

    /// Gold the victor takes from a beaten army: `gold / VictoryGoldDiv`, at least
    /// `MinVictoryGold` (mechanics.md 7).
    fn victory_gold(&self, gold: i32) -> i32 {
        let o = &self.content.options;
        (gold / o.victory_gold_div.max(1)).max(o.min_victory_gold)
    }

    /// Writes the battle back into the squad: HP, deployed cells, XP and levels. The dead
    /// (except the hero, who survives while his army does) leave the squad with their items;
    /// potion effects end. A won garrison fight captures a castle or fort (owner = player,
    /// its income starts) and gives ruins' treasure; a beaten army leaves the map.
    pub fn resolve_battle(&mut self, battle: &Battle) -> BattleResult {
        for r in battle.player_results() {
            let u = &mut self.squad[r.squad_index];
            u.hp = r.hp;
            u.slot = r.slot;
        }
        let mut level_ups = Vec::new();
        let c = self.content.clone();
        for a in battle.xp_awards(Team::Player) {
            let Some(i) = battle.fighters[a.fighter].squad_index else { continue };
            let gained = self.squad[i].gain_xp(&c, a.xp);
            if gained > 0 {
                level_ups.push((i, self.squad[i].level));
            }
        }
        for u in &mut self.squad {
            u.potions.clear();
            u.hp = u.hp.min(u.max_hp(&c));
        }
        let before = self.squad.len();
        let hero = self.squad.remove(0);
        let mut survivors: Vec<(usize, Unit)> = self.squad.drain(..).enumerate().filter(|(_, u)| u.hp > 0).collect();
        self.squad.push(hero);
        // Squad indices shift when the dead leave.
        let mut remap = vec![Some(0)];
        remap.extend((0..before - 1).map(|i| survivors.iter().position(|(j, _)| *j == i).map(|p| p + 1)));
        self.squad.extend(survivors.drain(..).map(|(_, u)| u));
        let level_ups: Vec<(usize, i32)> = level_ups.into_iter().filter_map(|(i, l)| remap[i].map(|n| (n, l))).collect();
        let lost = before - self.squad.len();
        let foe = self.foe.take();

        match (battle.outcome(), foe) {
            (Outcome::Victory, Some(Foe::Garrison(l))) => {
                let loc = &mut self.world.locations[l];
                loc.cleared = true;
                loc.garrison.clear();
                let reward = std::mem::take(&mut loc.treasure_gold);
                let treasure = std::mem::take(&mut loc.treasure);
                let rolls = std::mem::take(&mut loc.loot_rolls);
                let captured = loc.kind.capturable().then(|| {
                    loc.owner = Owner::Player;
                    loc.faction = 1;
                    loc.attitude = 3;
                    l
                });
                self.gold += reward;
                let mut found = treasure;
                found.extend((0..rolls).filter_map(|_| self.roll_item(Source::Loot)));
                let (loot, left_behind) = self.take_items(found);
                BattleResult::Victory { reward, lost, loot, left_behind, level_ups, captured }
            }
            (Outcome::Victory, Some(Foe::Army(i))) => {
                let army = self.world.armies.remove(i);
                let reward = self.victory_gold(army.gold);
                self.gold += reward;
                let mut found = army.items;
                if army.id == 0 && self.rng.range(1, 100) <= GANG_LOOT_CHANCE {
                    found.extend(self.roll_item(Source::Loot));
                }
                let (loot, left_behind) = self.take_items(found);
                BattleResult::Victory { reward, lost, loot, left_behind, level_ups, captured: None }
            }
            (Outcome::Victory, None) => {
                BattleResult::Victory { reward: 0, lost, loot: Vec::new(), left_behind: 0, level_ups, captured: None }
            }
            (Outcome::Defeat, _) => BattleResult::Defeat,
            (_, foe) => {
                if let Some(Foe::Army(i)) = foe {
                    self.world.armies[i].ignore_until = self.clock.total_minutes() + 120.0;
                }
                BattleResult::Withdrew { lost, level_ups }
            }
        }
    }

    pub fn won(&self) -> bool {
        self.world.all_camps_cleared()
    }

    /// A random item of the given source, if the table has any.
    fn roll_item(&mut self, source: Source) -> Option<ItemId> {
        let pool = self.content.items_from(source);
        if pool.is_empty() {
            return None;
        }
        Some(pool[self.rng.range(0, pool.len() as i32 - 1) as usize])
    }

    /// Puts found items into the pack. Returns (kept, left behind).
    fn take_items(&mut self, found: Vec<ItemId>) -> (Vec<ItemId>, usize) {
        let mut kept = Vec::new();
        let mut left_behind = 0;
        for item in found {
            if self.pack.len() < PACK_SIZE {
                self.pack.push(item);
                kept.push(item);
            } else {
                left_behind += 1;
            }
        }
        (kept, left_behind)
    }

    /// Every shop: its fixed goods plus random market items (within its price range).
    fn restock_markets(&mut self) {
        for l in 0..self.world.locations.len() {
            let Some(shop) = &self.world.locations[l].shop else { continue };
            let (random, (lo, hi)) = (shop.random, shop.price);
            let pool: Vec<ItemId> = self
                .content
                .items_from(Source::Market)
                .into_iter()
                .filter(|&i| hi <= 0 || (lo..=hi).contains(&self.content.item(i).cost))
                .collect();
            let mut stock = shop.fixed.clone();
            if !pool.is_empty() {
                stock.extend((0..random).map(|_| pool[self.rng.range(0, pool.len() as i32 - 1) as usize]));
            }
            if let Some(shop) = &mut self.world.locations[l].shop {
                shop.stock = stock;
            }
        }
    }

    /// Items for sale where the party stands, if there is a market.
    pub fn market_here(&self) -> Option<&[ItemId]> {
        let loc = &self.world.locations[self.location?];
        loc.shop.as_ref().filter(|_| !loc.hostile()).map(|s| s.stock.as_slice())
    }

    pub fn buy(&mut self, stock_index: usize) -> Result<ItemId, TradeError> {
        let item = *self.market_here().ok_or(TradeError::NoMarket)?.get(stock_index).ok_or(TradeError::NoSuchItem)?;
        let price = self.buy_price(item);
        if self.gold < price {
            return Err(TradeError::NotEnoughGold);
        }
        if self.pack.len() >= PACK_SIZE {
            return Err(TradeError::PackFull);
        }
        if let Some(shop) = self.location.and_then(|l| self.world.locations[l].shop.as_mut()) {
            shop.stock.remove(stock_index);
        }
        self.gold -= price;
        self.pack.push(item);
        Ok(item)
    }

    /// Sells a pack item for [`Game::sell_price`]. Returns the gold gained.
    pub fn sell(&mut self, pack_index: usize) -> Result<i32, TradeError> {
        self.market_here().ok_or(TradeError::NoMarket)?;
        let item = *self.pack.get(pack_index).ok_or(TradeError::NoSuchItem)?;
        if self.content.item(item).is_personal() {
            return Err(TradeError::NotForSale);
        }
        let price = self.sell_price(item);
        self.pack.remove(pack_index);
        self.gold += price;
        Ok(price)
    }

    /// Moves a pack item onto squad member `unit` (slot and class rules in [`items::slot_for`]).
    pub fn equip(&mut self, unit: usize, pack_index: usize) -> Result<(), EquipError> {
        let item = *self.pack.get(pack_index).ok_or(EquipError::NoSuchItem)?;
        let u = self.squad.get(unit).ok_or(EquipError::NoSuchItem)?;
        let slot = items::slot_for(&self.content, u, item)?;
        self.squad[unit].items[slot] = Some(item);
        self.pack.remove(pack_index);
        Ok(())
    }

    /// Squad member `unit` drinks the potion at `pack_index`. Returns HP restored.
    pub fn drink(&mut self, unit: usize, pack_index: usize) -> Result<i32, EquipError> {
        let item = *self.pack.get(pack_index).ok_or(EquipError::NoSuchItem)?;
        let c = self.content.clone();
        let u = self.squad.get_mut(unit).ok_or(EquipError::NoSuchItem)?;
        let healed = items::drink(&c, u, item)?;
        self.pack.remove(pack_index);
        Ok(healed)
    }

    /// Moves item slot `slot` of squad member `unit` back into the pack.
    pub fn unequip(&mut self, unit: usize, slot: usize) -> Result<(), EquipError> {
        if self.pack.len() >= PACK_SIZE {
            return Err(EquipError::PackFull);
        }
        let u = self.squad.get_mut(unit).ok_or(EquipError::NoSuchItem)?;
        let item = u.items.get_mut(slot).and_then(Option::take).ok_or(EquipError::NoSuchItem)?;
        u.hp = u.hp.min(u.max_hp(&self.content));
        self.pack.push(item);
        Ok(())
    }
}


/// A fresh unit for an army or garrison troop, at its level and full health.
fn troop_unit(content: &Content, t: &Troop) -> Unit {
    let mut u = Unit::new(content, t.unit, t.slot);
    u.level = t.level.max(1);
    u.heal_full(content);
    u
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::world::{demo_unit, GANG_REWARD};

    fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
    }

    fn content() -> Arc<Content> {
        Arc::new(Content::builtin())
    }

    fn new_game(hero: HeroClass, seed: u64) -> Game {
        Game::new(content(), hero, seed)
    }

    /// A game with no gangs on the map, for tests about travel and time.
    fn quiet_game(hero: HeroClass) -> Game {
        let mut g = new_game(hero, 1);
        g.world.armies.clear();
        g
    }

    fn unit(g: &Game, key: &str) -> UnitId {
        demo_unit(&g.content, key)
    }

    fn item(g: &Game, key: &str) -> ItemId {
        g.content.item_by_key(key).unwrap()
    }

    fn walk_until_stopped(g: &mut Game) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..10_000 {
            if !g.moving() {
                break;
            }
            events.extend(g.tick(0.05));
        }
        events
    }

    fn tile_of_location(g: &Game, name: &str) -> Tile {
        g.world.locations[g.world.index_of(name)].tile
    }

    /// Everyone but the player's hero drops dead.
    fn wipe_all_but_hero(b: &mut Battle) {
        for f in b.fighters.iter_mut().filter(|f| !f.is_hero) {
            f.hp = 0;
        }
    }

    #[test]
    fn starts_at_home_castle_in_the_morning() {
        let g = new_game(HeroClass::Knight, 1);
        assert_eq!(g.location, Some(0));
        assert_eq!(g.clock, Clock::demo_start());
        assert_eq!((g.hero().def, g.gold), (HeroClass::Knight.unit(), 100));
        assert_eq!(g.max_squad(), 12);
    }

    #[test]
    fn time_is_frozen_while_standing_still() {
        let mut g = new_game(HeroClass::Knight, 1);
        let parties: Vec<_> = g.world.armies.iter().map(|p| p.pos).collect();
        assert!(g.tick(5.0).is_empty());
        assert_eq!(g.clock, Clock::demo_start());
        assert_eq!(parties, g.world.armies.iter().map(|p| p.pos).collect::<Vec<_>>());
    }

    #[test]
    fn walking_to_a_village_takes_time_and_arrives() {
        let mut g = quiet_game(HeroClass::Knight);
        let millbrook = g.world.index_of("Millbrook");
        assert!(g.set_destination(tile_of_location(&g, "Millbrook")));
        assert_eq!(g.location, None);
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Arrived(millbrook)));
        assert_eq!(g.location, Some(millbrook));
        assert!(g.clock.total_minutes() > Clock::demo_start().total_minutes() + 60.0);
    }

    #[test]
    fn cannot_walk_into_the_sea() {
        let mut g = quiet_game(HeroClass::Knight);
        assert!(!g.set_destination((30, 40)), "open sea");
    }

    #[test]
    fn noon_pays_income_and_wages_and_marks_unpaid() {
        let mut g = quiet_game(HeroClass::Knight);
        g.hire(unit(&g, "spearman")).unwrap();
        g.hire(unit(&g, "archer")).unwrap();
        g.gold = 0;
        g.squad[1].hp = 1;
        let day = g.clock.day_index();
        let mut events = Vec::new();
        g.pass_time(3.0 * 60.0, &mut events); // 08:00 -> 11:00
        assert!(events.is_empty());
        g.pass_time(60.0, &mut events); // 12:00
        // Income 20; wages from cost: spearman 40/2×¼ = 5, archer 45/2×¼ = 5.6 → 6.
        assert_eq!(events, vec![Event::NewDay(DayReport { day, income: 20, mana: 0, wages: 11, unpaid: 0 })]);
        assert_eq!(g.gold, 9);

        g.gold = -20; // broke: 0 after income
        events.clear();
        g.pass_time(24.0 * 60.0, &mut events);
        assert_eq!(events, vec![Event::NewDay(DayReport { day: day + 1, income: 20, mana: 0, wages: 0, unpaid: 2 })]);
        assert!(g.squad[1].unpaid && g.squad[2].unpaid);
    }

    #[test]
    fn villages_refill_at_midnight() {
        let mut g = quiet_game(HeroClass::Knight);
        g.location = Some(g.world.index_of("Millbrook"));
        assert_eq!(g.tribute_available(), Some(10));
        assert!(g.priest_heal());
        assert_eq!(g.tribute_available(), None);
        let mut events = Vec::new();
        g.pass_time(15.0 * 60.0, &mut events); // 08:00 -> 23:00: the noon report only
        assert_eq!(events.len(), 1);
        assert_eq!(g.tribute_available(), None);
        g.pass_time(60.0, &mut events); // 00:00
        assert_eq!(g.tribute_available(), Some(10));
    }

    #[test]
    fn waiting_passes_time_and_moves_the_world() {
        let mut g = new_game(HeroClass::Knight, 3);
        let start = g.clock.total_minutes();
        let before: Vec<_> = g.world.armies.iter().map(|p| p.pos).collect();
        let events = g.wait(4);
        assert_eq!(g.clock.total_minutes(), start + 240.0);
        assert!(events.iter().any(|e| matches!(e, Event::NewDay(_))), "08:00 + 4 h crosses noon");
        assert_ne!(before, g.world.armies.iter().map(|p| p.pos).collect::<Vec<_>>(), "gangs patrol meanwhile");
        g.wait(1);
        assert_eq!(g.clock.total_minutes(), start + 300.0);
        assert!(!g.moving());
    }

    #[test]
    fn ranger_heals_the_army_every_day() {
        let mut g = quiet_game(HeroClass::Ranger);
        g.squad[0].hp = 10;
        let mut events = Vec::new();
        g.pass_time(4.0 * 60.0, &mut events); // noon
        assert_eq!(g.hero().hp, 10 + 55 * 20 / 100);
        let k = quiet_game(HeroClass::Knight);
        assert_eq!(k.daily_heal_percent(), 0);
    }

    #[test]
    fn unpaid_units_sit_out_battles() {
        let mut g = quiet_game(HeroClass::Knight);
        let spear = unit(&g, "spearman");
        g.hire(spear).unwrap();
        g.squad[1].unpaid = true;
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let b = g.start_battle();
        assert!(b.fighters.iter().all(|f| f.unit != spear));
        assert_eq!(b.attacker, Team::Player, "walking into a camp is an attack");
    }

    #[test]
    fn village_serves_once_per_day() {
        let mut g = quiet_game(HeroClass::Knight);
        g.set_destination(tile_of_location(&g, "Millbrook"));
        walk_until_stopped(&mut g);
        let gold = g.gold;
        match g.collect_tribute() {
            Some(Tribute::Gold(10)) => assert_eq!(g.gold, gold + 10),
            Some(Tribute::Item(item)) => assert_eq!(g.pack, vec![item]),
            other => panic!("{other:?}"),
        }
        assert_eq!(g.collect_tribute(), None);
        assert!(!g.priest_heal(), "already used today");
        let mut events = Vec::new();
        g.pass_time(24.0 * 60.0, &mut events);
        assert_eq!(g.tribute_available(), Some(10));
    }

    #[test]
    fn castle_heals_on_arrival() {
        let mut g = quiet_game(HeroClass::Knight);
        g.squad[0].hp = 5;
        g.set_destination(tile_of_location(&g, "Millbrook"));
        walk_until_stopped(&mut g);
        g.set_destination(tile_of_location(&g, "Oakford"));
        walk_until_stopped(&mut g);
        assert_eq!(g.hero().hp, 70);
    }

    #[test]
    fn walking_into_a_gang_starts_an_encounter() {
        let mut g = quiet_game(HeroClass::Knight);
        let target = tile_of_location(&g, "Millbrook");
        g.set_destination(target);
        // Two tiles ahead: inside the chase radius, so it closes in.
        let ahead = g.path[1];
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_gang(camp, ahead);
        let events = walk_until_stopped(&mut g);
        assert!(matches!(events.last(), Some(Event::Encounter(0))), "{events:?}");
        assert_eq!(g.foe, Some(Foe::Army(0)));
        assert!(!g.moving());
        assert_eq!(g.start_battle().attacker, Team::Enemy, "the gang attacks");
    }

    #[test]
    fn gangs_chase_a_nearby_party() {
        let mut g = quiet_game(HeroClass::Knight);
        g.set_destination(tile_of_location(&g, "Millbrook"));
        let camp = g.world.index_of("Bandit camp");
        let start = (g.tile().0 + 3, g.tile().1 + 2);
        assert!(g.world.map.passable(start));
        g.world.spawn_gang(camp, start);
        let before = distance(g.world.armies[0].pos, g.pos);
        for _ in 0..3 {
            g.tick(0.05);
        }
        assert!(g.world.armies[0].chasing);
        assert!(distance(g.world.armies[0].pos, g.pos) < before + 0.5, "it keeps up");
    }

    #[test]
    fn beating_a_gang_removes_it_pays_and_gives_xp() {
        let mut g = quiet_game(HeroClass::Knight);
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_gang(camp, (30, 20));
        g.foe = Some(Foe::Army(0));
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        let gold = g.gold;
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { reward: GANG_REWARD, lost: 0, captured: None, .. }));
        assert_eq!(g.gold, gold + GANG_REWARD);
        assert!(g.world.armies.is_empty());
        assert_eq!(g.foe, None);
        assert!(g.hero().xp > 0 || g.hero().level > 1, "XP after the battle");
    }

    #[test]
    fn stalemate_with_a_gang_buys_time_to_escape() {
        let mut g = quiet_game(HeroClass::Knight);
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_gang(camp, (30, 20));
        g.foe = Some(Foe::Army(0));
        let mut b = g.start_battle();
        b.begin();
        while b.outcome() == Outcome::Ongoing {
            b.skip();
        }
        assert!(matches!(g.resolve_battle(&b), BattleResult::Withdrew { lost: 0, .. }));
        assert!(g.world.armies[0].ignore_until > g.clock.total_minutes());
    }

    #[test]
    fn camps_send_out_new_gangs_every_few_days() {
        let mut g = quiet_game(HeroClass::Knight);
        let mut events = Vec::new();
        g.pass_time((4 + 24) as f32 * 60.0, &mut events); // two noons
        assert!(g.world.armies.is_empty());
        g.pass_time(24.0 * 60.0, &mut events); // the third noon
        assert_eq!(g.world.armies.len(), 2, "one gang from each camp");
    }

    #[test]
    fn hire_checks_offer_gold_and_cap() {
        let mut g = quiet_game(HeroClass::Knight);
        let (spear, archer, sword) = (unit(&g, "spearman"), unit(&g, "archer"), unit(&g, "swordsman"));
        g.hire(spear).unwrap();
        assert_eq!(g.gold, 60);
        assert_eq!(g.squad[1].slot.row, crate::rules::formation::Row::Front);
        assert_eq!(g.hire(sword), Err(HireError::NotOffered));
        g.gold = 10_000;
        while g.squad.len() < g.max_squad() {
            g.hire(archer).unwrap();
        }
        assert_eq!(g.hire(archer), Err(HireError::SquadFull));
        g.location = None;
        assert_eq!(g.hire(archer), Err(HireError::NotOffered));
    }

    #[test]
    fn camp_victory_clears_and_pays() {
        let mut g = quiet_game(HeroClass::Knight);
        g.hire(unit(&g, "spearman")).unwrap();
        let camp = g.world.index_of("Bandit camp");
        g.foe = Some(Foe::Garrison(camp));
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        let result = g.resolve_battle(&b);
        assert!(matches!(&result, BattleResult::Victory { reward: 100, lost: 1, loot, left_behind: 0, captured: None, .. } if loot.len() == 1));
        assert_eq!(g.pack.len(), 1);
        assert!(g.world.locations[camp].cleared);
        assert!(!g.won());
    }

    #[test]
    fn a_fallen_hero_survives_if_his_army_does() {
        let mut g = quiet_game(HeroClass::Knight);
        g.hire(unit(&g, "spearman")).unwrap();
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        b.begin();
        for f in b.fighters.iter_mut().filter(|f| f.is_hero || f.team == Team::Enemy) {
            f.hp = 0;
        }
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { lost: 0, .. }));
        assert_eq!(g.hero().hp, 1);
    }

    #[test]
    fn losing_the_whole_army_is_defeat() {
        let mut g = quiet_game(HeroClass::Knight);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        b.begin();
        b.fighters[0].hp = 0;
        assert_eq!(g.resolve_battle(&b), BattleResult::Defeat);
    }

    #[test]
    fn auto_played_camp_battles_always_finish() {
        for seed in 0..10 {
            for hero in HeroClass::ALL {
                let mut g = new_game(hero, seed);
                g.world.armies.clear();
                g.hire(unit(&g, "spearman")).unwrap();
                g.foe = Some(Foe::Garrison(g.world.index_of(if seed % 2 == 0 { "Bandit camp" } else { "Bandit lair" })));
                let mut b = g.start_battle();
                b.begin();
                let mut steps = 0;
                while b.outcome() == Outcome::Ongoing {
                    b.ai_step();
                    steps += 1;
                    assert!(steps < 5000, "seed {seed}: battle never ended");
                }
                g.resolve_battle(&b);
            }
        }
    }

    #[test]
    fn level_ups_are_reported() {
        let mut g = quiet_game(HeroClass::Knight);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit lair")));
        g.squad[0].xp = g.squad[0].xp_to_next(&g.content) - 1;
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        let BattleResult::Victory { level_ups, .. } = g.resolve_battle(&b) else { panic!() };
        assert!(g.hero().level >= 2);
        assert_eq!(level_ups, vec![(0, g.hero().level)]);
    }

    #[test]
    fn promotion_moves_unwearable_items_to_the_pack() {
        let mut g = quiet_game(HeroClass::Knight);
        let (spear, sword) = (unit(&g, "spearman"), unit(&g, "swordsman"));
        g.hire(spear).unwrap();
        assert_eq!(g.promote(1, sword), Err(PromoteError::NotAvailable));
        g.squad[1].level = 2;
        g.promote(1, sword).unwrap();
        assert_eq!((g.squad[1].def, g.squad[1].level), (sword, 1));
    }

    fn at_oakford(g: &mut Game) {
        g.location = Some(g.world.index_of("Oakford"));
    }

    #[test]
    fn markets_stock_market_items_and_restock_weekly() {
        let mut g = quiet_game(HeroClass::Knight);
        at_oakford(&mut g);
        let stock = g.market_here().unwrap().to_vec();
        assert_eq!(stock.len(), MARKET_STOCK);
        assert!(stock.iter().all(|&i| g.content.sources(i).contains(&Source::Market)));
        g.gold = 10_000;
        g.buy(0).unwrap();
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK - 1);
        let mut events = Vec::new();
        g.pass_time((4 + 24 * 6) as f32 * 60.0, &mut events); // the 7th noon
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK - 1, "not a week yet");
        g.pass_time(24.0 * 60.0, &mut events);
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK);
    }

    #[test]
    fn buying_and_selling() {
        let mut g = quiet_game(HeroClass::Knight);
        g.location = None;
        assert_eq!(g.buy(0), Err(TradeError::NoMarket), "on the road");
        at_oakford(&mut g);
        let item = g.market_here().unwrap()[0];
        let price = g.content.item(item).cost;
        g.gold = price - 1;
        assert_eq!(g.buy(0), Err(TradeError::NotEnoughGold));
        g.gold = price;
        assert_eq!(g.buy(0), Ok(item));
        assert_eq!((g.gold, g.pack.clone()), (0, vec![item]));
        assert_eq!(g.sell(0), Ok(price / 4), "ItemSaleCost 25%");
        assert!(g.pack.is_empty());
        assert_eq!(g.sell(0), Err(TradeError::NoSuchItem));
        g.pack = vec![item; PACK_SIZE];
        g.gold = 10_000;
        assert_eq!(g.buy(0), Err(TradeError::PackFull));
        g.location = Some(g.world.index_of("Millbrook"));
        assert_eq!(g.sell(0), Err(TradeError::NoMarket));
    }

    #[test]
    fn equip_and_unequip_through_the_pack() {
        let mut g = quiet_game(HeroClass::Knight);
        let (sword, axe, shield, bow) = (item(&g, "short_sword"), item(&g, "war_axe"), item(&g, "oak_shield"), item(&g, "hunting_bow"));
        g.pack = vec![sword, axe, shield, bow];
        g.equip(0, 0).unwrap();
        assert_eq!(g.equip(0, 0), Err(EquipError::SecondWeapon), "axe is a second weapon");
        assert_eq!(g.equip(0, 2), Err(EquipError::WrongClass), "the knight is no archer");
        g.equip(0, 1).unwrap();
        assert_eq!(g.pack, vec![axe, bow]);
        assert_eq!(g.hero().max_hp(&g.content), 75);
        g.heal_all();
        let shield_slot = g.hero().items.iter().position(|i| *i == Some(shield)).unwrap();
        g.unequip(0, shield_slot).unwrap();
        assert_eq!(g.hero().hp, 70, "HP capped to the new max");
        assert_eq!(g.pack, vec![axe, bow, shield]);
        assert_eq!(g.unequip(0, shield_slot), Err(EquipError::NoSuchItem));
        g.pack = vec![axe; PACK_SIZE];
        assert_eq!(g.unequip(0, 0), Err(EquipError::PackFull));
    }

    #[test]
    fn gear_and_potions_go_into_battle_and_potions_wear_off() {
        let mut g = quiet_game(HeroClass::Knight);
        g.pack = vec![item(&g, "might_potion"), item(&g, "chainmail"), item(&g, "heal_potion")];
        g.equip(0, 1).unwrap();
        assert_eq!(g.drink(0, 0), Ok(0), "might: no healing, lasts until the battle ends");
        g.squad[0].hp = 30;
        assert_eq!(g.drink(0, 0), Ok(20));
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        b.begin();
        use crate::rules::content::Stat;
        assert_eq!(b.fighters[0].stats[Stat::DefenceBlow], 13, "10 + chainmail 3");
        assert_eq!(b.fighters[0].stats[Stat::AttackBlow], 28, "24 + might 4");
        wipe_all_but_hero(&mut b);
        g.resolve_battle(&b);
        assert!(g.hero().potions.is_empty());
        let h = g.hero();
        assert_eq!(h.stats(&g.content)[Stat::AttackBlow], h.base_stats(&g.content)[Stat::AttackBlow], "might is gone");
    }

    #[test]
    fn dead_recruits_take_their_gear_with_them() {
        let mut g = quiet_game(HeroClass::Knight);
        g.hire(unit(&g, "spearman")).unwrap();
        let mail = item(&g, "chainmail");
        g.squad[1].items[0] = Some(mail);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        g.resolve_battle(&b);
        assert_eq!(g.squad.len(), 1);
        assert!(!g.pack.contains(&mail));
    }

    #[test]
    fn loot_that_does_not_fit_is_left_behind() {
        let mut g = quiet_game(HeroClass::Knight);
        g.pack = vec![item(&g, "heal_potion"); PACK_SIZE];
        let lair = g.world.index_of("Bandit lair");
        g.foe = Some(Foe::Garrison(lair));
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { left_behind: 2, .. }));
    }

    #[test]
    fn villages_sometimes_pay_in_items() {
        let mut items = 0;
        for seed in 0..200 {
            let mut g = quiet_game(HeroClass::Knight);
            g.rng = Rng::new(seed);
            g.location = Some(g.world.index_of("Millbrook"));
            if let Some(Tribute::Item(item)) = g.collect_tribute() {
                assert!(g.content.sources(item).contains(&Source::Tribute));
                items += 1;
            }
        }
        assert!((25..=80).contains(&items), "about 25%: {items}/200");
    }

    // --- Scenario worlds (hand-built; see `world::testkit`) ---

    use crate::dt::dtm::{BuildingType, Scenario};
    use crate::rules::world::testkit::{self as tk, army, building, hero, scenario, troop};

    /// A 24×6 grass strip; the knight starts at (2, 2) with two warriors and 200 gold.
    fn strip() -> Scenario {
        let mut s = scenario(24, 6);
        s.header.heroes[0] = hero(2, 2, 200, &[troop(4, 0, 2)]);
        s.header.heroes[0].artifacts = [7, 0, 0];
        s
    }

    fn start(s: &Scenario) -> Game {
        Game::from_scenario(Arc::new(tk::content()), s, HeroClass::Knight, 5)
    }

    fn wipe_enemies(b: &mut Battle) {
        for f in b.fighters.iter_mut().filter(|f| f.team == Team::Enemy) {
            f.hp = 0;
        }
    }

    #[test]
    fn scenario_game_starts_from_the_preset() {
        let g = start(&strip());
        assert_eq!((g.tile(), g.gold, g.mana, g.squad.len()), ((2, 2), 200, 0, 3));
        assert_eq!(g.hero().def, HeroClass::Knight.unit());
        assert_eq!(g.pack, vec![ItemId(7)]);
        assert_eq!(g.clock.label(), "1204, month 5, day 19, 9 h");
        assert!(!g.won(), "no camps: not won by clearing them");
    }

    #[test]
    fn hostile_armies_attack_on_contact_and_leave_loot() {
        let mut s = strip();
        let mut foe = army(1, 12, 2, -2, &[troop(4, 0, 2), troop(5, 0, 1)]);
        foe.gold_income = 120;
        foe.artifacts = [9, 0, 0];
        s.armies = vec![foe];
        let mut g = start(&s);
        assert!(g.set_destination((22, 2)));
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Encounter(0)));
        assert_eq!(g.foe, Some(Foe::Army(0)));
        assert!(g.world.map.distance(g.tile(), g.world.armies[0].tile(&g.world.map)) <= 1);
        let mut b = g.start_battle();
        assert_eq!(b.attacker, Team::Enemy, "the army attacks");
        assert_eq!(b.fighters.iter().filter(|f| f.team == Team::Enemy).count(), 3);
        b.begin();
        wipe_enemies(&mut b);
        let gold = g.gold;
        let r = g.resolve_battle(&b);
        assert!(matches!(&r, BattleResult::Victory { reward: 60, captured: None, loot, .. } if loot == &vec![ItemId(9)]), "{r:?}");
        assert_eq!(g.gold, gold + 60, "half its gold");
        assert!(g.world.armies.is_empty());
    }

    #[test]
    fn friendly_armies_greet_once_and_do_not_fight() {
        let mut s = strip();
        s.armies = vec![army(1, 10, 2, 1, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.set_destination((22, 2));
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Met(0)));
        assert_eq!(g.foe, None);
        g.set_destination((22, 2));
        let events = walk_until_stopped(&mut g);
        assert!(!events.iter().any(|e| matches!(e, Event::Met(_) | Event::Encounter(_))), "{events:?}");
        assert_eq!(g.tile(), (22, 2));
    }

    #[test]
    fn hostile_armies_chase_a_nearby_hero() {
        let mut s = strip();
        let mut foe = army(1, 7, 4, -2, &[troop(4, 0, 1)]);
        foe.patrols = 0;
        s.armies = vec![foe];
        let mut g = start(&s);
        let events = g.wait(4);
        assert!(g.world.armies[0].chasing);
        assert!(matches!(events.last(), Some(Event::Encounter(0))), "it comes for the waiting hero: {events:?}");
        assert!(g.clock.total_minutes() < g.world.start.total_minutes() + 240.0, "the fight cuts the wait short");
    }

    #[test]
    fn a_hostile_fort_is_taken_by_beating_its_garrison() {
        let mut s = strip();
        let mut fort = building(BuildingType::Fort, 16, 3, (2, 2));
        fort.faction = 4;
        fort.relations = [-2, 0, 0, 0];
        fort.gold_per_day = 40;
        fort.mana_per_day = 5;
        fort.garrison[0] = troop(4, 0, 2);
        fort.garrison_extra_defence = 12;
        s.buildings = vec![fort];
        let mut g = start(&s);
        let entry = g.world.locations[0].tile;
        assert!(g.set_destination((15, 2)), "clicking the walls means the entry");
        assert_eq!(g.path.last(), Some(&entry));
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Arrived(0)));
        assert_eq!(g.foe, Some(Foe::Garrison(0)));
        assert_eq!(g.daily_income(), 0);
        let mut b = g.start_battle();
        assert_eq!(b.attacker, Team::Player);
        b.begin();
        wipe_enemies(&mut b);
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { captured: Some(0), .. }));
        let fort = &g.world.locations[0];
        assert!(fort.owned() && !fort.defended() && fort.garrison.is_empty());
        assert_eq!((g.daily_income(), g.daily_mana()), (40, 5));
        let (gold, mana) = (g.gold, g.mana);
        let mut events = Vec::new();
        g.pass_time(24.0 * 60.0, &mut events);
        assert!(matches!(events.as_slice(), [Event::NewDay(DayReport { income: 40, mana: 5, .. })]), "{events:?}");
        assert_eq!(g.mana, mana + 5);
        assert!(g.gold >= gold + 40 - g.daily_wages());
    }

    #[test]
    fn passing_through_a_hostile_gate_stops_the_hero() {
        let mut s = strip();
        let mut fort = building(BuildingType::Fort, 10, 2, (1, 1));
        fort.relations = [-2, 0, 0, 0];
        fort.garrison[0] = troop(4, 0, 1);
        s.buildings = vec![fort];
        // Water above and below the gate: the only way east is through it.
        for x in [10u32] {
            for y in [0u32, 1, 3, 4, 5] {
                tk::set(&mut s, x, y, crate::dt::dtm::Surface::DeepSea);
            }
        }
        let mut g = start(&s);
        assert!(g.set_destination((20, 2)));
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Arrived(0)));
        assert_eq!((g.tile(), g.foe), ((10, 2), Some(Foe::Garrison(0))));
    }

    #[test]
    fn scenario_villages_pay_gold_and_mana_tribute() {
        let mut s = strip();
        let mut v = building(BuildingType::Village, 6, 2, (1, 1));
        v.gold_per_day = 25;
        v.gold_max = 60;
        v.mana_per_day = 4;
        v.mana_max = 10;
        v.relations = [1, 0, 0, 0];
        s.buildings = vec![v];
        let mut g = start(&s);
        g.set_destination((6, 2));
        assert_eq!(walk_until_stopped(&mut g).last(), Some(&Event::Arrived(0)));
        assert_eq!(g.tribute_available(), Some(25));
        let gold = g.gold;
        assert_eq!(g.collect_tribute(), Some(Tribute::Gold(25)));
        assert_eq!((g.gold, g.mana), (gold + 25, 4));
        assert_eq!(g.tribute_available(), None);
        g.wait(24);
        assert_eq!(g.tribute_available(), Some(25), "refilled at midnight");
    }
}

