//! The scenario's event engine in the running game: [`Game`] as the engine's [`EventWorld`],
//! and the moments the engine runs (mechanics.md §8.1).
//!
//! The engine runs at the start of a scenario, whenever game time passes (every slice of a
//! walk, a wait, a heal, a delay), when the hero steps into a building or onto an event
//! point, after a battle, when the player answers a question and when he pays for a rumour.
//! What it did comes back as [`Event::Script`] outcomes; the UI shows them (texts are read
//! from the scenario at runtime, never stored).
//!
//! Choices where the sources are silent are marked *(guess)* and listed in mechanics.md §8.1.

use super::content::{ItemId, UnitId};
use super::events::{ArmyId, EventEngine, EventId, EventOutcome, EventWorld, Place, UnitPick, SIDE_PLAYER};
use super::game::{troop_unit, Event, Foe, Game, PACK_SIZE, SPELL_BOOK_SIZE};
use super::town::ServiceError;
use super::units::{Stats, Unit};
use super::world::{Army, EventInfo, Troop};
use crate::dt::dtm::EventKind;

/// Gold a rumour costs in a main hall (the footage: 10).
pub const RUMOUR_PRICE: i32 = 10;
/// Radius revealed around an army an event shows *(guess)*.
const SHOW_ARMY_RADIUS: i32 = 3;
/// Radius of a lantern whose point has none set *(guess)*.
const LANTERN_RADIUS: i32 = 5;

/// How the scenario ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptEnd {
    Victory(EventId),
    Defeat(EventId),
}

/// A line of a main hall's list of quests and rumours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HallEntry {
    /// A rumour on offer, for [`RUMOUR_PRICE`].
    Rumour(EventId),
    /// A quest of this building that is in the journal.
    Quest(EventId),
    /// A quest of this building that is done.
    Done(EventId),
}

impl Event {
    /// Something the player should read before time goes on: a message, a question, a quest
    /// notice or the end of the scenario.
    pub fn needs_reading(&self) -> bool {
        match self {
            Event::Script(EventOutcome::Fired { message, .. }) => *message,
            Event::Script(EventOutcome::Declined(_) | EventOutcome::LoopGuard) => false,
            Event::Script(_) => true,
            _ => false,
        }
    }
}

/// Side code of a faction (1–4 → green, blue, yellow, red = 2–5) *(guess: the editor lists
/// the player, then the four colours; faction 1 is the player's own colour)*.
fn side(faction: u8) -> Option<u8> {
    (1..=4).contains(&faction).then_some(faction + 1)
}

impl Game {
    /// The scenario's event engine, if the game has one (not in the demo).
    pub fn script(&self) -> Option<&EventEngine> {
        self.script.as_deref()
    }

    /// Victory or defeat by a scenario event.
    pub fn script_end(&self) -> Option<ScriptEnd> {
        match self.script()?.ended()? {
            EventOutcome::Victory(id) => Some(ScriptEnd::Victory(*id)),
            EventOutcome::Defeat(id) => Some(ScriptEnd::Defeat(*id)),
            _ => None,
        }
    }

