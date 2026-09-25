use std::collections::BTreeSet;
use std::sync::Arc;

use crate::dt::dtm::Scenario;

use super::ai::{self, AiNews, AiStats, Beaten};
use super::battle::{Battle, Outcome, Team};
use super::clock::{Clock, Tick, MINUTES_PER_DAY};
use super::content::{Bonus, Content, HeroClass, ItemId, Source, SpellDef, UnitId};
use super::events::{ArmyId, EventEngine, EventOutcome};
use super::fog::{self, Fog};
use super::formation::Slot;
use super::items::{self, EquipError};
use super::magic::{self, ActiveSpell};
use super::save::ScenarioRef;
use super::ships::{Ship, SEA_MINUTES};
use super::map::{Tile, TileMap};
use super::rng::Rng;
use super::units::{PromoteError, Stats, Unit};
use super::world::{LocationKind, Owner, Stationed, Troop, World};

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
/// Markets re-roll their random goods every 7 days *(guess: the original's restock rule is
/// not known)*.
pub const RESTOCK_EVERY_DAYS: u64 = 7;
/// Unworn items the hero's backpack holds. The original's size is not known; its inventory
/// grid is 5 wide and scrolls, and the footage shows more than 25 items *(guess: 40)*.
pub const PACK_SIZE: usize = 40;
/// Spells the book holds: the original's spell book window has 3 × 5 cells.
pub const SPELL_BOOK_SIZE: usize = 15;
/// Items on sale in each demo market after a restock.
pub const MARKET_STOCK: usize = 6;
/// Percent chance that a beaten demo gang drops an item.
const GANG_LOOT_CHANCE: i32 = 30;
/// Percent chance that a demo village pays tribute with an item instead of gold.
const TRIBUTE_ITEM_CHANCE: i32 = 25;
/// A village's long blessing lasts this many times its spell's own time *(guess)*.
pub const BLESSING_FACTOR: u64 = 3;
/// A village's furs fetch this percentage of its gold tribute *(guess)*.
pub const FURS_PERCENT: i32 = 150;
/// The magic ritual gives one mana for this much of the gold tribute *(guess)*.
pub const RITUAL_GOLD_PER_MANA: i32 = 2;
/// The Ranger hero moves 20% faster on the map (mechanics.md 7).
const RANGER_SPEED: f32 = 1.2;

#[derive(Debug, PartialEq, Eq)]
pub enum HireError {
    NotOffered,
    /// Not enough gold (or mana, for units paid in mana).
    NotEnoughGold,
    SquadFull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Currency {
    Gold,
    Mana,
}

/// A price in gold or, for `Nature=Elemental` units (Community Update), in mana.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Price {
    pub amount: i32,
    pub currency: Currency,
}

impl Price {
    pub fn gold(amount: i32) -> Price {
        Price { amount, currency: Currency::Gold }
    }

    /// `amount` in the currency unit type `unit` is paid in.
    pub fn for_unit(content: &Content, unit: UnitId, amount: i32) -> Price {
        let currency = if content.paid_in_mana(unit) { Currency::Mana } else { Currency::Gold };
        Price { amount, currency }
    }
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
        /// Mana from surrendered enemies ("they pray for you").
        mana: i32,
        lost: usize,
        loot: Vec<ItemId>,
        left_behind: usize,
        level_ups: Vec<(usize, i32)>,
        captured: Option<usize>,
    },
    /// Nobody won: no XP (the original pays it only for a victory).
    Withdrew { lost: usize },
    Defeat,
}

/// The noon report (video notes: the daily report comes at 12:00): money and mana after
/// the day's income and wages.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DayReport {
    /// Absolute day index (see [`Clock::day_index`]).
    pub day: u64,
    /// Gold and mana from the player's buildings.
    pub income: i32,
    pub mana: i32,
    /// Wages paid in gold, and in mana (elementals).
    pub wages: i32,
    pub mana_wages: i32,
    /// Units that could not be paid: they sit out battles until paid.
    pub unpaid: usize,
    /// Units that left after going unpaid for `MaxTimeNotUpkeep`.
    pub deserted: Vec<UnitId>,
    /// Balance after the report.
    pub gold: i32,
    pub mana_total: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Event {
    /// The party stopped on a location (index into `world.locations`).
    Arrived(usize),
    /// A hostile army caught the party (index into `world.armies`).
    Encounter(usize),
    /// A friendly army met the party on the road; no battle.
    Met(usize),
    /// The party walked into a hostile castle or fort that had no garrison: it is his.
    Captured(usize),
    NewDay(DayReport),
    /// Something the scenario's event engine did: a message, a question, a quest, the end.
    /// World effects are already applied.
    Script(EventOutcome),
    /// AI armies fought within the hero's sight, or one took or besieged his building
    /// (`rules::ai`).
    Battle(AiNews),
    /// A squad member reached a new level outside battle (scenario XP): (squad index, level).
    LevelUp(usize, i32),
}

/// Who the next battle is against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Foe {
    /// The garrison of a location (a castle, a fort, ruins, a demo camp).
    Garrison(usize),
    Army(usize),
}

