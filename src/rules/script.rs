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

use super::content::{HeroClass, ItemId, UnitId, WageKind};
use super::events::{ArmyId, EventEngine, EventId, EventOutcome, EventWorld, Holder, Place, UnitPick, SIDE_PLAYER};
use super::magic::ActiveSpell;
use super::game::{troop_unit, Event, Foe, Game, PACK_SIZE, SPELL_BOOK_SIZE};
use super::town::ServiceError;
use super::units::Unit;
use super::world::{Army, EventInfo, Troop};
use crate::dt::dtm::EventKind;

/// Radius revealed around an army an event shows: 6 half-cells (world.md §3, 0x4ab6c3).
const SHOW_ARMY_RADIUS: i32 = 3;

/// How the scenario ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptEnd {
    Victory(EventId),
    Defeat(EventId),
}

/// A line of a main hall's list of quests and rumours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HallEntry {
    /// A rumour on offer. Hearing it is free; any price is the rumour event's own (its gold
    /// condition and result).
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
        self.record_outcomes(&out);
        // An event that took effect ends with the army recomputed and the hero's pairs with
        // the AI's armies marked to be rescored (0x4ab1ec → 0x497240(0, 1)).
        if out.iter().any(|o| matches!(o, EventOutcome::Fired { .. })) {
            self.mark_dirty(super::ai::HERO);
        }
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

    /// Hears rumour `id` of this main hall: it fires (or asks its question), then the events
    /// run on. There is no flat price: a rumour that costs something says so in its own event
    /// (a gold condition and a negative gold result), as the original's rumours do.
    pub fn hear_rumour(&mut self, id: EventId) -> Result<Vec<Event>, ServiceError> {
        if !self.rumours_here().contains(&id) || self.pending_question().is_some() {
            return Err(ServiceError::NotHere);
        }
        let Some(mut engine) = self.script.take() else { return Ok(Vec::new()) };
        let out = engine.hear_rumour(self, id);
        self.script = Some(engine);
        Ok(self.script_events(out))
    }

    /// An army that met the hero (`Met` or `Encounter` from [`Game::ai_contact`]): the
    /// meeting is recorded and the events run with it as the met army (0x4ade3c). An attack
    /// opens the battle only if none of them fired and the army is still there and hostile.
    /// Returns whether an event fired (only then does a greeting stop his walk).
    pub(crate) fn meet(&mut self, e: Event, events: &mut Vec<Event>) -> bool {
        self.meet_as(e, events, false)
    }

    /// The hero stepped onto army `e`'s cell (world.md §4.2 e, 0x4ad94c): the events run with
    /// it as the met army; the battle opens only if none of them fired (an army ill-disposed
    /// to him, attitude 0 included). If one fired, the army forgets him for now: its talk
    /// counter is −500 and it plans again.
    pub(crate) fn engage(&mut self, e: Event, events: &mut Vec<Event>) {
        self.meet_as(e, events, true);
    }

    fn meet_as(&mut self, e: Event, events: &mut Vec<Event>, on_step: bool) -> bool {
        let (Event::Met(i) | Event::Encounter(i)) = e else {
            events.push(e);
            return false;
        };
        let id = self.world.armies[i].id;
        if id == 0 || self.script.is_none() {
            events.push(e);
            return false;
        }
        self.met_armies.insert(id);
        let after = match self.script.take() {
            Some(mut engine) => {
                let out = engine.meet(self, id);
                self.script = Some(engine);
                self.script_events(out)
            }
            None => Vec::new(),
        };
        let now = self.world.armies.iter().position(|a| a.id == id);
        let fired = after.iter().any(|e| matches!(e, Event::Script(EventOutcome::Fired { .. } | EventOutcome::Question(_))));
        let fights = |a: &Army| if on_step { a.attitude <= 0 } else { a.hostile() };
        match e {
            // A battle the events started instead comes with `after`.
            Event::Encounter(_) if after.iter().any(|e| matches!(e, Event::Encounter(_))) => {}
            // An event fired: no battle (0x4ad94c, 0x4ade3c). Stepped onto, the army also
            // forgets him for now; an attacker does not.
            Event::Encounter(_) if fired => {
                self.foe = None;
                if let Some(j) = now.filter(|_| on_step) {
                    let a = &mut self.world.armies[j];
                    a.talk = super::game::TALKED;
                    a.path.clear();
                }
            }
            Event::Encounter(_) => match now.filter(|&j| fights(&self.world.armies[j])) {
                Some(j) => {
                    self.foe = Some(Foe::Army(j));
                    events.push(Event::Encounter(j));
                }
                None => self.foe = None,
            },
            _ => events.extend(now.map(Event::Met)),
        }
        events.extend(after);
        fired
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

    /// Brings a waiting army onto the map. Returns its index. A ship comes onto the water
    /// (`rules::ships`); a land army with no land near its post stays out.
    fn activate(&mut self, id: ArmyId) -> Option<usize> {
        if let Some(i) = self.army_index(id) {
            return Some(i);
        }
        // A waiting army, or a beaten one waiting for its respawn: 0x4969b8 brings back any
        // army off the map, clearing its destroyed and "beaten by" marks.
        let (mut a, respawning) = match self.world.inactive.iter().position(|a| a.id == id) {
            Some(k) => (self.world.inactive[k].clone(), None),
            None => {
                let k = self.world.respawns.iter().position(|r| r.army.id == id)?;
                (self.world.respawns[k].army.clone(), Some(k))
            }
        };
        let tile = self.world.placement(&a)?;
        match respawning {
            Some(k) => {
                self.world.respawns.remove(k);
                self.beaten_armies.remove(&id);
                self.ai_beaten.remove(&id);
            }
            None => {
                let k = self.world.inactive.iter().position(|a| a.id == id)?;
                self.world.inactive.remove(k);
            }
        }
        a.pos = self.world.map.center(tile);
        a.path.clear();
        a.chasing = false;
        if !super::ai::managed(&a) {
            self.world.armies.push(a);
            return Some(self.world.armies.len() - 1);
        }
        // The AI's record as 0x4969b8 sets it: every unit alive (the wounded keep their hit
        // points) and paid now, no path, standing in the building under it with its defence;
        // its home under it is its own again. It takes its place among the armies in their
        // order, its pairs are to be rescored and it draws four wander points.
        let now = self.clock.total_minutes() as u64;
        for t in a.troops.iter_mut() {
            if !t.alive() {
                t.hurt = 0;
            }
            t.died_at = None;
            t.kept_death = None;
            t.unpaid = false;
            t.last_paid = now;
        }
        a.mind.walked = 0;
        a.mind.no_path = true;
        let here = self.world.location_covering(tile);
        a.mind.standing = here;
        if let Some(l) = here {
            a.mind.defence = self.world.locations[l].garrison_defence;
            if a.home == Some(l) {
                let loc = &mut self.world.locations[l];
                loc.owner = super::world::Owner::Army(a.id);
                loc.take_sides(a.faction, a.ai.relations);
            }
        }
        let uid = a.uid;
        self.insert_army(a);
        self.mark_dirty(uid);
        let i = self.army_index(id)?;
        self.ai_wander(i);
        Some(i)
    }

    /// A text of the scenario with its escapes filled in: `#HERONAME` becomes the hero's
    /// name ([`Game::hero_name`]), `#HEROCLASS` his class's name.
    pub fn fill_text(&self, s: &str) -> String {
        let class = self.hero().name(&self.content);
        s.replace("#HERONAME", &self.hero_name()).replace("#HEROCLASS", class).replace('\r', "")
    }

    /// The name shown for squad member `u`: a named character's own name, else its class.
    pub fn unit_label(&self, u: &Unit) -> String {
        let named = (u.named as usize).checked_sub(1).and_then(|k| self.world.named_characters.get(k));
        match named {
            Some(n) if !n.is_empty() => n.clone(),
            _ => u.name(&self.content).to_string(),
        }
    }

    /// Reveals the cells within `radius` of `at` (lanterns, a shown army) in the fog of war,
    /// and records it in [`Game::pending_reveals`], and the cells it uncovered in
    /// [`Game::shown`] for the map to show.
    pub fn reveal_area(&mut self, at: (i32, i32), radius: i32) {
        let r = radius.max(1);
        self.pending_reveals.push((at.0, at.1, r));
        // Every cell the reveal can reach: the fog's own shape lies within r + 1.
        let reach = r + 1;
        let square = |fog: &crate::rules::fog::Fog| -> Vec<(i32, i32)> {
            let (x0, x1) = ((at.0 - reach).max(0), (at.0 + reach).min(fog.w - 1));
            let (y0, y1) = ((at.1 - reach).max(0), (at.1 + reach).min(fog.h - 1));
            (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).collect()
        };
        let dark: Vec<(i32, i32)> = if self.fog.enabled { square(&self.fog).into_iter().filter(|&t| !self.fog.explored(t)).collect() } else { Vec::new() };
        self.reveal(at.0, at.1, r);
        let cells: Vec<(i32, i32)> = dark.into_iter().filter(|&t| self.fog.explored(t)).collect();
        self.shown.push(super::game::Shown { at, cells });
    }
}