    /// Events that happened outside a tick or a wait (see [`Game::pending`]).
    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending)
    }

    /// Runs the event engine once. Returns its outcomes, then what its effects caused (noon
    /// reports during a delay, a battle).
    pub(crate) fn run_script(&mut self) -> Vec<Event> {
        let Some(mut engine) = self.script.take() else { return Vec::new() };
        let out = engine.tick(self);
        self.script = Some(engine);
        self.script_events(out)
    }

    fn script_events(&mut self, out: Vec<EventOutcome>) -> Vec<Event> {
        let mut events: Vec<Event> = out.into_iter().map(Event::Script).collect();
        let effects = std::mem::take(&mut self.effect_events);
        // A battle an event started: against the army as it stands after all the effects.
        let mut battle = false;
        for e in effects {
            match e {
                Event::Encounter(_) => battle = true,
                e => events.push(e),
            }
        }
        if battle {
            if let Some(Foe::Army(i)) = self.foe {
                events.push(Event::Encounter(i));
            }
        }
        events
    }

    /// The player's answer to the question waiting in the engine; then the events run on.
    /// Returns what followed.
    pub fn answer_question(&mut self, yes: bool) -> Vec<Event> {
        let Some(mut engine) = self.script.take() else { return Vec::new() };
        let out = engine.answer(self, yes);
        self.script = Some(engine);
        self.script_events(out)
    }

    /// The question waiting for an answer, if any.
    pub fn pending_question(&self) -> Option<EventId> {
        self.script()?.pending_question()
    }

    /// Rumours on offer where the hero stands.
    pub fn rumours_here(&self) -> Vec<EventId> {
        self.script().map_or_else(Vec::new, |e| e.rumours(self))
    }

    /// The main hall's list here: rumours on offer, then this building's quests in the
    /// journal and those done.
    pub fn hall_entries(&self) -> Vec<HallEntry> {
        let Some(engine) = self.script() else { return Vec::new() };
        let mut out: Vec<HallEntry> = self.rumours_here().into_iter().map(HallEntry::Rumour).collect();
        let Some(l) = self.location else { return out };
        let quest = |id: EventId| self.world.events.get(id as usize - 1).is_some_and(|e: &EventInfo| e.kind == Some(EventKind::Quest));
        for &id in self.world.locations[l].events.iter().filter(|&&id| id > 0 && quest(id)) {
            if engine.journal().contains(&id) {
                out.push(HallEntry::Quest(id));
            } else if engine.completed_quests().contains(&id) {
                out.push(HallEntry::Done(id));
            }
        }
        out
    }

    /// Pays [`RUMOUR_PRICE`] gold to hear rumour `id` of this main hall: it fires (or asks its
    /// question), then the events run on.
    pub fn hear_rumour(&mut self, id: EventId) -> Result<Vec<Event>, ServiceError> {
        if !self.rumours_here().contains(&id) || self.pending_question().is_some() {
            return Err(ServiceError::NotHere);
        }
        if self.gold < RUMOUR_PRICE {
            return Err(ServiceError::CannotAfford);
        }
        self.gold -= RUMOUR_PRICE;
        let Some(mut engine) = self.script.take() else { return Ok(Vec::new()) };
        let out = engine.hear_rumour(self, id);
        self.script = Some(engine);
        Ok(self.script_events(out))
    }

    /// An army met on the road (`Met` or `Encounter` from [`Game::contact`]): the meeting is
    /// recorded, the events run (a talk may come first), and the meeting stands only if the
    /// army is still there (and, for a battle, still hostile).
    pub(crate) fn meet(&mut self, e: Event, events: &mut Vec<Event>) {
        let (Event::Met(i) | Event::Encounter(i)) = e else {
            events.push(e);
            return;
        };
        let id = self.world.armies[i].id;
        if id == 0 || self.script.is_none() {
            events.push(e);
            return;
        }
        self.met_armies.insert(id);
        let after = self.run_script();
        let now = self.world.armies.iter().position(|a| a.id == id);
        match e {
            // A battle the events started instead comes with `after`.
            Event::Encounter(_) if after.iter().any(|e| matches!(e, Event::Encounter(_))) => {}
            Event::Encounter(_) => match now.filter(|&j| self.world.armies[j].hostile()) {
                Some(j) => {
                    self.foe = Some(Foe::Army(j));
                    events.push(Event::Encounter(j));
                }
                None => self.foe = None,
            },
            _ => events.extend(now.map(Event::Met)),
        }
        events.extend(after);
    }

    /// Removes active army `i` from the map, keeping the pending foe pointing at the right army.
    fn take_army(&mut self, i: usize) -> Army {
        match self.foe {
            Some(Foe::Army(j)) if j == i => self.foe = None,
            Some(Foe::Army(j)) if j > i => self.foe = Some(Foe::Army(j - 1)),
            _ => {}
        }
        self.world.armies.remove(i)
    }

    fn army_index(&self, id: ArmyId) -> Option<usize> {
        self.world.armies.iter().position(|a| a.id == id)
    }

    /// An army by id, on the map or waiting.
    fn army_mut(&mut self, id: ArmyId) -> Option<&mut Army> {
        let w = &mut self.world;
        w.armies.iter_mut().chain(w.inactive.iter_mut()).find(|a| a.id == id)
    }

    /// Brings a waiting army onto the map. Returns its index. Ships and armies placed on
    /// water stay out *(ships are not simulated yet)*.
    fn activate(&mut self, id: ArmyId) -> Option<usize> {
        if let Some(i) = self.army_index(id) {
            return Some(i);
        }
        let k = self.world.inactive.iter().position(|a| a.id == id)?;
        let mut a = self.world.inactive[k].clone();
        let map = &self.world.map;
        let tile = if map.passable(a.post) { Some(a.post) } else { map.nearest_passable(a.post, 8) };
        let tile = tile?;
        self.world.inactive.remove(k);
        a.pos = map.center(tile);
        a.path.clear();
        a.chasing = false;
        self.world.armies.push(a);
        Some(self.world.armies.len() - 1)
    }

    /// A text of the scenario with its escapes filled in: `#HERONAME` (and `#HEROCLASS`)
    /// become the hero's name, which is his class's name *(the original lets the player name
    /// the hero; Razdor has no name entry)*.
    pub fn fill_text(&self, s: &str) -> String {
        let name = self.hero().name(&self.content);
        s.replace("#HERONAME", name).replace("#HEROCLASS", name).replace('\r', "")
    }

    /// The name shown for squad member `u`: a named character's own name, else its class.
    pub fn unit_label(&self, u: &Unit) -> String {
        let named = (u.named as usize).checked_sub(1).and_then(|k| self.world.named_characters.get(k));
        match named {
            Some(n) if !n.is_empty() => n.clone(),
            _ => u.name(&self.content).to_string(),
        }
    }

    /// Reveals the cells within `radius` of `at` (lanterns, a shown army): recorded in
    /// [`Game::pending_reveals`] for the fog of war.
    pub fn reveal(&mut self, at: (i32, i32), radius: i32) {
        self.pending_reveals.push((at.0, at.1, radius.max(1)));
    }
}

