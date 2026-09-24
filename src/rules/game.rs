use super::battle::{Battle, Outcome};
use super::clock::Clock;
use super::formation::{free_slot, Slot};
use super::items::{catalog, free_slot_for, EquipError, ItemId, Source};
use super::map::{center, tile_at, Tile, TileMap};
use super::rng::Rng;
use super::units::{Unit, UnitKind};
use super::world::{LocationKind, World, GANG_REWARD};

/// Whole squad including the hero: one 2×6 formation.
pub const MAX_SQUAD: usize = 12;
/// Game minutes that pass per real second while the party walks (1 h ≈ 0.2 s).
pub const MINUTES_PER_SECOND: f32 = 300.0;
/// Gangs chase the player inside this many tiles.
pub const CHASE_RADIUS: f32 = 5.0;
/// Touching distance that starts a battle.
const CONTACT: f32 = 0.75;
/// Gangs are slower than the player: their terrain costs are multiplied by this.
const GANG_SLOWNESS: f32 = 1.25;
/// Largest slice of game time simulated at once, so chases stay smooth.
const STEP_MINUTES: f32 = 5.0;
const WANDER_RADIUS: i32 = 8;
const SPAWN_EVERY_DAYS: u32 = 3;
const MAX_GANGS_PER_CAMP: usize = 2;
/// Unworn items the party can carry.
pub const PACK_SIZE: usize = 16;
/// Items on sale in each market after a restock.
pub const MARKET_STOCK: usize = 6;
/// Percent chance that a beaten gang drops an item.
const GANG_LOOT_CHANCE: i32 = 30;
/// Percent chance that a village pays tribute with an item instead of gold.
const TRIBUTE_ITEM_CHANCE: i32 = 25;

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
    Victory { reward: i32, lost: usize, loot: Vec<ItemId>, left_behind: usize },
    Withdrew { lost: usize },
    Defeat,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayReport {
    pub day: u32,
    pub income: i32,
    pub wages: i32,
    pub unpaid: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The party stopped on a location.
    Arrived(usize),
    /// A gang caught the party (index into `world.parties`).
    Encounter(usize),
    NewDay(DayReport),
}

/// Who the next battle is against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Foe {
    Camp(usize),
    Party(usize),
}

pub struct Game {
    /// Squad member 0 is always the hero.
    pub squad: Vec<Unit>,
    pub gold: i32,
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
    rng: Rng,
    battles: u64,
    seed: u64,
}