/// The whole game state. It is saved with serde (`rules::save`), except the content and the
/// statics of the world and the event engine, which a load rebuilds from the scenario.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Game {
    #[serde(skip)]
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
    /// The hero's spell book (1-based spell indices), cast with [`Game::cast`].
    pub spells: Vec<u8>,
    /// Explored cells (`rules::fog`): off in the demo, on for scenarios.
    pub fog: Fog,
    /// Where the player clicked: the walk is planned again towards it as the fog lifts.
    pub goal: Option<Tile>,
    pub(crate) start_day: u64,
    pub(crate) rng: Rng,
    pub(crate) battles: u64,
    /// The scenario's event engine (`None` in the demo). Taken out while it runs.
    pub(crate) script: Option<Box<EventEngine>>,
    /// Events produced outside [`Game::tick`] and [`Game::wait`] (at the start, after a
    /// battle, an answer, a rumour); the UI drains them with [`Game::drain_events`].
    pub(crate) pending: Vec<Event>,
    /// Events the engine's effects produced during a run (delays, battles).
    pub(crate) effect_events: Vec<Event>,
    /// Areas revealed by events (lanterns, shown armies): (x, y, radius) in cells, for the
    /// fog of war to take.
    pub pending_reveals: Vec<(i32, i32, i32)>,
    /// Scenario armies (ids) the player has met / beaten.
    pub(crate) met_armies: BTreeSet<ArmyId>,
    pub(crate) beaten_armies: BTreeSet<ArmyId>,
    /// Scenario armies beaten by AI armies (`rules::ai`).
    #[serde(default)]
    pub(crate) ai_beaten: BTreeSet<ArmyId>,
    /// What the AI did so far (counts).
    #[serde(default)]
    pub ai_stats: AiStats,
    /// Reports of AI battles the player heard of, newest last.
    #[serde(default)]
    pub ai_log: Vec<AiNews>,
    /// 1 knight, 2 archmage, 3 ranger: the class the game started with (events check it).
    pub(crate) archetype: u8,
    /// Lasting world spells on the hero's army (`rules::magic`).
    pub(crate) effects: Vec<ActiveSpell>,
    /// The scenario the game plays (`rules::save`): the demo, or a map file of the install
    /// with a hash of its bytes. The UI sets it for maps ([`Game::set_origin`]).
    pub origin: Option<ScenarioRef>,
    /// An autosave is due (the noon report came): its name. The UI writes it and clears it.
    #[serde(skip)]
    pub autosave_due: Option<String>,
    /// The rented ship (`rules::ships`), if any.
    #[serde(default)]
    pub ship: Option<Ship>,
    /// The name the player gave the hero (`#HERONAME`); `None`: his class's name.
    #[serde(default)]
    pub hero_name: Option<String>,
}