impl EventWorld for Game {
    fn now(&self) -> u64 {
        self.clock.total_minutes() as u64
    }

    fn hero_archetype(&self) -> u8 {
        self.archetype
    }

    fn hero_level(&self) -> i64 {
        self.hero().level as i64
    }

    fn gold(&self) -> i64 {
        self.gold as i64
    }

    fn mana(&self) -> i64 {
        self.mana as i64
    }

    /// Living units, the hero included *(guess)*.
    fn squad_count(&self) -> i64 {
        self.squad.iter().filter(|u| u.alive()).count() as i64
    }

    /// Sum of the living units' tactical cost (the strength used for XP) *(guess)*.
    fn army_strength(&self) -> i64 {
        self.squad.iter().filter(|u| u.alive()).map(|u| self.content.tactical_cost(u.def, u.level) as i64).sum()
    }

    /// The player's buildings are his; a building held by an army is its faction's; a
    /// neutral one nobody's *(guess)*.
    fn building_owner(&self, building: u16) -> Option<u8> {
        let l = self.world.locations.iter().find(|l| l.id == building)?;
        match l.owner {
            super::world::Owner::Player => Some(SIDE_PLAYER),
            super::world::Owner::Army(_) => side(l.faction),
            super::world::Owner::Neutral => None,
        }
    }

    fn named_unit_holder(&self, unit: u8, named: u8) -> Option<u8> {
        let fits = |u: &Unit| u.named == named && (unit == 0 || u.def == UnitId(unit as u32));
        if self.squad.iter().any(|u| u.alive() && fits(u)) {
            return Some(SIDE_PLAYER);
        }
        let w = &self.world;
        let a = w.armies.iter().chain(w.inactive.iter()).find(|a| named != 0 && a.named == named)?;
        side(a.faction)
    }

    fn artifact_holder(&self, artifact: u8) -> Option<u8> {
        let item = ItemId(artifact as u32);
        let worn = self.squad.iter().any(|u| u.items.contains(&Some(item)));
        if worn || self.pack.contains(&item) {
            return Some(SIDE_PLAYER);
        }
        let w = &self.world;
        let a = w.armies.iter().chain(w.inactive.iter()).find(|a| a.items.contains(&item))?;
        side(a.faction)
    }

    fn player_defeated(&self, army: ArmyId) -> bool {
        self.beaten_armies.contains(&army)
    }

    /// Only the player fights battles so far: beaten by anyone is beaten by him.
    fn army_beaten(&self, army: ArmyId) -> bool {
        self.beaten_armies.contains(&army)
    }

    fn army_active(&self, army: ArmyId) -> bool {
        self.army_index(army).is_some()
    }