impl EventWorld for Game {
    fn now(&self) -> u64 {
        self.clock.total_minutes() as u64
    }

    fn hero_archetype(&self) -> u8 {
        self.archetype
    }

    /// The original compares its 0-based level: a level condition of 2 means our level 3.
    fn hero_level(&self) -> i64 {
        self.hero().level as i64 - 1
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

    /// Sum of the living units' tactical cost with their items (experience.md §1), as the
    /// original sums its army strength; that the dead are left out is a *(guess)*.
    fn army_strength(&self) -> i64 {
        self.squad.iter().filter(|u| u.alive()).map(|u| u.tactical(&self.content, 0) as i64).sum()
    }

    /// Whether it is the player's, and its faction (a captured building takes faction 1).
    fn building_state(&self, building: u16) -> Option<(bool, u8)> {
        let l = self.world.locations.iter().find(|l| l.id == building)?;
        Some((l.owner == super::world::Owner::Player, l.faction))
    }

    fn player_units(&self, unit: u8, named: u8) -> usize {
        let fits = |u: &Unit| match (unit, named) {
            (0xFF, 0) => u.from_event && u.named == 0,
            (_, 0) => u.def == UnitId(unit as u32),
            (_, n) => u.named == n,
        };
        self.squad.iter().filter(|u| u.alive() && fits(u)).count()
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

    /// Beaten by the player or by an AI army (`rules::ai` records AI battles).
    fn army_beaten(&self, army: ArmyId) -> bool {
        self.army_beaten_by_anyone(army)
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

    /// Event XP goes to the hero alone, as it is: no modifier, no cap; a negative amount
    /// does nothing (experience.md §5).
    fn add_experience(&mut self, xp: i64) {
        self.unit_gains(0, xp);
    }

    /// The event's gold is added as it is (the noon payment settles a debt); mana does not
    /// go below 0.
    fn add_gold(&mut self, gold: i64) {
        self.gold = (self.gold as i64 + gold).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
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
        let Some(slot) = self.content.formation.new_unit_slot(&taken) else { return };
        let mut level = 1;
        if let Some(a) = from_army.and_then(|a| self.army_mut(a)) {
            if let Some(k) = a.troops.iter().position(|t| t.unit == id) {
                level = a.troops.remove(k).level;
            }
        }
        let c = self.content.clone();
        let mut u = troop_unit(&c, &Troop::new(id, level, slot));
        u.named = named;
        u.from_event = true;
        // Kind 3: an event's unit draws no wage.
        u.wage_kind = WageKind::Event;
        u.last_paid = self.clock.total_minutes() as u64;
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
            if let Some(slot) = c.formation.new_unit_slot(&taken).filter(|_| taken.len() < c.formation.capacity()) {
                a.troops.push(Troop::new(u.def, u.level, slot));
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

    /// The spell takes effect on the army at once and for free, lasting 10 (or 5) times as
    /// long as a cast ([`Game::apply_spell_to_army_ext`]); an unknown spell does nothing.
    fn apply_spell(&mut self, spell: u8) {
        if let Some(def) = self.spell(spell as u32).cloned() {
            self.apply_spell_to_army_ext(&def, true);
        }
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
        } else if let Some(k) = self.world.respawns.iter().position(|r| r.army.id == army) {
            // A beaten army waiting for its respawn is no longer destroyed (0x496900): it
            // never comes back by itself, only an activation brings it (its "beaten by"
            // mark stays).
            let a = self.world.respawns.remove(k).army;
            self.world.inactive.push(a);
        }
    }

    fn show_army(&mut self, army: ArmyId) {
        if let Some(i) = self.army_index(army) {
            let t = self.world.armies[i].tile(&self.world.map);
            self.reveal_area(t, SHOW_ARMY_RADIUS);
        }
    }

    /// The army is moved next to the hero (world.md §4.4, 0x4980d8): of his 8 neighbours,
    /// in the original's direction order, the one with the lowest score, its cost on the LAND
    /// map (the SHIP map when he is at sea; blocked 100 000) + 50 000 on a building's cell +
    /// 100 000 when someone stands there, the first lowest winning. Only a free, open cell
    /// (below 100 000) is used: the army's position and home cell (its post) move there. A
    /// waiting army is not brought onto the map.
    fn move_army_to_hero(&mut self, army: ArmyId) {
        const BLOCKED: i64 = 100_000;
        let here = self.tile();
        let w = &self.world;
        let map = &w.map;
        let at_sea = self.aboard();
        let parked = self.parked_ship();
        let occupied = |t: super::map::Tile| t == here || parked == Some(t) || w.armies.iter().any(|a| a.tile(map) == t);
        let mut best: Option<(i64, super::map::Tile)> = None;
        for (dx, dy) in super::map::DIRECTIONS {
            let t = (here.0 + dx, here.1 + dy);
            if !map.in_bounds(t) {
                continue;
            }
            let cost = if at_sea { map.water_cost(t) } else { map.cost(t) };
            let score = cost.map_or(BLOCKED, i64::from) + if w.location_covering(t).is_some() { 50_000 } else { 0 } + if occupied(t) { BLOCKED } else { 0 };
            if best.is_none_or(|(b, _)| score < b) {
                best = Some((score, t));
            }
        }
        let Some((_, t)) = best.filter(|&(score, _)| score < BLOCKED) else { return };
        let pos = map.center(t);
        if let Some(a) = self.army_mut(army) {
            // The patrol box stays where it was.
            a.box_centre = Some(a.patrol_centre());
            a.pos = pos;
            a.post = t;
            a.path.clear();
        }
    }

    fn light_lantern(&mut self, point: u16) {
        // A lantern without a radius reveals nothing (world.md §3, 0x4ab762).
        if let Some(p) = self.world.points.iter().find(|p| p.id as u16 == point && p.radius > 0).copied() {
            self.reveal_area(p.tile, p.radius);
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

    // --- Community Update extensions (mechanics.md §8.1) --------------------------------------

    fn remove_army_spell(&mut self, spell: u8) {
        self.effects.retain(|e| e.spell != spell as u32);
    }

    /// The player's unit wears exactly these items; what it wore goes to the pack. AI units
    /// carry no items of their own in Razdor: for an army the items join its items (the
    /// loot) without changing its fight; a garrison takes none *(recorded, not simulated)*.
    fn equip_unit(&mut self, holder: Holder, unit: u8, items: [u8; 4]) {
        let c = self.content.clone();
        let valid = |i: u8| (i != 0).then_some(ItemId(i as u32)).filter(|&id| c.try_item(id).is_some());
        match holder {
            Holder::Player => {
                let Some(u) = self.squad.get_mut(unit as usize) else { return };
                let old: Vec<ItemId> = u.items.iter().flatten().copied().collect();
                let before = u.max_hp(&c);
                for (k, slot) in u.items.iter_mut().enumerate() {
                    *slot = items.get(k).copied().and_then(valid);
                }
                // A higher maximum comes with its hit points (`items::put_on`).
                let max = u.max_hp(&c);
                u.hp = (u.hp + (max - before).max(0)).min(max);
                for item in old {
                    if self.pack.len() < PACK_SIZE {
                        self.pack.push(item);
                    }
                }
            }
            Holder::Army(a) => {
                if let Some(a) = self.army_mut(a) {
                    a.items.extend(items.iter().copied().filter_map(valid));
                }
            }
            Holder::Building(_) => {}
        }
    }

    /// The unit keeps its level (and, in the player's army, its XP, items and HP fraction).
    fn replace_unit(&mut self, holder: Holder, unit: u8, with: u8) {
        let id = UnitId(with as u32);
        if self.content.try_unit(id).is_none() {
            return;
        }
        if holder == Holder::Player {
            let c = self.content.clone();
            if let Some(u) = self.squad.get_mut(unit as usize) {
                let (hp, max) = (u.hp, u.max_hp(&c).max(1));
                u.def = id;
                if u.alive() {
                    u.hp = (hp * u.max_hp(&c) / max).max(1);
                }
            }
        } else if let Some(t) = self.troops_of(holder).and_then(|t| t.get_mut(unit as usize)) {
            t.unit = id;
        }
    }

    /// The army walks at the new speed; the Community patch writes the hero's speed too
    /// (world.md §2.1, 0xc279e6), which his steps then use instead of his class's
    /// *(guess: the same `max(1, 5 − correction)`, without the loader's archmage rule)*.
    fn set_army_speed(&mut self, holder: Holder, correction: i8) {
        match holder {
            Holder::Army(a) => {
                if let Some(a) = self.army_mut(a) {
                    a.speed = Army::speed_for(correction, a.troops.first().map_or(0, |t| t.unit.0));
                }
            }
            Holder::Player => self.speed_set = Some(Army::speed_for(correction, 0)),
            _ => {}
        }
    }

    /// The army or building takes the group's attitude towards the player from the
    /// scenario's relations; a building that joins the player's group becomes his *(guess)*.
    fn set_faction(&mut self, holder: Holder, group: u8) {
        let attitude = self.world.relations[(group - 1) as usize][0];
        let army_attitude = super::world::relation(super::world::player_attitude_to(&self.world.relations, group), attitude);
        match holder {
            Holder::Army(a) => {
                if let Some(a) = self.army_mut(a) {
                    a.faction = group;
                    a.attitude = if group == 1 { 3 } else { army_attitude };
                }
            }
            Holder::Building(b) => {
                if let Some(l) = self.world.locations.iter_mut().find(|l| l.id == b) {
                    l.faction = group;
                    l.attitude = if group == 1 { 3 } else { attitude };
                    if group == 1 {
                        l.owner = super::world::Owner::Player;
                    }
                }
            }
            Holder::Player => {}
        }
    }

    /// Only the relation towards the player is kept (there is no AI diplomacy yet): other
    /// groups are a recorded no-op.
    fn set_relation(&mut self, holder: Holder, group: u8, value: i8) {
        if group != 0 {
            return;
        }
        match holder {
            Holder::Army(a) => {
                let relations = self.world.relations;
                if let Some(a) = self.army_mut(a) {
                    a.attitude = super::world::relation(super::world::player_attitude_to(&relations, a.faction), value);
                    a.ai.relations[0] = value;
                }
            }
            Holder::Building(b) => {
                if let Some(l) = self.world.locations.iter_mut().find(|l| l.id == b) {
                    l.attitude = value;
                }
            }
            Holder::Player => {}
        }
    }

    /// World spells are kept per army, so a spell for one unit goes on its whole army
    /// *(guess)*; garrisons hold none.
    fn set_spells(&mut self, holder: Holder, _unit: Option<u8>, spells: &[u8]) {
        let list: Vec<ActiveSpell> = spells.iter().map(|&s| ActiveSpell::new(s as u32, None)).collect();
        match holder {
            Holder::Player => self.effects = list,
            Holder::Army(a) => {
                if let Some(a) = self.army_mut(a) {
                    a.effects = list;
                }
            }
            Holder::Building(_) => {}
        }
    }

    fn set_named_unit(&mut self, holder: Holder, unit: u8, named: u8, class: u8) {
        let class = Some(UnitId(class as u32)).filter(|&id| class != 0 && self.content.try_unit(id).is_some());
        match holder {
            Holder::Player => {
                if let Some(u) = self.squad.get_mut(unit as usize) {
                    u.named = named;
                    if let Some(id) = class {
                        u.def = id;
                    }
                }
            }
            Holder::Army(a) => {
                if let Some(a) = self.army_mut(a) {
                    a.named = named;
                    if let (Some(id), Some(t)) = (class, a.troops.get_mut(unit as usize)) {
                        t.unit = id;
                    }
                }
            }
            Holder::Building(_) => {
                if let (Some(id), Some(t)) = (class, self.troops_of(holder).and_then(|t| t.get_mut(unit as usize))) {
                    t.unit = id;
                }
            }
        }
    }

    /// Opcode 13: the unit (or every unit, the dead too, as the original walks all the
    /// records) gains the XP as it is; an AI troop banks it towards its levels like the
    /// player's units (experience.md §5).
    fn give_unit_xp(&mut self, holder: Holder, unit: Option<u8>, xp: i64) {
        let c = self.content.clone();
        if holder == Holder::Player {
            for k in 0..self.squad.len() {
                if unit.is_none_or(|n| n as usize == k) {
                    self.unit_gains(k, xp);
                }
            }
            return;
        }
        let xp = xp.clamp(0, i32::MAX as i64) as i32;
        let Some(troops) = self.troops_of(holder) else { return };
        for (k, t) in troops.iter_mut().enumerate() {
            if unit.is_none_or(|n| n as usize == k) {
                super::ai::troop_gain_xp(&c, t, xp);
            }
        }
    }

    /// Spells are kept per army (see `set_spells`); a garrison has none.
    fn has_spells(&self, holder: Holder, _unit: Option<u8>, spells: &[u8]) -> bool {
        let now = self.clock.total_minutes() as u64;
        let on = |effects: &[ActiveSpell]| spells.iter().all(|&s| effects.iter().any(|e| e.spell == s as u32 && e.lasts_at(now)));
        match holder {
            Holder::Player => on(&self.effects),
            Holder::Army(a) => {
                let w = &self.world;
                w.armies.iter().chain(w.inactive.iter()).find(|x| x.id == a).is_some_and(|x| on(&x.effects))
            }
            Holder::Building(_) => spells.is_empty(),
        }
    }

    fn forget_spell(&mut self, spell: u8) {
        self.spells.retain(|&s| s != spell);
    }

    /// The hero's figure is not a model of the scenario: a recorded no-op for him.
    fn set_army_model(&mut self, holder: Holder, model: u8) {
        if let Holder::Army(a) = holder {
            if let Some(a) = self.army_mut(a) {
                a.model = model;
            }
        }
    }

    fn random(&mut self, lo: i64, hi: i64) -> i64 {
        self.event_rng.range(lo.clamp(i32::MIN as i64, i32::MAX as i64) as i32, hi.clamp(i32::MIN as i64, i32::MAX as i64) as i32) as i64
    }

    /// The army's post moves to the cell and, on the map, it sets off there (its patrol
    /// then goes on around it) *(guess)*.
    fn set_army_target(&mut self, army: ArmyId, x: i32, y: i32) {
        let to = (x, y);
        if let Some(i) = self.army_index(army) {
            let w = &self.world;
            let here = w.armies[i].tile(&w.map);
            let path = if w.map.passable(to) { super::ai::army_path(w, &w.armies[i], here, to, AI_TARGET_NODES) } else { Vec::new() };
            let a = &mut self.world.armies[i];
            a.post = to;
            a.chasing = false;
            a.path = path;
        } else if let Some(a) = self.army_mut(army) {
            a.post = to;
        }
    }

    fn army_at(&self, army: ArmyId, x: i32, y: i32) -> bool {
        self.army_index(army).is_some_and(|i| self.world.armies[i].tile(&self.world.map) == (x, y))
    }

    /// To the cell, or the nearest passable one within 8 cells (else he stays); the walk
    /// stops and the hero looks around.
    fn teleport_player(&mut self, x: i32, y: i32) {
        let map = &self.world.map;
        let to = if map.passable((x, y)) { Some((x, y)) } else { map.nearest_passable((x, y), 8) };
        let Some(t) = to else { return };
        self.pos = map.center(t);
        self.path.clear();
        self.goal = None;
        self.location = None;
        self.look_around();
    }
}

/// Search limit of the path to an AI army's scripted target.
const AI_TARGET_NODES: usize = 4000;

/// What the next map of a campaign starts with ([`Game::next_map`]). A field is `None` (or
/// empty) when the scenario does not carry it over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextMap {
    /// The map to load: the scenario's next-map name; after an opcode 15 branch its leading
    /// "number-variant" (or the whole name) is the chosen `N-V` *(guess: the guide names maps
    /// "N-V …" and gives the next map as "0-0")*.
    pub name: String,
    pub branch: Option<(i16, i16)>,
    pub gold: Option<i32>,
    /// Gods' favour: Razdor's mana *(guess)*.
    pub mana: Option<i32>,
    /// Fame carries over (Razdor has no fame yet).
    pub fame: bool,
    /// The hero's (level, XP) and his whole spell book (header byte 3).
    pub hero: Option<(i32, i32)>,
    pub spells: Option<Vec<u8>>,
    /// The hero's four worn items (byte 4); off, his slots are emptied.
    pub hero_items: Option<[Option<ItemId>; crate::rules::items::SLOTS]>,
    /// The pack.
    pub inventory: Vec<ItemId>,
    /// The squad without the hero, living units only.
    pub army: Vec<Unit>,
    /// The scenario's flags as stored (counters with their digit): they always carry over,
    /// as the original stashes its flag string with the army (4b5ef8) and puts it back on
    /// the next map (4b5ff8).
    pub flags: Vec<String>,
    /// The hero's class (his preset on the next map) and name; the hero's unit type too,
    /// should an event have changed it (the original always carries the hero's record).
    pub class: HeroClass,
    pub hero_name: Option<String>,
    pub hero_unit: UnitId,
    /// The journal's history (`rules::journal`), always carried: the next map is its next
    /// chapter.
    pub journal: crate::rules::journal::History,
}

/// The name of branch (map, variant) for a next-map name such as "0-0 …".
pub fn branch_name(next: &str, (map, variant): (i16, i16)) -> String {
    let t = next.trim();
    let digits = |s: &str| s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let a = digits(t);
    if a > 0 && t[a..].starts_with('-') {
        let rest = &t[a + 1..];
        let b = digits(rest);
        if b > 0 {
            return format!("{map}-{variant}{}", &rest[b..]);
        }
    }
    format!("{map}-{variant}")
}

impl Game {
    /// After a scenario victory, the next campaign map (named by the scenario, or chosen by
    /// an opcode 15 branch) and what carries over to it (header 0x110). `None` before a
    /// victory or when the scenario names no next map.
    pub fn next_map(&self) -> Option<NextMap> {
        let engine = self.script()?;
        if !matches!(self.script_end(), Some(ScriptEnd::Victory(_))) {
            return None;
        }
        let branch = engine.campaign_branch();
        let name = match branch {
            Some(b) => branch_name(engine.next_map_name(), b),
            None if engine.next_map_name().trim().is_empty() => return None,
            None => engine.next_map_name().trim().to_string(),
        };
        let carry = engine.carry_over().map(|b| b != 0);
        let hero = self.hero();
        Some(NextMap {
            name,
            branch,
            gold: carry[0].then_some(self.gold),
            mana: carry[1].then_some(self.mana),
            fame: carry[2],
            hero: carry[3].then_some((hero.level, hero.xp)),
            spells: carry[3].then(|| self.spells.clone()),
            hero_items: carry[4].then_some(hero.items),
            inventory: if carry[5] { self.pack.clone() } else { Vec::new() },
            army: if carry[6] { self.squad.iter().skip(1).filter(|u| u.alive()).cloned().collect() } else { Vec::new() },
            flags: engine.flags().map(str::to_string).collect(),
            class: match self.archetype {
                2 => HeroClass::Archmage,
                3 => HeroClass::Ranger,
                _ => HeroClass::Knight,
            },
            hero_name: self.hero_name.clone(),
            hero_unit: hero.def,
            journal: self.journal.clone(),
        })
    }

    /// The next map of a campaign, started with what the last one carries over
    /// ([`Game::apply_carry_over`]) before its opening events run: they may look for the
    /// carried army (РК2 checks the herald at once) or the last map's flags.
    pub fn from_campaign(content: std::sync::Arc<crate::rules::content::Content>, scenario: &crate::dt::dtm::Scenario, prev: &NextMap) -> Game {
        let mut g = Game::unstarted(content, scenario, prev.class);
        g.apply_carry_over(prev);
        g.start_script();
        g
    }

    /// Squad member `k` gains `xp` outside battle; a new level is reported on the map.
    fn unit_gains(&mut self, k: usize, xp: i64) {
        let c = self.content.clone();
        let Some(u) = self.squad.get_mut(k) else { return };
        if u.gain_xp(&c, xp.clamp(0, i32::MAX as i64) as i32) > 0 {
            let level = u.level;
            self.effect_events.push(Event::LevelUp(k, level));
        }
    }

    /// Starts this (next) campaign map with what `prev` carries over (header 0x110): the
    /// hero keeps his level, XP and spell book only when the scenario carries them (else
    /// level 1 with no XP, and this map's book), and his worn items when it carries them;
    /// gold is added, mana set, the pack's items go to the pack, and the carried army joins
    /// with its levels and XP where the formation has room, all of it paid as of now.
    pub fn apply_carry_over(&mut self, prev: &NextMap) {
        let c = self.content.clone();
        let now = self.clock.total_minutes() as u64;
        if let Some(engine) = self.script.as_mut() {
            engine.set_flags(prev.flags.iter().cloned());
        }
        if prev.hero_name.is_some() {
            self.hero_name = prev.hero_name.clone();
        }
        self.journal = prev.journal.clone();
        self.journal.next_chapter();
        let hero = &mut self.squad[0];
        if c.try_unit(prev.hero_unit).is_some() {
            hero.def = prev.hero_unit;
        }
        (hero.level, hero.xp) = prev.hero.unwrap_or((1, 0));
        if let Some(items) = prev.hero_items {
            hero.items = items;
        }
        hero.heal_full(&c);
        if let Some(book) = &prev.spells {
            self.spells = book.clone();
        }
        if let Some(g) = prev.gold {
            self.gold += g;
        }
        if let Some(m) = prev.mana {
            self.mana = m;
        }
        self.pack.extend(prev.inventory.iter().copied());
        for u in &prev.army {
            let taken: Vec<_> = self.squad.iter().map(|u| u.slot).collect();
            let Some(slot) = c.formation.new_unit_slot(&taken) else { break };
            let mut u = u.clone();
            u.slot = slot;
            u.unpaid = false;
            u.last_paid = now;
            self.squad.push(u);
        }
    }

    /// The troops of an AI army (on the map or waiting) or of a building's garrison.
    fn troops_of(&mut self, holder: Holder) -> Option<&mut Vec<Troop>> {
        match holder {
            Holder::Army(a) => self.army_mut(a).map(|a| &mut a.troops),
            Holder::Building(b) => self.world.locations.iter_mut().find(|l| l.id == b).map(|l| &mut l.garrison),
            Holder::Player => None,
        }
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
        Game::from_scenario(Arc::new(content()), s, HeroClass::Knight)
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
    fn an_event_casts_its_spell_on_the_army_through_the_world_spell_path() {
        use crate::rules::content::{testkit as ck, Content, Stat, StatMods};
        let mut e = ev(EventKind::Global);
        e.results.cast_spell = 1;
        let s = world(vec![e]);
        let base = content();
        // Spell 1: +10 hits at once, +2 initiative for 5 hours.
        let spell = crate::rules::content::SpellDef { time_work: Some(5), add: StatMods::from([(Stat::Initiative, 2)]), ..ck::spell(1, 0) };
        let c = Content::new(base.units.clone(), base.items.clone(), vec![spell], base.options.clone(), base.formation);
        let mut g = Game::from_scenario(Arc::new(c), &s, HeroClass::Knight);
        let now = g.clock.total_minutes() as u64;
        assert_eq!(fired(&g.drain_events()), vec![1]);
        // An event's spell lasts TimeWork × 10 (5 h → 50 h).
        assert_eq!(g.active_spells(), &[crate::rules::magic::ActiveSpell::new(1, Some(now + 50 * 60))]);
        let plain = g.squad[1].stats(&g.content)[Stat::Initiative];
        assert_eq!(g.stats_with_spells(1)[Stat::Initiative], plain + 2);
        g.wait(5);
        assert_eq!(g.active_spells().len(), 1, "longer than a cast of it");
        g.wait(45);
        assert!(g.active_spells().is_empty());
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
    fn an_activated_army_comes_in_its_place_alive_paid_and_with_wander_points() {
        // 0x4969b8: its units revived and paid, no path, its home under it its own, its pairs
        // dirty, four wander points drawn; it acts in its index order among the armies.
        let mut s = world(Vec::new());
        let mut home = building(BuildingType::Village, 10, 8, (1, 1));
        home.faction = 2;
        s.buildings = vec![home];
        let mut sleeper = army(3, 10, 8, -2, &[troop(4, 0, 2)]);
        sleeper.inactive = 1;
        sleeper.home_building = 1;
        s.armies = vec![army(1, 2, 9, 0, &[troop(4, 0, 1)]), sleeper, army(5, 14, 2, 0, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.drain_events();
        let k = g.world.inactive.iter().position(|a| a.id == 3).unwrap();
        g.world.inactive[k].troops[0].died_at = Some(1);
        g.world.inactive[k].troops[0].hurt = 5;
        g.world.inactive[k].troops[1].unpaid = true;
        g.world.armies[0].mind.clean.insert(3);
        let draws = g.rng.clone();
        EventWorld::activate_army(&mut g, 3);
        assert_eq!(g.world.armies.iter().map(|a| a.id).collect::<Vec<_>>(), [1, 3, 5]);
        let a = &g.world.armies[1];
        assert!(a.troops.iter().all(|t| t.alive() && t.hurt == 0 && !t.unpaid));
        assert!(a.mind.no_path && a.mind.standing == Some(0));
        assert_ne!(a.mind.wander, [(0, 0); 4]);
        assert_ne!(g.rng.state(), draws.state(), "the wander points drawn");
        assert!(!g.world.armies[0].mind.clean.contains(&3), "its pairs dirty");
        assert_eq!(g.world.locations[0].owner, crate::rules::world::Owner::Army(3));
    }

    #[test]
    fn an_event_brings_back_a_beaten_army_and_stops_one_from_respawning() {
        // 0x4969b8 brings back any army off the map, a destroyed one too (its marks cleared);
        // 0x496900 on a destroyed army clears that flag, so it never respawns by itself.
        let mut s = world(Vec::new());
        s.armies = vec![army(3, 10, 8, -2, &[troop(4, 0, 2)])];
        let mut g = start(&s);
        Game::army_beaten(&mut g, 0, crate::rules::ai::Beaten::ByAi);
        assert!(EventWorld::army_beaten(&g, 3) && g.world.armies.is_empty(), "no home: it stays destroyed");
        EventWorld::activate_army(&mut g, 3);
        assert!(g.world.armies.iter().any(|a| a.id == 3) && g.world.respawns.is_empty());
        assert!(!EventWorld::army_beaten(&g, 3), "its mark cleared");
        Game::army_beaten(&mut g, 0, crate::rules::ai::Beaten::ByPlayer);
        g.world.respawns[0].due = 0.0;
        EventWorld::deactivate_army(&mut g, 3);
        assert!(g.world.respawns.is_empty() && g.world.inactive.iter().any(|a| a.id == 3));
        assert!(EventWorld::player_defeated(&g, 3), "the mark stays");
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
    fn the_hero_keeps_his_starting_class_whatever_unit_he_becomes() {
        // World.md §2.1 (0x4b4300): sight, speed and the cast divisor are set from the class
        // at the start; an event that changes his unit does not change them. A Community
        // speed event (0xc279e6) sets his speed itself.
        use crate::rules::events::EventWorld as _;
        let mut g = start(&world(vec![]));
        assert_eq!((g.hero_speed(), g.sight_radius()), (5, 9));
        g.set_hero_class(3);
        assert_eq!(g.hero_class(), Some(HeroClass::Ranger), "his unit is the ranger's now");
        assert_eq!((g.start_class(), g.hero_speed(), g.sight_radius()), (HeroClass::Knight, 5, 9));
        assert_eq!(g.step_time((2, 2), (3, 2)), 25.0);
        g.set_army_speed(Holder::Player, -3);
        assert_eq!((g.hero_speed(), g.step_time((2, 2), (3, 2))), (8, 40.0));
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
        assert!(g.set_destination((11, 2)));
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
        let mut s = world(vec![ask.clone()]);
        s.armies = vec![army(1, 14, 10, 0, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        assert_eq!(g.drain_events(), vec![Event::Script(EventOutcome::Question(1))]);
        assert_eq!(g.pending_question(), Some(1));
        g.world.armies[0].mind.clean.insert(crate::rules::ai::HERO);
        let events = g.answer_question(true);
        assert_eq!(fired(&events), vec![1]);
        assert_eq!((g.gold, g.pack.clone()), (60, vec![ItemId(9)]));
        assert_eq!(g.pending_question(), None);
        // The event took effect: the AI rescores the hero (0x4ab1ec → 0x497240(0, 1)).
        assert!(!g.world.armies[0].mind.clean.contains(&crate::rules::ai::HERO));

        let mut g = start(&s);
        g.drain_events();
        g.world.armies[0].mind.clean.insert(crate::rules::ai::HERO);
        let events = g.answer_question(false);
        assert!(g.world.armies[0].mind.clean.contains(&crate::rules::ai::HERO), "declined: no effect");
        assert_eq!(events, vec![Event::Script(EventOutcome::Declined(1))]);
        assert_eq!(g.gold, 100);
        assert_eq!(g.script().unwrap().happened(1), Some(Answer::No));
    }

    #[test]
    fn hearing_a_rumour_is_free_its_event_sets_any_cost() {
        let mut rumour = ev(EventKind::Rumour);
        rumour.results.mana = 3;
        rumour.results.gold = -4; // the rumour's own price, from its event
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
        assert_eq!((g.gold, g.mana), (11, 3), "no flat price: only the event's own -4 gold");
        assert_eq!(g.hear_rumour(1), Err(ServiceError::NotHere), "heard");
        assert_eq!(g.hall_entries(), vec![HallEntry::Quest(2)]);
    }

    #[test]
    fn the_journal_history_records_quests_rumours_and_messages_with_dates() {
        use crate::rules::journal::{EntryKind, Tab};
        let mut rumour = ev(EventKind::Rumour);
        (rumour.title, rumour.message) = ("Word in the inn".into(), "The mill is haunted.".into());
        let mut quest = ev(EventKind::Quest);
        (quest.title, quest.message) = ("The mill".into(), "Free the mill, #HERONAME.".into());
        quest.results.chained_event = 3;
        let mut done = ev(EventKind::Global);
        (done.title, done.message) = ("Freed".into(), "The miller thanks you.".into());
        done.results.completes_quest = 2;
        done.subordinate = 1; // only through the quest's chain
        let mut silent = ev(EventKind::Global);
        silent.message = String::new();
        let mut hello = ev(EventKind::Global);
        (hello.title, hello.message) = ("Dawn".into(), "Hello, #HERONAME.".into());
        let mut s = world(vec![rumour, quest, done, silent, hello]);
        s.header.heroes[0] = hero(9, 3, 15, &[]);
        let mut town = building(BuildingType::Town, 9, 2, (1, 1));
        town.event_slots[..2].copy_from_slice(&[1, 2]);
        town.event_count = 2;
        s.buildings = vec![town];
        let mut g = start(&s);
        g.drain_events();
        g.set_hero_name("Ivan");
        let opened = g.clock.total_minutes() as u64;
        let messages = g.journal_rows(Tab::Messages);
        assert!(messages.iter().any(|r| r.title == "Dawn" && r.text == "Hello, Ivan."), "the opening message, the name filled in when shown: {messages:?}");
        assert!(g.journal.entries.iter().all(|e| e.event != 4), "a silent event is not recorded");
        assert!(messages.iter().all(|r| r.date.is_some_and(|d| d.total_minutes() as u64 == opened)));

        g.set_destination((9, 2));
        walk(&mut g);
        let arrived = g.clock.total_minutes() as u64;
        assert!(arrived > opened);
        assert_eq!(g.journal.find(EntryKind::Quest, 2).map(|e| (e.minutes, e.title.as_str())), Some((arrived, "The mill")));
        assert_eq!(g.journal.find(EntryKind::Completed, 2).map(|e| e.minutes), Some(arrived), "the chained event completed it");
        assert!(g.journal_rows(Tab::Active).is_empty());
        let completed = g.journal_rows(Tab::Completed);
        assert_eq!(completed.len(), 1);
        assert_eq!((completed[0].title.as_str(), completed[0].text.as_str()), ("The mill", "Free the mill, Ivan."));
        assert_eq!(g.journal_rows(Tab::Messages)[0].title, "Freed", "newest first");

        g.hear_rumour(1).unwrap();
        let rumours = g.journal_rows(Tab::Rumours);
        assert_eq!(rumours.len(), 1);
        assert_eq!((rumours[0].title.as_str(), rumours[0].text.as_str()), ("Word in the inn", "The mill is haunted."));
        assert!(g.journal_rows(Tab::Messages).iter().all(|r| r.title != "The mill"), "a quest is not a message too");
    }

    #[test]
    fn active_quests_come_from_the_engine_even_without_history() {
        use crate::rules::journal::Tab;
        let mut quest = ev(EventKind::Quest);
        (quest.title, quest.message) = ("Old quest".into(), "From an old save.".into());
        let mut s = world(vec![quest]);
        s.header.heroes[0] = hero(9, 3, 15, &[]);
        let mut town = building(BuildingType::Town, 9, 2, (1, 1));
        town.event_slots[0] = 1;
        town.event_count = 1;
        s.buildings = vec![town];
        let mut g = start(&s);
        g.set_destination((9, 2));
        walk(&mut g);
        assert_eq!(g.journal_rows(Tab::Active).len(), 1);
        g.journal = Default::default(); // a save from before the history
        let rows = g.journal_rows(Tab::Active);
        assert_eq!((rows[0].title.as_str(), rows[0].text.as_str(), rows[0].date), ("Old quest", "From an old save.", None));
    }

    #[test]
    fn the_history_carries_over_to_the_next_map_as_a_new_chapter() {
        let mut g = start(&world(vec![]));
        g.journal.record(crate::rules::journal::EntryKind::Message, 1, 0, "t", "x");
        let next = NextMap {
            name: "Road".into(),
            branch: None,
            gold: None,
            mana: None,
            fame: false,
            hero: None,
            spells: None,
            hero_items: None,
            inventory: Vec::new(),
            army: Vec::new(),
            flags: Vec::new(),
            class: HeroClass::Knight,
            hero_name: None,
            hero_unit: g.squad[0].def,
            journal: g.journal.clone(),
        };
        let mut fresh = start(&world(vec![]));
        fresh.apply_carry_over(&next);
        assert_eq!(fresh.journal.entries.len(), 1);
        assert_eq!(fresh.journal.chapter, 1);
    }

    #[test]
    fn a_free_rumour_needs_no_gold() {
        let mut s = world(vec![ev(EventKind::Rumour)]);
        s.header.heroes[0] = hero(9, 3, 0, &[]);
        let mut town = building(BuildingType::Town, 9, 2, (1, 1));
        town.event_slots[0] = 1;
        town.event_count = 1;
        s.buildings = vec![town];
        let mut g = start(&s);
        g.set_destination((9, 2));
        walk(&mut g);
        assert_eq!(g.gold, 0);
        let events = g.hear_rumour(1).unwrap();
        assert_eq!(fired(&events), vec![1]);
        assert_eq!(g.gold, 0);
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
        // He walks into the hostile army next door; the player wins.
        assert!(g.set_destination((3, 2)));
        let events = walk(&mut g);
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
        e.results.delay_hours = 15;
        let mut s = world(vec![e]);
        // 22:00: the delay runs into the next day's noon, the hero's first.
        s.header.start_time = 624_354_300 - 11 * 60;
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
    fn a_shown_place_lists_only_the_cells_it_uncovered() {
        let mut g = start(&world(vec![]));
        g.fog = crate::rules::fog::Fog::new(g.fog.w, g.fog.h);
        g.fog.reveal(10, 4, 1);
        g.shown.clear();
        g.reveal_area((10, 4), 3);
        let first = g.shown.pop().unwrap();
        assert_eq!(first.at, (10, 4));
        assert!(!first.cells.is_empty() && first.cells.iter().all(|&t| g.fog.explored(t)));
        assert!(!first.cells.contains(&(10, 4)), "the cell already seen is not faded in again");
        g.reveal_area((10, 4), 3);
        assert!(g.shown.pop().unwrap().cells.is_empty(), "nothing new the second time");
    }

    #[test]
    fn lanterns_are_recorded_for_the_fog() {
        let mut e = ev(EventKind::Global);
        e.results.light_lanterns = [7, 0, 0, 0];
        let mut s = world(vec![e]);
        s.points = vec![point(7, 10, 4, 6)];
        let g = start(&s);
        assert_eq!(g.pending_reveals, vec![(10, 4, 6)]);
        let shown = g.shown.last().expect("the lantern is shown on the map");
        assert_eq!(shown.at, (10, 4));
        assert!(shown.cells.iter().all(|&t| g.fog.explored(t)), "only cells it uncovered, all lit now");
        assert!(!g.fog.enabled || g.fog.explored((10, 4)), "the lantern lights the fog");
    }

    #[test]
    fn an_event_on_stepping_onto_an_army_takes_the_place_of_the_battle() {
        // World.md §4.2 e (0x4ad94c): the events run with the army met; if one fires, no
        // battle, and the army's talk counter towards him drops to −500.
        let mut talk = ev(EventKind::Global);
        talk.conditions.meet_army = 2;
        let mut s = world(vec![talk]);
        let mut guard = army(2, 6, 2, -2, &[troop(4, 0, 1)]);
        (guard.patrols, guard.patrol_radius) = (1, 0);
        s.armies = vec![guard];
        let mut g = start(&s);
        g.drain_events();
        assert!(g.set_destination((6, 2)));
        let events = walk(&mut g);
        assert_eq!(fired(&events), vec![1]);
        assert!(!events.iter().any(|e| matches!(e, Event::Encounter(_))), "{events:?}");
        assert_eq!((g.foe, g.tile(), g.world.armies[0].talk), (None, (5, 2), -500));
        // Without an event: the battle.
        let mut s = world(vec![]);
        s.armies = vec![army(2, 6, 2, -2, &[troop(4, 0, 1)])];
        s.armies[0].patrols = 1;
        let mut g = start(&s);
        g.world.armies[0].patrol_radius = 0;
        assert!(g.set_destination((6, 2)));
        let events = walk(&mut g);
        assert_eq!(events.last(), Some(&Event::Encounter(0)));
        assert_eq!(g.foe, Some(Foe::Army(0)));
    }

    /// The hero walks from (2, 2) to (8, 4) while army 2 (`attitude`) steps from (5, 2) to
    /// (4, 2), next to him after his first step, to (3, 3); an event fires on meeting it.
    fn walk_past_army_with_event(attitude: i8) -> (Game, Vec<Event>) {
        let mut talk = ev(EventKind::Global);
        talk.conditions.meet_army = 2;
        let mut s = world(vec![talk]);
        s.armies = vec![army(2, 5, 2, attitude, &[troop(4, 0, 1)])];
        let mut g = start(&s);
        g.drain_events();
        let a = &mut g.world.armies[0];
        a.mind.scripted = true;
        a.path = vec![(4, 2)];
        // A hostile one's cached battle score says it wins.
        a.mind.scores.insert(super::super::ai::HERO, 1);
        assert!(g.set_destination((8, 4)));
        let events = walk(&mut g);
        (g, events)
    }

    #[test]
    fn a_greeting_whose_event_fires_stops_the_walk() {
        // 0x4ade3c: the greeting runs the events with the army met; one fired, he stops.
        let (g, events) = walk_past_army_with_event(1);
        assert_eq!(fired(&events), vec![1]);
        assert_eq!((g.tile(), g.moving(), g.world.armies[0].talk), ((3, 3), false, -500));
    }

    #[test]
    fn an_attack_whose_event_fires_brings_no_battle() {
        // 0x4ade3c: an AI attack runs the events with the attacker first; the battle opens
        // only if none fired. Unlike an army stepped onto, it keeps its talk counter.
        let (g, events) = walk_past_army_with_event(-2);
        assert_eq!(fired(&events), vec![1]);
        assert!(!events.iter().any(|e| matches!(e, Event::Encounter(_))), "{events:?}");
        assert_eq!((g.foe, g.tile(), g.moving()), (None, (3, 3), false));
        assert_ne!(g.world.armies[0].talk, super::super::game::TALKED);
    }

    #[test]
    fn a_lantern_without_a_radius_reveals_nothing() {
        // World.md §3: both lantern loops test radius > 0 (0x4ab762, 0x4b5a32).
        let mut e = ev(EventKind::Global);
        e.results.light_lanterns = [7, 0, 0, 0];
        let mut s = world(vec![e]);
        s.points = vec![point(7, 13, 9, 0)];
        let g = start(&s);
        assert!(g.pending_reveals.is_empty() && g.shown.is_empty());
        assert!(!g.fog.explored((13, 9)));
    }

    #[test]
    fn an_army_moved_to_the_hero_takes_the_cheapest_free_neighbour() {
        // World.md §4.4 (0x4980d8): the hero at (2, 2); score = cost + 50 000 on a building +
        // 100 000 when taken; the first lowest in direction order (NW, N, NE, E, SE, S, SW, W).
        use crate::rules::events::EventWorld as _;
        let mut s = world(vec![]);
        // Road to the south-east (3) is the cheapest; a village on the road east is dearer.
        set(&mut s, 3, 3, crate::dt::dtm::Surface::Road);
        set(&mut s, 3, 2, crate::dt::dtm::Surface::Road);
        s.buildings = vec![building(BuildingType::Village, 3, 2, (1, 1))];
        s.armies = vec![army(2, 12, 10, 1, &[troop(4, 0, 1)]), army(3, 14, 2, 1, &[troop(4, 0, 1)])];
        s.armies[1].inactive = 1;
        let mut g = start(&s);
        g.move_army_to_hero(2);
        let a = &g.world.armies[0];
        assert_eq!((a.tile(&g.world.map), a.post), ((3, 3), (3, 3)), "the road cell, home moved too");
        assert_eq!(a.patrol_centre(), (12, 10), "the patrol box is not recomputed");
        // Taken now: the next one goes to the first grass neighbour, NW, though it waits off
        // the map (it stays there).
        g.move_army_to_hero(3);
        let w = &g.world.inactive[0];
        assert_eq!((w.tile(&g.world.map), w.post), ((1, 1), (1, 1)));
        assert!(g.world.armies.iter().all(|a| a.id != 3), "not brought onto the map");
    }

    // --- Community Update opcodes ---------------------------------------------------------------

    /// A Community opcode event: "no meeting", patrol value `code`, resources (XP, gold, mana).
    fn op(code: i8, x: i16, g: i16, m: i16) -> DtEvent {
        let mut e = ev(EventKind::Global);
        e.message.clear();
        (e.results.no_meeting, e.results.patrol_delta) = (1, code);
        (e.results.experience, e.results.gold, e.results.mana) = (x, g, m);
        e
    }

    #[test]
    fn community_opcodes_change_the_players_army() {
        let mut learn = ev(EventKind::Global);
        learn.results.spells_learned = [4, 6, 0, 0];
        let mut equip = op(6, 0, 1, 0);
        equip.results.artifacts_add = [7, 0, 0, 0];
        let mut lasting = op(11, 0, -1, 0);
        lasting.results.spells_learned = [1, 0, 0, 0];
        let mut forget = op(16, 0, 0, 0);
        forget.results.spells_learned = [4, 0, 0, 0];
        let mut s = world(vec![learn, equip, op(7, 0, 1, 3), op(13, 0, -1, 10), lasting, forget, op(20, 10, 5, 0)]);
        s.named_characters = vec![crate::dt::dtm::NamedCharacter { unit: 5, name: "Aide".into() }];
        s.events.push(op(12, 0, 1, 1));
        let mut g = start(&s);
        g.drain_events();
        assert_eq!(g.squad[1].items[0], Some(ItemId(7)));
        assert_eq!(g.pack, Vec::<ItemId>::new(), "the item is worn, not given");
        assert_eq!((g.squad[1].def, g.squad[1].named), (UnitId(5), 1), "replaced by type 3, then named character 1 of type 5");
        assert_eq!((g.squad[0].xp, g.squad[1].xp), (10, 10));
        assert_eq!(g.spells, vec![6]);
        assert_eq!(g.active_spells(), &[ActiveSpell::new(1, None)]);
        assert!(g.has_spells(Holder::Player, None, &[1]) && !g.has_spells(Holder::Player, None, &[1, 2]));
        assert_eq!(g.tile(), (10, 5));
        assert_eq!(g.gold, 100, "the resources are arguments");
        // "No meeting" + a spell lifts it.
        g.remove_army_spell(1);
        assert!(g.active_spells().is_empty());
    }

    #[test]
    fn community_opcodes_change_ai_armies_and_buildings() {
        let mut lasting = op(11, 2, -1, 0);
        lasting.results.spells_learned = [3, 0, 0, 0];
        let events = vec![
            op(7, 2, 0, 5),
            op(8, 2, 8, 0),
            op(9, 2, 4, 0),
            op(10, 2, 0, 2),
            lasting,
            op(12, 2, 1, 1),
            op(13, -1, -1, 1000),
            op(17, 2, 12, 0),
            op(19, 2, 14, 10),
            op(9, -1, 1, 0),
        ];
        let mut s = world(events);
        s.named_characters = vec![crate::dt::dtm::NamedCharacter { unit: 3, name: "Aide".into() }];
        s.armies = vec![army(2, 12, 10, 1, &[troop(4, 0, 1), troop(1, 0, 1)])];
        let mut fort = building(BuildingType::Fort, 9, 2, (1, 1));
        fort.garrison[0] = troop(4, 0, 1);
        s.buildings = vec![fort];
        let mut g = start(&s);
        g.drain_events();
        let a = g.world.armies.iter().find(|a| a.id == 2).unwrap();
        assert_eq!(a.troops.iter().map(|t| t.unit).collect::<Vec<_>>(), vec![UnitId(5), UnitId(3)]);
        assert_eq!(a.speed, Army::speed_for(-3, 5), "5 − (−3) = 8");
        // Its own attitude becomes 2, but the relation (world::relation) is the player's −2
        // towards the enemy group, as in the original.
        assert_eq!((a.faction, a.attitude), (4, -2), "enemy group: hostile whatever its own attitude");
        assert_eq!(a.effects, vec![ActiveSpell::new(3, None)]);
        assert_eq!((a.named, a.model), (1, 12));
        assert_eq!(a.post, (14, 10));
        assert!(!a.path.is_empty(), "it sets off");
        assert!(g.has_spells(Holder::Army(2), None, &[3]));
        assert!(g.army_at(2, 12, 10) && !g.army_at(2, 14, 10));
        let l = &g.world.locations[0];
        let mut level = 1;
        let mut left = 1000;
        while left >= g.content.xp_to_next(UnitId(4), level) {
            left -= g.content.xp_to_next(UnitId(4), level);
            level += 1;
        }
        assert_eq!(l.garrison[0].level, level);
        assert_eq!(l.garrison[0].xp, left, "an AI troop keeps what is left towards the next level");
        assert!(level > 1);
        assert_eq!((l.faction, l.owner), (1, crate::rules::world::Owner::Player), "the fort joins the player");
    }

    #[test]
    fn event_xp_goes_to_the_hero_as_it_is_and_new_levels_are_reported() {
        let mut gift = ev(EventKind::Global);
        gift.results.experience = 100;
        let mut g = start(&world(vec![gift]));
        let events = g.drain_events();
        // The knight of the test content needs 60, then 84: level 2 with 40 left.
        assert_eq!((g.squad[0].level, g.squad[0].xp), (2, 40));
        assert_eq!(g.squad[1].xp, 0, "only the hero");
        assert!(events.iter().any(|e| matches!(e, Event::LevelUp(0, 2))));
        let mut loss = ev(EventKind::Global);
        loss.results.experience = -50;
        let g = start(&world(vec![loss]));
        assert_eq!((g.squad[0].level, g.squad[0].xp), (1, 0), "negative XP does nothing");
    }

    #[test]
    fn level_conditions_count_from_zero() {
        let mut e = ev(EventKind::Global);
        (e.conditions.stats_check, e.conditions.level) = (1, 1);
        e.results.gold = 5;
        let mut g = start(&world(vec![e]));
        assert_eq!(g.gold, 100, "level 1 is the original's level 0");
        g.squad[0].level = 2;
        g.wait(1);
        assert_eq!(g.gold, 105, "level 2 passes a level-1 condition");
    }

    #[test]
    fn the_next_map_starts_with_what_carries_over() {
        let mut g = start(&world(vec![]));
        g.squad[0].level = 4;
        g.squad[0].xp = 33;
        g.squad[1].level = 3;
        let next = NextMap {
            name: "Road".into(),
            branch: None,
            gold: Some(70),
            mana: Some(9),
            fame: false,
            hero: Some((g.squad[0].level, g.squad[0].xp)),
            spells: Some(vec![2, 5]),
            hero_items: Some([Some(ItemId(7)), None, None, None]),
            inventory: vec![ItemId(7)],
            army: vec![Unit { unpaid: true, last_paid: 0, ..g.squad[1].clone() }],
            flags: Vec::new(),
            class: HeroClass::Knight,
            hero_name: None,
            hero_unit: g.squad[0].def,
            journal: crate::rules::journal::History::default(),
        };
        let mut fresh = start(&world(vec![]));
        fresh.squad.truncate(1);
        let gold = fresh.gold;
        fresh.apply_carry_over(&next);
        assert_eq!((fresh.squad[0].level, fresh.squad[0].xp), (4, 33));
        assert_eq!(fresh.spells, vec![2, 5], "byte 3 keeps the spell book");
        assert_eq!(fresh.squad[0].items[0], Some(ItemId(7)), "byte 4: the hero's worn items");
        let now = fresh.clock.total_minutes() as u64;
        assert!(!fresh.squad[1].unpaid && fresh.squad[1].last_paid == now, "the army comes paid");
        assert_eq!((fresh.gold, fresh.mana), (gold + 70, 9));
        assert_eq!(fresh.squad.len(), 2);
        assert_eq!(fresh.squad[1].level, 3, "the army keeps its levels");
        assert_eq!(fresh.pack, vec![ItemId(7)]);
        // Without the flag the hero starts over at level 1.
        let mut again = start(&world(vec![]));
        again.squad[0].level = 5;
        let book = again.spells.clone();
        again.apply_carry_over(&NextMap { hero: None, spells: None, hero_items: None, army: Vec::new(), inventory: Vec::new(), gold: None, mana: None, ..next });
        assert_eq!((again.squad[0].level, again.squad[0].xp), (1, 0));
        assert_eq!((again.spells.clone(), again.squad[0].items), (book, [None; 4]));
    }

    /// РК2's mines: a fort's own event asks for the peasants once the fort is the player's.
    /// Beating its garrison takes it and enters it, so the event is checked then.
    #[test]
    fn a_building_taken_from_its_garrison_runs_its_own_events() {
        let mut mine = ev(EventKind::Local);
        let c = &mut mine.conditions;
        (c.buildings_check, c.buildings, c.buildings_owner) = (1, [1, 0, 0], [1, 0, 0]);
        mine.results.gold = 9;
        let mut s = world(vec![mine]);
        let mut fort = building(BuildingType::Fort, 5, 2, (1, 1));
        fort.garrison[0] = troop(4, 0, 1);
        (fort.faction, fort.relations) = (4, [-3, 0, 0, 0]);
        fort.event_slots[0] = 1;
        fort.event_count = 1;
        s.buildings = vec![fort];
        let mut g = start(&s);
        g.drain_events();
        let gold = g.gold;
        assert!(g.set_destination((5, 2)));
        walk(&mut g);
        assert!(matches!(g.foe, Some(Foe::Garrison(_))), "the garrison fights");
        let mut b = g.start_battle();
        b.begin();
        for f in b.fighters.iter_mut().filter(|f| f.team == crate::rules::battle::Team::Enemy) {
            f.hp = 0;
        }
        g.resolve_battle(&b);
        assert_eq!(fired(&g.drain_events()), vec![1]);
        assert!(g.gold >= gold + 9);
    }

    /// РК1 → РК2 → РК3: the next map starts with the carried army and the flags (the
    /// original stashes the flag string with the army, 4b5ef8, and puts it back, 4b5ff8)
    /// before its opening events run. РК2 opens with "the herald died" unless he came along,
    /// and РК3's king rewards the band beaten in РК2 by its flag.
    #[test]
    fn a_campaign_map_starts_with_the_carried_army_and_flags() {
        let mut band = ev(EventKind::Global);
        band.title = "Band%+Band".into();
        band.flags = crate::dt::dtm::FlagScript::from_title(&band.title);
        band.results.units_add = [4, 0, 0, 0];
        band.results.units_add_named = [1, 0, 0, 0];
        band.results.chained_event = 2;
        let mut win = ev(EventKind::Global);
        win.subordinate = 1;
        let mut s = world(vec![band, win]);
        s.header.victory_event = 2;
        s.next_map = "Next.DTm".into();
        s.header.carry_over = [1, 1, 1, 1, 1, 1, 1];
        s.named_characters = vec![crate::dt::dtm::NamedCharacter { unit: 4, name: "Herald".into() }];
        let g = Game::from_scenario(Arc::new(content()), &s, HeroClass::Archmage);
        let next = g.next_map().expect("a victory with a next map");
        assert_eq!(next.flags, vec!["Band1".to_string()]);
        assert_eq!(next.class, HeroClass::Archmage);

        let mut died = ev(EventKind::Global);
        died.conditions.units_check = 1;
        (died.conditions.units, died.conditions.units_named, died.conditions.units_owner) = ([4, 0, 0], [1, 0, 0], [6, 0, 0]);
        let mut reward = ev(EventKind::Global);
        reward.title = "Reward%=Band".into();
        reward.flags = crate::dt::dtm::FlagScript::from_title(&reward.title);
        reward.results.gold = 5;
        let mut s2 = world(vec![died, reward]);
        s2.header.defeat_event = 1;
        s2.named_characters = s.named_characters.clone();
        let mut g2 = Game::from_campaign(Arc::new(content()), &s2, &next);
        assert_eq!(fired(&g2.drain_events()), vec![2], "the herald came along; the band's flag holds");
        assert_eq!(g2.script_end(), None);
        assert!(g2.script().unwrap().flag("Band"));
        assert_eq!(g2.archetype, 2);
        assert!(g2.squad.iter().any(|u| u.named == 1));
    }

    #[test]
    fn next_map_after_a_campaign_victory() {
        let mut branch = op(15, 3, 2, 0);
        branch.results.chained_event = 2;
        let mut win = ev(EventKind::Global);
        win.subordinate = 1;
        let mut s = world(vec![branch, win]);
        s.header.victory_event = 2;
        s.next_map = "0-0 Road".into();
        s.header.carry_over = [1, 0, 0, 1, 0, 1, 1];
        let g = start(&s);
        assert_eq!(g.script_end(), Some(ScriptEnd::Victory(2)));
        let next = g.next_map().unwrap();
        assert_eq!(next.name, "3-2 Road");
        assert_eq!(next.branch, Some((3, 2)));
        assert_eq!((next.gold, next.mana, next.fame), (Some(100), None, false));
        assert_eq!(next.hero, Some((1, 0)));
        assert_eq!(next.army.len(), 1);
        assert!(next.inventory.is_empty() && next.hero_items.is_none());
        assert_eq!(next.spells, Some(g.spells.clone()), "byte 3: the book goes with the level");

        // No branch: the scenario's next map; none before a victory.
        let mut win = ev(EventKind::Global);
        win.start_time = 624_354_300 + 120;
        let mut s = world(vec![win]);
        s.header.victory_event = 1;
        s.next_map = "Road".into();
        let mut g = start(&s);
        assert_eq!(g.next_map(), None);
        g.wait(4);
        assert_eq!(g.next_map().map(|n| (n.name, n.gold, n.army.len())), Some(("Road".to_string(), None, 0)));
        assert_eq!(branch_name("Road", (4, 1)), "4-1");
        assert_eq!(branch_name("12-3.DTm", (4, 1)), "4-1.DTm");
    }
    #[test]
    fn hero_name_escapes() {
        let g = start(&world(vec![]));
        let name = g.hero().name(&g.content).to_string();
        assert_eq!(g.fill_text("Hail, #HERONAME!\r\n"), format!("Hail, {name}!\n"));
        let mut g = g;
        g.set_hero_name("  Ivo ");
        assert_eq!(g.fill_text("#HERONAME the #HEROCLASS"), format!("Ivo the {name}"));
        g.set_hero_name(" ");
        assert_eq!(g.hero_name(), name, "an empty name is the class's");
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
            let mut g = Game::from_scenario(c.clone(), &s, class);
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