/// Moves `pos` along `path` for up to `minutes` of game time; `cost` gives the minutes per
/// cell. Stops early on a cell `stop` accepts (the rest of the path is dropped).
/// Returns the minutes used.
fn walk(map: &TileMap, pos: &mut (f32, f32), path: &mut Vec<Tile>, minutes: f32, cost: &dyn Fn(Tile) -> f32, stop: &dyn Fn(Tile) -> bool) -> f32 {
    let mut left = minutes;
    while left > 0.0 {
        let Some(&next) = path.first() else { break };
        let per_tile = cost(next);
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
            fog: Fog::disabled(0, 0),
            goal: None,
            start_day: clock.day_index(),
            rng: Rng::new(seed ^ 0x9e37_79b9),
            battles: 0,
            script: None,
            pending: Vec::new(),
            effect_events: Vec::new(),
            pending_reveals: Vec::new(),
            met_armies: BTreeSet::new(),
            beaten_armies: BTreeSet::new(),
            ai_beaten: BTreeSet::new(),
            ai_stats: AiStats::default(),
            ai_log: Vec::new(),
            archetype: 1,
            effects: Vec::new(),
            origin: None,
            autosave_due: None,
            ship: None,
            hero_name: None,
        };
        // The scenario garrisons of the player's own buildings are his troops there, already
        // past their paid first day.
        let since = (clock.total_minutes() as u64).saturating_sub(MINUTES_PER_DAY);
        let c = g.content.clone();
        for l in g.world.locations.iter_mut().filter(|l| l.owned() && !l.garrison.is_empty()) {
            let troops = std::mem::take(&mut l.garrison);
            l.stationed.extend(troops.iter().map(|t| Stationed { unit: troop_unit(&c, t), since }));
        }
        g.restock_markets();
        g.fog = Fog::disabled(g.world.map.w, g.world.map.h);
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
        g.spells = g.content.start_spells(hero);
        g.archetype = archetype_of(hero);
        g.origin = Some(ScenarioRef::Demo);
        g
    }

    /// Records which map file the game plays, for saves.
    pub fn set_origin(&mut self, origin: ScenarioRef) {
        self.origin = Some(origin);
    }

    /// A new game on an original scenario, with the hero preset of `hero`.
    pub fn from_scenario(content: Arc<Content>, scenario: &Scenario, hero: HeroClass, seed: u64) -> Self {
        let world = World::from_scenario(scenario, &content);
        let start = world.hero_start(scenario, &content, hero);
        let mut leader = Unit::new(&content, hero.unit(), start.hero_slot);
        leader.heal_full(&content);
        let mut squad = vec![leader];
        squad.extend(start.troops.iter().map(|t| troop_unit(&content, t)));
        let mut g = Game::with_world(content, world, squad, start.tile, seed);
        g.fog = fog::for_scenario(&g.world.map, Some(scenario), true);
        g.look_around();
        g.gold = start.gold;
        g.mana = start.mana;
        g.pack = start.items;
        g.spells = start.spells;
        g.archetype = archetype_of(hero);
        g.script = Some(Box::new(EventEngine::new(scenario)));
        // The scenario's opening events.
        let opening = g.run_script();
        g.pending.extend(opening);
        g
    }

    pub fn hero(&self) -> &Unit {
        &self.squad[0]
    }

    /// The hero's name for `#HERONAME`: the one the player chose, else his class's name.
    pub fn hero_name(&self) -> String {
        match &self.hero_name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => self.hero().name(&self.content).to_string(),
        }
    }

    /// Names the hero (an empty name means his class's name).
    pub fn set_hero_name(&mut self, name: &str) {
        let n = name.trim();
        self.hero_name = (!n.is_empty()).then(|| n.to_string());
    }

    pub fn hero_class(&self) -> Option<HeroClass> {
        HeroClass::of_unit(self.hero().def)
    }

    /// Army cap: the formation's size.
    pub fn max_squad(&self) -> usize {
        self.content.formation.capacity()
    }

    pub(crate) fn squad_has(&self, b: &Bonus) -> bool {
        self.squad.iter().any(|u| u.alive() && u.stats(&self.content).has(b))
    }

    /// Daily wage of unit `u`: from its cost and hiring kind (mechanics.md 1.5), in mana
    /// for elementals; `AddPayment` in the army cuts every wage by 30%. Corpses are not paid.
    pub fn unit_wage(&self, u: &Unit) -> Price {
        let w = if u.alive() { self.content.wage_for(u.def, u.wage_kind) } else { 0 };
        let w = if self.squad_has(&Bonus::AddPayment) { w * 70 / 100 } else { w };
        Price::for_unit(&self.content, u.def, w)
    }

    /// Daily wage of squad member `i` (the hero is free), in its currency.
    pub fn wage(&self, i: usize) -> i32 {
        if i == 0 {
            return 0;
        }
        self.unit_wage(&self.squad[i]).amount
    }

    pub fn can_afford(&self, p: Price) -> bool {
        match p.currency {
            Currency::Gold => self.gold >= p.amount,
            Currency::Mana => self.mana >= p.amount,
        }
    }

    /// Pays `p` if the party has enough.
    pub(crate) fn spend(&mut self, p: Price) -> bool {
        if !self.can_afford(p) {
            return false;
        }
        match p.currency {
            Currency::Gold => self.gold -= p.amount,
            Currency::Mana => self.mana -= p.amount,
        }
        true
    }

    /// Price markup of a building by its attitude towards the player: none from 1 up, then
    /// +15% for each step below (+15% at 0, +45% at −2). Fitted to the footage *(guess)*:
    /// with the hero's −30% trader discount, two markets of attitude 1 sell at exactly 70%
    /// of the base price and one of attitude −2 at 70% × 1.45.
    pub fn relation_markup(attitude: i8) -> i32 {
        (1 - attitude as i32).max(0) * 15
    }

    /// Price to buy `item` here: its cost with the building's relation markup; a `Merchant`
    /// in the army takes 30% off.
    pub fn buy_price(&self, item: ItemId) -> i32 {
        let attitude = self.location.map_or(3, |l| self.world.locations[l].attitude);
        let p = self.content.item(item).cost.max(0) * (100 + Self::relation_markup(attitude)) / 100;
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
    pub(crate) fn slowness(&self) -> f32 {
        if self.hero_class() == Some(HeroClass::Ranger) {
            1.0 / RANGER_SPEED
        } else {
            1.0
        }
    }

    /// Minutes the hero needs to walk `path`.
    pub fn travel_minutes(&self, path: &[Tile]) -> f32 {
        self.world.map.path_minutes_by(self.tile(), path, &|t| self.cell_minutes(t)) as f32
    }

    /// Minutes left on the current route.
    pub fn minutes_left(&self) -> f32 {
        self.travel_minutes(&self.path)
    }

    /// Unit types the barracks here offers now (the player's or a friendly town, castle,
    /// fort or church, with stock left).
    pub fn recruits_here(&self) -> Vec<UnitId> {
        match self.location.map(|l| &self.world.locations[l]) {
            Some(l) if !l.hostile() && l.hires() => l.recruits.iter().filter(|r| r.stock != Some(0)).map(|r| r.unit).collect(),
            _ => Vec::new(),
        }
    }

    /// Wages due at the next report: the squad, and units left in a garrison since the last
    /// report (a garrison unit is paid at one noon only, mechanics.md 1.5). Gold and mana.
    fn wages_due(&self) -> (i32, i32) {
        let now = self.clock.total_minutes() as u64;
        let half = MINUTES_PER_DAY / 2;
        let noon_today = now / MINUTES_PER_DAY * MINUTES_PER_DAY + half;
        let last_noon = if noon_today <= now { noon_today } else { noon_today.saturating_sub(MINUTES_PER_DAY) };
        let squad = self.squad.iter().skip(1);
        let stationed = self
            .world
            .locations
            .iter()
            .filter(|l| l.owned())
            .flat_map(|l| l.stationed.iter())
            .filter(|s| s.since > last_noon)
            .map(|s| &s.unit);
        let (mut gold, mut mana) = (0, 0);
        for u in squad.chain(stationed) {
            let p = self.unit_wage(u);
            match p.currency {
                Currency::Gold => gold += p.amount,
                Currency::Mana => mana += p.amount,
            }
        }
        (gold, mana)
    }

    /// Gold wages due at the next report.
    pub fn daily_wages(&self) -> i32 {
        self.wages_due().0
    }

    /// Mana wages due at the next report (elementals).
    pub fn daily_mana_wages(&self) -> i32 {
        self.wages_due().1
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
        let path = self.plan(to);
        if path.is_empty() {
            return false;
        }
        self.path = path;
        self.goal = Some(to);
        self.location = None;
        true
    }

    /// The route a click on `to` walks now: over explored ground only, towards the nearest
    /// explored cell if `to` is in the dark ([`fog::plan`]).
    /// With a ship the route may board it, sail and land ([`Game::step_minutes`]).
    pub fn plan(&self, to: Tile) -> Vec<Tile> {
        self.plan_from(self.tile(), to)
    }

    fn plan_from(&self, from: Tile, to: Tile) -> Vec<Tile> {
        fog::plan_by(&self.world.map, &self.fog, from, to, &|a, b| self.step_minutes(a, b))
    }

    /// Reveals the hero's surroundings. Returns true if new ground came into view.
    pub fn look_around(&mut self) -> bool {
        self.fog.reveal_around(self.world.map.grid, self.pos, fog::SIGHT_RADIUS)
    }

    /// Lights a lantern: reveals radius `r` (cell widths) around cell `(x, y)`.
    pub fn reveal(&mut self, x: i32, y: i32, r: i32) {
        self.fog.reveal(self.world.map.grid, x, y, r);
    }

    /// After a step: plan the walk to the clicked spot again if new ground came into view
    /// or the route ran out short of it; give up when no explored way gets closer.
    fn feel_the_way(&mut self, revealed: bool, stopped: bool) {
        let Some(goal) = self.goal else { return };
        let here = self.tile();
        if stopped || here == goal || self.foe.is_some() {
            self.goal = None;
            return;
        }
        if self.path.last() == Some(&goal) || !(revealed || self.path.is_empty()) {
            return;
        }
        // Finish the step under way (it may be a gate), then follow the new route.
        let path = match self.path.first() {
            Some(&next) => {
                let rest = self.plan_from(next, goal);
                std::iter::once(next).chain(rest).collect()
            }
            None => self.plan(goal),
        };
        if path.is_empty() {
            self.goal = None;
        } else {
            self.path = path;
        }
    }

    pub fn stop(&mut self) {
        self.path.clear();
        self.goal = None;
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
            let before = path.len();
            let cost = |t: Tile| hero_cell_minutes(world, slowness, t);
            let used = walk(&world.map, pos, path, slice, &cost, &at_gate);
            let stopped = path.is_empty() && before > 0 && at_gate(world.map.tile_at(*pos));
            self.update_ship();
            budget -= slice;
            let revealed = self.look_around();
            self.feel_the_way(revealed, stopped);
            self.pass_time(used, &mut events);
            if let Some(e) = self.contact() {
                self.goal = None;
                self.meet(e, &mut events);
                return events;
            }
            if !self.moving() {
                if let Some(l) = self.world.location_at(self.tile()) {
                    let taken = self.arrive(l);
                    events.push(Event::Arrived(l));
                    if taken {
                        events.push(Event::Captured(l));
                    }
                    // Local events of the building.
                    events.extend(self.run_script());
                }
            }
            if events.iter().any(Event::needs_reading) {
                // Stop and read: time stands still while a message is open.
                self.path.clear();
                break;
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
                self.meet(e, &mut events);
                break;
            }
            if events.iter().any(Event::needs_reading) {
                break;
            }
        }
        events
    }

    /// An army on a neighbouring cell: a hostile one attacks, a friendly one greets once.
    pub(crate) fn contact(&mut self) -> Option<Event> {
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

    /// Stepping into a building: a hostile garrison bars the way; a hostile castle or fort
    /// without one is simply taken (owner, income). Healing is paid, in the building's
    /// barracks (see `rules::town`). Returns true when a building was taken.
    fn arrive(&mut self, l: usize) -> bool {
        self.location = Some(l);
        let loc = &mut self.world.locations[l];
        if loc.defended() {
            self.foe = Some(Foe::Garrison(l));
        } else if loc.kind.capturable() && !loc.owned() && loc.hostile() {
            loc.owner = Owner::Player;
            loc.faction = 1;
            loc.attitude = 3;
            return true;
        }
        false
    }

    pub(crate) fn pass_time(&mut self, minutes: f32, events: &mut Vec<Event>) {
        for tick in self.clock.advance(minutes as f64) {
            match tick {
                Tick::Midnight(_) => {
                    let days = self.content.options.max_day_count_for_new_unit;
                    for l in self.world.locations.iter_mut() {
                        l.refill();
                        l.recruits.iter_mut().for_each(|r| r.regrow(days));
                    }
                }
                Tick::Noon(day) => {
                    let report = self.new_day(day);
                    events.push(Event::NewDay(report));
                    // The original autosaves every day at 12:00, named by the date.
                    self.autosave_due = Some(super::save::date_name(&self.clock));
                }
            }
        }
        self.bury_old_corpses();
        self.expire_spells();
        self.move_armies(minutes, events);
        // Time passed: the scenario's events run.
        events.extend(self.run_script());
    }

    /// Corpses past `MaxTimeResurection` can no longer be raised and are buried.
    fn bury_old_corpses(&mut self) {
        let now = self.clock.total_minutes() as u64;
        let window = self.content.options.max_time_resurection.max(0) as u64;
        let mut i = 1;
        while i < self.squad.len() {
            match self.squad[i].died_at {
                Some(t) if !self.squad[i].alive() && now > t + window => {
                    self.squad.remove(i);
                }
                _ => i += 1,
            }
        }
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

    /// The noon report: income and mana from the player's buildings, then wages. A unit
    /// that cannot be paid is marked unpaid and sits out battles; one unpaid for
    /// `MaxTimeNotUpkeep` leaves the army *(guess: the original's consequence is not
    /// decoded)*, its items going to the pack. Units left in a garrison are paid on their
    /// first day there only. Then the daily healing (medic or ranger; garrisons
    /// `GarrisonAutoHeal`%), and every 7 days the markets restock.
    fn new_day(&mut self, day: u64) -> DayReport {
        let income = self.daily_income();
        let mana = self.daily_mana();
        self.gold += income;
        self.mana += mana;
        let now = self.clock.total_minutes() as u64;
        let (mut wages, mut mana_wages, mut unpaid) = (0, 0, 0);
        let mut pay = |g: &mut Game, p: Price| -> bool {
            if p.amount <= 0 {
                return true;
            }
            let ok = g.spend(p);
            if ok {
                match p.currency {
                    Currency::Gold => wages += p.amount,
                    Currency::Mana => mana_wages += p.amount,
                }
            }
            ok
        };
        for i in 1..self.squad.len() {
            if !self.squad[i].alive() {
                continue;
            }
            let p = self.unit_wage(&self.squad[i]);
            let paid = pay(self, p);
            let u = &mut self.squad[i];
            u.unpaid = !paid;
            u.unpaid_days = if paid { 0 } else { u.unpaid_days + 1 };
            unpaid += usize::from(!paid);
        }
        for l in 0..self.world.locations.len() {
            if !self.world.locations[l].owned() {
                continue;
            }
            for k in 0..self.world.locations[l].stationed.len() {
                let s = &self.world.locations[l].stationed[k];
                if now.saturating_sub(s.since) < MINUTES_PER_DAY {
                    let p = self.unit_wage(&s.unit);
                    let paid = pay(self, p);
                    self.world.locations[l].stationed[k].unit.unpaid = !paid;
                }
            }
        }
        let limit = self.content.options.max_time_not_upkeep.max(1) as i64;
        let mut deserted = Vec::new();
        let mut i = 1;
        while i < self.squad.len() {
            if self.squad[i].unpaid_days as i64 * MINUTES_PER_DAY as i64 >= limit {
                let u = self.squad.remove(i);
                self.take_items(u.items.iter().flatten().copied().collect());
                deserted.push(u.def);
            } else {
                i += 1;
            }
        }
        let c = self.content.clone();
        let heal = self.daily_heal_percent();
        if heal > 0 {
            for u in self.squad.iter_mut().filter(|u| u.alive()) {
                let max = u.max_hp(&c);
                u.hp = (u.hp + max * heal / 100).min(max);
            }
        }
        let garrison_heal = c.options.garrison_auto_heal;
        for s in self.world.locations.iter_mut().flat_map(|l| l.stationed.iter_mut()) {
            let max = s.unit.max_hp(&c);
            s.unit.hp = (s.unit.hp + max * garrison_heal / 100).min(max);
        }
        let n = day.saturating_sub(self.start_day) + 1; // the game's first noon is day 1
        if (n - 1).is_multiple_of(RESTOCK_EVERY_DAYS) && n > 1 {
            self.restock_markets();
        }
        if self.world.demo && n.is_multiple_of(SPAWN_EVERY_DAYS) {
            let camps: Vec<_> = self.world.camps().filter(|(_, l)| !l.cleared).map(|(i, l)| (i, l.tile)).collect();
            for (camp, tile) in camps {
                if self.world.armies.iter().filter(|p| p.home == Some(camp)).count() < MAX_GANGS_PER_CAMP {
                    self.world.spawn_gang(camp, tile);
                }
            }
        }
        self.ai_new_day();
        DayReport { day, income, mana, wages, mana_wages, unpaid, deserted, gold: self.gold, mana_total: self.mana }
    }

    /// Armies move for `minutes`: the AI plans the routes of the armies it steers
    /// (`rules::ai`); ships and the demo's gangs chase a nearby hostile hero or patrol. Then
    /// AI armies act on the goals they reached and fight each other.
    fn move_armies(&mut self, minutes: f32, events: &mut Vec<Event>) {
        self.ai_plan();
        let now = self.clock.total_minutes();
        let hero_tile = self.tile();
        let entries: Vec<Tile> = self.world.locations.iter().map(|l| l.tile).collect();
        let mut armies = std::mem::take(&mut self.world.armies);
        let world = &self.world;
        let map = &world.map;
        for a in armies.iter_mut() {
            let here = a.tile(map);
            let sails = a.sails();
            if ai::managed(a) {
                let slowness = a.slowness;
                let cost = |t: Tile| map.minutes(t).unwrap_or(60) as f32 * slowness;
                walk(map, &mut a.pos, &mut a.path, minutes, &cost, &|_| false);
                continue;
            }
            // Ships stay on the water: they chase the hero to the water next to him.
            let goal = if sails { Game::sea_chase_goal(world, here, hero_tile) } else { Some(hero_tile) };
            let near = a.hostile() && now >= a.ignore_until && map.distance(here, hero_tile) <= CHASE_RADIUS && goal.is_some();
            let route = |from: Tile, to: Tile| {
                if sails {
                    map.path_by(from, to, AI_PATH_NODES, &|x, y| world.sea_step(x, y))
                } else {
                    map.path_limited(from, to, AI_PATH_NODES)
                }
            };
            if let (true, Some(goal)) = (near, goal) {
                if !a.chasing || a.path.last() != Some(&goal) {
                    a.path = route(here, goal);
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
                    let fits = if sails { world.is_sea(t) } else { map.passable(t) };
                    if fits && !entries.contains(&t) && map.distance(t, a.post) <= r {
                        a.path = route(here, t);
                        if !a.path.is_empty() {
                            break;
                        }
                    }
                }
                // Rest between patrol legs, or after failing to find one *(guess)*.
                a.rest_until = now + self.rng.range(30, 180) as f64;
            }
            let slowness = a.slowness;
            let cost = |t: Tile| if sails { SEA_MINUTES as f32 * slowness } else { map.minutes(t).unwrap_or(60) as f32 * slowness };
            walk(map, &mut a.pos, &mut a.path, minutes, &cost, &|_| false);
        }
        self.world.armies = armies;
        self.ai_after_walk(events);
    }

    /// Price to hire unit type `kind`: its `Cost` (in mana for elementals).
    pub fn hire_price(&self, kind: UnitId) -> Price {
        Price::for_unit(&self.content, kind, self.content.unit(kind).cost.max(0))
    }

    /// Hire a unit type offered here, at its `Cost`, into the first free cell. The
    /// barracks stock goes down by one.
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
        if !self.spend(self.hire_price(kind)) {
            return Err(HireError::NotEnoughGold);
        }
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
    /// cannot wear go to the pack. The hero cannot be promoted.
    pub fn promote(&mut self, unit: usize, to: UnitId) -> Result<(), PromoteError> {
        let c = self.content.clone();
        if unit == 0 {
            return Err(PromoteError::NotAvailable);
        }
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

    /// The village innkeeper pays off the unpaid units instead of the tribute being
    /// collected (mechanics.md 5.3). Returns how many were paid off, `None` if the village
    /// has nothing to give today.
    pub fn innkeeper_pay(&mut self) -> Option<usize> {
        self.use_village()?;
        let mut n = 0;
        for u in self.squad.iter_mut().filter(|u| u.unpaid) {
            u.unpaid = false;
            u.unpaid_days = 0;
            n += 1;
        }
        Some(n)
    }

    /// The spell a village's long blessing casts: the cheapest (in mana, then by id) lasting
    /// spell on the hero's own army whose modifiers are all gains *(guess: the original's
    /// blessing is not in the data files)*.
    pub fn village_blessing(&self) -> Option<&SpellDef> {
        self.content
            .spells
            .iter()
            .filter(|s| !magic::targets_enemy(s) && magic::is_lasting(s) && magic::Duration::of(s) != magic::Duration::Permanent)
            .filter(|s| s.add.values().chain(s.percent.values()).all(|&v| v >= 0) && s.life_lose_percent.is_none())
            .min_by_key(|s| (s.cost_mana, s.id))
    }

    /// Instead of the tribute: the village's long blessing ([`Game::village_blessing`]) is
    /// cast on the army for free, lasting [`BLESSING_FACTOR`] times the spell's own time
    /// *(guess)*. Returns the spell, `None` if the village has nothing to give today or the
    /// game has no such spell.
    pub fn village_bless(&mut self) -> Option<u32> {
        self.village_ready()?;
        let spell = self.village_blessing()?.clone();
        self.use_village()?;
        self.apply_spell_to_army(&spell);
        let now = self.clock.total_minutes() as u64;
        if let magic::Duration::Minutes(m) = magic::Duration::of(&spell) {
            for e in self.effects.iter_mut().filter(|e| e.spell == spell.id) {
                e.until = Some(now + m * BLESSING_FACTOR);
            }
        }
        Some(spell.id)
    }

    /// Gold the village's furs fetch now: [`FURS_PERCENT`]% of the waiting gold tribute.
    pub fn furs_value(&self) -> Option<i32> {
        self.village_ready().map(|l| self.world.locations[l].tribute_gold * FURS_PERCENT / 100)
    }

    /// Instead of the tribute: the village's furs, worth more gold than the tribute but no
    /// mana *(guess)*. Returns the gold.
    pub fn sell_furs(&mut self) -> Option<i32> {
        let gold = self.furs_value()?;
        self.use_village()?;
        self.gold += gold;
        Some(gold)
    }

    /// Mana the village's magic ritual gives now: the waiting mana plus one per
    /// [`RITUAL_GOLD_PER_MANA`] gold of the tribute.
    pub fn ritual_value(&self) -> Option<i32> {
        let v = &self.world.locations[self.village_ready()?];
        Some(v.tribute_mana + v.tribute_gold / RITUAL_GOLD_PER_MANA)
    }

    /// Instead of the tribute: a magic power ritual turns the whole tribute into mana
    /// *(guess)*. Returns the mana.
    pub fn magic_ritual(&mut self) -> Option<i32> {
        let mana = self.ritual_value()?;
        self.use_village()?;
        self.mana += mana;
        Some(mana)
    }

    /// Battle against the pending foe. Unpaid units refuse to fight. Walking into a garrison
    /// makes the player the attacker (the building's extra defence helps the garrison); an
    /// army that catches the player attacks.
    pub fn start_battle(&mut self) -> Battle {
        // An army fights with its items worn (`ai::army_units`).
        // The beaten army's experience correction scales the player's XP; a garrison's is 100.
        let correction = match self.foe {
            Some(Foe::Army(i)) => match self.world.armies[i].ai.exp_correction {
                0 => 100,
                c => c,
            },
            _ => 100,
        };
        let (enemies, attacker, defence) = match self.foe {
            Some(Foe::Garrison(l)) => {
                let loc = &self.world.locations[l];
                (loc.garrison.iter().map(|t| troop_unit(&self.content, t)).collect(), Team::Player, loc.garrison_defence)
            }
            Some(Foe::Army(i)) => (ai::army_units(&self.content, &self.world.armies[i]), Team::Enemy, 0),
            None => (Vec::new(), Team::Player, 0),
        };
        let enemies: Vec<Unit> = enemies;
        let player: Vec<_> = self.squad.iter().enumerate().filter(|(i, u)| *i == 0 || (u.alive() && !u.unpaid)).collect();
        self.battles += 1;
        let mut b = Battle::new(self.content.clone(), &player, &enemies, attacker);
        b.set_xp_correction(correction);
        // Lasting world spells change the stats of both sides.
        b.apply_spells(Team::Player, &self.army_spells());
        if let Some(Foe::Army(i)) = self.foe {
            b.apply_spells(Team::Enemy, &self.spells_on_army(i));
        }
        if defence > 0 {
            b.set_building_defence(Team::Enemy, defence);
        }
        b
    }

    /// Gold the victor takes from a beaten army carrying `gold`: `gold / VictoryGoldDiv`, at
    /// least `MinVictoryGold`, or everything when it has less (mechanics.md 2.6).
    pub fn victory_gold(&self, gold: i32) -> i32 {
        let o = &self.content.options;
        let gold = gold.max(0);
        if gold <= o.min_victory_gold {
            gold
        } else {
            (gold / o.victory_gold_div.max(1)).max(o.min_victory_gold)
        }
    }

    /// Mana from the enemies' surrender: when every remaining enemy has `Surrender > 0`
    /// the side gives up, and those units' values pray for the victor (48bfb4); units killed
    /// before give none. In the footage a fort garrison of one `Surrender=20` unit gave 20.
    fn surrender_mana(&self, battle: &Battle) -> i32 {
        battle.surrender_mana(Team::Player)
    }

    /// Writes the battle back into the squad: HP, deployed cells, XP and levels. The dead
    /// (except the hero, who survives while his army does) stay in the army as corpses until
    /// resurrected or buried; the dead hold no items, so theirs go to the pack. Potion effects
    /// end. A won garrison fight captures a castle or fort (owner = player, its income counts
    /// at once, and one day of it is paid as the prize, as in the footage) and gives ruins'
    /// treasure; a beaten army leaves the map and pays [`Game::victory_gold`] and its items.
    /// Surrendered enemies give mana.
    /// Then the scenario's events run (an army beaten); what they do waits in
    /// [`Game::drain_events`].
    pub fn resolve_battle(&mut self, battle: &Battle) -> BattleResult {
        let result = self.settle_battle(battle);
        let after = self.run_script();
        self.pending.extend(after);
        result
    }

    fn settle_battle(&mut self, battle: &Battle) -> BattleResult {
        for r in battle.player_results() {
            let u = &mut self.squad[r.squad_index];
            u.hp = r.hp;
            u.slot = r.slot;
        }
        let mut level_ups = Vec::new();
        let c = self.content.clone();
        for a in battle.player_xp() {
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
        let now = self.clock.total_minutes() as u64;
        let mut dropped = Vec::new();
        let mut lost = 0;
        for u in self.squad.iter_mut().skip(1) {
            if u.hp <= 0 && u.died_at.is_none() {
                u.hp = 0;
                u.died_at = Some(now);
                u.unpaid = false;
                u.unpaid_days = 0;
                dropped.extend(u.items.iter_mut().filter_map(Option::take));
                lost += 1;
            }
        }
        let (_, mut dropped_left) = self.take_items(dropped);
        let foe = self.foe.take();
        let mana = if battle.outcome() == Outcome::Victory { self.surrender_mana(battle) } else { 0 };
        self.mana += mana;

        match (battle.outcome(), foe) {
            (Outcome::Victory, Some(Foe::Garrison(l))) => {
                let loc = &mut self.world.locations[l];
                loc.cleared = true;
                loc.garrison.clear();
                let mut reward = std::mem::take(&mut loc.treasure_gold);
                let treasure = std::mem::take(&mut loc.treasure);
                let rolls = std::mem::take(&mut loc.loot_rolls);
                let captured = loc.kind.capturable().then(|| {
                    loc.owner = Owner::Player;
                    loc.faction = 1;
                    loc.attitude = 3;
                    reward += loc.gold_income;
                    l
                });
                self.gold += reward;
                let mut found = treasure;
                found.extend((0..rolls).filter_map(|_| self.roll_item(Source::Loot)));
                let (loot, left_behind) = self.take_items(found);
                dropped_left += left_behind;
                BattleResult::Victory { reward, mana, lost, loot, left_behind: dropped_left, level_ups, captured }
            }
            (Outcome::Victory, Some(Foe::Army(i))) => {
                let reward = self.victory_gold(self.world.armies[i].gold);
                let army = &mut self.world.armies[i];
                army.gold -= reward;
                let (id, mut found) = (army.id, std::mem::take(&mut army.items));
                // Off the map; a lord retreats, others may respawn (`rules::ai`).
                self.army_beaten(i, Beaten::ByPlayer);
                self.gold += reward;
                if id == 0 && self.rng.range(1, 100) <= GANG_LOOT_CHANCE {
                    found.extend(self.roll_item(Source::Loot));
                }
                let (loot, left_behind) = self.take_items(found);
                dropped_left += left_behind;
                BattleResult::Victory { reward, mana, lost, loot, left_behind: dropped_left, level_ups, captured: None }
            }
            (Outcome::Victory, None) => {
                BattleResult::Victory { reward: 0, mana, lost, loot: Vec::new(), left_behind: dropped_left, level_ups, captured: None }
            }
            (Outcome::Defeat, _) => BattleResult::Defeat,
            (_, foe) => {
                if let Some(Foe::Army(i)) = foe {
                    self.world.armies[i].ignore_until = self.clock.total_minutes() + 120.0;
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

    /// Every shop: its fixed goods (those not sold yet) plus `random` different market
    /// items whose base price lies in the shop's range (mechanics.md 4). Fixed goods, once
    /// bought, are gone for good; the random part is drawn anew at each restock *(guess)*.
    pub(crate) fn restock_markets(&mut self) {
        let market = self.content.items_from(Source::Market);
        for l in 0..self.world.locations.len() {
            let Some(shop) = &self.world.locations[l].shop else { continue };
            let (random, (lo, hi)) = (shop.random, shop.price);
            let mut stock = shop.fixed.clone();
            let mut pool: Vec<ItemId> = market
                .iter()
                .copied()
                .filter(|&i| hi <= 0 || (lo..=hi).contains(&self.content.item(i).cost))
                .filter(|i| !stock.contains(i))
                .collect();
            for _ in 0..random {
                if pool.is_empty() {
                    break;
                }
                let k = self.rng.range(0, pool.len() as i32 - 1) as usize;
                stock.push(pool.swap_remove(k));
            }
            if let Some(shop) = &mut self.world.locations[l].shop {
                shop.stock = stock;
            }
        }
    }

    /// Items for sale where the party stands, if there is a market. Ill-disposed markets
    /// trade too, at a markup ([`Game::relation_markup`]; the footage shows a market of
    /// attitude −2 trading).
    pub fn market_here(&self) -> Option<&[ItemId]> {
        let loc = &self.world.locations[self.location?];
        loc.shop.as_ref().map(|s| s.stock.as_slice())
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
            if let Some(k) = shop.fixed.iter().position(|&i| i == item) {
                shop.fixed.remove(k);
            }
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


/// Minutes per cell the hero spends on `t`: [`SEA_MINUTES`] at sea, else the terrain times
/// `slowness`.
pub(crate) fn hero_cell_minutes(world: &World, slowness: f32, t: Tile) -> f32 {
    if world.is_sea(t) {
        SEA_MINUTES as f32
    } else {
        world.map.minutes(t).unwrap_or(60) as f32 * slowness
    }
}

/// The event engine's archetype code of a hero class.
fn archetype_of(hero: HeroClass) -> u8 {
    match hero {
        HeroClass::Knight => 1,
        HeroClass::Archmage => 2,
        HeroClass::Ranger => 3,
    }
}

/// A fresh unit for an army or garrison troop, at its level and full health.
pub(crate) fn troop_unit(content: &Content, t: &Troop) -> Unit {
    let mut u = Unit::new(content, t.unit, t.slot);
    u.level = t.level.max(1);
    u.xp = t.xp;
    u.heal_full(content);
    u.hp = (u.hp - t.hurt).max(1);
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
        let report = |day, wages, unpaid, gold| DayReport {
            day,
            income: 20,
            mana: 0,
            wages,
            mana_wages: 0,
            unpaid,
            deserted: vec![],
            gold,
            mana_total: 0,
        };
        assert_eq!(events, vec![Event::NewDay(report(day, 11, 0, 9))]);
        assert_eq!(g.gold, 9);

        g.gold = -20; // broke: 0 after income
        events.clear();
        g.pass_time(24.0 * 60.0, &mut events);
        assert_eq!(events, vec![Event::NewDay(report(day + 1, 0, 2, 0))]);
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
    fn castle_healing_is_paid() {
        let mut g = quiet_game(HeroClass::Knight);
        g.squad[0].hp = 5;
        g.set_destination(tile_of_location(&g, "Millbrook"));
        walk_until_stopped(&mut g);
        g.set_destination(tile_of_location(&g, "Oakford"));
        walk_until_stopped(&mut g);
        assert_eq!(g.hero().hp, 5, "no free healing on arrival");
        let price = g.heal_price(0).unwrap();
        let gold = g.gold;
        g.heal(0).unwrap();
        assert_eq!((g.hero().hp, g.gold), (70, gold - price.amount));
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
    fn reaching_the_turn_limit_against_a_gang_is_a_victory() {
        // The original has no draw: after the first action of turn `BattleEndTurn` the
        // player wins if any of his units stand (battle.md §5).
        let mut g = quiet_game(HeroClass::Knight);
        let camp = g.world.index_of("Bandit camp");
        g.world.spawn_gang(camp, (30, 20));
        g.foe = Some(Foe::Army(0));
        let mut b = g.start_battle();
        b.begin();
        while b.outcome() == Outcome::Ongoing {
            b.skip();
        }
        assert_eq!(b.round, 25);
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { reward: GANG_REWARD, .. }));
        assert!(g.world.armies.is_empty(), "the gang counts as beaten");
    }

    #[test]
    fn the_beaten_armys_correction_scales_the_players_xp() {
        let gain = |correction: i32| {
            let mut g = quiet_game(HeroClass::Knight);
            let camp = g.world.index_of("Bandit camp");
            g.world.spawn_gang(camp, (30, 20));
            g.world.armies[0].ai.exp_correction = correction;
            g.foe = Some(Foe::Army(0));
            let mut b = g.start_battle();
            b.begin();
            wipe_all_but_hero(&mut b);
            let share = b.xp_awards(Team::Player)[0].xp;
            let xp = b.player_xp()[0].xp;
            g.resolve_battle(&b);
            assert!(g.hero().xp > 0 || g.hero().level > 1);
            (share, xp)
        };
        let (share, normal) = gain(100);
        let (_, double) = gain(200);
        // The demo's options: modifier 100, difficulty 100.
        assert_eq!(normal, share);
        assert_eq!(double, 2 * share);
    }

    #[test]
    fn the_hero_is_not_promoted_and_promotion_is_free() {
        let mut g = quiet_game(HeroClass::Knight);
        let (spear, sword) = (unit(&g, "spearman"), unit(&g, "swordsman"));
        g.hire(spear).unwrap();
        g.squad[1].level = 3;
        g.squad[1].xp = 40;
        g.squad[0].level = 5;
        assert_eq!(g.promote(0, sword), Err(PromoteError::NotAvailable), "the hero rises by levels only");
        let gold = g.gold;
        g.promote(1, sword).unwrap();
        assert_eq!((g.squad[1].def, g.squad[1].level, g.squad[1].xp, g.gold), (sword, 1, 0, gold));
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
    fn dead_recruits_stay_as_corpses_and_drop_their_gear() {
        let mut g = quiet_game(HeroClass::Knight);
        g.hire(unit(&g, "spearman")).unwrap();
        let mail = item(&g, "chainmail");
        g.squad[1].items[0] = Some(mail);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        let mut b = g.start_battle();
        b.begin();
        wipe_all_but_hero(&mut b);
        g.resolve_battle(&b);
        assert_eq!(g.squad.len(), 2);
        assert!(!g.squad[1].alive() && g.squad[1].items[0].is_none());
        assert!(g.pack.contains(&mail), "the dead hold no items");
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
        assert_eq!((g.hero().level, g.hero().xp), (1, 0), "no starting XP in the preset");
        let mut s = strip();
        s.header.heroes[0].mana = 150;
        assert_eq!(start(&s).mana, 150, "the preset's second value is mana");
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
        // 120 to start with, plus its daily income at the noon the walk may have passed.
        let carried = g.world.armies[0].gold;
        assert!(carried >= 120);
        let r = g.resolve_battle(&b);
        assert!(matches!(&r, BattleResult::Victory { reward, captured: None, loot, .. } if *reward == carried / 2 && loot == &vec![ItemId(9)]), "{r:?}");
        assert_eq!(g.gold, gold + carried / 2, "half its gold");
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
        // The fort is still in the dark: the walk heads for its entry as the fog lifts.
        assert_eq!(g.goal, Some(entry));
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
    fn the_fog_lifts_around_the_walking_hero() {
        let mut g = start(&strip());
        g.world.armies.clear();
        assert!(g.fog.enabled && g.fog.explored((2, 2)) && g.fog.explored((9, 2)) && !g.fog.explored((10, 2)));
        assert!(!Game::new(content(), HeroClass::Knight, 1).fog.enabled, "the demo has no fog");
        assert!(g.set_destination((22, 3)), "a click into the dark walks towards it");
        assert!(g.path.iter().all(|&t| g.fog.explored(t)), "over explored ground only");
        for _ in 0..10_000 {
            if !g.moving() {
                break;
            }
            if let Some(&next) = g.path.first() {
                assert!(g.fog.explored(next), "never steps into the dark");
            }
            g.tick(0.05);
        }
        assert_eq!(g.tile(), (22, 3), "feels its way there");
        assert!(g.fog.explored((20, 0)) && g.goal.is_none());
        g.set_destination((2, 2));
        g.stop();
        assert!(g.goal.is_none() && !g.moving());
        let mut g = start(&strip());
        assert!(!g.fog.explored((21, 5)));
        g.reveal(20, 5, 2);
        assert!(g.fog.explored((21, 5)) && g.fog.explored((22, 5)) && !g.fog.explored((23, 5)));
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

    fn at_village(gold: i32, mana: i32) -> Game {
        let mut s = strip();
        let mut v = building(BuildingType::Village, 6, 2, (1, 1));
        v.gold_per_day = gold as u16;
        v.gold_max = gold as u16;
        v.mana_per_day = mana as u8;
        v.mana_max = mana as u8;
        v.relations = [1, 0, 0, 0];
        s.buildings = vec![v];
        let mut g = start(&s);
        g.set_destination((6, 2));
        walk_until_stopped(&mut g);
        assert_eq!(g.location, Some(0));
        g
    }

    #[test]
    fn village_furs_and_ritual_replace_the_tribute() {
        let mut g = at_village(40, 6);
        let (gold, mana) = (g.gold, g.mana);
        assert_eq!((g.furs_value(), g.ritual_value()), (Some(60), Some(26)));
        assert_eq!(g.sell_furs(), Some(60));
        assert_eq!((g.gold, g.mana), (gold + 60, mana), "more gold than the tribute, no mana");
        assert_eq!((g.sell_furs(), g.magic_ritual(), g.collect_tribute()), (None, None, None), "once a day");
        g.wait(24);
        assert_eq!(g.magic_ritual(), Some(26));
        assert_eq!((g.gold, g.mana), (gold + 60 - g.daily_wages(), mana + 26));
        assert_eq!(g.village_bless(), None, "already used today");
    }

    #[test]
    fn village_blessing_is_a_long_lasting_spell() {
        let mut g = at_village(40, 6);
        // The testkit has no spells: no blessing, and the tribute is kept.
        assert_eq!(g.village_bless(), None);
        assert_eq!(g.tribute_available(), Some(40));
        // The demo's cheapest blessing is spell 3 (8 hours): blessed for three times as long.
        let mut d = quiet_game(HeroClass::Knight);
        d.location = Some(d.world.index_of("Millbrook"));
        let spell = d.village_blessing().map(|s| (s.id, s.time_work));
        assert_eq!(spell, Some((3, Some(8))));
        let now = d.clock.total_minutes() as u64;
        assert_eq!(d.village_bless(), Some(3));
        assert_eq!(d.active_spells().iter().map(|e| (e.spell, e.until)).collect::<Vec<_>>(), [(3, Some(now + 3 * 8 * 60))]);
        assert_eq!(d.tribute_available(), None);
        d.wait(12);
        assert_eq!(d.active_spells().len(), 1, "still blessed after its own 8 hours");
        d.wait(12);
        assert!(d.active_spells().is_empty());
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