    /// On the map within a cell of its home building's entry; a waiting army is at home
    /// *(guess)*.
    fn army_at_home(&self, army: ArmyId) -> bool {
        if let Some(i) = self.army_index(army) {
            let a = &self.world.armies[i];
            let map = &self.world.map;
            return a.home.is_some_and(|h| map.distance(a.tile(map), self.world.locations[h].tile) <= 1);
        }
        self.world.inactive.iter().any(|a| a.id == army)
    }

    fn met_army(&self, army: ArmyId) -> bool {
        self.met_armies.contains(&army)
    }

    fn place(&self) -> Option<Place> {
        if let Some(l) = self.location {
            let id = self.world.locations[l].id;
            if id != 0 {
                return Some(Place::Building(id));
            }
        }
        let t = self.tile();
        self.world.points.iter().find(|p| p.tile == t).map(|p| Place::Point(p.id))
    }

    fn add_experience(&mut self, xp: i64) {
        let c = self.content.clone();
        self.squad[0].gain_xp(&c, xp.clamp(0, i32::MAX as i64) as i32);
    }

    /// Gold never goes below 0 *(guess)*.
    fn add_gold(&mut self, gold: i64) {
        self.gold = (self.gold as i64 + gold).clamp(0, i32::MAX as i64) as i32;
    }

    fn add_mana(&mut self, mana: i64) {
        self.mana = (self.mana as i64 + mana).clamp(0, i32::MAX as i64) as i32;
    }

    /// Joins the army in a free cell of its row; a full army takes nobody. Taken from
    /// `from_army`, the unit keeps its level there and leaves that army.
    fn add_unit(&mut self, unit: u8, named: u8, from_army: Option<ArmyId>) {
        let id = UnitId(unit as u32);
        if self.content.try_unit(id).is_none() || self.squad.len() >= self.max_squad() {
            return;
        }
        let taken: Vec<_> = self.squad.iter().map(|u| u.slot).collect();
        let row = Stats::of_level(&self.content, id, 1).preferred_row();
        let Some(slot) = self.content.formation.free_slot(&taken, row) else { return };
        let mut level = 1;
        if let Some(a) = from_army.and_then(|a| self.army_mut(a)) {
            if let Some(k) = a.troops.iter().position(|t| t.unit == id) {
                level = a.troops.remove(k).level;
            }
        }
        let c = self.content.clone();
        let mut u = troop_unit(&c, &Troop { unit: id, level, slot });
        u.named = named;
        u.from_event = true;
        self.squad.push(u);
    }

    /// The last one to join leaves first; its items go to the pack. Sent to `to_army`, it
    /// joins that army's troops.
    fn remove_unit(&mut self, pick: UnitPick, named: u8, to_army: Option<ArmyId>) {
        let fits = |u: &Unit| match pick {
            UnitPick::Type(t) => u.def == UnitId(t as u32) && (named == 0 || u.named == named),
            UnitPick::AddedByEvent => u.from_event,
            UnitPick::Any => true,
        };
        let Some(i) = (1..self.squad.len()).rev().find(|&i| fits(&self.squad[i])) else { return };
        let u = self.squad.remove(i);
        let items: Vec<ItemId> = u.items.iter().flatten().copied().collect();
        for item in items {
            if self.pack.len() < PACK_SIZE {
                self.pack.push(item);
            }
        }
        let c = self.content.clone();
        if let Some(a) = to_army.and_then(|a| self.army_mut(a)) {
            let taken: Vec<_> = a.troops.iter().map(|t| t.slot).collect();
            let row = Stats::of_level(&c, u.def, 1).preferred_row();
            if let Some(slot) = c.formation.free_slot(&taken, row).filter(|_| taken.len() < c.formation.capacity()) {
                a.troops.push(Troop { unit: u.def, level: u.level, slot });
                if u.named != 0 {
                    a.named = u.named;
                }
            }
        }
    }

    fn give_item(&mut self, artifact: u8) {
        let item = ItemId(artifact as u32);
        if self.content.try_item(item).is_some() && self.pack.len() < PACK_SIZE {
            self.pack.push(item);
        }
    }

    /// From the pack, else from whoever wears it.
    fn take_item(&mut self, artifact: u8) {
        let item = ItemId(artifact as u32);
        if let Some(k) = self.pack.iter().position(|&i| i == item) {
            self.pack.remove(k);
            return;
        }
        let c = self.content.clone();
        for u in &mut self.squad {
            if let Some(slot) = u.items.iter_mut().find(|s| **s == Some(item)) {
                *slot = None;
                u.hp = u.hp.min(u.max_hp(&c));
                return;
            }
        }
    }

