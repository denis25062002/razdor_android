use std::collections::BTreeSet;
use std::sync::Arc;

use crate::dt::dtm::Scenario;

use super::ai::{self, AiNews, AiStats, Beaten};
use super::battle::{Battle, Outcome, Team};
use super::clock::{Clock, Tick};
use super::content::{Bonus, Content, HeroClass, ItemId, Source, Stat, UnitId, WageKind};
use super::economy::VillageOffer;
use super::events::{ArmyId, EventEngine, EventOutcome};
use super::fog::{self, Fog};
use super::formation::Slot;
use super::items::{self, EquipError};
use super::journal::History;
use super::magic::{self, ActiveSpell};
use super::save::ScenarioRef;
use super::ships::Ship;
use super::map::{step_minutes, Tile, TileMap, ROAD};
use super::rng::{EventRng, Rng, WORLD_MUSIC_DRAW};
use super::units::{PromoteError, Stats, Unit};
use super::world::{Army, LocationKind, Owner, Stationed, Troop, World, AI_BUDGET_CAP};

/// Real seconds each hero step and each wait tick plays over: the original's
/// `WalkDelay = 150 + (100 − WalkSpeed) × 2.5` ms at the shipped `WalkSpeed=100` (world.md
/// §2). Game time per real second follows from the step's own minutes.
pub const STEP_SECONDS: f32 = 0.15;
/// Game minutes of a wait tick (world.md §6): waiting 1 h is 2 ticks, 4 h 8 ticks.
pub const WAIT_TICK_MINUTES: f32 = 30.0;
/// The hero's speed by class (world.md §2): knight and archmage 5, ranger 4 (his steps
/// take 80% of the time).
pub const KNIGHT_SPEED: u32 = 5;
pub const RANGER_SPEED: u32 = 4;
/// The demo's gangs chase the player inside this many cells (Razdor's own demo rule).
pub const CHASE_RADIUS: i32 = 6;
/// Armies meet (and hostile ones attack) on neighbouring cells, diagonals included
/// (world.md §4).
const CONTACT: i32 = 1;
/// Cells a demo gang's or a ship's pathfinder may expand per search.
const AI_PATH_NODES: usize = 4000;
/// A friendly army greets the player again only after he has gone this far away.
const MEET_AGAIN_DISTANCE: i32 = 4;
const SPAWN_EVERY_DAYS: u64 = 3;
const MAX_GANGS_PER_CAMP: usize = 2;
/// Unworn items the hero's backpack holds: 256 slots, shown as a scrolling grid 5 wide.
pub const PACK_SIZE: usize = 256;
/// Spells the book holds (the original's message comes at 15).
pub const SPELL_BOOK_SIZE: usize = 15;
/// Items on sale in each demo market after a restock.
pub const MARKET_STOCK: usize = 6;
/// Percent chance that a beaten demo gang drops an item.
const GANG_LOOT_CHANCE: i32 = 30;
/// Percent chance that a demo village pays tribute with an item instead of gold.
pub(crate) const TRIBUTE_ITEM_CHANCE: i32 = 25;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// A place an event showed on the map: its centre and the cells that were dark before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub at: Tile,
    pub cells: Vec<Tile>,
}

impl DayReport {
    /// Nothing came in or went out: no income, no wages, nobody unpaid or gone. The report
    /// window is then not shown.
    pub fn is_empty(&self) -> bool {
        self.income == 0 && self.mana == 0 && self.wages == 0 && self.mana_wages == 0 && self.unpaid == 0 && self.deserted.is_empty()
    }
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
    /// Entering village `at`, the hero took its tribute (economy.md §3: on entering, all the
    /// gold and mana, no button): what it paid, and the mana.
    Tribute { at: usize, paid: Tribute, mana: i32 },
    /// A spell read on the map landed, or was lost (`Game::begin_cast`).
    SpellCast { spell: u32, target: magic::CastTarget, outcome: magic::CastOutcome },
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
    /// The original's one generator (`rules::rng`): not saved; a map load sets it to 1, a
    /// save load starts it from the load sequence (`rules::save`).
    #[serde(skip)]
    pub(crate) rng: Rng,
    /// The Community event generator (opcode 18), seeded from the clock at every load.
    #[serde(skip)]
    pub(crate) event_rng: EventRng,
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
    /// Places an event has just shown (lanterns, shown armies) with the cells it uncovered,
    /// for the map to fly to and fade in; the interface takes them. Not saved.
    #[serde(skip)]
    pub shown: Vec<Shown>,
    /// Scenario armies (ids) the player has met / beaten.
    pub(crate) met_armies: BTreeSet<ArmyId>,
    /// The army the player clicked (its `uid`): reaching it always starts a meeting ("click
    /// it to talk or fight"), even if it greeted him before.
    #[serde(default)]
    pub(crate) talk_to: Option<u32>,
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
    /// What the player learnt, with dates (`rules::journal`); empty in older saves.
    #[serde(default)]
    pub journal: History,
    /// The offer of the village the hero stands in, made on entering (`rules::economy`).
    #[serde(default)]
    pub(crate) offer: Option<(usize, VillageOffer)>,
    /// The village that made the last offer (cleared when its tribute is taken) and what
    /// was offered.
    #[serde(default)]
    pub(crate) offered_at: Option<usize>,
    #[serde(default)]
    pub(crate) last_offer: Option<VillageOffer>,
    /// Real seconds into the step (or wait tick) under way ([`STEP_SECONDS`] each).
    #[serde(skip)]
    pub(crate) step_elapsed: f32,
    /// Wait ticks still to play in real time ([`Game::begin_wait`], or a reading).
    #[serde(skip)]
    pub(crate) wait_ticks: u32,
    /// The spell being read while the wait ticks play ([`Game::begin_cast`]).
    #[serde(skip)]
    pub(crate) reading: Option<magic::Reading>,
    /// Real seconds since the world last moved, for drawing armies between cells.
    #[serde(skip)]
    pub(crate) since_step: f32,
    /// "Improved enemy AI in battle" (the original's `OptValue9`, "expert" in Razdor's
    /// settings): the player's choice, set by the interface, not part of the save.
    #[serde(skip)]
    pub improved_ai: bool,
    /// The AI's simulated battles of the day (`rules::ai`).
    #[serde(skip)]
    pub(crate) sims: ai::Sims,
}

/// Army `a` walks its path while its banked minutes cover the next step: `cost(next) ×
/// speed`, ×1.5 diagonally (world.md §2); `cost` gives the cost units of a cell, `None`
/// where it cannot go (the route is dropped). Remembers where it stood for drawing.
fn step_army(map: &TileMap, a: &mut Army, cost: &dyn Fn(Tile) -> Option<u16>) {
    while let Some(&next) = a.path.first() {
        let Some(c) = cost(next) else {
            a.path.clear();
            break;
        };
        let need = step_minutes(map.grid, a.tile(map), next, c, a.speed.max(1));
        if a.budget < need {
            break;
        }
        a.budget -= need;
        a.pos = map.center(next);
        a.path.remove(0);
        a.walk.points.push(a.pos);
        a.walk.minutes.push(need);
    }
}