/// Moves `pos` along `path` for up to `minutes` of game time. Returns the minutes used.
fn walk(map: &TileMap, pos: &mut (f32, f32), path: &mut Vec<Tile>, minutes: f32, slowness: f32) -> f32 {
    let mut left = minutes;
    while left > 0.0 {
        let Some(&next) = path.first() else { break };
        let per_tile = map.terrain(next).minutes().unwrap_or(60.0) * slowness;
        let goal = center(next);
        let (dx, dy) = (goal.0 - pos.0, goal.1 - pos.1);
        let need = (dx * dx + dy * dy).sqrt() * per_tile;
        if need <= left {
            *pos = goal;
            path.remove(0);
            left -= need;
        } else {
            let k = left / need;
            pos.0 += dx * k;
            pos.1 += dy * k;
            left = 0.0;
        }
    }
    minutes - left
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

impl Game {
    pub fn new(hero: UnitKind, seed: u64) -> Self {
        let world = World::standard();
        let home = world.locations[0].tile;
        let mut g = Game {
            squad: vec![Unit::new(hero, free_slot(&[], hero.stats().attack.preferred_row()).unwrap())],
            gold: hero.starting_gold(),
            clock: Clock::start(),
            pos: center(home),
            path: Vec::new(),
            world,
            location: Some(0),
            foe: None,
            pack: Vec::new(),
            rng: Rng::new(seed ^ 0x9e37_79b9),
            battles: 0,
            seed,
        };
        g.restock_markets();
        g
    }

    pub fn hero(&self) -> &Unit {
        &self.squad[0]
    }

    pub fn tile(&self) -> Tile {
        tile_at(self.pos)
    }

    pub fn moving(&self) -> bool {
        !self.path.is_empty()
    }

    fn today(&self) -> u32 {
        self.clock.day()
    }

    pub fn recruits_here(&self) -> &[UnitKind] {
        match self.location.map(|l| &self.world.locations[l].kind) {
            Some(LocationKind::Castle { recruits, .. }) => recruits,
            _ => &[],
        }
    }

    /// Total wages due at the next midnight.
    pub fn daily_wages(&self) -> i32 {
        self.squad.iter().map(|u| u.kind.wage()).sum()
    }

    pub fn daily_income(&self) -> i32 {
        self.world
            .locations
            .iter()
            .map(|l| match l.kind {
                LocationKind::Castle { income, owned: true, .. } => income,
                _ => 0,
            })
            .sum()
    }

    /// Walk to `to` along the cheapest path. Returns false if it can't be reached.
    pub fn set_destination(&mut self, to: Tile) -> bool {
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
        while budget > 0.0 && self.moving() {
            let slice = budget.min(STEP_MINUTES);
            let used = walk(&self.world.map, &mut self.pos, &mut self.path, slice, 1.0);
            budget -= slice;
            self.pass_time(used, &mut events);
            if let Some(i) = self.touching_party() {
                self.path.clear();
                self.foe = Some(Foe::Party(i));
                events.push(Event::Encounter(i));
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

    fn touching_party(&self) -> Option<usize> {
        let now = self.clock.total_minutes();
        self.world.parties.iter().position(|p| now >= p.ignore_until && distance(p.pos, self.pos) < CONTACT)
    }

    fn arrive(&mut self, l: usize) {
        self.location = Some(l);
        let loc = &self.world.locations[l];
        match loc.kind {
            LocationKind::Castle { .. } | LocationKind::Church => self.squad.iter_mut().for_each(Unit::heal_full),
            LocationKind::Camp { .. } if !loc.cleared => self.foe = Some(Foe::Camp(l)),
            _ => {}
        }
    }

    fn pass_time(&mut self, minutes: f32, events: &mut Vec<Event>) {
        let midnights = self.clock.advance(minutes as f64);
        for k in 0..midnights {
            let day = self.today() + 1 - midnights + k;
            events.push(Event::NewDay(self.new_day(day)));
        }
        self.move_parties(minutes);
    }

    fn new_day(&mut self, day: u32) -> DayReport {
        let income = self.daily_income();
        self.gold += income;
        let (mut wages, mut unpaid) = (0, 0);
        for u in self.squad.iter_mut().skip(1) {
            let w = u.kind.wage();
            if self.gold >= w {
                self.gold -= w;
                wages += w;
                u.unpaid = false;
            } else {
                u.unpaid = true;
                unpaid += 1;
            }
        }
        if (day - 1) % 7 == 0 {
            self.restock_markets(); // Monday
        }
        if day % SPAWN_EVERY_DAYS == 0 {
            let camps: Vec<_> = self.world.camps().filter(|(_, l)| !l.cleared).map(|(i, l)| (i, l.tile)).collect();
            for (camp, tile) in camps {
                if self.world.parties.iter().filter(|p| p.home == camp).count() < MAX_GANGS_PER_CAMP {
                    self.world.spawn_party(camp, tile);
                }
            }
        }
        DayReport { day, income, wages, unpaid }
    }

    fn move_parties(&mut self, minutes: f32) {
        let now = self.clock.total_minutes();
        let (hero_pos, hero_tile) = (self.pos, self.tile());
        let World { map, locations, parties } = &mut self.world;
        for p in parties.iter_mut() {
            let near = now >= p.ignore_until && distance(p.pos, hero_pos) <= CHASE_RADIUS;
            if near {
                if !p.chasing || p.path.last() != Some(&hero_tile) {
                    p.path = map.path(tile_at(p.pos), hero_tile);
                    p.chasing = true;
                }
            } else if p.chasing {
                p.chasing = false;
                p.path.clear();
            }
            if p.path.is_empty() && !p.chasing {
                let home = locations[p.home].tile;
                for _ in 0..10 {
                    let t = (
                        home.0 + self.rng.range(-WANDER_RADIUS, WANDER_RADIUS),
                        home.1 + self.rng.range(-WANDER_RADIUS, WANDER_RADIUS),
                    );
                    if map.passable(t) && locations.iter().all(|l| l.tile != t) {
                        p.path = map.path(tile_at(p.pos), t);
                        if !p.path.is_empty() {
                            break;
                        }
                    }
                }
            }
            walk(map, &mut p.pos, &mut p.path, minutes, GANG_SLOWNESS);
        }
    }

    pub fn hire(&mut self, kind: UnitKind) -> Result<(), HireError> {
        if !self.recruits_here().contains(&kind) {
            return Err(HireError::NotOffered);
        }
        let taken: Vec<Slot> = self.squad.iter().map(|u| u.slot).collect();
        let slot = match free_slot(&taken, kind.stats().attack.preferred_row()) {
            Some(slot) if self.squad.len() < MAX_SQUAD => slot,
            _ => return Err(HireError::SquadFull),
        };
        if self.gold < kind.cost() {
            return Err(HireError::NotEnoughGold);
        }
        self.gold -= kind.cost();
        self.squad.push(Unit::new(kind, slot));
        Ok(())
    }

    /// Village here whose once-a-day service is still available today.
    fn village_ready(&self) -> Option<usize> {
        let l = self.location?;
        match self.world.locations[l].kind {
            LocationKind::Village { used_on_day, .. } if used_on_day != Some(self.today()) => Some(l),
            _ => None,
        }
    }

    /// Tribute the village here would pay today, if it hasn't been used yet.
    pub fn tribute_available(&self) -> Option<i32> {
        match self.world.locations[self.village_ready()?].kind {
            LocationKind::Village { tribute, .. } => Some(tribute),
            _ => None,
        }
    }

    fn use_village(&mut self) -> Option<i32> {
        let l = self.village_ready()?;
        let today = self.today();
        match &mut self.world.locations[l].kind {
            LocationKind::Village { tribute, used_on_day } => {
                *used_on_day = Some(today);
                Some(*tribute)
            }
            _ => None,
        }
    }

    /// Collects today's tribute: usually gold, sometimes an item (gold if the pack is full).
    pub fn collect_tribute(&mut self) -> Option<Tribute> {
        let gold = self.use_village()?;
        if self.pack.len() < PACK_SIZE && self.rng.range(1, 100) <= TRIBUTE_ITEM_CHANCE {
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
            self.squad.iter_mut().for_each(Unit::heal_full);
        }
        used
    }

    /// Battle against the pending foe. Unpaid units refuse to fight.
    pub fn start_battle(&mut self) -> Battle {
        let enemies = match self.foe {
            Some(Foe::Camp(l)) => match &self.world.locations[l].kind {
                LocationKind::Camp { enemies, .. } => enemies.clone(),
                _ => Vec::new(),
            },
            Some(Foe::Party(i)) => self.world.parties[i].enemies.clone(),
            None => Vec::new(),
        };
        let player: Vec<_> = self.squad.iter().enumerate().filter(|(_, u)| !u.unpaid).collect();
        self.battles += 1;
        Battle::new(&player, &enemies, self.seed.wrapping_add(self.battles * 7919))
    }

    pub fn resolve_battle(&mut self, battle: &Battle) -> BattleResult {
        for (i, hp, slot, items) in battle.player_results() {
            let u = &mut self.squad[i];
            u.hp = hp;
            u.slot = slot;
            u.items = items;
        }
        let before = self.squad.len();
        let hero = self.squad.remove(0);
        self.squad.retain(|u| u.hp > 0);
        self.squad.insert(0, hero);
        let lost = before - self.squad.len();
        let foe = self.foe.take();

        match (battle.outcome(), foe) {
            (Outcome::Victory, Some(Foe::Camp(l))) => {
                let loc = &mut self.world.locations[l];
                loc.cleared = true;
                let (reward, drops) = match loc.kind {
                    LocationKind::Camp { reward, loot, .. } => (reward, loot),
                    _ => (0, 0),
                };
                self.gold += reward;
                let (loot, left_behind) = self.take_loot(drops);
                BattleResult::Victory { reward, lost, loot, left_behind }
            }
            (Outcome::Victory, Some(Foe::Party(i))) => {
                self.world.parties.remove(i);
                self.gold += GANG_REWARD;
                let drops = u32::from(self.rng.range(1, 100) <= GANG_LOOT_CHANCE);
                let (loot, left_behind) = self.take_loot(drops);
                BattleResult::Victory { reward: GANG_REWARD, lost, loot, left_behind }
            }
            (Outcome::Victory, None) => BattleResult::Victory { reward: 0, lost, loot: Vec::new(), left_behind: 0 },
            (Outcome::Defeat, _) => BattleResult::Defeat,
            (_, foe) => {
                if let Some(Foe::Party(i)) = foe {
                    self.world.parties[i].ignore_until = self.clock.total_minutes() + 120.0;
                }
                BattleResult::Withdrew { lost }
            }
        }
    }

    pub fn won(&self) -> bool {
        self.world.all_camps_cleared()
    }

    /// A random item of the given source, if the table has any.
    fn roll_item(&mut self, source: Source) -> Option<ItemId> {
        let pool = catalog().from_source(source);
        if pool.is_empty() {
            return None;
        }
        Some(pool[self.rng.range(0, pool.len() as i32 - 1) as usize])
    }

    /// Rolls `drops` loot items into the pack. Returns (kept, left behind).
    fn take_loot(&mut self, drops: u32) -> (Vec<ItemId>, usize) {
        let mut kept = Vec::new();
        let mut left_behind = 0;
        for _ in 0..drops {
            let Some(item) = self.roll_item(Source::Loot) else { continue };
            if self.pack.len() < PACK_SIZE {
                self.pack.push(item);
                kept.push(item);
            } else {
                left_behind += 1;
            }
        }
        (kept, left_behind)
    }

    fn restock_markets(&mut self) {
        for l in 0..self.world.locations.len() {
            if !matches!(self.world.locations[l].kind, LocationKind::Castle { .. }) {
                continue;
            }
            let stock: Vec<ItemId> = (0..MARKET_STOCK).filter_map(|_| self.roll_item(Source::Market)).collect();
            if let LocationKind::Castle { market, .. } = &mut self.world.locations[l].kind {
                *market = stock;
            }
        }
    }

    /// Items for sale where the party stands, if there is a market.
    pub fn market_here(&self) -> Option<&[ItemId]> {
        match &self.world.locations[self.location?].kind {
            LocationKind::Castle { market, .. } => Some(market),
            _ => None,
        }
    }

    pub fn buy(&mut self, stock_index: usize) -> Result<ItemId, TradeError> {
        let item = *self.market_here().ok_or(TradeError::NoMarket)?.get(stock_index).ok_or(TradeError::NoSuchItem)?;
        if self.gold < item.def().price {
            return Err(TradeError::NotEnoughGold);
        }
        if self.pack.len() >= PACK_SIZE {
            return Err(TradeError::PackFull);
        }
        if let Some(LocationKind::Castle { market, .. }) = self.location.map(|l| &mut self.world.locations[l].kind) {
            market.remove(stock_index);
        }
        self.gold -= item.def().price;
        self.pack.push(item);
        Ok(item)
    }

    /// Sells a pack item for half its price. Returns the gold gained.
    pub fn sell(&mut self, pack_index: usize) -> Result<i32, TradeError> {
        self.market_here().ok_or(TradeError::NoMarket)?;
        if pack_index >= self.pack.len() {
            return Err(TradeError::NoSuchItem);
        }
        let price = self.pack.remove(pack_index).def().sell_price();
        self.gold += price;
        Ok(price)
    }

    /// Moves a pack item onto squad member `unit`.
    pub fn equip(&mut self, unit: usize, pack_index: usize) -> Result<(), EquipError> {
        let item = *self.pack.get(pack_index).ok_or(EquipError::NoSuchItem)?;
        let u = self.squad.get_mut(unit).ok_or(EquipError::NoSuchItem)?;
        let slot = free_slot_for(&u.items, item)?;
        u.items[slot] = Some(item);
        self.pack.remove(pack_index);
        Ok(())
    }

    /// Moves item slot `slot` of squad member `unit` back into the pack.
    pub fn unequip(&mut self, unit: usize, slot: usize) -> Result<(), EquipError> {
        if self.pack.len() >= PACK_SIZE {
            return Err(EquipError::PackFull);
        }
        let u = self.squad.get_mut(unit).ok_or(EquipError::NoSuchItem)?;
        let item = u.items.get_mut(slot).and_then(Option::take).ok_or(EquipError::NoSuchItem)?;
        u.hp = u.hp.min(u.stats().max_hp);
        self.pack.push(item);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use UnitKind::*;

    /// A game with no gangs on the map, for tests about travel and time.
    fn quiet_game(hero: UnitKind) -> Game {
        let mut g = Game::new(hero, 1);
        g.world.parties.clear();
        g
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

    #[test]
    fn starts_at_home_castle_in_the_morning() {
        let g = Game::new(Knight, 1);
        assert_eq!(g.location, Some(0));
        assert_eq!(g.clock.label(), "Day 1, Monday 08:00");
    }

    #[test]
    fn time_is_frozen_while_standing_still() {
        let mut g = Game::new(Knight, 1);
        let parties: Vec<_> = g.world.parties.iter().map(|p| p.pos).collect();
        assert!(g.tick(5.0).is_empty());
        assert_eq!(g.clock, Clock::start());
        assert_eq!(parties, g.world.parties.iter().map(|p| p.pos).collect::<Vec<_>>());
    }

    #[test]
    fn walking_to_a_village_takes_time_and_arrives() {
        let mut g = quiet_game(Knight);
        let millbrook = g.world.index_of("Millbrook");
        assert!(g.set_destination(tile_of_location(&g, "Millbrook")));
        assert_eq!(g.location, None);
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Arrived(millbrook)));
        assert_eq!(g.location, Some(millbrook));
        assert!(g.clock.total_minutes() > Clock::start().total_minutes() + 60.0);
    }

    #[test]
    fn cannot_walk_into_the_sea() {
        let mut g = quiet_game(Knight);
        assert!(!g.set_destination((30, 40)), "open sea");
    }

    #[test]
    fn midnight_pays_income_and_wages_and_marks_unpaid() {
        let mut g = quiet_game(Knight);
        g.hire(Spearman).unwrap();
        g.hire(Archer).unwrap();
        g.gold = 0;
        g.squad[1].hp = 1;
        let mut events = Vec::new();
        g.pass_time(16.0 * 60.0, &mut events); // 08:00 -> 00:00
        // Income 20, then spearman 3 and archer 4.
        assert_eq!(events, vec![Event::NewDay(DayReport { day: 2, income: 20, wages: 7, unpaid: 0 })]);
        assert_eq!(g.gold, 13);

        g.gold = -20; // broke: 0 after income
        events.clear();
        g.pass_time(24.0 * 60.0, &mut events);
        assert_eq!(events, vec![Event::NewDay(DayReport { day: 3, income: 20, wages: 0, unpaid: 2 })]);
        assert!(g.squad[1].unpaid && g.squad[2].unpaid);
    }

    #[test]
    fn unpaid_units_sit_out_battles() {
        let mut g = quiet_game(Knight);
        g.hire(Spearman).unwrap();
        g.squad[1].unpaid = true;
        g.foe = Some(Foe::Camp(g.world.index_of("Bandit camp")));
        let b = g.start_battle();
        assert!(b.fighters.iter().all(|f| f.kind != Spearman));
    }

    #[test]
    fn village_serves_once_per_day() {
        let mut g = quiet_game(Knight);
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
        let mut g = quiet_game(Knight);
        g.squad[0].hp = 5;
        g.set_destination(tile_of_location(&g, "Millbrook"));
        walk_until_stopped(&mut g);
        g.set_destination(tile_of_location(&g, "Oakford"));
        walk_until_stopped(&mut g);
        assert_eq!(g.hero().hp, 60);
    }

    #[test]
    fn walking_into_a_gang_starts_an_encounter() {
        let mut g = quiet_game(Knight);
        let target = tile_of_location(&g, "Millbrook");
        g.set_destination(target);
        // Two tiles ahead: inside the chase radius, so it closes in.
        let ahead = g.path[1];
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_party(camp, ahead);
        let events = walk_until_stopped(&mut g);
        assert!(matches!(events.last(), Some(Event::Encounter(0))), "{events:?}");
        assert_eq!(g.foe, Some(Foe::Party(0)));
        assert!(!g.moving());
    }

    #[test]
    fn gangs_chase_a_nearby_party() {
        let mut g = quiet_game(Knight);
        g.set_destination(tile_of_location(&g, "Millbrook"));
        let camp = g.world.index_of("Bandit camp");
        let start = (g.tile().0 + 3, g.tile().1 + 2);
        assert!(g.world.map.passable(start));
        g.world.spawn_party(camp, start);
        let before = distance(g.world.parties[0].pos, g.pos);
        for _ in 0..3 {
            g.tick(0.05);
        }
        assert!(g.world.parties[0].chasing);
        assert!(distance(g.world.parties[0].pos, g.pos) < before + 0.5, "it keeps up");
    }

    #[test]
    fn beating_a_gang_removes_it_and_pays() {
        let mut g = quiet_game(Knight);
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_party(camp, (30, 20));
        g.foe = Some(Foe::Party(0));
        let mut b = g.start_battle();
        for f in b.fighters.iter_mut().filter(|f| f.kind != Knight) {
            f.hp = 0;
        }
        let gold = g.gold;
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { reward: GANG_REWARD, lost: 0, .. }));
        assert_eq!(g.gold, gold + GANG_REWARD);
        assert!(g.world.parties.is_empty());
        assert_eq!(g.foe, None);
    }

    #[test]
    fn stalemate_with_a_gang_buys_time_to_escape() {
        let mut g = quiet_game(Knight);
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_party(camp, (30, 20));
        g.foe = Some(Foe::Party(0));
        let mut b = g.start_battle();
        b.begin();
        while b.outcome() == Outcome::Ongoing {
            b.skip();
        }
        assert_eq!(g.resolve_battle(&b), BattleResult::Withdrew { lost: 0 });
        assert!(g.world.parties[0].ignore_until > g.clock.total_minutes());
    }

    #[test]
    fn camps_send_out_new_gangs_every_few_days() {
        let mut g = quiet_game(Knight);
        let mut events = Vec::new();
        g.pass_time((16 + 24 * 2) as f32 * 60.0, &mut events); // to day 4 00:00: day 3 midnight passed
        assert_eq!(g.world.parties.len(), 2, "one gang from each camp");
    }

    #[test]
    fn hire_checks_offer_gold_and_cap() {
        let mut g = quiet_game(Knight);
        g.hire(Spearman).unwrap();
        assert_eq!(g.gold, 70);
        assert_eq!(g.hire(Swordsman), Err(HireError::NotOffered));
        g.gold = 10_000;
        while g.squad.len() < MAX_SQUAD {
            g.hire(Archer).unwrap();
        }
        assert_eq!(g.hire(Archer), Err(HireError::SquadFull));
        g.location = None;
        assert_eq!(g.hire(Archer), Err(HireError::NotOffered));
    }

    #[test]
    fn camp_victory_clears_and_pays() {
        let mut g = quiet_game(Knight);
        g.hire(Spearman).unwrap();
        let camp = g.world.index_of("Bandit camp");
        g.foe = Some(Foe::Camp(camp));
        let mut b = g.start_battle();
        for f in b.fighters.iter_mut().filter(|f| f.kind != Knight) {
            f.hp = 0;
        }
        let result = g.resolve_battle(&b);
        assert!(matches!(&result, BattleResult::Victory { reward: 100, lost: 1, loot, left_behind: 0 } if loot.len() == 1));
        assert_eq!(g.pack.len(), 1);
        assert!(g.world.locations[camp].cleared);
        assert!(!g.won());
    }

    #[test]
    fn auto_played_camp_battles_always_finish() {
        for seed in 0..30 {
            let mut g = Game::new(Knight, seed);
            g.hire(Spearman).unwrap();
            g.hire(Spearman).unwrap();
            g.foe = Some(Foe::Camp(g.world.index_of("Bandit camp")));
            let mut b = g.start_battle();
            b.begin();
            let mut steps = 0;
            while b.outcome() == Outcome::Ongoing {
                b.ai_step();
                steps += 1;
                assert!(steps < 2000, "seed {seed}: battle never ended");
            }
            g.resolve_battle(&b);
        }
    }

    fn at_oakford(g: &mut Game) {
        g.location = Some(g.world.index_of("Oakford"));
    }

    #[test]
    fn markets_stock_market_items_and_restock_on_monday() {
        let mut g = quiet_game(Knight);
        at_oakford(&mut g);
        let stock = g.market_here().unwrap().to_vec();
        assert_eq!(stock.len(), MARKET_STOCK);
        assert!(stock.iter().all(|i| i.def().sources.contains(&Source::Market)));
        g.gold = 10_000;
        g.buy(0).unwrap();
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK - 1);
        let mut events = Vec::new();
        g.pass_time((16 + 24 * 5) as f32 * 60.0, &mut events); // to Sunday 00:00
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK - 1, "not Monday yet");
        g.pass_time(24.0 * 60.0, &mut events);
        assert_eq!(g.clock.weekday(), "Monday");
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK);
    }

    #[test]
    fn buying_and_selling() {
        let mut g = quiet_game(Knight);
        g.location = None;
        assert_eq!(g.buy(0), Err(TradeError::NoMarket), "on the road");
        at_oakford(&mut g);
        let item = g.market_here().unwrap()[0];
        g.gold = item.def().price - 1;
        assert_eq!(g.buy(0), Err(TradeError::NotEnoughGold));
        g.gold = item.def().price;
        assert_eq!(g.buy(0), Ok(item));
        assert_eq!((g.gold, g.pack.clone()), (0, vec![item]));
        assert_eq!(g.sell(0), Ok(item.def().price / 2));
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
        let mut g = quiet_game(Knight);
        let (sword, axe, shield) = (ItemId::named("short_sword"), ItemId::named("war_axe"), ItemId::named("oak_shield"));
        g.pack = vec![sword, axe, shield];
        g.equip(0, 0).unwrap();
        assert_eq!(g.equip(0, 0), Err(EquipError::SameType), "axe is a second weapon");
        g.equip(0, 1).unwrap();
        assert_eq!(g.pack, vec![axe]);
        assert_eq!(g.hero().stats().max_hp, 65);
        g.squad[0].heal_full();
        let shield_slot = g.hero().items.iter().position(|i| *i == Some(shield)).unwrap();
        g.unequip(0, shield_slot).unwrap();
        assert_eq!(g.hero().hp, 60, "HP capped to the new max");
        assert_eq!(g.pack, vec![axe, shield]);
        assert_eq!(g.unequip(0, shield_slot), Err(EquipError::NoSuchItem));
        g.pack = vec![axe; PACK_SIZE];
        assert_eq!(g.unequip(0, 0), Err(EquipError::PackFull));
    }

    #[test]
    fn gear_goes_into_battle_and_potions_are_spent() {
        let mut g = quiet_game(Knight);
        g.squad[0].items[0] = Some(ItemId::named("might_potion"));
        g.squad[0].items[1] = Some(ItemId::named("chainmail"));
        g.foe = Some(Foe::Camp(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        b.begin();
        assert_eq!(b.fighters[0].stats.armor, 7);
        while b.active() != Some(0) {
            b.skip();
        }
        b.drink(0).unwrap();
        for f in b.fighters.iter_mut().filter(|f| f.team == crate::rules::battle::Team::Enemy) {
            f.hp = 0;
        }
        g.resolve_battle(&b);
        assert_eq!(g.hero().items, [None, Some(ItemId::named("chainmail")), None, None]);
    }

    #[test]
    fn dead_recruits_take_their_gear_with_them() {
        let mut g = quiet_game(Knight);
        g.hire(Spearman).unwrap();
        g.squad[1].items[0] = Some(ItemId::named("chainmail"));
        g.foe = Some(Foe::Camp(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        for f in b.fighters.iter_mut().filter(|f| f.kind != Knight) {
            f.hp = 0;
        }
        g.resolve_battle(&b);
        assert_eq!(g.squad.len(), 1);
        assert!(!g.pack.contains(&ItemId::named("chainmail")));
    }

    #[test]
    fn loot_that_does_not_fit_is_left_behind() {
        let mut g = quiet_game(Knight);
        g.pack = vec![ItemId::named("heal_potion"); PACK_SIZE];
        let lair = g.world.index_of("Bandit lair");
        g.foe = Some(Foe::Camp(lair));
        let mut b = g.start_battle();
        for f in b.fighters.iter_mut().filter(|f| f.kind != Knight) {
            f.hp = 0;
        }
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { left_behind: 2, .. }));
    }

    #[test]
    fn villages_sometimes_pay_in_items() {
        let mut items = 0;
        for seed in 0..200 {
            let mut g = quiet_game(Knight);
            g.rng = Rng::new(seed);
            g.location = Some(g.world.index_of("Millbrook"));
            if let Some(Tribute::Item(item)) = g.collect_tribute() {
                assert!(item.def().sources.contains(&Source::Tribute));
                items += 1;
            }
        }
        assert!((25..=80).contains(&items), "about 25%: {items}/200");
    }
}