    fn learn_spell(&mut self, spell: u8) {
        if spell != 0 && !self.spells.contains(&spell) && self.spells.len() < SPELL_BOOK_SIZE {
            self.spells.push(spell);
        }
    }

    /// Recorded for Stage 7 (spells on the world map).
    fn apply_spell(&mut self, spell: u8) {
        self.cast_on_army.push(spell);
    }

    fn activate_army(&mut self, army: ArmyId) {
        self.activate(army);
    }

    fn deactivate_army(&mut self, army: ArmyId) {
        if let Some(i) = self.army_index(army) {
            let mut a = self.take_army(i);
            a.path.clear();
            a.chasing = false;
            self.world.inactive.push(a);
        }
    }

    fn show_army(&mut self, army: ArmyId) {
        if let Some(i) = self.army_index(army) {
            let t = self.world.armies[i].tile(&self.world.map);
            self.reveal(t, SHOW_ARMY_RADIUS);
        }
    }

    /// The army comes onto the map next to the hero *(guess: it is activated if waiting)*.
    fn move_army_to_hero(&mut self, army: ArmyId) {
        let Some(i) = self.activate(army) else { return };
        let here = self.tile();
        let map = &self.world.map;
        let spot = map.grid.neighbours(here).find(|&n| map.passable(n) && self.world.location_at(n).is_none());
        if let Some(t) = spot {
            let a = &mut self.world.armies[i];
            a.pos = map.center(t);
            a.path.clear();
        }
    }

    fn forget_meeting(&mut self, army: ArmyId) {
        self.met_armies.remove(&army);
        if let Some(a) = self.army_mut(army) {
            a.met = false;
        }
    }

    fn light_lantern(&mut self, point: u16) {
        if let Some(p) = self.world.points.iter().find(|p| p.id as u16 == point).copied() {
            self.reveal(p.tile, if p.radius > 0 { p.radius } else { LANTERN_RADIUS });
        }
    }

    fn change_patrol(&mut self, army: ArmyId, delta: i8) {
        if let Some(a) = self.army_mut(army) {
            a.patrol_radius = (a.patrol_radius + delta as i32).max(0);
            a.patrols = a.patrol_radius > 0;
        }
    }

    /// The hero becomes unit type `unit`, keeping level, XP and items; the class bonuses
    /// (tied to the three hero types) are lost.
    fn set_hero_class(&mut self, unit: u8) {
        let id = UnitId(unit as u32);
        if self.content.try_unit(id).is_none() {
            return;
        }
        let c = self.content.clone();
        let h = &mut self.squad[0];
        h.def = id;
        h.hp = h.hp.min(h.max_hp(&c)).max(1);
    }

    /// A battle with the army (brought onto the map if waiting); the UI opens it.
    fn start_battle(&mut self, army: ArmyId) {
        if let Some(i) = self.activate(army) {
            self.path.clear();
            self.foe = Some(Foe::Army(i));
            self.effect_events.push(Event::Encounter(i));
        }
    }

    /// Time passes for the player alone: the world goes on (armies, noon reports).
    fn delay_player(&mut self, minutes: u64) {
        let mut events = Vec::new();
        self.path.clear();
        self.pass_time(minutes as f32, &mut events);
        self.effect_events.extend(events);
    }