impl Game {
    fn with_world(content: Arc<Content>, world: World, squad: Vec<Unit>, tile: Tile) -> Game {
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
            rng: Rng::map_load(),
            event_rng: EventRng::from_clock(),
            battles: 0,
            script: None,
            pending: Vec::new(),
            effect_events: Vec::new(),
            pending_reveals: Vec::new(),
            shown: Vec::new(),
            met_armies: BTreeSet::new(),
            talk_to: None,
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
            journal: History::default(),
            offer: None,
            offered_at: None,
            last_offer: None,
            step_elapsed: 0.0,
            wait_ticks: 0,
            reading: None,
            since_step: 0.0,
            improved_ai: false,
            sims: ai::Sims::default(),
        };
        // The hero draws no wage; everyone counts as paid at the start.
        let now = clock.total_minutes() as u64;
        g.squad[0].wage_kind = WageKind::Leader;
        g.squad.iter_mut().for_each(|u| u.last_paid = now);
        // The scenario garrisons of the player's own buildings are his troops there (never
        // paid).
        let since = now;
        let c = g.content.clone();
        for l in g.world.locations.iter_mut().filter(|l| l.owned() && !l.garrison.is_empty()) {
            let troops = std::mem::take(&mut l.garrison);
            l.stationed.extend(troops.iter().map(|t| Stationed { unit: troop_unit(&c, t), since }));
        }
        // The map load's draws (engine.md §3.2): the state is 1, the markets are stocked,
        // then the world music draws its first change time.
        g.restock_markets();
        g.rng.random(WORLD_MUSIC_DRAW);
        g.fog = Fog::disabled(g.world.map.w, g.world.map.h);
        g
    }

    /// A new demo game. `content` must hold the demo units (see [`World::standard`]).
    pub fn new(content: Arc<Content>, hero: HeroClass) -> Self {
        let world = World::standard(&content);
        let home = world.locations[0].tile;
        let id = hero.unit();
        let slot = content.formation.free_slot(&[], Stats::of_level(&content, id, 1).preferred_row()).expect("empty formation");
        let squad = vec![Unit::new(&content, id, slot)];
        let gold = content.start_gold(hero);
        let mut g = Game::with_world(content, world, squad, home);
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
    pub fn from_scenario(content: Arc<Content>, scenario: &Scenario, hero: HeroClass) -> Self {
        let mut g = Game::unstarted(content, scenario, hero);
        g.start_script();
        g
    }

    /// The scenario's opening events (they wait in [`Game::drain_events`]).
    pub(crate) fn start_script(&mut self) {
        let opening = self.run_script();
        self.pending.extend(opening);
    }

    /// A game on a scenario whose opening events have not run yet.
    pub(crate) fn unstarted(content: Arc<Content>, scenario: &Scenario, hero: HeroClass) -> Self {
        let mut world = World::from_scenario(scenario, &content);
        let start = world.hero_start(scenario, &content, hero);
        for &l in &start.owned {
            world.give_to_player(l);
        }
        let mut leader = Unit::new(&content, hero.unit(), start.hero_slot);
        leader.heal_full(&content);
        let mut squad = vec![leader];
        squad.extend(start.troops.iter().map(|t| troop_unit(&content, t)));
        let mut g = Game::with_world(content, world, squad, start.tile);
        // A preset on the water ("Тихая пристань") puts him there, at sea: aboard a ship
        // *(guess: the original plans on its MIXED map while he is on water)*.
        if g.world.is_sea(start.tile) {
            g.ship = Some(Ship { tile: start.tile, aboard: true });
        }
        g.fog = fog::for_scenario(&g.world.map, Some(scenario), true);
        g.look_around();
        g.gold = start.gold;
        g.mana = start.mana;
        g.pack = start.items;
        g.spells = start.spells;
        g.archetype = archetype_of(hero);
        g.script = Some(Box::new(EventEngine::new(scenario)));
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

    pub fn tile(&self) -> Tile {
        self.world.map.tile_at(self.pos)
    }

    pub fn moving(&self) -> bool {
        !self.path.is_empty()
    }

    /// The hero's speed: minutes per cost unit of an orthogonal step ([`KNIGHT_SPEED`],
    /// [`RANGER_SPEED`]).
    pub fn hero_speed(&self) -> u32 {
        if self.hero_class() == Some(HeroClass::Ranger) {
            RANGER_SPEED
        } else {
            KNIGHT_SPEED
        }
    }

    /// Game minutes of the hero's step from `from` onto its neighbour `to` (world.md §1–2):
    /// the cost of the cell he leaves (water at sea, else the land under him) times his
    /// speed, ×1.5 diagonally.
    pub fn step_time(&self, from: Tile, to: Tile) -> f32 {
        let w = &self.world;
        let left = if w.is_sea(from) { w.map.water_cost(from) } else { w.map.cost(from) };
        step_minutes(w.map.grid, from, to, left.unwrap_or(ROAD), self.hero_speed())
    }

    /// Minutes the hero needs to walk `path`.
    pub fn travel_minutes(&self, path: &[Tile]) -> f32 {
        self.world.map.path_minutes_by(self.tile(), path, &|a, b| self.step_time(a, b)) as f32
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

    /// Squad member `unit` goes to cell `slot` of the formation (the army screen and the
    /// barracks, by dragging); a unit standing there takes its old cell, as at a battle's
    /// deployment. False if `slot` is not a cell of the formation.
    pub fn move_unit(&mut self, unit: usize, slot: Slot) -> bool {
        if unit >= self.squad.len() || !self.content.formation.slots().any(|s| s == slot) {
            return false;
        }
        let from = self.squad[unit].slot;
        if let Some(other) = self.squad.iter().position(|u| u.slot == slot) {
            self.squad[other].slot = from;
        }
        self.squad[unit].slot = slot;
        true
    }

    /// The pursuit of the army clicked (the original's "automatically pursue the chosen
    /// army"): while the hero walks to meet it, the route is planned again to where it stands
    /// now, until they meet (`contact`). An army gone from the map ends the pursuit.
    fn follow_army(&mut self) {
        let Some(uid) = self.talk_to else { return };
        let map = &self.world.map;
        let Some(at) = self.world.armies.iter().find(|a| a.uid == uid).map(|a| a.tile(map)) else {
            self.talk_to = None;
            return;
        };
        if self.goal != Some(at) {
            let path = self.plan(at);
            if !path.is_empty() {
                self.path = path;
                self.goal = Some(at);
            }
        }
    }

    /// The route [`Game::set_destination`] would walk to `to`, without setting off (the map
    /// shows it on a first click): empty if it can't be reached.
    pub fn route_to(&self, to: Tile) -> Vec<Tile> {
        let to = self.world.location_at(to).map_or(to, |l| self.world.locations[l].tile);
        self.plan(to)
    }

    /// Walk to `to` along the cheapest path. A click on a building means the building: the
    /// walk ends on the first of its cells it reaches. Returns false if it can't be reached.
    pub fn set_destination(&mut self, to: Tile) -> bool {
        // A click on an army means meeting it (the help: "click it to talk or fight").
        let map = &self.world.map;
        self.talk_to = self.world.armies.iter().find(|a| a.tile(map) == to).map(|a| a.uid);
        let to = self.world.location_at(to).map_or(to, |l| self.world.locations[l].tile);
        let path = self.plan(to);
        if path.is_empty() {
            return false;
        }
        self.path = path;
        self.goal = Some(to);
        self.location = None;
        self.wait_ticks = 0;
        self.reading = None;
        true
    }

    /// The route a click on `to` walks now (world.md §1, the hero's mask): over explored
    /// ground only, towards the nearest explored cell if `to` is in the dark ([`fog::plan`]);
    /// through towns, villages and bridges but not through castles and forts ill-disposed
    /// towards him, or ruins not his, unless it is the building clicked or the one he stands
    /// in; around every other army, friendly or hostile, except the one clicked (the original's
    /// planner closes only stationary guards, world.md §1, but in play no army can be walked
    /// through); at sea not under bridges. A click on a building ends on any of its cells. With a ship the
    /// route may board it, sail and land ([`Game::step_cost`]).
    pub fn plan(&self, to: Tile) -> Vec<Tile> {
        self.plan_from(self.tile(), to)
    }

    fn plan_from(&self, from: Tile, to: Tile) -> Vec<Tile> {
        let w = &self.world;
        let target = w.location_at(to);
        let standing = w.location_at(from);
        let at_sea = w.is_sea(from);
        let armies: Vec<Tile> = self.army_cells().filter(|&t| t != to).collect();
        let closed = |t: Tile| {
            let barred = w.location_covering(t).is_some_and(|l| {
                let loc = &w.locations[l];
                (at_sea && loc.kind.is_bridge()) || (Some(l) != target && Some(l) != standing && loc.bars_hero())
            });
            barred || armies.contains(&t)
        };
        let step = |a: Tile, b: Tile| if closed(b) { None } else { self.step_cost(a, b, at_sea) };
        match target {
            Some(l) => {
                let loc = &w.locations[l];
                fog::plan_to_any(&w.map, &self.fog, from, &|t| w.location_at(t) == Some(l), loc.tile, &step)
            }
            None => fog::plan_by(&w.map, &self.fog, from, to, &step),
        }
    }

    /// The cells armies stand on: the hero cannot walk through them.
    fn army_cells(&self) -> impl Iterator<Item = Tile> + '_ {
        self.world.armies.iter().map(|a| a.tile(&self.world.map))
    }

    /// How far the hero sees, in cells: 9 for the knight, 8 for the archmage, 10 for the
    /// ranger (world.md §3).
    pub fn sight_radius(&self) -> i32 {
        fog::sight_radius(self.hero_class().unwrap_or(HeroClass::Knight))
    }

    /// Reveals the hero's surroundings. Returns true if new ground came into view.
    pub fn look_around(&mut self) -> bool {
        let r = self.sight_radius();
        self.fog.reveal(self.tile().0, self.tile().1, r)
    }

    /// Lights a lantern: reveals radius `r` (cells) around cell `(x, y)`.
    pub fn reveal(&mut self, x: i32, y: i32, r: i32) {
        self.fog.reveal(x, y, r);
    }

    /// The route ends where the click meant: the clicked cell, or a cell of the clicked
    /// building.
    fn route_complete(&self, goal: Tile) -> bool {
        match self.path.last() {
            Some(&end) => end == goal || self.world.location_at(goal).is_some_and(|l| self.world.location_at(end) == Some(l)),
            None => false,
        }
    }

    /// After a step: plan the walk to the clicked spot again if new ground came into view
    /// or the route ran out short of it; give up when no explored way gets closer.
    fn feel_the_way(&mut self, revealed: bool) {
        let Some(goal) = self.goal else { return };
        let here = self.tile();
        if here == goal || self.foe.is_some() {
            self.goal = None;
            return;
        }
        if self.route_complete(goal) || !(revealed || self.path.is_empty()) {
            return;
        }
        let path = self.plan(goal);
        if path.is_empty() {
            self.goal = None;
        }
        self.path = path;
    }

    pub fn stop(&mut self) {
        self.path.clear();
        self.goal = None;
        self.talk_to = None;
        self.wait_ticks = 0;
        self.reading = None;
        self.step_elapsed = 0.0;
    }

    /// Waits in real time (the UI's 1 h and 4 h): `hours × 2` wait ticks of
    /// [`WAIT_TICK_MINUTES`], one every [`STEP_SECONDS`], played by [`Game::tick`].
    pub fn begin_wait(&mut self, hours: u32) {
        if self.foe.is_some() {
            return;
        }
        self.path.clear();
        self.goal = None;
        self.reading = None;
        self.wait_ticks = hours * 2;
    }

    /// A real-time rest is under way (not a reading: [`Game::reading`]).
    pub fn waiting(&self) -> bool {
        self.wait_ticks > 0 && self.reading.is_none()
    }

    /// Where to draw the hero: between his cell and the next as the step plays.
    pub fn display_pos(&self) -> (f32, f32) {
        match self.path.first() {
            Some(&next) => {
                let k = (self.step_elapsed / STEP_SECONDS).clamp(0.0, 1.0);
                let b = self.world.map.center(next);
                (self.pos.0 + (b.0 - self.pos.0) * k, self.pos.1 + (b.1 - self.pos.1) * k)
            }
            None => self.pos,
        }
    }

    /// Where to draw army `a`: along the steps it took in the last step or wait tick, played
    /// over that window's real time as the original does ([`Walk`]).
    pub fn army_display_pos(&self, a: &Army) -> (f32, f32) {
        // Moved otherwise since (a battle, a respawn, an event): drawn where it is.
        if a.walk.points.last() != Some(&a.pos) {
            return a.pos;
        }
        a.walk.at(self.since_step / STEP_SECONDS).unwrap_or(a.pos)
    }

    /// Advance the world by `real_dt` seconds (world.md §2): each hero step and each wait
    /// tick plays over [`STEP_SECONDS`]; the game time a step takes is its own cost. Time only
    /// flows while the party walks or waits.
    pub fn tick(&mut self, real_dt: f32) -> Vec<Event> {
        let mut events = Vec::new();
        self.since_step += real_dt;
        if self.foe.is_some() || (!self.moving() && self.wait_ticks == 0) {
            self.step_elapsed = 0.0;
            if self.foe.is_some() {
                self.end_reading(&mut events);
                self.wait_ticks = 0;
            }
            return events;
        }
        self.step_elapsed += real_dt;
        while self.step_elapsed >= STEP_SECONDS && (self.moving() || self.wait_ticks > 0) {
            self.step_elapsed -= STEP_SECONDS;
            self.since_step = 0.0;
            let go = if self.moving() {
                self.hero_step(&mut events)
            } else {
                self.wait_ticks -= 1;
                let go = self.wait_tick(&mut events);
                if self.wait_ticks == 0 || self.foe.is_some() {
                    // The reading is done, or an enemy fell on the hero over his book.
                    self.end_reading(&mut events);
                }
                go
            };
            if !go || events.iter().any(Event::needs_reading) {
                // Stop and read: time stands still while a message is open. A reading
                // goes on after it.
                self.path.clear();
                if self.reading.is_none() {
                    self.wait_ticks = 0;
                }
                break;
            }
        }
        if !self.moving() && self.wait_ticks == 0 {
            self.step_elapsed = 0.0;
        }
        events
    }

    /// The hero takes the next step of his route (world.md §1): it is charged the cell he
    /// leaves; stepping onto a cell of another building (not a bridge) enters it and ends the
    /// walk; the world moves on by the step's time; an army next to him stops him. An army
    /// that has stepped onto his route makes him plan around it, or stop if there is no way.
    /// Returns false when the walk ended.
    fn hero_step(&mut self, events: &mut Vec<Event>) -> bool {
        self.follow_army();
        let Some(&(mut next)) = self.path.first() else { return false };
        if self.army_cells().any(|t| t == next) && Some(next) != self.goal {
            let Some(goal) = self.goal else {
                self.path.clear();
                return false;
            };
            self.path = self.plan(goal);
            match self.path.first() {
                Some(&around) => next = around,
                None => {
                    self.goal = None;
                    return false;
                }
            }
        }
        let from = self.tile();
        let w = &self.world;
        let allowed = if w.is_sea(next) { self.ship.is_some() } else { w.map.passable(next) };
        if !allowed {
            self.path.clear();
            return false;
        }
        let minutes = self.step_time(from, next);
        // Only the building clicked, or the one the route ends in, is entered; others on the
        // way are crossed without a visit (no window, tribute or events).
        let target = self.goal.and_then(|g| w.location_at(g));
        let last = self.path.len() == 1;
        let entered = w.location_at(next).filter(|&l| w.location_at(from) != Some(l) && (last || target == Some(l)));
        self.path.remove(0);
        self.pos = self.world.map.center(next);
        self.update_ship();
        let revealed = self.look_around();
        if entered.is_some() {
            self.path.clear();
            self.goal = None;
        }
        self.pass_time(minutes, events);
        if let Some(e) = self.contact() {
            self.goal = None;
            self.meet(e, events);
            return false;
        }
        if let Some(l) = entered {
            let taken = self.arrive(l);
            events.push(Event::Arrived(l));
            if taken {
                events.push(Event::Captured(l));
            }
            events.extend(self.auto_tribute(l));
            // Local events of the building.
            events.extend(self.run_script());
            return false;
        }
        self.feel_the_way(revealed);
        self.moving()
    }

    /// One wait tick: [`WAIT_TICK_MINUTES`] pass, armies move; an army reaching the party
    /// ends the wait. Returns false when it ended.
    fn wait_tick(&mut self, events: &mut Vec<Event>) -> bool {
        self.pass_time(WAIT_TICK_MINUTES, events);
        if let Some(e) = self.contact() {
            self.meet(e, events);
            return false;
        }
        !events.iter().any(Event::needs_reading)
    }

    /// Stand still for `hours` at once (1 h = 2 wait ticks, 4 h = 8; world.md §6): time
    /// passes, armies move, the daily moments happen. A hostile army reaching the party, or
    /// a message to read, ends the wait.
    pub fn wait(&mut self, hours: u32) -> Vec<Event> {
        let mut events = Vec::new();
        if self.foe.is_some() {
            return events;
        }
        self.stop();
        for _ in 0..hours * 2 {
            if !self.wait_tick(&mut events) {
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
        let talk_to = self.talk_to;
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
            } else if !a.met || talk_to == Some(a.uid) {
                a.met = true;
                found = Some(Event::Met(i));
            }
        }
        if matches!(found, Some(Event::Met(_) | Event::Encounter(_))) {
            self.talk_to = None;
        }
        if let Some(e) = &found {
            self.path.clear();
            if self.reading.is_none() {
                self.wait_ticks = 0;
            }
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
        self.visit_village(l);
        let loc = &mut self.world.locations[l];
        if loc.defended() {
            self.foe = Some(Foe::Garrison(l));
        } else if loc.kind.capturable() && !loc.owned() && loc.hostile() {
            self.world.give_to_player(l);
            return true;
        }
        false
    }

    /// Game time passes, in slices of at most one wait tick: the day's moments (00:00 and
    /// 12:00, world.md §6), spells run out, armies move with the minutes banked, the
    /// scenario's events run.
    pub(crate) fn pass_time(&mut self, minutes: f32, events: &mut Vec<Event>) {
        let mut left = minutes.max(0.0);
        // A new stretch for drawing: the steps of this time play in the next window.
        for a in &mut self.world.armies {
            a.walk.points.clear();
            a.walk.points.push(a.pos);
            a.walk.minutes.clear();
            a.walk.banked = (a.budget + left).min(AI_BUDGET_CAP);
        }
        loop {
            let slice = left.min(WAIT_TICK_MINUTES);
            left -= slice;
            self.pass_slice(slice, events);
            if left <= 0.0 {
                break;
            }
        }
    }

    fn pass_slice(&mut self, minutes: f32, events: &mut Vec<Event>) {
        for tick in self.clock.advance(minutes as f64) {
            match tick {
                Tick::Midnight(_) => self.midnight(),
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

    /// 00:00 (world.md §6): villages refill (slower as they fill), barracks may gain a unit,
    /// garrisons heal `GarrisonAutoHeal`% — the player's and the AI's.
    fn midnight(&mut self) {
        // Village refill, barracks growth, market redraw and garrison/medic healing
        // (economy.md), then the AI's night (world.md §6).
        self.economy_midnight();
        self.ai_midnight();
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

    /// The noon report: the player's income and wages (`Game::pay_noon`: a unit that
    /// cannot be paid sits out battles; on a short day units long unpaid leave), then the
    /// Ranger's daily heal.
    fn new_day(&mut self, day: u64) -> DayReport {
        let pay = self.pay_noon();
        self.noon_heal();
        let n = day.saturating_sub(self.start_day) + 1; // the game's first noon is day 1
        if self.world.demo && n.is_multiple_of(SPAWN_EVERY_DAYS) {
            let camps: Vec<_> = self.world.camps().filter(|(_, l)| !l.cleared).map(|(i, l)| (i, l.tile)).collect();
            for (camp, tile) in camps {
                if self.world.armies.iter().filter(|p| p.home == Some(camp)).count() < MAX_GANGS_PER_CAMP {
                    self.world.spawn_gang(camp, tile);
                }
            }
        }
        self.ai_new_day();
        let super::economy::NoonPay { income, mana, wages, mana_wages, unpaid, deserted } = pay;
        DayReport { day, income, mana, wages, mana_wages, unpaid, deserted, gold: self.gold, mana_total: self.mana }
    }

    /// Armies move for `minutes` (world.md §2): each banks them (up to [`AI_BUDGET_CAP`]) and
    /// takes the steps they cover. The AI plans the routes of the armies it steers
    /// (`rules::ai`); ships and the demo's gangs chase a nearby hostile hero or patrol. Then AI
    /// armies act on the goals they reached and fight each other.
    fn move_armies(&mut self, minutes: f32, events: &mut Vec<Event>) {
        self.ai_plan();
        let now = self.clock.total_minutes();
        let hero_tile = self.tile();
        let mut armies = std::mem::take(&mut self.world.armies);
        let world = &self.world;
        let map = &world.map;
        for a in armies.iter_mut() {
            a.budget = (a.budget + minutes).min(AI_BUDGET_CAP);
            let here = a.tile(map);
            let sails = a.sails();
            if ai::managed(a) {
                step_army(map, a, &|t| map.cost(t));
                continue;
            }
            // Ships stay on the water: they chase the hero to the water next to him.
            let goal = if sails { Game::sea_chase_goal(world, here, hero_tile) } else { Some(hero_tile) };
            let near = a.hostile() && now >= a.ignore_until && map.distance(here, hero_tile) <= CHASE_RADIUS && goal.is_some();
            let route = |a: &Army, from: Tile, to: Tile| {
                if sails {
                    map.path_by(from, to, AI_PATH_NODES, &|_, y| world.sea_step(y))
                } else {
                    ai::army_path(world, a, from, to, AI_PATH_NODES)
                }
            };
            if let (true, Some(goal)) = (near, goal) {
                if !a.chasing || a.path.last() != Some(&goal) {
                    a.path = route(a, here, goal);
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
                    if fits && world.location_at(t).is_none() && map.distance(t, a.post) <= r {
                        a.path = route(a, here, t);
                        if !a.path.is_empty() {
                            break;
                        }
                    }
                }
                // Rest between patrol legs, or after failing to find one *(guess)*.
                a.rest_until = now + self.rng.range(30, 180) as f64;
            }
            if sails {
                step_army(map, a, &|t| world.sea_step(t));
            } else {
                step_army(map, a, &|t| map.cost(t));
            }
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
        let mut u = Unit::new(&self.content, kind, slot);
        u.last_paid = self.clock.total_minutes() as u64;
        self.squad.push(u);
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
        b.set_improved_ai(self.improved_ai);
        // Lasting world spells change the stats of both sides.
        // Spells on a leader alone (`OneEnemy`, `p-LifeLose`) hold its first unit.
        b.apply_spells(Team::Player, &self.army_spells());
        if let Some(f) = b.fighters.iter_mut().find(|f| f.squad_index == Some(0)) {
            magic::apply_to_fighter(f, &self.leader_spells());
        }
        if let Some(Foe::Army(i)) = self.foe {
            b.apply_spells(Team::Enemy, &self.spells_on_army(i));
            if let Some(f) = b.fighters.iter_mut().find(|f| f.team == Team::Enemy) {
                magic::apply_to_fighter(f, &self.spells_on_leader(i));
            }
        }
        // An enemy army attacked in a building of its own side (one hostile to the hero)
        // defends with that building's defence, as a garrison does.
        let army_home = match self.foe {
            Some(Foe::Army(i)) => self.world.location_covering(self.world.armies[i].tile(&self.world.map)).map(|l| &self.world.locations[l]).filter(|l| l.hostile()).map_or(0, |l| l.garrison_defence),
            _ => 0,
        };
        if defence.max(army_home) > 0 {
            b.set_building_defence(Team::Enemy, defence.max(army_home));
        }
        // The hero fighting in a building of his own or of a friend (attitude above 0): its
        // extra defence is added to every defence of his units (battle.md §0, 485908), as for
        // any garrison at home.
        let here = self.location.or_else(|| self.world.location_covering(self.tile()));
        if let Some(own) = here.map(|l| &self.world.locations[l]).filter(|l| (l.owned() || l.attitude > 0) && l.garrison_defence > 0) {
            b.set_building_defence(Team::Player, own.garrison_defence);
        }
        self.play_battle_start(&b);
        b
    }

    /// The play log (`diag::play`): both sides of a battle as it starts, every unit with its
    /// stats, bonuses and worn items.
    fn play_battle_start(&self, b: &Battle) {
        let c = &self.content;
        let foe = match self.foe {
            Some(Foe::Army(i)) => self.world.armies.get(i).map(|a| format!("army {} «{}» (items {:?})", a.id, a.name, a.items.iter().map(|&i| c.item(i).name.clone()).collect::<Vec<_>>())),
            Some(Foe::Garrison(l)) => self.world.locations.get(l).map(|l| format!("garrison of building {} «{}»", l.id, l.name)),
            None => None,
        };
        let mut text = format!(
            "BATTLE starts against {} at {:?}; building defence player {} / enemy {}; AI {}",
            foe.unwrap_or_else(|| "?".into()),
            self.tile(),
            b.building_defence(Team::Player),
            b.building_defence(Team::Enemy),
            if self.improved_ai { "expert" } else { "easy" }
        );
        for f in &b.fighters {
            let s = &f.stats;
            let items: Vec<String> = f.items.iter().flatten().map(|&i| c.item(i).name.clone()).collect();
            text.push_str(&format!(
                "\n  {:?} {:?} {} lv{} hp {}/{} AB {} AS {} DB {} DS {} MP {} Ini {} Mnvr {} prot L/E/D {}/{}/{} regen {} vamp {} bonuses {:?} items {:?}",
                f.team, f.slot, f.name, f.level, f.hp, s.max_hp(), s[Stat::AttackBlow], s[Stat::AttackShot], s[Stat::DefenceBlow], s[Stat::DefenceShot], s[Stat::MagicPower],
                s[Stat::Initiative], s[Stat::Manevres], s[Stat::ProtectLife], s[Stat::ProtectElemental], s[Stat::ProtectDeath], s[Stat::Regen], s[Stat::Vampirizm], s.bonuses, items
            ));
        }
        crate::diag::play(&self.clock.label(), &text);
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
        crate::diag::play(&self.clock.label(), &format!("BATTLE log:\n  {}\nBATTLE ends: {:?} after {} turns", battle.log.join("\n  "), battle.outcome(), battle.round));
        let garrison = match self.foe {
            Some(Foe::Garrison(l)) => Some(self.world.locations[l].id),
            _ => None,
        };
        let result = self.settle_battle(battle);
        // A building taken from its garrison is entered: its events are checked now, as
        // when the hero walks into it (the original opens its window, 4bbc84, which scans).
        if let (Some(id), BattleResult::Victory { .. }) = (garrison, &result) {
            if let Some(engine) = self.script.as_mut().filter(|_| id != 0) {
                engine.visit(super::events::Place::Building(id));
            }
        }
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
                // The garrison's gold (ruins: their treasure), the building's stock and one
                // day's income; no division.
                let reward = std::mem::take(&mut loc.treasure_gold) + std::mem::take(&mut loc.tribute_gold) + loc.gold_income.max(0);
                let treasure = std::mem::take(&mut loc.treasure);
                let rolls = std::mem::take(&mut loc.loot_rolls);
                // Whatever it is (castle, fort, ruins…), a place whose garrison is beaten is
                // the hero's now; only the demo's bandit camps burn instead.
                let captured = (loc.kind != LocationKind::Camp).then(|| {
                    loc.owner = Owner::Player;
                    loc.faction = 1;
                    loc.attitude = 3;
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
                let (gold, wages) = self.player_victory_gold(&self.world.armies[i]);
                let reward = gold + wages;
                // An army whose home castle or fort stands empty loses it to the player.
                let home = self.world.armies[i].home.filter(|&h| {
                    let l = &self.world.locations[h];
                    l.kind.capturable() && !l.owned() && l.garrison.is_empty()
                });
                let army = &mut self.world.armies[i];
                army.gold -= gold;
                let (id, mut found) = (army.id, std::mem::take(&mut army.items));
                // Every item its units wore goes too (`ai::army_units` puts them on).
                // Off the map; a lord retreats, others may respawn (`rules::ai`).
                self.army_beaten(i, Beaten::ByPlayer);
                self.gold += reward;
                if id == 0 && self.rng.range(1, 100) <= GANG_LOOT_CHANCE {
                    found.extend(self.roll_item(Source::Loot));
                }
                let (loot, left_behind) = self.take_items(found);
                dropped_left += left_behind;
                if let Some(h) = home {
                    let loc = &mut self.world.locations[h];
                    loc.owner = Owner::Player;
                    loc.faction = 1;
                    loc.attitude = 3;
                }
                BattleResult::Victory { reward, mana, lost, loot, left_behind: dropped_left, level_ups, captured: home }
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
    pub(crate) fn roll_item(&mut self, source: Source) -> Option<ItemId> {
        let pool = self.content.items_from(source);
        if pool.is_empty() {
            return None;
        }
        Some(pool[self.rng.range(0, pool.len() as i32 - 1) as usize])
    }

    /// Puts found items into the pack. Returns (kept, left behind).
    pub(crate) fn take_items(&mut self, found: Vec<ItemId>) -> (Vec<ItemId>, usize) {
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

    /// Items for sale where the party stands, if there is a market. Ill-disposed markets
    /// trade too, dearer ([`Game::buy_price`]; the footage shows a market of attitude −2
    /// trading).
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
        if !self.can_sell(item) {
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
        items::put_on(&self.content, &mut self.squad[unit], slot, item);
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

    /// Hands the item in slot `slot` of squad member `from` straight to squad member `to`
    /// (dragged from one unit onto another on the army screen).
    pub fn give(&mut self, from: usize, slot: usize, to: usize) -> Result<(), EquipError> {
        let item = self.squad.get(from).and_then(|u| u.items.get(slot).copied().flatten()).ok_or(EquipError::NoSuchItem)?;
        let target = self.squad.get(to).ok_or(EquipError::NoSuchItem)?;
        let free = items::slot_for(&self.content, target, item)?;
        let u = &mut self.squad[from];
        u.items[slot] = None;
        u.hp = u.hp.min(u.max_hp(&self.content));
        items::put_on(&self.content, &mut self.squad[to], free, item);
        Ok(())
    }
}


/// The event engine's archetype code of a hero class.
pub(crate) fn archetype_of(hero: HeroClass) -> u8 {
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

    /// A new demo game whose generator then stands at `seed` (a new map always starts it
    /// at 1; tests vary it).
    fn new_game(hero: HeroClass, seed: u32) -> Game {
        let mut g = Game::new(content(), hero);
        g.rng = Rng::new(seed);
        g
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
        // The village makes no offer on this stream, so its tribute is taken on arrival.
        let n = events.len();
        assert_eq!(events[n - 2], Event::Arrived(millbrook));
        assert!(matches!(events[n - 1], Event::Tribute { at, .. } if at == millbrook), "{events:?}");
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
        assert!(g.collect_tribute().is_some());
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
        assert_eq!(g.hero().hp, 10 + 55 * 15 / 100, "15% at noon");
        let mut k = quiet_game(HeroClass::Knight);
        k.squad[0].hp = 10;
        k.pass_time(4.0 * 60.0, &mut events);
        assert_eq!(k.hero().hp, 10, "only the Ranger");
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
        let events = walk_until_stopped(&mut g);
        // No offer on this stream: the tribute (an item) is taken on arrival.
        assert!(events.iter().any(|e| matches!(e, Event::Tribute { paid: Tribute::Item(_), .. })), "{events:?}");
        assert_eq!(g.collect_tribute(), None, "already collected");
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
        // Half its gold, and its daily wages.
        let want = GANG_REWARD + crate::rules::ai::army_wages(&g.content, &g.world.armies[0].troops);
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { reward, lost: 0, captured: None, .. } if reward == want));
        assert_eq!(g.gold, gold + want);
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
        // The loot itself follows economy.md (half the gang's gold plus its wages).
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { .. }));
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
    fn a_quick_battle_resolves_exactly_like_a_played_one() {
        let setup = |seed: u32, lair: bool| {
            let mut g = new_game(HeroClass::Knight, seed);
            g.world.armies.clear();
            g.hire(unit(&g, "spearman")).unwrap();
            g.hire(unit(&g, "spearman")).unwrap();
            g.foe = Some(Foe::Garrison(g.world.index_of(if lair { "Bandit lair" } else { "Bandit camp" })));
            g
        };
        let json = |g: &Game| serde_json::to_string(g).unwrap();
        let mut outcomes = Vec::new();
        for seed in 0..6 {
            let (mut quick, mut played) = (setup(seed, seed % 2 == 1), setup(seed, seed % 2 == 1));
            let mut b = quick.start_battle();
            let outcome = b.auto_play_to_end();
            assert_ne!(outcome, Outcome::Ongoing);
            let quick_result = quick.resolve_battle(&b);
            let mut b = played.start_battle();
            b.begin();
            while b.outcome() == Outcome::Ongoing {
                b.ai_step();
            }
            let played_result = played.resolve_battle(&b);
            assert_eq!(quick_result, played_result, "seed {seed}");
            assert_eq!(json(&quick), json(&played), "seed {seed}: the same game after it");
            assert_eq!(quick.drain_events(), played.drain_events());
            outcomes.push(outcome);
        }
        assert!(outcomes.contains(&Outcome::Victory));
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
    fn markets_stock_market_items_and_restock_every_midnight() {
        let mut g = quiet_game(HeroClass::Knight);
        at_oakford(&mut g);
        let stock = g.market_here().unwrap().to_vec();
        assert_eq!(stock.len(), MARKET_STOCK);
        assert!(stock.iter().all(|&i| g.content.sources(i).contains(&Source::Market)));
        g.gold = 10_000;
        g.buy(0).unwrap();
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK - 1);
        let mut events = Vec::new();
        g.pass_time(15.0 * 60.0, &mut events); // 08:00 -> 23:00
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK - 1, "not midnight yet");
        g.pass_time(60.0, &mut events);
        assert_eq!(g.market_here().unwrap().len(), MARKET_STOCK, "drawn anew at midnight");
        let stock = g.market_here().unwrap();
        assert!(stock.windows(2).all(|w| g.content.item(w[0]).cost <= g.content.item(w[1]).cost), "sorted by price");
    }

    #[test]
    fn buying_and_selling() {
        let mut g = quiet_game(HeroClass::Knight);
        g.location = None;
        assert_eq!(g.buy(0), Err(TradeError::NoMarket), "on the road");
        at_oakford(&mut g);
        let item = g.market_here().unwrap()[0];
        let cost = g.content.item(item).cost;
        let price = g.buy_price(item);
        assert_eq!(price, crate::rules::economy::relation_price(cost, 3, true), "his own castle: ×0.75");
        g.gold = price - 1;
        assert_eq!(g.buy(0), Err(TradeError::NotEnoughGold));
        g.gold = price;
        assert_eq!(g.buy(0), Ok(item));
        assert_eq!((g.gold, g.pack.clone()), (0, vec![item]));
        assert_eq!(g.sell(0), Ok(cost / 4), "ItemSaleCost 25% (the demo's F is 100)");
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
        let c = g.content.clone();
        g.squad.iter_mut().for_each(|u| u.heal_full(&c));
        let shield_slot = g.hero().items.iter().position(|i| *i == Some(shield)).unwrap();
        g.unequip(0, shield_slot).unwrap();
        assert_eq!(g.hero().hp, 70, "HP capped to the new max");
        assert_eq!(g.pack, vec![axe, bow, shield]);
        assert_eq!(g.unequip(0, shield_slot), Err(EquipError::NoSuchItem));
        g.pack = vec![axe; PACK_SIZE];
        assert_eq!(g.unequip(0, 0), Err(EquipError::PackFull));
    }

    #[test]
    fn an_item_raising_max_hp_brings_its_hit_points() {
        let mut g = quiet_game(HeroClass::Knight);
        let shield = item(&g, "oak_shield");
        let c = g.content.clone();
        g.squad[0].heal_full(&c);
        assert_eq!((g.hero().hp, g.hero().max_hp(&c)), (70, 70));
        g.pack = vec![shield];
        g.equip(0, 0).unwrap();
        assert_eq!((g.hero().hp, g.hero().max_hp(&c)), (75, 75), "healed with the new maximum, not 70/75");
        // Wounded, the lost hit points stay lost.
        let slot = g.hero().items.iter().position(|i| *i == Some(shield)).unwrap();
        g.unequip(0, slot).unwrap();
        g.squad[0].hp = 60;
        g.equip(0, 0).unwrap();
        assert_eq!(g.hero().hp, 65);
    }

    #[test]
    fn a_worn_item_is_handed_to_another_unit() {
        let mut g = quiet_game(HeroClass::Knight);
        let (shield, sword) = (item(&g, "oak_shield"), item(&g, "short_sword"));
        let taken: Vec<_> = g.squad.iter().map(|u| u.slot).collect();
        let slot = g.content.formation.free_slot(&taken, crate::rules::formation::Row::Front).unwrap();
        g.squad.push(Unit::new(&g.content, unit(&g, "spearman"), slot));
        g.pack = vec![shield, sword];
        g.equip(0, 0).unwrap();
        g.equip(1, 0).unwrap();
        let at = g.hero().items.iter().position(|i| *i == Some(shield)).unwrap();
        g.give(0, at, 1).unwrap();
        assert!(!g.hero().items.contains(&Some(shield)));
        assert!(g.squad[1].items.contains(&Some(shield)));
        let back = g.squad[1].items.iter().position(|i| *i == Some(shield)).unwrap();
        g.pack = vec![item(&g, "oak_shield")];
        g.equip(0, 0).unwrap();
        assert_eq!(g.give(1, back, 0), Err(EquipError::SameType), "the hero has a shield again");
        assert!(g.squad[1].items.contains(&Some(shield)), "a refused hand-over keeps the item");
        assert_eq!(g.give(0, 3, 1), Err(EquipError::NoSuchItem));
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
    #[test]
    fn walking_across_a_friendly_building_does_not_enter_it() {
        let mut s = strip();
        let mut v = building(BuildingType::Village, 10, 3, (2, 2));
        v.relations = [1, 0, 0, 0];
        v.gold_per_day = 25;
        v.gold_max = 50;
        s.buildings = vec![v];
        let mut g = start(&s);
        g.world.armies.clear();
        // No fog: these are about buildings on the route.
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        assert!(g.set_destination((20, 2)));
        assert!(g.path.iter().any(|&t| g.world.location_covering(t) == Some(0)), "the road runs through the village");
        let events = walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (20, 2), "walked on to the point clicked");
        assert!(!events.iter().any(|e| matches!(e, Event::Arrived(_) | Event::Tribute { .. })), "{events:?}");
        assert_eq!(g.world.locations[0].tribute_gold, 25, "passing by takes no tribute");
        // Clicking the village itself enters it.
        assert!(g.set_destination((9, 2)));
        let events = walk_until_stopped(&mut g);
        assert!(events.contains(&Event::Arrived(0)), "{events:?}");
    }

    #[test]
    fn the_route_goes_around_an_enemy_building_unless_it_is_clicked() {
        let mut s = strip();
        // A hostile town across rows 0–4 of columns 11–12; row 5 stays open.
        let mut t = building(BuildingType::Town, 12, 4, (2, 5));
        t.relations = [-2, 0, 0, 0];
        t.faction = 4;
        s.buildings = vec![t];
        let mut g = start(&s);
        g.world.armies.clear();
        // No fog: these are about buildings on the route.
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        assert!(g.set_destination((20, 2)));
        assert!(g.path.iter().all(|&t| g.world.location_covering(t).is_none()), "around it: {:?}", g.path);
        let events = walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (20, 2));
        assert!(!events.iter().any(|e| matches!(e, Event::Arrived(_))));
        // Clicked, it is the destination: the walk ends inside it.
        assert!(g.set_destination((11, 2)));
        assert!(g.path.last().is_some_and(|&t| g.world.location_covering(t) == Some(0)));
    }

    fn strip() -> Scenario {
        let mut s = scenario(24, 6);
        s.header.heroes[0] = hero(2, 2, 200, &[troop(4, 0, 2)]);
        s.header.heroes[0].artifacts = [7, 0, 0];
        s
    }

    fn start(s: &Scenario) -> Game {
        Game::from_scenario(Arc::new(tk::content()), s, HeroClass::Knight)
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
        // 120 to start with (word 17 is starting gold); a feudal army pays its wages at noon.
        let carried = g.world.armies[0].gold;
        assert!(carried > 0 && carried <= 120, "{carried}");
        // Half its gold (no minimum) and its daily wages.
        let wages = crate::rules::ai::army_wages(&g.content, &g.world.armies[0].troops);
        assert!(wages > 0);
        let r = g.resolve_battle(&b);
        assert!(matches!(&r, BattleResult::Victory { reward, captured: None, loot, .. } if *reward == carried / 2 + wages && loot == &vec![ItemId(9)]), "{r:?}");
        assert_eq!(g.gold, gold + carried / 2 + wages);
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
    fn the_hero_follows_the_army_he_clicked_until_they_meet() {
        let mut s = strip();
        let mut friend = army(1, 12, 2, 1, &[troop(4, 0, 1)]);
        friend.patrols = 0;
        s.armies = vec![friend];
        let mut g = start(&s);
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        let at = g.world.armies[0].tile(&g.world.map);
        assert!(g.set_destination(at));
        // It moves on before the hero gets there.
        g.world.armies[0].pos = g.world.map.center((20, 2));
        g.world.armies[0].post = (20, 2);
        let events = walk_until_stopped(&mut g);
        assert!(events.contains(&Event::Met(0)), "met where it went: {events:?}");
        assert!(g.world.map.distance(g.tile(), (20, 2)) <= 1, "next to it at {:?}", g.tile());
    }

    #[test]
    fn clicking_a_friendly_army_meets_it_again() {
        // The help: "click it to talk or fight". Passing by greets once; a click always talks.
        let mut s = strip();
        let mut friend = army(1, 10, 2, 1, &[troop(4, 0, 1)]);
        friend.patrols = 0;
        s.armies = vec![friend];
        let mut g = start(&s);
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        g.set_destination((22, 2));
        assert_eq!(walk_until_stopped(&mut g).last(), Some(&Event::Met(0)));
        assert!(g.world.armies[0].met, "greeted once");
        // Standing next to it, the player clicks it: a new meeting.
        let at = g.world.armies[0].tile(&g.world.map);
        assert!(g.world.map.distance(g.tile(), at) <= 1);
        assert!(g.set_destination(at));
        let events = walk_until_stopped(&mut g);
        assert!(events.contains(&Event::Met(0)), "{events:?}");
        assert_eq!(g.foe, None);
    }

    #[test]
    fn hostile_armies_chase_a_nearby_hero() {
        let mut s = strip();
        let mut foe = army(1, 7, 4, -2, &[troop(4, 0, 1)]);
        foe.patrols = 0;
        // The AI goes only for battles it would win: a bold one for this.
        foe.aggression = 100;
        s.armies = vec![foe];
        let mut g = start(&s);
        let events = g.wait(4);
        assert!(g.world.armies[0].chasing);
        assert!(matches!(events.last(), Some(Event::Encounter(0))), "it comes for the waiting hero: {events:?}");
        assert!(g.clock.total_minutes() < g.world.start.total_minutes() + 240.0, "the fight cuts the wait short");
    }

    #[test]
    fn fighting_in_his_own_building_gives_the_heros_side_its_defence() {
        // battle.md §0 (485908): every unit's defence gets + building defence, for the side in
        // its own building; the footage's panel: "in its own building, a bonus to all defences".
        let mut s = strip();
        let mut castle = building(BuildingType::Castle, 6, 3, (2, 2));
        castle.garrison_extra_defence = 12;
        s.buildings = vec![castle];
        s.armies = vec![army(1, 12, 2, -2, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.world.locations[0].owner = Owner::Player;
        // Standing outside: no bonus.
        g.foe = Some(Foe::Army(0));
        assert_eq!(g.start_battle().building_defence(Team::Player), 0);
        // In his own castle: its extra defence for every unit of his.
        g.location = Some(0);
        let b = g.start_battle();
        assert_eq!(b.building_defence(Team::Player), 12);
        assert_eq!(b.building_defence(Team::Enemy), 0, "the attackers stand outside");
    }

    #[test]
    fn a_friends_building_helps_the_hero_and_an_enemys_helps_the_enemy() {
        let mut s = strip();
        let mut castle = building(BuildingType::Castle, 6, 3, (2, 2));
        castle.garrison_extra_defence = 12;
        s.buildings = vec![castle];
        s.armies = vec![army(1, 12, 2, -2, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.world.locations[0].owner = Owner::Neutral;
        g.world.locations[0].attitude = 2;
        g.foe = Some(Foe::Army(0));
        g.location = Some(0);
        assert_eq!(g.start_battle().building_defence(Team::Player), 12, "a friend's castle");
        g.world.locations[0].attitude = -3;
        g.location = None;
        let at = g.world.locations[0].tile;
        g.world.armies[0].pos = g.world.map.center(at);
        let b = g.start_battle();
        assert_eq!((b.building_defence(Team::Enemy), b.building_defence(Team::Player)), (12, 0), "the enemy at home in a hostile castle");
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
        let centre = g.world.locations[0].tile;
        assert!(g.set_destination((15, 2)), "clicking any cell of it means the fort");
        assert_eq!(g.goal, Some(centre));
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
        // Income ×F/100 for the player (F = 120 without "impossible difficulty").
        assert_eq!((g.daily_income(), g.daily_mana()), (48, 5));
        let (gold, mana) = (g.gold, g.mana);
        let mut events = Vec::new();
        g.pass_time(24.0 * 60.0, &mut events);
        assert!(matches!(events.as_slice(), [Event::NewDay(DayReport { income: 48, mana: 5, .. })]), "{events:?}");
        assert_eq!(g.mana, mana + 5);
        assert!(g.gold >= gold + 48 - g.daily_wages());
    }

    #[test]
    fn beating_any_garrison_makes_the_place_the_heros() {
        // Ruins (like castles and forts): once their guards are beaten the place is his.
        let mut s = strip();
        let mut ruins = building(BuildingType::Ruins, 16, 3, (2, 2));
        ruins.faction = 4;
        ruins.relations = [-2, 0, 0, 0];
        ruins.garrison[0] = troop(4, 0, 2);
        s.buildings = vec![ruins];
        let mut g = start(&s);
        g.set_destination(g.world.locations[0].tile);
        walk_until_stopped(&mut g);
        assert_eq!(g.foe, Some(Foe::Garrison(0)));
        let mut b = g.start_battle();
        b.begin();
        wipe_enemies(&mut b);
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { captured: Some(0), .. }));
        let ruins = &g.world.locations[0];
        assert!(ruins.owned() && !ruins.defended() && !ruins.hostile(), "the ruins are his now");
    }

    #[test]
    fn the_fog_lifts_around_the_walking_hero() {
        let mut g = start(&strip());
        g.world.armies.clear();
        // The knight sees 9 cells.
        assert!(g.fog.enabled && g.fog.explored((2, 2)) && g.fog.explored((11, 2)) && !g.fog.explored((12, 2)));
        assert!(!Game::new(content(), HeroClass::Knight).fog.enabled, "the demo has no fog");
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
    fn a_hostile_fort_bars_the_route_unless_clicked_or_stood_in() {
        let mut s = strip();
        let mut fort = building(BuildingType::Fort, 10, 2, (1, 1));
        fort.relations = [-2, 0, 0, 0];
        fort.garrison[0] = troop(4, 0, 1);
        s.buildings = vec![fort];
        // Water above and below the fort: the only way east is through it.
        for y in [0u32, 1, 3, 4, 5] {
            tk::set(&mut s, 10, y, crate::dt::dtm::Surface::DeepSea);
        }
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        assert!(g.world.locations[0].bars_hero());
        assert!(!g.set_destination((20, 2)), "no route through an ill-disposed fort");
        assert!(g.set_destination((10, 2)), "the fort itself can be clicked");
        let events = walk_until_stopped(&mut g);
        assert_eq!(events.last(), Some(&Event::Arrived(0)));
        assert_eq!((g.tile(), g.foe), ((10, 2), Some(Foe::Garrison(0))));
        let mut b = g.start_battle();
        b.begin();
        wipe_enemies(&mut b);
        g.resolve_battle(&b);
        assert!(!g.world.locations[0].bars_hero(), "taken: his own");
        assert!(g.set_destination((20, 2)));
        // A neutral fort (attitude 0, no garrison) bars the way too, but not the hero
        // standing in it.
        let mut s2 = s.clone();
        s2.buildings[0].relations = [0, 0, 0, 0];
        s2.buildings[0].garrison[0] = troop(0, 0, 0);
        let mut g = start(&s2);
        g.fog = Fog::disabled(24, 6);
        assert!(!g.set_destination((20, 2)));
        assert!(g.set_destination((10, 2)));
        walk_until_stopped(&mut g);
        assert_eq!((g.location, g.foe), (Some(0), None));
        assert!(!g.world.locations[0].owned(), "not ill-disposed: not taken");
        assert!(g.set_destination((20, 2)), "from inside it he walks on");
    }

    #[test]
    fn towns_and_villages_on_the_way_are_crossed_without_a_visit() {
        let mut s = strip();
        // A 3 × 3 village across the road east (x 9..=11, y 1..=3).
        let mut v = building(BuildingType::Village, 11, 3, (3, 3));
        v.relations = [1, 0, 0, 0];
        s.buildings = vec![v];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        let l = &g.world.locations[0];
        assert!(!l.bars_hero() && l.cells().all(|t| g.world.map.cost(t) == Some(crate::rules::map::ROAD)));
        // Walking east along row 2 crosses the village's cells (road) without a visit: only the
        // building clicked, or the one a route ends in, is entered.
        assert!(g.set_destination((20, 2)));
        assert!(g.path.iter().any(|&t| g.world.location_at(t) == Some(0)), "the route may cross it");
        let events = walk_until_stopped(&mut g);
        assert!(!events.iter().any(|e| matches!(e, Event::Arrived(_))), "{events:?}");
        assert_eq!((g.tile(), g.location), ((20, 2), None));
    }

    #[test]
    fn each_step_plays_over_150_ms_and_costs_the_cell_left() {
        let mut s = strip();
        tk::set(&mut s, 2, 2, crate::dt::dtm::Surface::Road);
        tk::set(&mut s, 3, 2, crate::dt::dtm::Surface::Marsh);
        let mut g = start(&s);
        assert!(g.set_destination((5, 2)));
        assert_eq!(g.path, vec![(3, 2), (4, 2), (5, 2)]);
        let t0 = g.clock.total_minutes();
        g.tick(STEP_SECONDS * 0.9);
        assert_eq!((g.tile(), g.clock.total_minutes()), ((2, 2), t0), "the step is still playing");
        g.tick(STEP_SECONDS * 0.2);
        // Leaving the road: 3 × 5 = 15 minutes.
        assert_eq!((g.tile(), g.clock.total_minutes()), ((3, 2), t0 + 15.0));
        g.tick(STEP_SECONDS);
        // Leaving the marsh: 8 × 5 = 40 minutes, though the grass entered costs 25.
        assert_eq!((g.tile(), g.clock.total_minutes()), ((4, 2), t0 + 55.0));
        // A diagonal step is 1.5 times as long; the ranger is 4/5 as slow.
        assert_eq!(g.step_time((4, 2), (5, 3)), 37.5);
        let mut s = strip();
        s.header.heroes[2] = s.header.heroes[0].clone();
        let r = Game::from_scenario(Arc::new(tk::content()), &s, HeroClass::Ranger);
        assert_eq!((r.hero_speed(), r.step_time((4, 2), (5, 2)), r.step_time((4, 2), (5, 3))), (4, 20.0, 30.0));
        assert_eq!(r.sight_radius(), 10);
    }

    #[test]
    fn waiting_is_ticks_of_half_an_hour_played_in_real_time() {
        let mut g = start(&strip());
        let t0 = g.clock.total_minutes();
        g.wait(1);
        assert_eq!(g.clock.total_minutes(), t0 + 60.0, "1 h = 2 ticks");
        g.begin_wait(4);
        assert!(g.waiting());
        g.tick(STEP_SECONDS * 3.5);
        assert_eq!(g.clock.total_minutes(), t0 + 60.0 + 90.0, "three ticks so far");
        for _ in 0..10 {
            g.tick(STEP_SECONDS);
        }
        assert_eq!(g.clock.total_minutes(), t0 + 60.0 + 240.0, "4 h = 8 ticks");
        assert!(!g.waiting());
        g.tick(1.0);
        assert_eq!(g.clock.total_minutes(), t0 + 300.0, "then time stands still");
    }

    #[test]
    fn armies_move_only_on_the_minutes_banked_from_the_hero() {
        let mut s = strip();
        // A lord who wanders: patrol radius 8 around (12, 2).
        let mut lord = army(1, 12, 2, 1, &[troop(4, 0, 1)]);
        lord.patrols = 1;
        lord.patrol_radius = 8;
        s.armies = vec![lord];
        let mut g = start(&s);
        let at = g.world.armies[0].pos;
        g.tick(10.0);
        assert_eq!(g.world.armies[0].pos, at, "the hero stands still: so does the world");
        // One orthogonal grass step ahead of it.
        let a = &mut g.world.armies[0];
        a.mind.goal = crate::rules::ai::Goal::Wander((13, 2));
        a.mind.think_at = f64::MAX;
        a.path = vec![(13, 2)];
        let mut events = Vec::new();
        g.pass_time(20.0, &mut events);
        assert_eq!(g.world.armies[0].pos, at, "20 minutes do not pay a 25-minute grass step");
        g.pass_time(10.0, &mut events);
        assert_ne!(g.world.armies[0].pos, at, "30 do");
        // The bank holds 200 minutes at most.
        g.world.armies[0].path.clear();
        g.world.armies[0].mind.goal = crate::rules::ai::Goal::Idle;
        g.world.armies[0].mind.think_at = f64::MAX;
        g.pass_time(24.0 * 60.0, &mut events);
        assert_eq!(g.world.armies[0].budget, 200.0);
    }

    #[test]
    fn armies_walk_their_steps_within_the_heros_step() {
        let mut s = strip();
        let mut lord = army(1, 12, 2, 1, &[troop(4, 0, 1)]);
        lord.patrols = 1;
        lord.patrol_radius = 8;
        s.armies = vec![lord];
        let mut g = start(&s);
        let a = &mut g.world.armies[0];
        let (x0, y0) = a.pos;
        a.mind.goal = crate::rules::ai::Goal::Wander((15, 2));
        a.mind.think_at = f64::MAX;
        a.path = vec![(13, 2), (14, 2), (15, 2)];
        // 50 banked + 25 minutes: two grass steps (25 each) and a third left unpaid.
        a.budget = 50.0;
        let mut events = Vec::new();
        g.pass_time(25.0, &mut events);
        g.since_step = 0.0;
        let a = &g.world.armies[0];
        assert_eq!(a.pos, (x0 + 3.0, y0), "the bank pays all three steps");
        let at = |g: &mut Game, k: f32| {
            g.since_step = k * STEP_SECONDS;
            g.army_display_pos(&g.world.armies[0])
        };
        // 75 minutes banked: the first two steps take a third of the window each, the last
        // the rest; the figure moves steadily from cell to cell, never jumping.
        assert_eq!(at(&mut g, 0.0), (x0, y0));
        assert!((at(&mut g, 1.0 / 6.0).0 - (x0 + 0.5)).abs() < 1e-4);
        assert!((at(&mut g, 1.0 / 3.0).0 - (x0 + 1.0)).abs() < 1e-4);
        assert!((at(&mut g, 0.5).0 - (x0 + 1.5)).abs() < 1e-4);
        assert_eq!(at(&mut g, 1.0), (x0 + 3.0, y0));
        let mut last = x0;
        for i in 1..=30 {
            let x = at(&mut g, i as f32 / 30.0).0;
            assert!(x >= last && x - last < 0.2, "step {i}: {last} -> {x}");
            last = x;
        }
        // Moved by other means since: drawn where it is.
        g.world.armies[0].pos = (1.0, 1.0);
        assert_eq!(at(&mut g, 0.5), (1.0, 1.0));
    }

    #[test]
    fn a_noon_with_no_money_moving_is_empty() {
        let quiet = DayReport { day: 3, income: 0, mana: 0, wages: 0, mana_wages: 0, unpaid: 0, deserted: vec![], gold: 150, mana_total: 7 };
        assert!(quiet.is_empty(), "the balance alone does not count");
        assert!(!DayReport { income: 10, ..quiet.clone() }.is_empty());
        assert!(!DayReport { mana_wages: 2, ..quiet.clone() }.is_empty());
        assert!(!DayReport { unpaid: 1, ..quiet.clone() }.is_empty(), "the unpaid are news");
        assert!(!DayReport { deserted: vec![UnitId(4)], ..quiet }.is_empty());
    }

    #[test]
    fn the_expert_setting_gives_battles_the_improved_ai() {
        let mut g = quiet_game(HeroClass::Knight);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        assert_eq!(g.start_battle().ai_level, 1, "easy: the original's normal AI");
        g.improved_ai = true;
        assert_eq!(g.start_battle().ai_level, 2, "expert: improved enemy AI in battle");
    }

    #[test]
    fn a_unit_dragged_onto_another_cell_moves_or_swaps() {
        use crate::rules::formation::Row;
        let mut g = quiet_game(HeroClass::Knight);
        let taken: Vec<_> = g.squad.iter().map(|u| u.slot).collect();
        let free = g.content.formation.free_slot(&taken, Row::Back).unwrap();
        g.squad.push(Unit::new(&g.content, unit(&g, "archer"), free));
        let (hero_at, archer_at) = (g.squad[0].slot, g.squad[1].slot);
        let empty = g.content.formation.slots().find(|s| *s != hero_at && *s != archer_at).unwrap();
        assert!(g.move_unit(1, empty));
        assert_eq!(g.squad[1].slot, empty, "to an empty cell");
        assert!(g.move_unit(1, hero_at));
        assert_eq!((g.squad[1].slot, g.squad[0].slot), (hero_at, empty), "onto the hero: they swap");
        assert!(!g.move_unit(1, crate::rules::formation::Slot::new(Row::Front, 99)), "not a cell");
    }

    #[test]
    fn the_shown_route_is_the_one_walked() {
        let mut g = start(&strip());
        g.fog = Fog::disabled(24, 6);
        let shown = g.route_to((20, 2));
        assert!(!shown.is_empty());
        assert!(!g.moving(), "showing a route does not set off");
        assert!(g.set_destination((20, 2)));
        assert_eq!(g.path, shown);
    }

    #[test]
    fn no_army_can_be_walked_through() {
        let mut s = strip();
        for y in [0u32, 1, 3, 4, 5] {
            tk::set(&mut s, 10, y, crate::dt::dtm::Surface::DeepSea);
        }
        let mut friend = army(1, 10, 2, 1, &[troop(4, 0, 1)]);
        friend.patrols = 1;
        friend.patrol_radius = 5;
        s.armies = vec![friend];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        assert!(!g.set_destination((20, 2)), "a patrolling army standing in the gap blocks it");
        assert!(g.set_destination((10, 2)), "the army itself can be clicked");
    }

    #[test]
    fn stationary_guards_block_the_route_except_the_one_clicked() {
        let mut s = strip();
        for y in [0u32, 1, 3, 4, 5] {
            tk::set(&mut s, 10, y, crate::dt::dtm::Surface::DeepSea);
        }
        let mut guard = army(1, 10, 2, 1, &[troop(4, 0, 1)]);
        guard.patrols = 1;
        guard.patrol_radius = 0;
        s.armies = vec![guard];
        let mut g = start(&s);
        g.fog = Fog::disabled(24, 6);
        assert!(!g.set_destination((20, 2)), "a guard standing in the gap");
        assert!(g.set_destination((10, 2)), "the guard himself can be clicked");
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
        // Entering takes it all at once (economy.md §3), unless the village asks something first.
        let events = walk_until_stopped(&mut g);
        if g.village_offer().is_some() {
            assert_eq!(g.decline_offer(), Some(Tribute::Gold(25)));
        } else {
            let arrived = events.iter().position(|e| e == &Event::Arrived(0)).expect("arrived");
            assert_eq!(events.get(arrived + 1), Some(&Event::Tribute { at: 0, paid: Tribute::Gold(25), mana: 4 }), "{events:?}");
        }
        assert_eq!(g.mana, 4);
        assert_eq!(g.tribute_available(), None);
        g.wait(24);
        assert_eq!(g.tribute_available(), Some(25), "refilled at midnight");
    }
}


