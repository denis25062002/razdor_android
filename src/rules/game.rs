use std::sync::Arc;

use super::battle::{Battle, Outcome, Team};
use super::clock::Clock;
use super::content::{Bonus, Content, HeroClass, ItemId, Source, UnitId};
use super::formation::Slot;
use super::items::{self, EquipError};
use super::map::{center, tile_at, Tile, TileMap};
use super::rng::Rng;
use super::units::{PromoteError, Stats, Unit};
use super::world::{LocationKind, World, GANG_REWARD};

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
    /// `level_ups`: (squad index, new level).
    Victory { reward: i32, lost: usize, loot: Vec<ItemId>, left_behind: usize, level_ups: Vec<(usize, i32)> },
    Withdrew { lost: usize, level_ups: Vec<(usize, i32)> },
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
    pub content: Arc<Content>,
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
    /// A new demo game. `content` must hold the demo units (see [`World::standard`]).
    pub fn new(content: Arc<Content>, hero: HeroClass, seed: u64) -> Self {
        let world = World::standard(&content);
        let home = world.locations[0].tile;
        let id = hero.unit();
        let slot = content.formation.free_slot(&[], Stats::of_level(&content, id, 1).preferred_row()).expect("empty formation");
        let mut g = Game {
            squad: vec![Unit::new(&content, id, slot)],
            gold: content.start_gold(hero),
            content,
            clock: Clock::start(),
            pos: center(home),
            path: Vec::new(),
            world,
            location: Some(0),
            foe: None,
            pack: Vec::new(),
            rng: Rng::new(seed ^ 0x9e37_79b9),
            battles: 0,
        };
        g.restock_markets();
        g
    }

    pub fn hero(&self) -> &Unit {
        &self.squad[0]
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
        tile_at(self.pos)
    }

    pub fn moving(&self) -> bool {
        !self.path.is_empty()
    }

    fn today(&self) -> u32 {
        self.clock.day()
    }

    pub fn recruits_here(&self) -> &[UnitId] {
        match self.location.map(|l| &self.world.locations[l].kind) {
            Some(LocationKind::Castle { recruits, .. }) => recruits,
            _ => &[],
        }
    }

    /// Total wages due at the next midnight.
    pub fn daily_wages(&self) -> i32 {
        (0..self.squad.len()).map(|i| self.wage(i)).sum()
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
            LocationKind::Castle { .. } | LocationKind::Church => self.heal_all(),
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

    fn heal_all(&mut self) {
        let c = self.content.clone();
        self.squad.iter_mut().for_each(|u| u.heal_full(&c));
    }

    /// Percent of max HP the army heals each day: 15 with an `ArmyMedic`, 20 with a Ranger
    /// hero (mechanics.md 1.2, 1.3). They do not add up *(guess)*.
    pub fn daily_heal_percent(&self) -> i32 {
        let medic = if self.squad_has(&Bonus::ArmyMedic) { 15 } else { 0 };
        let ranger = if HeroClass::of_unit(self.hero().def) == Some(HeroClass::Ranger) { 20 } else { 0 };
        medic.max(ranger)
    }

    fn new_day(&mut self, day: u32) -> DayReport {
        let income = self.daily_income();
        self.gold += income;
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
        if (day - 1).is_multiple_of(7) {
            self.restock_markets(); // Monday
        }
        if day.is_multiple_of(SPAWN_EVERY_DAYS) {
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
        let World { map, locations, parties, .. } = &mut self.world;
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
            self.heal_all();
        }
        used
    }

    /// Battle against the pending foe. Unpaid units refuse to fight. Walking into a camp
    /// makes the player the attacker; a gang that catches the player attacks.
    pub fn start_battle(&mut self) -> Battle {
        let (enemies, attacker) = match self.foe {
            Some(Foe::Camp(l)) => match &self.world.locations[l].kind {
                LocationKind::Camp { enemies, .. } => (enemies.clone(), Team::Player),
                _ => (Vec::new(), Team::Player),
            },
            Some(Foe::Party(i)) => (self.world.parties[i].enemies.clone(), Team::Enemy),
            None => (Vec::new(), Team::Player),
        };
        let enemies: Vec<Unit> = enemies.iter().map(|&(id, slot)| Unit::new(&self.content, id, slot)).collect();
        let player: Vec<_> = self.squad.iter().enumerate().filter(|(i, u)| *i == 0 || !u.unpaid).collect();
        self.battles += 1;
        Battle::new(self.content.clone(), &player, &enemies, attacker)
    }

    /// Writes the battle back into the squad: HP, deployed cells, XP and levels. The dead
    /// (except the hero, who survives while his army does) leave the squad with their items;
    /// potion effects end.
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
            (Outcome::Victory, Some(Foe::Camp(l))) => {
                let loc = &mut self.world.locations[l];
                loc.cleared = true;
                let (reward, drops) = match loc.kind {
                    LocationKind::Camp { reward, loot, .. } => (reward, loot),
                    _ => (0, 0),
                };
                self.gold += reward;
                let (loot, left_behind) = self.take_loot(drops);
                BattleResult::Victory { reward, lost, loot, left_behind, level_ups }
            }
            (Outcome::Victory, Some(Foe::Party(i))) => {
                self.world.parties.remove(i);
                self.gold += GANG_REWARD;
                let drops = u32::from(self.rng.range(1, 100) <= GANG_LOOT_CHANCE);
                let (loot, left_behind) = self.take_loot(drops);
                BattleResult::Victory { reward: GANG_REWARD, lost, loot, left_behind, level_ups }
            }
            (Outcome::Victory, None) => BattleResult::Victory { reward: 0, lost, loot: Vec::new(), left_behind: 0, level_ups },
            (Outcome::Defeat, _) => BattleResult::Defeat,
            (_, foe) => {
                if let Some(Foe::Party(i)) = foe {
                    self.world.parties[i].ignore_until = self.clock.total_minutes() + 120.0;
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
        let price = self.buy_price(item);
        if self.gold < price {
            return Err(TradeError::NotEnoughGold);
        }
        if self.pack.len() >= PACK_SIZE {
            return Err(TradeError::PackFull);
        }
        if let Some(LocationKind::Castle { market, .. }) = self.location.map(|l| &mut self.world.locations[l].kind) {
            market.remove(stock_index);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::world::demo_unit;

    fn content() -> Arc<Content> {
        Arc::new(Content::builtin())
    }

    fn new_game(hero: HeroClass, seed: u64) -> Game {
        Game::new(content(), hero, seed)
    }

    /// A game with no gangs on the map, for tests about travel and time.
    fn quiet_game(hero: HeroClass) -> Game {
        let mut g = new_game(hero, 1);
        g.world.parties.clear();
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
        assert_eq!(g.clock.label(), "Day 1, Monday 08:00");
        assert_eq!((g.hero().def, g.gold), (HeroClass::Knight.unit(), 100));
        assert_eq!(g.max_squad(), 12);
    }

    #[test]
    fn time_is_frozen_while_standing_still() {
        let mut g = new_game(HeroClass::Knight, 1);
        let parties: Vec<_> = g.world.parties.iter().map(|p| p.pos).collect();
        assert!(g.tick(5.0).is_empty());
        assert_eq!(g.clock, Clock::start());
        assert_eq!(parties, g.world.parties.iter().map(|p| p.pos).collect::<Vec<_>>());
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
        assert!(g.clock.total_minutes() > Clock::start().total_minutes() + 60.0);
    }

    #[test]
    fn cannot_walk_into_the_sea() {
        let mut g = quiet_game(HeroClass::Knight);
        assert!(!g.set_destination((30, 40)), "open sea");
    }

    #[test]
    fn midnight_pays_income_and_wages_and_marks_unpaid() {
        let mut g = quiet_game(HeroClass::Knight);
        g.hire(unit(&g, "spearman")).unwrap();
        g.hire(unit(&g, "archer")).unwrap();
        g.gold = 0;
        g.squad[1].hp = 1;
        let mut events = Vec::new();
        g.pass_time(16.0 * 60.0, &mut events); // 08:00 -> 00:00
        // Income 20; wages from cost: spearman 40/2×¼ = 5, archer 45/2×¼ = 5.6 → 6.
        assert_eq!(events, vec![Event::NewDay(DayReport { day: 2, income: 20, wages: 11, unpaid: 0 })]);
        assert_eq!(g.gold, 9);

        g.gold = -20; // broke: 0 after income
        events.clear();
        g.pass_time(24.0 * 60.0, &mut events);
        assert_eq!(events, vec![Event::NewDay(DayReport { day: 3, income: 20, wages: 0, unpaid: 2 })]);
        assert!(g.squad[1].unpaid && g.squad[2].unpaid);
    }

    #[test]
    fn ranger_heals_the_army_every_day() {
        let mut g = quiet_game(HeroClass::Ranger);
        g.squad[0].hp = 10;
        let mut events = Vec::new();
        g.pass_time(16.0 * 60.0, &mut events);
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
        g.foe = Some(Foe::Camp(g.world.index_of("Bandit camp")));
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
        g.world.spawn_party(camp, ahead);
        let events = walk_until_stopped(&mut g);
        assert!(matches!(events.last(), Some(Event::Encounter(0))), "{events:?}");
        assert_eq!(g.foe, Some(Foe::Party(0)));
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
        g.world.spawn_party(camp, start);
        let before = distance(g.world.parties[0].pos, g.pos);
        for _ in 0..3 {
            g.tick(0.05);
        }
        assert!(g.world.parties[0].chasing);
        assert!(distance(g.world.parties[0].pos, g.pos) < before + 0.5, "it keeps up");
    }

    #[test]
    fn beating_a_gang_removes_it_pays_and_gives_xp() {
        let mut g = quiet_game(HeroClass::Knight);
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_party(camp, (30, 20));
        g.foe = Some(Foe::Party(0));
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        let gold = g.gold;
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { reward: GANG_REWARD, lost: 0, .. }));
        assert_eq!(g.gold, gold + GANG_REWARD);
        assert!(g.world.parties.is_empty());
        assert_eq!(g.foe, None);
        assert!(g.hero().xp > 0 || g.hero().level > 1, "XP after the battle");
    }

    #[test]
    fn stalemate_with_a_gang_buys_time_to_escape() {
        let mut g = quiet_game(HeroClass::Knight);
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_party(camp, (30, 20));
        g.foe = Some(Foe::Party(0));
        let mut b = g.start_battle();
        b.begin();
        while b.outcome() == Outcome::Ongoing {
            b.skip();
        }
        assert!(matches!(g.resolve_battle(&b), BattleResult::Withdrew { lost: 0, .. }));
        assert!(g.world.parties[0].ignore_until > g.clock.total_minutes());
    }

    #[test]
    fn camps_send_out_new_gangs_every_few_days() {
        let mut g = quiet_game(HeroClass::Knight);
        let mut events = Vec::new();
        g.pass_time((16 + 24 * 2) as f32 * 60.0, &mut events); // to day 4 00:00: day 3 midnight passed
        assert_eq!(g.world.parties.len(), 2, "one gang from each camp");
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
        g.foe = Some(Foe::Camp(camp));
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        let result = g.resolve_battle(&b);
        assert!(matches!(&result, BattleResult::Victory { reward: 100, lost: 1, loot, left_behind: 0, .. } if loot.len() == 1));
        assert_eq!(g.pack.len(), 1);
        assert!(g.world.locations[camp].cleared);
        assert!(!g.won());
    }

    #[test]
    fn a_fallen_hero_survives_if_his_army_does() {
        let mut g = quiet_game(HeroClass::Knight);
        g.hire(unit(&g, "spearman")).unwrap();
        g.foe = Some(Foe::Camp(g.world.index_of("Bandit camp")));
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
        g.foe = Some(Foe::Camp(g.world.index_of("Bandit camp")));
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
                g.world.parties.clear();
                g.hire(unit(&g, "spearman")).unwrap();
                g.foe = Some(Foe::Camp(g.world.index_of(if seed % 2 == 0 { "Bandit camp" } else { "Bandit lair" })));
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
        g.foe = Some(Foe::Camp(g.world.index_of("Bandit lair")));
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
    fn markets_stock_market_items_and_restock_on_monday() {
        let mut g = quiet_game(HeroClass::Knight);
        at_oakford(&mut g);
        let stock = g.market_here().unwrap().to_vec();
        assert_eq!(stock.len(), MARKET_STOCK);
        assert!(stock.iter().all(|&i| g.content.sources(i).contains(&Source::Market)));
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
        g.foe = Some(Foe::Camp(g.world.index_of("Bandit camp")));
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
        g.foe = Some(Foe::Camp(g.world.index_of("Bandit camp")));
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
        g.foe = Some(Foe::Camp(lair));
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
}