    fn hero_to_one_hp(&mut self) {
        self.squad[0].hp = 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::events::Answer;
    use crate::dt::dtm::{BuildingType, Event as DtEvent, Scenario};
    use crate::rules::content::HeroClass;
    use crate::rules::world::testkit::*;
    use std::sync::Arc;

    const DAY: u16 = 1440;

    /// A once-event of `kind`, open all day every day.
    fn ev(kind: EventKind) -> DtEvent {
        DtEvent { kind: kind as u8, repeat: DAY, duration: DAY, once: 1, message: "m".into(), title: "t".into(), ..DtEvent::default() }
    }

    fn world(events: Vec<DtEvent>) -> Scenario {
        let mut s = scenario(16, 12);
        s.header.heroes[0] = hero(2, 2, 100, &[troop(4, 0, 1)]);
        s.events = events;
        s
    }

    fn point(id: u8, x: u16, y: u16, radius: u8) -> crate::dt::dtm::Point {
        crate::dt::dtm::Point {
            x,
            y,
            id,
            model: 9,
            serial: id as u16,
            event_slots: [0; 10],
            priorities: [0; 4],
            active_duration: 0,
            radius,
            event_count: 0,
            active: 0,
            unknown_41: [0; 58],
        }
    }

    fn start(s: &Scenario) -> Game {
        Game::from_scenario(Arc::new(content()), s, HeroClass::Knight, 1)
    }

    fn fired(events: &[Event]) -> Vec<EventId> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Script(EventOutcome::Fired { event, .. }) => Some(*event),
                _ => None,
            })
            .collect()
    }

    fn walk(g: &mut Game) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..10_000 {
            if !g.moving() {
                break;
            }
            events.extend(g.tick(0.05));
        }
        events
    }

    #[test]
    fn opening_events_change_the_game() {
        let mut e = ev(EventKind::Global);
        let r = &mut e.results;
        (r.gold, r.mana, r.experience) = (50, 7, 10);
        r.units_add = [5, 0, 0, 0];
        r.artifacts_add = [7, 0, 0, 0];
        r.activate_armies = [3, 0];
        r.spells_learned = [4, 0, 0, 0];
        let mut s = world(vec![e]);
        let mut sleeper = army(3, 10, 8, -2, &[troop(4, 0, 2)]);
        sleeper.inactive = 1;
        s.armies = vec![sleeper];
        let mut g = start(&s);
        let events = g.drain_events();
        assert_eq!(fired(&events), vec![1]);
        assert!(events.iter().any(Event::needs_reading));
        assert_eq!((g.gold, g.mana), (150, 7));
        assert_eq!(g.squad.len(), 3, "hero, the preset's unit and the one the event adds");
        assert!(g.squad[2].from_event && g.squad[2].def == UnitId(5));
        assert_eq!(g.pack, vec![ItemId(7)]);
        assert_eq!(g.spells, vec![4]);
        assert!(g.world.armies.iter().any(|a| a.id == 3) && g.world.inactive.is_empty());
        assert!(g.drain_events().is_empty());
    }

    #[test]
    fn events_take_units_and_items_and_armies_away() {
        let mut give = ev(EventKind::Global);
        (give.results.units_add, give.results.units_add_named) = ([5, 0, 0, 0], [1, 0, 0, 0]);
        give.results.artifacts_add = [7, 0, 0, 0];
        let mut take = ev(EventKind::Global);
        take.start_time = 624_354_300 + 60;
        take.results.units_remove = [0xFE, 0, 0, 0];
        take.results.removed_units_to_army = 2;
        take.results.artifacts_remove = [7, 0, 0, 0];
        take.results.deactivate_army = 2;
        take.results.hero_one_hp = 1;
        let mut s = world(vec![give, take]);
        s.named_characters = vec![crate::dt::dtm::NamedCharacter { unit: 5, name: "Aide".into() }];
        s.armies = vec![army(2, 12, 10, 1, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.drain_events();
        assert_eq!(g.unit_label(&g.squad[2]), "Aide");
        use crate::rules::events::EventWorld as _;
        assert_eq!(g.named_unit_holder(5, 1), Some(SIDE_PLAYER));
        g.wait(1);
        assert_eq!(g.squad.len(), 2, "the unit the event added leaves again");
        assert!(g.pack.is_empty());
        assert!(g.world.armies.is_empty());
        let a = &g.world.inactive[0];
        assert_eq!((a.id, a.troops.len(), a.named), (2, 2, 1), "it joined army 2, now waiting");
        assert_eq!(g.named_unit_holder(5, 1), Some(4), "army 2 is of faction 3 (yellow)");
        assert_eq!(g.hero().hp, 1);
    }

    #[test]
    fn a_local_event_fires_on_entering_its_building() {
        let mut e = ev(EventKind::Local);
        e.results.gold = 25;
        let mut s = world(vec![e]);
        s.header.heroes[0] = hero(2, 2, 100, &[]); // no wages on the way
        let mut town = building(BuildingType::Town, 9, 2, (1, 1));
        town.event_slots[0] = 1;
        town.event_count = 1;
        s.buildings = vec![town];
        let mut g = start(&s);
        assert!(fired(&g.drain_events()).is_empty(), "not at the start: the hero is elsewhere");
        assert!(g.set_destination((9, 2)));
        let events = walk(&mut g);
        assert!(events.contains(&Event::Arrived(0)));
        assert_eq!(fired(&events), vec![1]);
        assert_eq!(g.gold, 125);
    }

    #[test]
    fn a_point_event_stops_the_walk_to_be_read() {
        let e = ev(EventKind::Local);
        let mut s = world(vec![e]);
        let mut p = point(4, 6, 2, 0);
        p.event_slots[0] = 1;
        p.event_count = 1;
        s.points = vec![p];
        let mut g = start(&s);
        g.drain_events();
        assert!(g.set_destination((12, 2)));
        let events = walk(&mut g);
        assert_eq!(fired(&events), vec![1]);
        assert_eq!(g.tile(), (6, 2), "stopped on the point");
        assert!(!g.moving());
    }

    #[test]
    fn a_question_is_answered_through_the_game() {
        let mut ask = ev(EventKind::Global);
        ask.conditions.confirm_question = 1;
        ask.question = "q".into();
        ask.results.gold = -40;
        ask.results.artifacts_add = [9, 0, 0, 0];
        let mut g = start(&world(vec![ask.clone()]));
        assert_eq!(g.drain_events(), vec![Event::Script(EventOutcome::Question(1))]);
        assert_eq!(g.pending_question(), Some(1));
        let events = g.answer_question(true);
        assert_eq!(fired(&events), vec![1]);
        assert_eq!((g.gold, g.pack.clone()), (60, vec![ItemId(9)]));
        assert_eq!(g.pending_question(), None);

        let mut g = start(&world(vec![ask]));
        g.drain_events();
        let events = g.answer_question(false);
        assert_eq!(events, vec![Event::Script(EventOutcome::Declined(1))]);
        assert_eq!(g.gold, 100);
        assert_eq!(g.script().unwrap().happened(1), Some(Answer::No));
    }

    #[test]
    fn rumours_cost_ten_gold_in_the_main_hall() {
        let mut rumour = ev(EventKind::Rumour);
        rumour.results.mana = 3;
        let mut quest = ev(EventKind::Quest);
        quest.title = "q%+Q".into();
        quest.flags = crate::dt::dtm::FlagScript::from_title(&quest.title);
        let mut s = world(vec![rumour, quest]);
        s.header.heroes[0] = hero(9, 3, 15, &[]);
        let mut town = building(BuildingType::Town, 9, 2, (1, 1));
        town.event_slots[..2].copy_from_slice(&[1, 2]);
        town.event_count = 2;
        s.buildings = vec![town];
        let mut g = start(&s);
        g.set_destination((9, 2));
        walk(&mut g);
        assert_eq!(g.location, Some(0));
        assert_eq!(g.hall_entries(), vec![HallEntry::Rumour(1), HallEntry::Quest(2)], "the quest was given on arrival");
        let events = g.hear_rumour(1).unwrap();
        assert_eq!(fired(&events), vec![1]);
        assert_eq!((g.gold, g.mana), (5, 3));
        assert_eq!(g.hear_rumour(1), Err(ServiceError::NotHere), "heard");
        assert_eq!(g.hall_entries(), vec![HallEntry::Quest(2)]);
    }

    #[test]
    fn victory_and_defeat_events_end_the_game() {
        let mut win = ev(EventKind::Global);
        (win.conditions.defeated_check, win.conditions.defeated_armies) = (1, [2, 0]);
        let mut s = world(vec![win]);
        s.header.victory_event = 1;
        s.armies = vec![army(2, 3, 2, -2, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.drain_events();
        assert_eq!(g.script_end(), None);
        // The hostile army next door attacks at once; the player wins.
        let events = g.wait(1);
        assert!(matches!(events.last(), Some(Event::Encounter(0))), "{events:?}");
        let mut b = g.start_battle();
        for f in b.fighters.iter_mut().filter(|f| f.team == crate::rules::battle::Team::Enemy) {
            f.hp = 0;
        }
        assert!(matches!(g.resolve_battle(&b), crate::rules::game::BattleResult::Victory { .. }));
        let events = g.drain_events();
        assert!(events.contains(&Event::Script(EventOutcome::Victory(1))));
        assert_eq!(g.script_end(), Some(ScriptEnd::Victory(1)));

        let mut lose = ev(EventKind::Global);
        lose.start_time = 624_354_300 + 120;
        let mut s = world(vec![lose]);
        s.header.defeat_event = 1;
        let mut g = start(&s);
        g.wait(4);
        assert_eq!(g.script_end(), Some(ScriptEnd::Defeat(1)));
    }

    #[test]
    fn an_event_can_start_a_battle_and_delay_the_player() {
        let mut e = ev(EventKind::Global);
        e.results.start_battle_with = 5;
        e.results.delay_hours = 5;
        let mut s = world(vec![e]);
        s.header.start_time = 624_354_300 - 9 * 60 + 8 * 60; // 08:00
        let mut sleeper = army(5, 12, 10, -2, &[troop(4, 0, 1)]);
        sleeper.inactive = 1;
        s.armies = vec![sleeper];
        let mut g = start(&s);
        let events = g.drain_events();
        assert!(events.iter().any(|e| matches!(e, Event::NewDay(_))), "the delay crosses noon: {events:?}");
        assert_eq!(events.last(), Some(&Event::Encounter(0)));
        assert_eq!(g.foe, Some(Foe::Army(0)));
        assert_eq!(g.world.armies[0].id, 5);
    }

    #[test]
    fn lanterns_are_recorded_for_the_fog() {
        let mut e = ev(EventKind::Global);
        e.results.light_lanterns = [7, 0, 0, 0];
        let mut s = world(vec![e]);
        s.points = vec![point(7, 10, 4, 6)];
        let g = start(&s);
        assert_eq!(g.pending_reveals, vec![(10, 4, 6)]);
    }

    #[test]
    fn hero_name_escapes() {
        let g = start(&world(vec![]));
        let name = g.hero().name(&g.content).to_string();
        assert_eq!(g.fill_text("Hail, #HERONAME!\r\n"), format!("Hail, {name}!\n"));
    }
}

#[cfg(test)]
mod real_maps {
    //! РК1 with the player's install; skipped without `RAZDOR_DT_DIR`. Numbers only.
    use super::*;
    use crate::dt::install::DtInstall;
    use crate::rules::content::{Content, HeroClass};
    use std::sync::Arc;

    /// Answers every question Yes; counts the messages.
    fn settle(g: &mut Game, events: Vec<Event>, messages: &mut usize) {
        let mut queue = events;
        for _ in 0..200 {
            if queue.is_empty() {
                break;
            }
            let mut next = Vec::new();
            for e in queue {
                match e {
                    Event::Script(EventOutcome::Fired { message: true, .. }) => *messages += 1,
                    Event::Script(EventOutcome::Question(_)) => {
                        *messages += 1;
                        next.extend(g.answer_question(true));
                    }
                    _ => {}
                }
            }
            next.extend(g.drain_events());
            queue = next;
        }
    }

    #[test]
    fn rk1_opening_and_first_buildings_for_every_class() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Arc::new(Content::from_dt(&dt));
        let s = dt.maps.iter().find(|m| m.name.starts_with("РК1")).unwrap().load().unwrap();
        for class in HeroClass::ALL {
            let mut g = Game::from_scenario(c.clone(), &s, class, 3);
            let mut messages = 0;
            let opening = g.drain_events();
            settle(&mut g, opening, &mut messages);
            let at_start = messages;
            // The two buildings nearest on foot.
            let mut visited = Vec::new();
            for _ in 0..2 {
                let from = g.tile();
                let Some((l, _)) = g.world.nearest_location(from, |l| !l.kind.is_bridge() && !visited.contains(&l.id)) else { break };
                visited.push(g.world.locations[l].id);
                g.set_destination(g.world.locations[l].tile);
                for _ in 0..40_000 {
                    if g.foe.is_some() || g.pending_question().is_some() {
                        break;
                    }
                    if !g.moving() {
                        if g.location == Some(l) {
                            break;
                        }
                        // Stopped to read: go on.
                        if !g.set_destination(g.world.locations[l].tile) {
                            break;
                        }
                    }
                    let events = g.tick(0.05);
                    settle(&mut g, events, &mut messages);
                }
                if g.foe.is_some() {
                    break;
                }
                // Leave the building for the next walk.
                g.location = None;
            }
            let engine = g.script().unwrap();
            println!(
                "РК1 {class:?}: {at_start} messages at the start, {messages} after {} buildings; {} firings, {} quests ({} done), day {}",
                visited.len(),
                engine.total_fired(),
                engine.journal().len() + engine.completed_quests().len(),
                engine.completed_quests().len(),
                g.clock.day_index() - g.world.start.day_index()
            );
            assert!(messages >= 1, "{class:?}: at least one dialog");
        }
    }
}
