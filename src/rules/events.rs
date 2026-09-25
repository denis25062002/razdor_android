//! Scenario events and quests: the engine of the editor manual (`docs/reference/mechanics.md` §6,
//! record layout in `docs/reference/dtm-format.md` §9).
//!
//! Pure rules. The engine owns only the script state (what happened, flags, the journal, a
//! pending question); everything about the world goes through [`EventWorld`], which the game
//! implements. Texts are never copied into outcomes: an outcome names the event, and the UI
//! reads the title, question or message from the loaded scenario.
//!
//! Choices where the sources are silent are marked *(guess)*; they are listed in
//! `mechanics.md` §8.

use crate::dt::dtm::{Event, EventKind, Scenario};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

/// 1-based event id, in file order.
pub type EventId = u16;
/// 1-based army id.
pub type ArmyId = u8;

/// Owner / side codes of the editor: 1 player, 2–5 green, blue, yellow, red.
pub const SIDE_PLAYER: u8 = 1;
/// Condition owner code "not the player" (anyone else, or nobody).
pub const OWNER_NOT_PLAYER: u8 = 6;
/// Picture codes of byte 82.
pub const PICTURE_DEFEAT: u8 = 200;
pub const PICTURE_VICTORY: u8 = 201;

/// At most this many events fire in one [`EventEngine::tick`] (or answer, or rumour); more
/// means a script loop.
pub const LOOP_GUARD: usize = 256;
/// Chains of subordinate events deeper than this are cut.
const CHAIN_DEPTH: usize = 32;
/// Community opcode 18: the random flag is this name plus one character.
pub const RANDOM_FLAG: &str = "RAND";

/// A flag opcode 18 made: `RAND` and one character.
fn is_random_flag(f: &str) -> bool {
    f.strip_prefix(RANDOM_FLAG).is_some_and(|rest| rest.chars().count() == 1)
}

/// Where the player stands: in a building (1-based index in the scenario) or on an event point
/// (its point id).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Place {
    Building(u16),
    Point(u8),
}

/// Which unit an event removes from the player's army.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitPick {
    /// A unit of this type.
    Type(u8),
    /// 0xFE: a unit that an event added (the last one to join leaves first).
    AddedByEvent,
    /// 0xFF: any unit (the last one to join leaves first).
    Any,
}

/// The answer to an event's yes/no question. Events without a question count as `Yes`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    Yes,
    No,
}

/// A Community Update extension found in an event (mechanics.md §6, §8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Extension {
    /// "No meeting" + patrol value 1–20 (event editing, AI armies, teleports, …); the
    /// resource fields (and some condition fields) are its arguments.
    Opcode(u8),
    /// "No meeting" + a spell: remove that spell from the player instead of casting it.
    RemoveSpell,
    /// "No meeting" + named squads: also check the named unit's class.
    NamedUnitClass,
}

/// The target of a Community opcode's first argument: the player's army (0), a scenario army
/// (its id, 1–255) or a building (a negative number: minus its 1-based index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Holder {
    Player,
    Army(ArmyId),
    Building(u16),
}

impl Holder {
    pub fn from_code(v: i16) -> Option<Holder> {
        match v {
            0 => Some(Holder::Player),
            1..=255 => Some(Holder::Army(v as u8)),
            v if v < 0 => Some(Holder::Building(v.unsigned_abs())),
            _ => None,
        }
    }
}

/// Opcode 8's speed code as the editor's speed correction: 1 → +5 … 5 → +1, 6 → −1 … 8 → −3
/// and slower beyond; 0 → 0 *(guess: the guide gives only 1 → +5 and 8 → −3, so the code
/// skips 0)*.
pub fn speed_correction(code: i16) -> i8 {
    match code {
        ..=0 => 0,
        1..=5 => (6 - code) as i8,
        c => (5 - c as i32).max(-100) as i8,
    }
}

/// One "event editing" setting of opcodes 1–5: what to do (1 add, 2 set, 3–5 compare), to
/// which event (relative to the current one), which field (its byte offset in the record)
/// and the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventEdit {
    pub action: i16,
    pub shift: i16,
    pub field: i16,
    pub value: i16,
}

/// The event editing settings of an opcode 1–5 event: the first from the patrol value and
/// the resources (XP = shift, gold = field, mana = value), the second, when its action (the
/// squad count condition) is 1–5, from the conditions (gold = shift, level = field,
/// holiness and mana = value).
pub fn event_edits(e: &Event) -> Vec<EventEdit> {
    let Some(Extension::Opcode(op @ 1..=5)) = extension(e) else { return Vec::new() };
    let (r, c) = (&e.results, &e.conditions);
    let mut v = vec![EventEdit { action: op as i16, shift: r.experience, field: r.gold, value: r.mana }];
    if (1..=5).contains(&c.squad_count) {
        v.push(EventEdit { action: c.squad_count, shift: c.gold, field: c.level, value: c.holiness_mana });
    }
    v
}

/// A field of an event record, addressed by its byte offset (dtm-format.md §9).
enum Field<'a> {
    U8(&'a mut u8),
    I8(&'a mut i8),
    U16(&'a mut u16),
    I16(&'a mut i16),
    U32(&'a mut u32),
}

impl Field<'_> {
    fn get(&self) -> i64 {
        match self {
            Field::U8(v) => **v as i64,
            Field::I8(v) => **v as i64,
            Field::U16(v) => **v as i64,
            Field::I16(v) => **v as i64,
            Field::U32(v) => **v as i64,
        }
    }

    /// Sets the value, clamped to the field's range.
    fn set(&mut self, x: i64) {
        match self {
            Field::U8(v) => **v = x.clamp(0, u8::MAX as i64) as u8,
            Field::I8(v) => **v = x.clamp(i8::MIN as i64, i8::MAX as i64) as i8,
            Field::U16(v) => **v = x.clamp(0, u16::MAX as i64) as u16,
            Field::I16(v) => **v = x.clamp(i16::MIN as i64, i16::MAX as i64) as i16,
            Field::U32(v) => **v = x.clamp(0, u32::MAX as i64) as u32,
        }
    }
}

/// The field starting at byte `off` of the event record; `None` inside a multi-byte field,
/// for unknown bytes and for the texts.
fn field(e: &mut Event, off: u16) -> Option<Field<'_>> {
    use Field::*;
    let (c, r) = (&mut e.conditions, &mut e.results);
    let o = off as usize;
    Some(match off {
        0 => U8(&mut e.group_colour),
        1 => U8(&mut e.kind),
        2 => U32(&mut e.start_time),
        6 => U16(&mut e.repeat),
        8 => U16(&mut e.duration),
        10 => U8(&mut e.archetype),
        11 => I16(&mut c.squad_count),
        13 => I16(&mut c.army_strength),
        15 => U8(&mut c.army_inactive),
        16 => U8(&mut r.patrol_army),
        17 => I8(&mut r.patrol_delta),
        18 => U8(&mut c.stats_check),
        19 => I16(&mut c.level),
        21 => I16(&mut c.gold),
        25 => I16(&mut c.holiness_mana),
        29 => U8(&mut c.buildings_check),
        30..=32 => U8(&mut c.buildings[o - 30]),
        33..=35 => U8(&mut c.buildings_owner[o - 33]),
        36 => U8(&mut c.units_check),
        37..=39 => U8(&mut c.units[o - 37]),
        40..=42 => U8(&mut c.units_named[o - 40]),
        43..=45 => U8(&mut c.units_owner[o - 43]),
        46 => U8(&mut c.artifacts_check),
        47..=49 => U8(&mut c.artifacts[o - 47]),
        50..=52 => U8(&mut c.artifacts_owner[o - 50]),
        53 => U8(&mut c.defeated_check),
        54..=55 => U8(&mut c.defeated_armies[o - 54]),
        56 => U8(&mut c.happened_yes_check),
        57 | 59 => U16(&mut c.happened_yes[(o - 57) / 2]),
        61 => U8(&mut c.not_happened_check),
        62 | 64 => U16(&mut c.not_happened[(o - 62) / 2]),
        66 => U8(&mut c.beaten_check),
        67..=68 => U8(&mut c.beaten_armies[o - 67]),
        69 => U8(&mut c.happened_no_check),
        70 | 72 => U16(&mut c.happened_no[(o - 70) / 2]),
        74 => U8(&mut c.meet_army),
        75 => U8(&mut c.army_active),
        76 => U8(&mut c.confirm_question),
        77 => U16(&mut r.relative_event),
        79 => U16(&mut r.relative_delay_hours),
        81 => U8(&mut r.cast_spell),
        82 => U8(&mut r.picture),
        83 => I16(&mut r.experience),
        85 => I16(&mut r.gold),
        89 => I16(&mut r.mana),
        93..=96 => U8(&mut r.spells_learned[o - 93]),
        97..=100 => U8(&mut r.units_add[o - 97]),
        101..=104 => U8(&mut r.units_add_named[o - 101]),
        105..=108 => U8(&mut r.units_remove[o - 105]),
        109..=112 => U8(&mut r.units_remove_named[o - 109]),
        113..=116 => U8(&mut r.artifacts_add[o - 113]),
        117..=120 => U8(&mut r.artifacts_remove[o - 117]),
        121..=122 => U8(&mut r.activate_armies[o - 121]),
        123 => U8(&mut r.deactivate_army),
        124 => U16(&mut r.completes_quest),
        126 => U16(&mut r.delay_hours),
        128 | 130 | 132 | 134 => U16(&mut r.light_lanterns[(o - 128) / 2]),
        136 => U8(&mut r.removed_units_to_army),
        137 => U8(&mut r.new_hero_class),
        138 => U16(&mut r.chained_event),
        140 => U8(&mut e.subordinate),
        141 => U8(&mut e.once),
        142 => U8(&mut r.units_from_army),
        143 => U8(&mut r.move_to_hero),
        144 => U8(&mut r.show_army),
        145 => U8(&mut r.hero_one_hp),
        146 => U8(&mut c.army_at_home),
        147 => U8(&mut r.start_battle_with),
        148 => U8(&mut r.no_meeting),
        149 => U8(&mut r.repeat_after_yes),
        _ => return None,
    })
}

/// The value of the field at byte `off` of an event record (see [`EventEngine::event_field`]).
pub fn event_field(e: &Event, off: u16) -> Option<i64> {
    field(&mut e.clone(), off).map(|f| f.get())
}

/// What the game has to show or do after the engine ran. World effects (gold, units, armies,
/// …) have already been applied through [`EventWorld`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventOutcome {
    /// The event fired. `message`: it has a message to show (otherwise it fired silently).
    Fired { event: EventId, message: bool },
    /// The event asks its yes/no question; call [`EventEngine::answer`]. Nothing else runs
    /// until then.
    Question(EventId),
    /// The player answered No; the event did not take effect.
    Declined(EventId),
    QuestAdded(EventId),
    QuestCompleted(EventId),
    /// The scenario's victory or defeat event fired; the engine stops.
    Victory(EventId),
    Defeat(EventId),
    /// [`LOOP_GUARD`] events fired in one run; the rest waits for the next tick.
    LoopGuard,
}

/// The game as the events see it. Owners and holders are side codes (1 player, 2–5 factions);
/// `None` is nobody (neutral, dead, lost).
pub trait EventWorld {
    // Queries.
    /// Game time in minutes since year 0 (the scale of the event start times).
    fn now(&self) -> u64;
    /// 1 knight, 2 archmage, 3 ranger.
    fn hero_archetype(&self) -> u8;
    fn hero_level(&self) -> i64;
    fn gold(&self) -> i64;
    /// Holiness and mana.
    fn mana(&self) -> i64;
    /// Squads in the player's army.
    fn squad_count(&self) -> i64;
    fn army_strength(&self) -> i64;
    /// `building` is the 1-based index of the scenario's building.
    fn building_owner(&self, building: u16) -> Option<u8>;
    /// Who has the named character `named` (of unit type `unit`) in an army.
    fn named_unit_holder(&self, unit: u8, named: u8) -> Option<u8>;
    fn artifact_holder(&self, artifact: u8) -> Option<u8>;
    /// The player beat this army.
    fn player_defeated(&self, army: ArmyId) -> bool;
    /// Anyone beat this army.
    fn army_beaten(&self, army: ArmyId) -> bool;
    fn army_active(&self, army: ArmyId) -> bool;
    fn army_at_home(&self, army: ArmyId) -> bool;
    /// The player met this army and the meeting has not been cleared.
    fn met_army(&self, army: ArmyId) -> bool;
    fn place(&self) -> Option<Place>;

    // Effects.
    fn add_experience(&mut self, xp: i64);
    fn add_gold(&mut self, gold: i64);
    fn add_mana(&mut self, mana: i64);
    /// `named` 0 = an ordinary unit; `from_army` is the army it leaves to join.
    fn add_unit(&mut self, unit: u8, named: u8, from_army: Option<ArmyId>);
    /// `to_army` is the army the unit joins.
    fn remove_unit(&mut self, pick: UnitPick, named: u8, to_army: Option<ArmyId>);
    fn give_item(&mut self, artifact: u8);
    fn take_item(&mut self, artifact: u8);
    fn learn_spell(&mut self, spell: u8);
    /// Cast a spell on the player's army.
    fn apply_spell(&mut self, spell: u8);
    fn activate_army(&mut self, army: ArmyId);
    fn deactivate_army(&mut self, army: ArmyId);
    fn show_army(&mut self, army: ArmyId);
    fn move_army_to_hero(&mut self, army: ArmyId);
    /// Clear the meeting with this army ("no meeting with army").
    fn forget_meeting(&mut self, army: ArmyId);
    /// Light a lantern (point id): reveal its area.
    fn light_lantern(&mut self, point: u16);
    fn change_patrol(&mut self, army: ArmyId, delta: i8);
    /// The hero becomes this unit type; class bonuses are lost.
    fn set_hero_class(&mut self, unit: u8);
    fn start_battle(&mut self, army: ArmyId);
    fn delay_player(&mut self, minutes: u64);
    fn hero_to_one_hp(&mut self);

    // Community Update extensions. `unit` is a member of the holder's army in joining order
    // (0 = its leader, the hero for the player); `None` means everyone.
    /// "No meeting" + a spell: lift that lasting spell from the player's army.
    fn remove_army_spell(&mut self, spell: u8);
    /// Opcode 6: the unit wears exactly `items` (0 = an empty slot), fitting or not.
    fn equip_unit(&mut self, holder: Holder, unit: u8, items: [u8; 4]);
    /// Opcode 7: the unit becomes unit type `with`.
    fn replace_unit(&mut self, holder: Holder, unit: u8, with: u8);
    /// Opcode 8: the army's speed correction (the editor's −3..5).
    fn set_army_speed(&mut self, holder: Holder, correction: i8);
    /// Opcode 9: the army or building joins group 1 player, 2 ally, 3 neighbour, 4 enemy.
    fn set_faction(&mut self, holder: Holder, group: u8);
    /// Opcode 10: its relation (−3..3) towards group 0 player, 1 ally, 2 neighbour, 3 enemy.
    fn set_relation(&mut self, holder: Holder, group: u8, value: i8);
    /// Opcode 11: these spells last for good on the unit(s), replacing the lasting ones.
    fn set_spells(&mut self, holder: Holder, unit: Option<u8>, spells: &[u8]);
    /// Opcode 12: the unit becomes the scenario's named character `named` (1-based) of unit
    /// type `class` (0: keep its type).
    fn set_named_unit(&mut self, holder: Holder, unit: u8, named: u8, class: u8);
    /// Opcode 13: experience for the unit(s).
    fn give_unit_xp(&mut self, holder: Holder, unit: Option<u8>, xp: i64);
    /// Opcode 14: all these spells last on the unit(s).
    fn has_spells(&self, holder: Holder, unit: Option<u8>, spells: &[u8]) -> bool;
    /// Opcode 16: the spell leaves the hero's spell book.
    fn forget_spell(&mut self, spell: u8);
    /// Opcode 17: the army's figure on the map (model 0–12).
    fn set_army_model(&mut self, holder: Holder, model: u8);
    /// Opcode 18: a random number in `lo..=hi`.
    fn random(&mut self, lo: i64, hi: i64) -> i64;
    /// Opcode 19: the AI army heads for cell (x, y).
    fn set_army_target(&mut self, army: ArmyId, x: i32, y: i32);
    /// Opcode 19 (condition): the army stands on cell (x, y).
    fn army_at(&self, army: ArmyId, x: i32, y: i32) -> bool;
    /// Opcode 20: the player's army moves to cell (x, y) and looks around.
    fn teleport_player(&mut self, x: i32, y: i32);
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct EventState {
    /// The last answer (`No` only after a No; opening the question clears it).
    answer: Option<Answer>,
    /// Times the event fired, a No included (a No counts as happened).
    times: u32,
    /// Start time set by another event's "relative event" result.
    start: Option<u64>,
    /// Game minute it last fired (or was answered No).
    #[serde(default)]
    last_fired: Option<u64>,
}

/// The script state of a scenario: see the module docs. A save keeps the state only; the
/// events and places come from the scenario again ([`EventEngine::restore_statics`]).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventEngine {
    #[serde(skip)]
    events: Vec<Event>,
    state: Vec<EventState>,
    #[serde(skip)]
    places: HashMap<Place, Vec<EventId>>,
    flags: BTreeSet<String>,
    journal: Vec<EventId>,
    completed: Vec<EventId>,
    #[serde(skip)]
    victory: EventId,
    #[serde(skip)]
    defeat: EventId,
    pending: Option<EventId>,
    last_place: Option<Place>,
    /// The hero just entered the building he stands in: its events are checked in this run
    /// only (the original does not check them again while the building's window is open).
    #[serde(default)]
    fresh_visit: bool,
    ended: Option<EventOutcome>,
    #[serde(skip)]
    extensions: Vec<(EventId, Extension)>,
    /// Community opcodes 1–2: fields of events changed by other events (event, byte offset,
    /// value), put back into the events when a save is loaded.
    #[serde(default)]
    edits: Vec<(EventId, u16, i64)>,
    /// Community opcode 15: the campaign branch chosen (map number, variant).
    #[serde(default)]
    branch: Option<(i16, i16)>,
    /// The unit type of each named character (1-based), for opcode 12.
    #[serde(skip)]
    named_units: Vec<u8>,
    /// The scenario's next map (campaigns) and what carries over to it (header 0x110).
    #[serde(skip)]
    next_map: String,
    #[serde(skip)]
    carry_over: [u8; 7],
}

/// Minutes in a day: an event's repeat counts in whole days.
const DAY: u64 = 1440;
/// A duration-0 event fires again at the earliest this many minutes after it fired.
const REFIRE_MINUTES: u64 = 60;

/// Whether the time window is open at `now` (economy.md §6): never before `start`; with a
/// repeat of R minutes, on every (R / 1440)-th day since the start, for `max(duration, 1)`
/// hours from the start's time of day; without one, until `start + max(duration, 1)` hours,
/// or with no end for duration 0 while the event never fired.
fn window_open(start: u64, repeat: u64, duration_hours: u64, fired: bool, now: u64) -> bool {
    let Some(t) = now.checked_sub(start) else { return false };
    let length = duration_hours.max(1) * 60;
    if repeat > 0 {
        let every = (repeat / DAY).max(1);
        let k = t / DAY;
        k.is_multiple_of(every) && t - k * DAY <= length
    } else {
        (duration_hours == 0 && !fired) || t <= length
    }
}

/// The sign encodes ≥ (positive) or ≤ (negative); 0 disables the check.
fn compare(value: i64, threshold: i16) -> bool {
    match threshold {
        0 => true,
        t if t > 0 => value >= t as i64,
        t => value <= -(t as i64),
    }
}

fn owner_matches(code: u8, holder: Option<u8>) -> bool {
    match code {
        OWNER_NOT_PLAYER => holder != Some(SIDE_PLAYER),
        // 0 is the first entry of the editor's list, the player *(guess)*.
        0 => holder == Some(SIDE_PLAYER),
        c => holder == Some(c),
    }
}

fn nonzero<T: Copy + Default + PartialEq>(v: &[T]) -> impl Iterator<Item = T> + '_ {
    v.iter().copied().filter(|x| *x != T::default())
}

/// The Community extension an event uses, if any.
pub fn extension(e: &Event) -> Option<Extension> {
    let r = &e.results;
    if r.no_meeting == 0 {
        return None;
    }
    if (1..=20).contains(&r.patrol_delta) {
        Some(Extension::Opcode(r.patrol_delta as u8))
    } else if r.cast_spell != 0 {
        Some(Extension::RemoveSpell)
    } else if e.conditions.units_check != 0 {
        Some(Extension::NamedUnitClass)
    } else {
        None
    }
}

impl EventEngine {
    /// The engine for a scenario: its events, the local events of its buildings and points,
    /// and its victory and defeat events.
    pub fn new(s: &Scenario) -> EventEngine {
        let mut places = Vec::new();
        for (i, b) in s.buildings.iter().enumerate() {
            places.push((Place::Building(i as u16 + 1), b.events().collect()));
        }
        for p in &s.points {
            places.push((Place::Point(p.id), p.events().collect()));
        }
        let mut g = EventEngine::from_parts(s.events.clone(), places, s.header.victory_event, s.header.defeat_event);
        g.named_units = s.named_characters.iter().map(|n| n.unit).collect();
        g.next_map = s.next_map.clone();
        g.carry_over = s.header.carry_over;
        g
    }

    /// The engine for hand-made events. `places` lists the local events of each place.
    pub fn from_parts(
        events: Vec<Event>,
        places: Vec<(Place, Vec<EventId>)>,
        victory: EventId,
        defeat: EventId,
    ) -> EventEngine {
        let n = events.len();
        let mut map: HashMap<Place, Vec<EventId>> = HashMap::new();
        for (p, ids) in places {
            let list = map.entry(p).or_default();
            for id in ids {
                if (1..=n).contains(&(id as usize)) && !list.contains(&id) {
                    list.push(id);
                }
            }
        }
        let extensions = Self::extensions_of(&events);
        EventEngine {
            state: vec![EventState::default(); n],
            events,
            places: map,
            flags: BTreeSet::new(),
            journal: Vec::new(),
            completed: Vec::new(),
            victory,
            defeat,
            pending: None,
            last_place: None,
            fresh_visit: false,
            ended: None,
            extensions,
            edits: Vec::new(),
            branch: None,
            named_units: Vec::new(),
            next_map: String::new(),
            carry_over: [0; 7],
        }
    }

    /// Unit types of the scenario's named characters, in order (hand-made engines).
    pub fn set_named_units(&mut self, units: Vec<u8>) {
        self.named_units = units;
    }

    /// The next map of a campaign, as the scenario names it (may be empty).
    pub fn next_map_name(&self) -> &str {
        &self.next_map
    }

    /// Sets the scenario's next map and carry-over flags (hand-made engines).
    pub fn set_next_map(&mut self, name: &str, carry_over: [u8; 7]) {
        self.next_map = name.to_string();
        self.carry_over = carry_over;
    }

    /// What carries over to the next map (header 0x110, in UI order: gold, gods' favour,
    /// fame, experience/level, personal artifacts, whole inventory, whole army).
    pub fn carry_over(&self) -> [u8; 7] {
        self.carry_over
    }

    /// The campaign branch an opcode 15 event chose: (map number, variant).
    pub fn campaign_branch(&self) -> Option<(i16, i16)> {
        self.branch
    }

    /// The value of the field at byte `off` of event `id` as it stands now (Community
    /// opcodes 1–5 read and change these).
    pub fn event_field(&self, id: EventId, off: u16) -> Option<i64> {
        event_field(self.event(id)?, off)
    }

    /// Sets the field at byte `off` of event `id` (clamped to its range) and records it for
    /// saves.
    fn set_event_field(&mut self, id: EventId, off: u16, v: i64) {
        let Some(e) = (id as usize).checked_sub(1).and_then(|i| self.events.get_mut(i)) else { return };
        let Some(mut f) = field(e, off) else { return };
        f.set(v);
        let now = f.get();
        match self.edits.iter_mut().find(|(i, o, _)| (*i, *o) == (id, off)) {
            Some(edit) => edit.2 = now,
            None => self.edits.push((id, off, now)),
        }
        self.extensions = Self::extensions_of(&self.events);
    }

    fn extensions_of(events: &[Event]) -> Vec<(EventId, Extension)> {
        events.iter().enumerate().filter_map(|(i, e)| Some((i as EventId + 1, extension(e)?))).collect()
    }

    /// Puts back what a save leaves out (the events, places, victory and defeat events)
    /// from `fresh`, the engine of the same scenario. Fails if the event count differs.
    pub fn restore_statics(&mut self, fresh: EventEngine) -> Result<(), String> {
        if self.state.len() != fresh.events.len() {
            return Err(format!("{} events saved, the map has {}", self.state.len(), fresh.events.len()));
        }
        self.events = fresh.events;
        self.places = fresh.places;
        self.victory = fresh.victory;
        self.defeat = fresh.defeat;
        self.named_units = fresh.named_units;
        self.next_map = fresh.next_map;
        self.carry_over = fresh.carry_over;
        for (id, off, v) in std::mem::take(&mut self.edits) {
            self.set_event_field(id, off, v);
        }
        self.extensions = Self::extensions_of(&self.events);
        Ok(())
    }

    // ---------------------------------------------------------------------------------------
    // State
    // ---------------------------------------------------------------------------------------

    pub fn event(&self, id: EventId) -> Option<&Event> {
        (id as usize).checked_sub(1).and_then(|i| self.events.get(i))
    }

    fn st(&self, id: EventId) -> &EventState {
        &self.state[id as usize - 1]
    }

    fn st_mut(&mut self, id: EventId) -> &mut EventState {
        &mut self.state[id as usize - 1]
    }

    /// Whether the event happened, and how: `No` after a No answer, `Yes` otherwise;
    /// `None` if it never fired (a No counts as happened).
    pub fn happened(&self, id: EventId) -> Option<Answer> {
        self.event(id)?;
        let st = self.st(id);
        match (st.times, st.answer) {
            (0, _) => None,
            (_, Some(Answer::No)) => Some(Answer::No),
            _ => Some(Answer::Yes),
        }
    }

    /// How many times the event fired (a No answer included).
    pub fn times_fired(&self, id: EventId) -> u32 {
        self.event(id).map_or(0, |_| self.st(id).times)
    }

    /// Total firings over all events.
    pub fn total_fired(&self) -> u32 {
        self.state.iter().map(|s| s.times).sum()
    }

    /// Whether flag `name` is set: exactly (`X2`, `RAND5`), or as a counter `name` + digit.
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name) || self.counter(name).is_some()
    }

    /// The flags as stored: counters carry their digit (`Foo1`).
    pub fn flags(&self) -> impl Iterator<Item = &str> {
        self.flags.iter().map(String::as_str)
    }

    /// The stored flag and value of counter `name` (`name` + one digit).
    fn counter(&self, name: &str) -> Option<(String, u32)> {
        self.flags.iter().find_map(|f| {
            let rest = f.strip_prefix(name)?;
            let mut c = rest.chars();
            let d = c.next()?.to_digit(10)?;
            c.next().is_none().then(|| (f.clone(), d))
        })
    }

    /// `+X`: sets `X1`, or raises the digit of `X` (up to 9).
    fn raise_flag(&mut self, name: &str) {
        let d = match self.counter(name) {
            Some((f, d)) => {
                self.flags.remove(&f);
                (d + 1).min(9)
            }
            None => 1,
        };
        self.flags.insert(format!("{name}{d}"));
    }

    /// `-X`: lowers the digit of `X`, removing it at 0 (a plain flag `X` is removed).
    fn lower_flag(&mut self, name: &str) {
        if let Some((f, d)) = self.counter(name) {
            self.flags.remove(&f);
            if d > 1 {
                self.flags.insert(format!("{name}{}", d - 1));
            }
        } else {
            self.flags.remove(name);
        }
    }

    /// Active quests (quest events that fired and are not completed), in the order received.
    pub fn journal(&self) -> &[EventId] {
        &self.journal
    }

    pub fn completed_quests(&self) -> &[EventId] {
        &self.completed
    }

    /// The event whose question waits for [`EventEngine::answer`].
    pub fn pending_question(&self) -> Option<EventId> {
        self.pending
    }

    /// `Victory` or `Defeat` once the scenario is over.
    pub fn ended(&self) -> Option<&EventOutcome> {
        self.ended.as_ref()
    }

    /// Community extensions in the scenario.
    pub fn extensions(&self) -> &[(EventId, Extension)] {
        &self.extensions
    }

    /// The start of an event's window: its own, or the one another event set.
    pub fn start_time(&self, id: EventId) -> Option<u64> {
        let e = self.event(id)?;
        Some(self.st(id).start.unwrap_or(e.start_time as u64))
    }

    // ---------------------------------------------------------------------------------------
    // Entry points
    // ---------------------------------------------------------------------------------------

    /// Run the events: fire the first eligible one, repeat until none fires. Call it whenever
    /// time passes or the player arrives somewhere. Rumours are not fired here (see
    /// [`EventEngine::rumours`]).
    pub fn tick(&mut self, w: &mut dyn EventWorld) -> Vec<EventOutcome> {
        let mut out = Vec::new();
        let place = w.place();
        if place != self.last_place {
            self.last_place = place;
            if let Some(p) = place {
                self.visit(p);
            }
        }
        self.run(w, &mut out);
        out
    }

    /// The hero enters building `place` (again): its events are checked in the next run.
    pub fn visit(&mut self, place: Place) {
        self.fresh_visit = matches!(place, Place::Building(_));
    }

    /// Answer the pending question, then run the events on.
    pub fn answer(&mut self, w: &mut dyn EventWorld, yes: bool) -> Vec<EventOutcome> {
        let mut out = Vec::new();
        if let Some(id) = self.pending.take() {
            self.fire(id, if yes { Answer::Yes } else { Answer::No }, w, &mut out, 0);
            self.run(w, &mut out);
        }
        out
    }

    /// Rumours the player can ask about where he stands.
    pub fn rumours(&self, w: &dyn EventWorld) -> Vec<EventId> {
        let Some(place) = w.place() else { return Vec::new() };
        let now = w.now();
        let ids = self.places.get(&place).map(Vec::as_slice).unwrap_or_default();
        ids.iter()
            .copied()
            .filter(|&id| {
                let e = &self.events[id as usize - 1];
                e.kind() == Some(EventKind::Rumour)
                    && !self.done(id)
                    && (e.subordinate != 0 || self.is_open(id, now))
                    && self.conditions_hold(id, w)
            })
            .collect()
    }

    /// The player picks a rumour: it fires (or asks its question), then the events run on.
    pub fn hear_rumour(&mut self, w: &mut dyn EventWorld, id: EventId) -> Vec<EventOutcome> {
        let mut out = Vec::new();
        if self.pending.is_none() && self.ended.is_none() && self.rumours(w).contains(&id) {
            self.start(id, w, &mut out, 0);
            self.run(w, &mut out);
        }
        out
    }

    // ---------------------------------------------------------------------------------------
    // The loop
    // ---------------------------------------------------------------------------------------

    fn run(&mut self, w: &mut dyn EventWorld, out: &mut Vec<EventOutcome>) {
        let mut fired = 0;
        while self.pending.is_none() && self.ended.is_none() {
            let Some(id) = self.first_eligible(w) else { break };
            if fired == LOOP_GUARD {
                out.push(EventOutcome::LoopGuard);
                break;
            }
            fired += 1;
            self.start(id, w, out, 0);
        }
        if self.pending.is_none() {
            self.fresh_visit = false;
        }
    }

    /// Done for good: a once-event that fired (a No uses it up too).
    fn done(&self, id: EventId) -> bool {
        self.events[id as usize - 1].fires_once() && self.st(id).times > 0
    }

    /// The time window is open at `now` ([`window_open`]).
    fn is_open(&self, id: EventId, now: u64) -> bool {
        let e = &self.events[id as usize - 1];
        let st = self.st(id);
        let start = st.start.unwrap_or(e.start_time as u64);
        window_open(start, e.repeat as u64, e.duration as u64, st.times > 0, now)
    }

    /// The firing guard: not again in the same minute; a duration-0 event not within
    /// [`REFIRE_MINUTES`] of its last firing.
    fn may_refire(&self, id: EventId, now: u64) -> bool {
        match self.st(id).last_fired {
            None => true,
            Some(t) if self.events[id as usize - 1].duration == 0 => now >= t + REFIRE_MINUTES,
            Some(t) => now > t,
        }
    }

    /// The events the loop checks where the player stands, in the original's order: the
    /// global events in file order, then the local events and quests of the event point or
    /// building he stands on, in its list order (a building's only on entering it). Rumours
    /// are never checked (the player picks them); subordinate events only through a chain.
    fn candidates(&self, place: Option<Place>) -> Vec<EventId> {
        let own = |id: &EventId| self.events[*id as usize - 1].subordinate == 0;
        let mut ids: Vec<EventId> = (1..=self.events.len() as EventId)
            .filter(|id| own(id) && self.events[*id as usize - 1].kind() == Some(EventKind::Global))
            .collect();
        let here = match place {
            Some(Place::Building(_)) if !self.fresh_visit => None,
            p => p.and_then(|p| self.places.get(&p)),
        };
        for &id in here.into_iter().flatten() {
            if own(&id) && matches!(self.events[id as usize - 1].kind(), Some(EventKind::Local | EventKind::Quest)) {
                ids.push(id);
            }
        }
        ids
    }

    /// The first event that may fire: in scope, not done, its window open, past its firing
    /// guard, its conditions holding. An event without "once" fires again on every later
    /// check while all that holds.
    fn first_eligible(&mut self, w: &dyn EventWorld) -> Option<EventId> {
        let now = w.now();
        self.candidates(w.place())
            .into_iter()
            .find(|&id| !self.done(id) && self.is_open(id, now) && self.may_refire(id, now) && self.conditions_hold(id, w))
    }

    /// Fire an event, or ask its question first.
    fn start(&mut self, id: EventId, w: &mut dyn EventWorld, out: &mut Vec<EventOutcome>, depth: usize) {
        let e = &self.events[id as usize - 1];
        let asked_yes = self.st(id).answer == Some(Answer::Yes) && e.results.repeat_after_yes == 0;
        if e.conditions.confirm_question != 0 && !asked_yes {
            // Opening the question clears the last answer.
            self.st_mut(id).answer = None;
            self.pending = Some(id);
            out.push(EventOutcome::Question(id));
        } else {
            self.fire(id, Answer::Yes, w, out, depth);
        }
    }

    fn fire(&mut self, id: EventId, answer: Answer, w: &mut dyn EventWorld, out: &mut Vec<EventOutcome>, depth: usize) {
        let now = w.now();
        let st = self.st_mut(id);
        st.answer = Some(answer);
        // A No counts as happened too: it uses up a once-event and starts the guard.
        st.times += 1;
        st.last_fired = Some(now);
        if answer == Answer::No {
            out.push(EventOutcome::Declined(id));
            return;
        }
        let e = self.events[id as usize - 1].clone();
        out.push(EventOutcome::Fired { event: id, message: !e.message.is_empty() });
        if let Some(f) = &e.flags {
            if let Some(x) = &f.set {
                self.raise_flag(x);
            }
            if let Some(x) = &f.clear {
                self.lower_flag(x);
            }
        }
        self.apply(id, &e, w);
        if e.kind() == Some(EventKind::Quest) && !self.journal.contains(&id) && !self.completed.contains(&id) {
            self.journal.push(id);
            out.push(EventOutcome::QuestAdded(id));
        }
        let q = e.results.completes_quest;
        if q != 0 && !self.completed.contains(&q) {
            self.journal.retain(|j| *j != q);
            self.completed.push(q);
            out.push(EventOutcome::QuestCompleted(q));
        }
        let rel = e.results.relative_event;
        if self.event(rel).is_some() {
            self.st_mut(rel).start = Some(now + e.results.relative_delay_hours as u64 * 60);
        }
        if id == self.victory || id == self.defeat {
            let end = if id == self.victory { EventOutcome::Victory(id) } else { EventOutcome::Defeat(id) };
            out.push(end.clone());
            self.ended = Some(end);
            return;
        }
        let next = e.results.chained_event;
        if depth < CHAIN_DEPTH && self.event(next).is_some() && !self.done(next) {
            // A chained event ignores its window and place, but not its conditions.
            if self.conditions_hold(next, w) {
                self.start(next, w, out, depth + 1);
            }
        }
    }

    /// The world effects of an event's results.
    fn apply(&mut self, id: EventId, e: &Event, w: &mut dyn EventWorld) {
        let r = &e.results;
        let ext = extension(e);
        let opcode = matches!(ext, Some(Extension::Opcode(_)));
        // Opcodes 6, 11, 14 and 16 take their items or spells from these lists.
        let op = match ext {
            Some(Extension::Opcode(op)) => op,
            _ => 0,
        };
        if !opcode {
            // Opcodes use the resource fields as their arguments.
            if r.experience != 0 {
                w.add_experience(r.experience as i64);
            }
            if r.gold != 0 {
                w.add_gold(r.gold as i64);
            }
            if r.mana != 0 {
                w.add_mana(r.mana as i64);
            }
            if r.patrol_army != 0 && r.patrol_delta != 0 {
                w.change_patrol(r.patrol_army, r.patrol_delta);
            }
        }
        if r.cast_spell != 0 {
            if ext == Some(Extension::RemoveSpell) {
                w.remove_army_spell(r.cast_spell);
            } else {
                w.apply_spell(r.cast_spell);
            }
        }
        if !matches!(op, 11 | 14 | 16) {
            for s in nonzero(&r.spells_learned) {
                w.learn_spell(s);
            }
        }
        let from = (r.units_from_army != 0).then_some(r.units_from_army);
        for (i, u) in r.units_add.iter().enumerate() {
            if *u != 0 {
                w.add_unit(*u, r.units_add_named[i], from);
            }
        }
        let to = (r.removed_units_to_army != 0).then_some(r.removed_units_to_army);
        for (i, u) in r.units_remove.iter().enumerate() {
            let pick = match *u {
                0 => continue,
                0xFE => UnitPick::AddedByEvent,
                0xFF => UnitPick::Any,
                t => UnitPick::Type(t),
            };
            w.remove_unit(pick, r.units_remove_named[i], to);
        }
        if op != 6 {
            for a in nonzero(&r.artifacts_add) {
                w.give_item(a);
            }
        }
        for a in nonzero(&r.artifacts_remove) {
            w.take_item(a);
        }
        for a in nonzero(&r.activate_armies) {
            w.activate_army(a);
        }
        if r.deactivate_army != 0 {
            w.deactivate_army(r.deactivate_army);
        }
        if r.show_army != 0 {
            w.show_army(r.show_army);
        }
        if r.move_to_hero != 0 {
            if let Some(a) = to.or(from) {
                w.move_army_to_hero(a);
            }
        }
        for p in nonzero(&r.light_lanterns) {
            w.light_lantern(p);
        }
        if r.new_hero_class != 0 {
            w.set_hero_class(r.new_hero_class);
        }
        if r.delay_hours != 0 {
            w.delay_player(r.delay_hours as u64 * 60);
        }
        if r.hero_one_hp != 0 {
            w.hero_to_one_hp();
        }
        if r.no_meeting != 0 && e.conditions.meet_army != 0 {
            w.forget_meeting(e.conditions.meet_army);
        }
        if op != 0 {
            self.run_opcode(id, op, e, w);
        }
        if r.start_battle_with != 0 {
            w.start_battle(r.start_battle_with);
        }
    }

    /// The effect of a Community opcode (mechanics.md §6; arguments: XP `x`, gold `g`, mana
    /// `m`).
    fn run_opcode(&mut self, id: EventId, op: u8, e: &Event, w: &mut dyn EventWorld) {
        let r = &e.results;
        let (x, g, m) = (r.experience, r.gold, r.mana);
        let holder = Holder::from_code(x);
        // −1 (any negative) means the whole army; slots past 255 do not exist.
        let units = |v: i16| if v < 0 { None } else { Some(v.min(255) as u8) };
        let unit = u8::try_from(g).ok();
        let spells: Vec<u8> = nonzero(&r.spells_learned).collect();
        match op {
            1..=5 => {
                for ed in event_edits(e) {
                    let (Some(target), Ok(off)) = (self.shifted(id, ed.shift), u16::try_from(ed.field)) else { continue };
                    let Some(old) = self.event_field(target, off) else { continue };
                    match ed.action {
                        1 => self.set_event_field(target, off, old + ed.value as i64),
                        2 => self.set_event_field(target, off, ed.value as i64),
                        _ => {}
                    }
                }
            }
            6 => {
                if let (Some(h), Some(u)) = (holder, unit) {
                    w.equip_unit(h, u, r.artifacts_add);
                }
            }
            7 => {
                if let (Some(h), Some(u), Ok(with)) = (holder, unit, u8::try_from(m)) {
                    w.replace_unit(h, u, with);
                }
            }
            8 => {
                if let Some(h) = holder {
                    w.set_army_speed(h, speed_correction(g));
                }
            }
            9 => {
                if let (Some(h), Some(group)) = (holder, unit.filter(|u| (1..=4).contains(u))) {
                    w.set_faction(h, group);
                }
            }
            10 => {
                if let (Some(h), Some(group)) = (holder, unit.filter(|u| *u <= 3)) {
                    w.set_relation(h, group, m.clamp(-3, 3) as i8);
                }
            }
            11 => {
                if let Some(h) = holder {
                    w.set_spells(h, units(g), &spells);
                }
            }
            12 => {
                if let (Some(h), Some(u), Ok(named)) = (holder, unit, u8::try_from(m)) {
                    let class = (named as usize).checked_sub(1).and_then(|k| self.named_units.get(k)).copied().unwrap_or(0);
                    w.set_named_unit(h, u, named, class);
                }
            }
            13 => {
                if let Some(h) = holder {
                    w.give_unit_xp(h, units(g), m as i64);
                }
            }
            15 => self.branch = Some((x, g)),
            16 => {
                for s in spells {
                    w.forget_spell(s);
                }
            }
            17 => {
                if let (Some(h @ (Holder::Player | Holder::Army(_))), Some(model)) = (holder, unit) {
                    w.set_army_model(h, model);
                }
            }
            18 => {
                // A flag RAND<c>, c a random character between the two codes (cp1251); an
                // existing RAND flag is drawn anew.
                let (lo, hi) = (x.clamp(0, 255) as i64, g.clamp(0, 255) as i64);
                let code = w.random(lo.min(hi), lo.max(hi)).clamp(0, 255) as u8;
                let name = format!("{RANDOM_FLAG}{}", crate::dt::text::decode(&[code]));
                self.flags.retain(|f| !is_random_flag(f));
                self.flags.insert(name);
            }
            19 => {
                if let Ok(army @ 1..=255) = u8::try_from(x) {
                    w.set_army_target(army, g as i32, m as i32);
                }
            }
            20 => w.teleport_player(x as i32, g as i32),
            _ => {}
        }
    }

    /// Event `id` moved by `shift` places, if it exists.
    fn shifted(&self, id: EventId, shift: i16) -> Option<EventId> {
        let t = EventId::try_from(id as i32 + shift as i32).ok()?;
        self.event(t).map(|_| t)
    }

    /// The conditions Community opcodes add: comparisons with other events' fields (3–5),
    /// spells on an army (14) and an AI army's position (19). A comparison with an event or
    /// field that does not exist is ignored *(guess)*.
    fn opcode_conditions_hold(&self, id: EventId, e: &Event, w: &dyn EventWorld) -> bool {
        let Some(Extension::Opcode(op)) = extension(e) else { return true };
        let (r, c) = (&e.results, &e.conditions);
        match op {
            1..=5 => event_edits(e).iter().all(|ed| {
                let Some(target) = self.shifted(id, ed.shift) else { return true };
                let Some(have) = u16::try_from(ed.field).ok().and_then(|off| self.event_field(target, off)) else { return true };
                let v = ed.value as i64;
                match ed.action {
                    3 => have <= v,
                    4 => have == v,
                    5 => have >= v,
                    _ => true,
                }
            }),
            14 => {
                let spells: Vec<u8> = nonzero(&r.spells_learned).collect();
                let units = if r.gold < 0 { None } else { u8::try_from(r.gold).ok() };
                match Holder::from_code(r.experience) {
                    Some(h) => w.has_spells(h, units, &spells),
                    None => false,
                }
            }
            19 => match u8::try_from(c.army_strength) {
                Ok(army @ 1..=255) => w.army_at(army, c.gold as i32, c.holiness_mana as i32),
                _ => true,
            },
            _ => true,
        }
    }

    // ---------------------------------------------------------------------------------------
    // Conditions
    // ---------------------------------------------------------------------------------------

    /// All conditions except the time window, the place and the question.
    fn conditions_hold(&self, id: EventId, w: &dyn EventWorld) -> bool {
        let e = &self.events[id as usize - 1];
        if !self.opcode_conditions_hold(id, e, w) {
            return false;
        }
        let c = &e.conditions;
        if e.archetype != 0 && e.archetype != w.hero_archetype() {
            return false;
        }
        if let Some(f) = &e.flags {
            if f.require_set.as_ref().is_some_and(|x| !self.flag(x)) {
                return false;
            }
            if f.require_unset.as_ref().is_some_and(|x| self.flag(x)) {
                return false;
            }
        }
        // Opcodes 1–5 and 19 use these fields as arguments.
        if !matches!(extension(e), Some(Extension::Opcode(_))) {
            // The army's strength is dear to compute: only when the event asks for it.
            if !compare(w.squad_count(), c.squad_count) || (c.army_strength != 0 && !compare(w.army_strength(), c.army_strength)) {
                return false;
            }
            if c.stats_check != 0
                && !(compare(w.hero_level(), c.level)
                    && compare(w.gold(), c.gold)
                    && compare(w.mana(), c.holiness_mana))
            {
                return false;
            }
        }
        let owned = |ids: &[u8; 3], owners: &[u8; 3], holder: &dyn Fn(usize) -> Option<u8>| {
            (0..3).all(|i| ids[i] == 0 || owner_matches(owners[i], holder(i)))
        };
        if c.buildings_check != 0
            && !owned(&c.buildings, &c.buildings_owner, &|i| w.building_owner(c.buildings[i] as u16))
        {
            return false;
        }
        if c.units_check != 0
            && !owned(&c.units, &c.units_owner, &|i| w.named_unit_holder(c.units[i], c.units_named[i]))
        {
            return false;
        }
        if c.artifacts_check != 0 && !owned(&c.artifacts, &c.artifacts_owner, &|i| w.artifact_holder(c.artifacts[i])) {
            return false;
        }
        if c.defeated_check != 0 && !nonzero(&c.defeated_armies).all(|a| w.player_defeated(a)) {
            return false;
        }
        if c.beaten_check != 0 && !nonzero(&c.beaten_armies).all(|a| w.army_beaten(a)) {
            return false;
        }
        let answered = |id: EventId, a: Answer| self.happened(id) == Some(a);
        if c.happened_yes_check != 0 && !nonzero(&c.happened_yes).all(|id| answered(id, Answer::Yes)) {
            return false;
        }
        if c.happened_no_check != 0 && !nonzero(&c.happened_no).all(|id| answered(id, Answer::No)) {
            return false;
        }
        if c.not_happened_check != 0 && !nonzero(&c.not_happened).all(|id| self.happened(id).is_none()) {
            return false;
        }
        if c.meet_army != 0 && !w.met_army(c.meet_army) {
            return false;
        }
        if c.army_active != 0 && !w.army_active(c.army_active) {
            return false;
        }
        if c.army_inactive != 0 && w.army_active(c.army_inactive) {
            return false;
        }
        if c.army_at_home != 0 && !w.army_at_home(c.army_at_home) {
            return false;
        }
        true
    }
}

#[cfg(test)]
pub(crate) mod mock {
    //! A world of plain fields for the engine tests.
    use super::*;
    use std::collections::{HashMap, HashSet};

    /// An effect the engine asked for, in order.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Fx {
        Xp(i64),
        Gold(i64),
        Mana(i64),
        AddUnit(u8, u8, Option<ArmyId>),
        RemoveUnit(UnitPick, u8, Option<ArmyId>),
        GiveItem(u8),
        TakeItem(u8),
        Learn(u8),
        Spell(u8),
        Activate(ArmyId),
        Deactivate(ArmyId),
        Show(ArmyId),
        MoveToHero(ArmyId),
        ForgetMeeting(ArmyId),
        Lantern(u16),
        Patrol(ArmyId, i8),
        Class(u8),
        Battle(ArmyId),
        Delay(u64),
        OneHp,
        Unspell(u8),
        Equip(Holder, u8, [u8; 4]),
        Replace(Holder, u8, u8),
        Speed(Holder, i8),
        Faction(Holder, u8),
        Relation(Holder, u8, i8),
        SetSpells(Holder, Option<u8>, Vec<u8>),
        Named(Holder, u8, u8, u8),
        UnitXp(Holder, Option<u8>, i64),
        Forget(u8),
        Model(Holder, u8),
        Target(ArmyId, i32, i32),
        Teleport(i32, i32),
    }

    #[derive(Clone, Debug, Default)]
    pub struct MockWorld {
        pub now: u64,
        pub archetype: u8,
        pub level: i64,
        pub gold: i64,
        pub mana: i64,
        pub squads: i64,
        pub strength: i64,
        pub building_owner: HashMap<u16, u8>,
        /// (unit, named) → holder.
        pub named: HashMap<(u8, u8), u8>,
        pub artifacts: HashMap<u8, u8>,
        pub defeated: HashSet<ArmyId>,
        pub beaten: HashSet<ArmyId>,
        pub active: HashSet<ArmyId>,
        pub home: HashSet<ArmyId>,
        pub met: HashSet<ArmyId>,
        pub place: Option<Place>,
        /// Lasting spells by holder (opcodes 11 and 14).
        pub spells_on: HashMap<Holder, Vec<u8>>,
        /// Army cells (opcode 19).
        pub army_cells: HashMap<ArmyId, (i32, i32)>,
        /// What `random` returns, in turn (else the low end).
        pub rolls: Vec<i64>,
        pub log: Vec<Fx>,
    }

    impl MockWorld {
        pub fn new() -> MockWorld {
            MockWorld { archetype: 1, level: 1, ..MockWorld::default() }
        }
    }

    impl EventWorld for MockWorld {
        fn now(&self) -> u64 {
            self.now
        }
        fn hero_archetype(&self) -> u8 {
            self.archetype
        }
        fn hero_level(&self) -> i64 {
            self.level
        }
        fn gold(&self) -> i64 {
            self.gold
        }
        fn mana(&self) -> i64 {
            self.mana
        }
        fn squad_count(&self) -> i64 {
            self.squads
        }
        fn army_strength(&self) -> i64 {
            self.strength
        }
        fn building_owner(&self, building: u16) -> Option<u8> {
            self.building_owner.get(&building).copied()
        }
        fn named_unit_holder(&self, unit: u8, named: u8) -> Option<u8> {
            self.named.get(&(unit, named)).copied()
        }
        fn artifact_holder(&self, artifact: u8) -> Option<u8> {
            self.artifacts.get(&artifact).copied()
        }
        fn player_defeated(&self, army: ArmyId) -> bool {
            self.defeated.contains(&army)
        }
        fn army_beaten(&self, army: ArmyId) -> bool {
            self.beaten.contains(&army)
        }
        fn army_active(&self, army: ArmyId) -> bool {
            self.active.contains(&army)
        }
        fn army_at_home(&self, army: ArmyId) -> bool {
            self.home.contains(&army)
        }
        fn met_army(&self, army: ArmyId) -> bool {
            self.met.contains(&army)
        }
        fn place(&self) -> Option<Place> {
            self.place
        }
        fn add_experience(&mut self, xp: i64) {
            self.log.push(Fx::Xp(xp));
        }
        fn add_gold(&mut self, gold: i64) {
            self.gold += gold;
            self.log.push(Fx::Gold(gold));
        }
        fn add_mana(&mut self, mana: i64) {
            self.mana += mana;
            self.log.push(Fx::Mana(mana));
        }
        fn add_unit(&mut self, unit: u8, named: u8, from_army: Option<ArmyId>) {
            self.squads += 1;
            if named != 0 {
                self.named.insert((unit, named), SIDE_PLAYER);
            }
            self.log.push(Fx::AddUnit(unit, named, from_army));
        }
        fn remove_unit(&mut self, pick: UnitPick, named: u8, to_army: Option<ArmyId>) {
            self.squads -= 1;
            if let UnitPick::Type(u) = pick {
                self.named.remove(&(u, named));
            }
            self.log.push(Fx::RemoveUnit(pick, named, to_army));
        }
        fn give_item(&mut self, artifact: u8) {
            self.artifacts.insert(artifact, SIDE_PLAYER);
            self.log.push(Fx::GiveItem(artifact));
        }
        fn take_item(&mut self, artifact: u8) {
            self.artifacts.remove(&artifact);
            self.log.push(Fx::TakeItem(artifact));
        }
        fn learn_spell(&mut self, spell: u8) {
            self.log.push(Fx::Learn(spell));
        }
        fn apply_spell(&mut self, spell: u8) {
            self.log.push(Fx::Spell(spell));
        }
        fn activate_army(&mut self, army: ArmyId) {
            self.active.insert(army);
            self.log.push(Fx::Activate(army));
        }
        fn deactivate_army(&mut self, army: ArmyId) {
            self.active.remove(&army);
            self.log.push(Fx::Deactivate(army));
        }
        fn show_army(&mut self, army: ArmyId) {
            self.log.push(Fx::Show(army));
        }
        fn move_army_to_hero(&mut self, army: ArmyId) {
            self.log.push(Fx::MoveToHero(army));
        }
        fn forget_meeting(&mut self, army: ArmyId) {
            self.met.remove(&army);
            self.log.push(Fx::ForgetMeeting(army));
        }
        fn light_lantern(&mut self, point: u16) {
            self.log.push(Fx::Lantern(point));
        }
        fn change_patrol(&mut self, army: ArmyId, delta: i8) {
            self.log.push(Fx::Patrol(army, delta));
        }
        fn set_hero_class(&mut self, unit: u8) {
            self.log.push(Fx::Class(unit));
        }
        fn start_battle(&mut self, army: ArmyId) {
            self.log.push(Fx::Battle(army));
        }
        fn delay_player(&mut self, minutes: u64) {
            self.now += minutes;
            self.log.push(Fx::Delay(minutes));
        }
        fn hero_to_one_hp(&mut self) {
            self.log.push(Fx::OneHp);
        }
        fn remove_army_spell(&mut self, spell: u8) {
            self.log.push(Fx::Unspell(spell));
        }
        fn equip_unit(&mut self, holder: Holder, unit: u8, items: [u8; 4]) {
            self.log.push(Fx::Equip(holder, unit, items));
        }
        fn replace_unit(&mut self, holder: Holder, unit: u8, with: u8) {
            self.log.push(Fx::Replace(holder, unit, with));
        }
        fn set_army_speed(&mut self, holder: Holder, correction: i8) {
            self.log.push(Fx::Speed(holder, correction));
        }
        fn set_faction(&mut self, holder: Holder, group: u8) {
            self.log.push(Fx::Faction(holder, group));
        }
        fn set_relation(&mut self, holder: Holder, group: u8, value: i8) {
            self.log.push(Fx::Relation(holder, group, value));
        }
        fn set_spells(&mut self, holder: Holder, unit: Option<u8>, spells: &[u8]) {
            self.spells_on.insert(holder, spells.to_vec());
            self.log.push(Fx::SetSpells(holder, unit, spells.to_vec()));
        }
        fn set_named_unit(&mut self, holder: Holder, unit: u8, named: u8, class: u8) {
            self.log.push(Fx::Named(holder, unit, named, class));
        }
        fn give_unit_xp(&mut self, holder: Holder, unit: Option<u8>, xp: i64) {
            self.log.push(Fx::UnitXp(holder, unit, xp));
        }
        fn has_spells(&self, holder: Holder, _unit: Option<u8>, spells: &[u8]) -> bool {
            let on = self.spells_on.get(&holder).cloned().unwrap_or_default();
            spells.iter().all(|s| on.contains(s))
        }
        fn forget_spell(&mut self, spell: u8) {
            self.log.push(Fx::Forget(spell));
        }
        fn set_army_model(&mut self, holder: Holder, model: u8) {
            self.log.push(Fx::Model(holder, model));
        }
        fn random(&mut self, lo: i64, hi: i64) -> i64 {
            if self.rolls.is_empty() {
                lo
            } else {
                self.rolls.remove(0).clamp(lo, hi)
            }
        }
        fn set_army_target(&mut self, army: ArmyId, x: i32, y: i32) {
            self.log.push(Fx::Target(army, x, y));
        }
        fn army_at(&self, army: ArmyId, x: i32, y: i32) -> bool {
            self.army_cells.get(&army) == Some(&(x, y))
        }
        fn teleport_player(&mut self, x: i32, y: i32) {
            self.log.push(Fx::Teleport(x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::{Fx, MockWorld};
    use super::*;
    use crate::dt::dtm::FlagScript;

    const DAY: u64 = 1440;

    /// A once-event of `kind` that is open all the time from minute 0 (daily windows).
    fn ev(kind: EventKind) -> Event {
        Event { kind: kind as u8, repeat: DAY as u16, duration: DAY as u16, once: 1, ..Event::default() }
    }

    fn global() -> Event {
        ev(EventKind::Global)
    }

    fn many(mut e: Event) -> Event {
        e.once = 0;
        e
    }

    fn titled(mut e: Event, script: &str) -> Event {
        e.title = format!("t%{script}");
        e.flags = FlagScript::from_title(&e.title);
        e
    }

    fn with_message(mut e: Event) -> Event {
        e.message = "m".into();
        e
    }

    fn engine(events: Vec<Event>) -> EventEngine {
        EventEngine::from_parts(events, Vec::new(), 0, 0)
    }

    fn fired(out: &[EventOutcome]) -> Vec<EventId> {
        out.iter()
            .filter_map(|o| match o {
                EventOutcome::Fired { event, .. } => Some(*event),
                _ => None,
            })
            .collect()
    }

    fn tick_at(g: &mut EventEngine, w: &mut MockWorld, now: u64) -> Vec<EventId> {
        w.now = now;
        fired(&g.tick(w))
    }

    #[test]
    fn time_window_and_repeat() {
        // Daily from minute 1000, open for 1 hour: a many-event fires on every later check.
        let mut e = many(global());
        (e.start_time, e.repeat, e.duration) = (1000, DAY as u16, 1);
        let mut g = engine(vec![e]);
        let mut w = MockWorld::new();
        assert!(tick_at(&mut g, &mut w, 999).is_empty(), "before the start");
        assert_eq!(tick_at(&mut g, &mut w, 1000), vec![1]);
        assert!(tick_at(&mut g, &mut w, 1000).is_empty(), "not twice in the same minute");
        assert_eq!(tick_at(&mut g, &mut w, 1030), vec![1], "again on a later check");
        assert!(tick_at(&mut g, &mut w, 1061).is_empty(), "the hour is over");
        assert_eq!(tick_at(&mut g, &mut w, 1000 + DAY + 59), vec![1], "next day's window");
        assert_eq!(g.times_fired(1), 3);

        // Every second day.
        let mut e = many(global());
        (e.start_time, e.repeat, e.duration) = (0, 2 * DAY as u16, 2);
        let mut g = engine(vec![e]);
        assert!(tick_at(&mut g, &mut w, DAY + 10).is_empty());
        assert_eq!(tick_at(&mut g, &mut w, 2 * DAY + 10), vec![1]);

        // No repeat: one window. Duration 0: no end until it fires, then a 60-minute guard
        // and a one-hour window.
        let mut once = many(global());
        (once.start_time, once.repeat, once.duration) = (0, 0, 1);
        let mut open = many(global());
        (open.start_time, open.repeat, open.duration) = (0, 0, 0);
        let mut g = engine(vec![once, open]);
        assert_eq!(tick_at(&mut g, &mut w, 10), vec![1, 2]);
        assert_eq!(tick_at(&mut g, &mut w, 30), vec![1], "duration 0: 60 minutes between firings");
        assert!(tick_at(&mut g, &mut w, 70).is_empty(), "both windows closed");
        let mut late = many(global());
        (late.start_time, late.repeat, late.duration) = (100, 0, 0);
        let mut g = engine(vec![late]);
        assert_eq!(tick_at(&mut g, &mut w, 5 * DAY), vec![1], "never fired: open with no end");
        assert!(tick_at(&mut g, &mut w, 5 * DAY + 60).is_empty(), "fired: its hour after the start is long past");
    }

    #[test]
    fn once_versus_many() {
        let mut g = engine(vec![global(), many(global())]);
        let mut w = MockWorld::new();
        for day in 0..3 {
            tick_at(&mut g, &mut w, day * DAY + 5);
            tick_at(&mut g, &mut w, day * DAY + 600);
        }
        assert_eq!((g.times_fired(1), g.times_fired(2)), (1, 6), "a many-event fires on every check");
    }

    type Setup = fn(&mut Event);
    type World = fn(&mut MockWorld);

    /// Each condition group: (name, make the event, a world that passes, a world that fails).
    fn condition_cases() -> Vec<(&'static str, Setup, World, World)> {
        vec![
            ("archetype", |e| e.archetype = 2, |w| w.archetype = 2, |w| w.archetype = 3),
            ("squads at least", |e| e.conditions.squad_count = 3, |w| w.squads = 3, |w| w.squads = 2),
            ("squads at most", |e| e.conditions.squad_count = -3, |w| w.squads = 3, |w| w.squads = 4),
            ("strength", |e| e.conditions.army_strength = 1500, |w| w.strength = 1600, |w| w.strength = 1400),
            ("level", |e| (e.conditions.stats_check, e.conditions.level) = (1, 5), |w| w.level = 5, |w| w.level = 4),
            ("gold at most", |e| (e.conditions.stats_check, e.conditions.gold) = (1, -209), |w| w.gold = 209, |w| {
                w.gold = 210
            }),
            ("mana", |e| (e.conditions.stats_check, e.conditions.holiness_mana) = (1, 40), |w| w.mana = 40, |w| {
                w.mana = 39
            }),
            (
                "building of the player",
                |e| {
                    let c = &mut e.conditions;
                    (c.buildings_check, c.buildings, c.buildings_owner) = (1, [8, 0, 0], [1, 0, 0]);
                },
                |w| _ = w.building_owner.insert(8, 1),
                |w| _ = w.building_owner.insert(8, 5),
            ),
            (
                "building of red",
                |e| {
                    let c = &mut e.conditions;
                    (c.buildings_check, c.buildings, c.buildings_owner) = (1, [8, 0, 0], [5, 0, 0]);
                },
                |w| _ = w.building_owner.insert(8, 5),
                |w| _ = w.building_owner.insert(8, 1),
            ),
            (
                "building not the player's",
                |e| {
                    let c = &mut e.conditions;
                    (c.buildings_check, c.buildings, c.buildings_owner) = (1, [8, 0, 0], [6, 0, 0]);
                },
                |_| {},
                |w| _ = w.building_owner.insert(8, 1),
            ),
            (
                "named unit not with the player",
                |e| {
                    let c = &mut e.conditions;
                    (c.units_check, c.units, c.units_named, c.units_owner) = (1, [74, 0, 0], [1, 0, 0], [6, 0, 0]);
                },
                |_| {},
                |w| _ = w.named.insert((74, 1), 1),
            ),
            (
                "artifacts with the player",
                |e| {
                    let c = &mut e.conditions;
                    (c.artifacts_check, c.artifacts, c.artifacts_owner) = (1, [43, 44, 0], [1, 1, 0]);
                },
                |w| w.artifacts.extend([(43, 1), (44, 1)]),
                |w| _ = w.artifacts.insert(43, 1),
            ),
            (
                "player defeated armies",
                |e| (e.conditions.defeated_check, e.conditions.defeated_armies) = (1, [3, 4]),
                |w| w.defeated.extend([3, 4]),
                |w| _ = w.defeated.insert(3),
            ),
            (
                "army beaten by anyone",
                |e| (e.conditions.beaten_check, e.conditions.beaten_armies) = (1, [3, 0]),
                |w| _ = w.beaten.insert(3),
                |_| {},
            ),
            ("meet army", |e| e.conditions.meet_army = 14, |w| _ = w.met.insert(14), |_| {}),
            ("army active", |e| e.conditions.army_active = 4, |w| _ = w.active.insert(4), |_| {}),
            ("army inactive", |e| e.conditions.army_inactive = 4, |_| {}, |w| _ = w.active.insert(4)),
            ("army at home", |e| e.conditions.army_at_home = 13, |w| _ = w.home.insert(13), |_| {}),
        ]
    }

    #[test]
    fn conditions_by_group() {
        for (name, setup, pass, fail) in condition_cases() {
            let mut e = global();
            setup(&mut e);
            for (world, expect) in [(pass, true), (fail, false)] {
                let mut w = MockWorld::new();
                world(&mut w);
                let mut g = engine(vec![e.clone()]);
                assert_eq!(!tick_at(&mut g, &mut w, 0).is_empty(), expect, "{name}");
            }
        }
    }

    #[test]
    fn a_cleared_check_byte_disables_its_ids() {
        let mut e = global();
        e.conditions.defeated_armies = [3, 0];
        e.conditions.not_happened = [1, 0];
        let mut g = engine(vec![e]);
        assert_eq!(tick_at(&mut g, &mut MockWorld::new(), 0), vec![1]);
    }

    #[test]
    fn event_history_conditions() {
        let mut first = global();
        first.start_time = 100;
        let mut yes = global();
        (yes.conditions.happened_yes_check, yes.conditions.happened_yes) = (1, [1, 0]);
        let mut not = global();
        (not.conditions.not_happened_check, not.conditions.not_happened) = (1, [1, 0]);
        // 1 fires at 100; 3 (1 not happened) only before that, 2 (1 happened) only after.
        let mut g = engine(vec![first, yes, not]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![3]);
        assert_eq!(tick_at(&mut g, &mut w, 100), vec![1, 2]);
    }

    #[test]
    fn flags_from_the_title_script() {
        let events = vec![
            titled(global(), "+Foo"),      // 1: sets Foo
            titled(global(), "=Foo"),      // 2: needs Foo
            titled(global(), "-Foo=Foo"),  // 3: needs Foo, clears it
            titled(global(), "=/Foo"),     // 4: needs Foo unset
            titled(global(), "+Bar=/Foo"), // 5: needs Foo unset, sets Bar
        ];
        let mut g = engine(events);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1, 2, 3, 4, 5]);
        assert!(!g.flag("Foo"));
        assert!(g.flag("Bar"));
        assert_eq!(g.flags().count(), 1);
    }

    #[test]
    fn flag_condition_blocks_until_set() {
        let mut g = engine(vec![titled(global(), "=Foo"), titled(global(), "+Foo")]);
        let mut w = MockWorld::new();
        // The first matching event fires, then the loop starts over: 2 enables 1.
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![2, 1]);
    }

    #[test]
    fn chained_subordinate_events() {
        let mut parent = global();
        parent.start_time = 100;
        parent.results.chained_event = 2;
        let mut sub = with_message(ev(EventKind::Local));
        (sub.subordinate, sub.once, sub.repeat, sub.duration) = (1, 0, 0, 0);
        (sub.conditions.happened_yes_check, sub.conditions.happened_yes) = (1, [1, 0]);
        sub.results.gold = 50;
        sub.results.chained_event = 3;
        // 3 is chained but its conditions fail: it does not fire.
        let mut blocked = global();
        blocked.subordinate = 1;
        blocked.conditions.meet_army = 9;
        let mut g = engine(vec![parent, sub, blocked]);
        let mut w = MockWorld::new();
        assert!(tick_at(&mut g, &mut w, 0).is_empty(), "a subordinate event never fires on its own");
        w.now = 100;
        let out = g.tick(&mut w);
        assert_eq!(fired(&out), vec![1, 2]);
        assert!(out.contains(&EventOutcome::Fired { event: 2, message: true }));
        assert_eq!(w.log, vec![Fx::Gold(50)]);
        assert_eq!(g.times_fired(3), 0);
    }

    #[test]
    fn relative_event_start_is_set_by_another() {
        let mut trigger = global();
        trigger.start_time = 600;
        (trigger.results.relative_event, trigger.results.relative_delay_hours) = (2, 5);
        let mut later = with_message(global());
        later.start_time = 1_036_800_000; // "never" until moved
        let mut g = engine(vec![trigger, later]);
        let mut w = MockWorld::new();
        assert!(tick_at(&mut g, &mut w, 0).is_empty());
        assert_eq!(tick_at(&mut g, &mut w, 610), vec![1]);
        assert_eq!(g.start_time(2), Some(610 + 300));
        assert!(tick_at(&mut g, &mut w, 900).is_empty());
        assert_eq!(tick_at(&mut g, &mut w, 910), vec![2]);
    }

    #[test]
    fn questions_yes_and_no_paths() {
        let mut ask = with_message(global());
        ask.conditions.confirm_question = 1;
        ask.results.gold = -150;
        ask.results.artifacts_add = [14, 0, 0, 0];
        let mut on_yes = global();
        (on_yes.conditions.happened_yes_check, on_yes.conditions.happened_yes) = (1, [1, 0]);
        let mut on_no = global();
        (on_no.conditions.happened_no_check, on_no.conditions.happened_no) = (1, [1, 0]);
        let mut not = global();
        (not.conditions.not_happened_check, not.conditions.not_happened) = (1, [1, 0]);
        not.start_time = 20;

        // No: nothing is applied, but it counts as happened: the No branch runs and a
        // once-question is used up.
        let events = vec![ask, on_yes, on_no, not];
        let mut g = engine(events.clone());
        let mut w = MockWorld { gold: 300, ..MockWorld::new() };
        w.now = 10;
        assert_eq!(g.tick(&mut w), vec![EventOutcome::Question(1)]);
        assert_eq!(g.pending_question(), Some(1));
        assert!(g.tick(&mut w).is_empty(), "nothing runs while a question waits");
        let out = g.answer(&mut w, false);
        assert_eq!(out[0], EventOutcome::Declined(1));
        assert_eq!(fired(&out), vec![3]);
        assert_eq!((g.happened(1), g.times_fired(1)), (Some(Answer::No), 1));
        assert!(w.log.is_empty());
        assert!(tick_at(&mut g, &mut w, DAY + 10).is_empty(), "used up; 'not happened' does not hold");

        // Yes: the results apply and the Yes branch runs; a once-event is done.
        let mut g = engine(events);
        w.now = 10;
        assert_eq!(g.tick(&mut w), vec![EventOutcome::Question(1)]);
        let out = g.answer(&mut w, true);
        assert_eq!(fired(&out), vec![1, 2]);
        assert_eq!(w.log, vec![Fx::Gold(-150), Fx::GiveItem(14)]);
        assert_eq!(g.happened(1), Some(Answer::Yes));
        assert!(tick_at(&mut g, &mut w, 3 * DAY).is_empty());
    }

    #[test]
    fn a_many_question_answered_no_comes_back_on_a_later_check() {
        let mut ask = many(global());
        ask.conditions.confirm_question = 1;
        let mut g = engine(vec![ask]);
        let mut w = MockWorld::new();
        assert_eq!(g.tick(&mut w), vec![EventOutcome::Question(1)]);
        g.answer(&mut w, false);
        assert!(g.tick(&mut w).is_empty(), "not in the same minute");
        w.now = 5;
        assert_eq!(g.tick(&mut w), vec![EventOutcome::Question(1)]);
        assert_eq!(g.happened(1), Some(Answer::Yes), "opening the question clears the No");
    }

    #[test]
    fn repeatable_question_asked_again_only_with_repeat_after_yes() {
        let mut ask = many(global());
        ask.conditions.confirm_question = 1;
        let mut g = engine(vec![ask.clone()]);
        let mut w = MockWorld::new();
        g.tick(&mut w);
        g.answer(&mut w, true);
        assert_eq!(tick_at(&mut g, &mut w, DAY), vec![1], "fires without asking once answered Yes");
        ask.results.repeat_after_yes = 1;
        let mut g = engine(vec![ask]);
        w.now = 0;
        g.tick(&mut w);
        g.answer(&mut w, true);
        w.now = DAY;
        assert_eq!(g.tick(&mut w), vec![EventOutcome::Question(1)]);
    }

    #[test]
    fn quest_journal_add_and_complete() {
        let quest = with_message(ev(EventKind::Quest));
        let mut done = global();
        (done.conditions.happened_yes_check, done.conditions.happened_yes) = (1, [1, 0]);
        (done.conditions.defeated_check, done.conditions.defeated_armies) = (1, [4, 0]);
        done.results.completes_quest = 1;
        done.results.gold = 25;
        let mut g = EventEngine::from_parts(vec![quest, done], vec![(Place::Building(6), vec![1])], 0, 0);
        let mut w = MockWorld::new();
        assert!(tick_at(&mut g, &mut w, 0).is_empty(), "a quest is given at its building");
        w.place = Some(Place::Building(6));
        let out = g.tick(&mut w);
        assert_eq!(out, vec![EventOutcome::Fired { event: 1, message: true }, EventOutcome::QuestAdded(1)]);
        assert_eq!(g.journal(), &[1]);
        w.place = None;
        w.defeated.insert(4);
        let out = g.tick(&mut w);
        assert!(out.contains(&EventOutcome::QuestCompleted(1)));
        assert!(g.journal().is_empty());
        assert_eq!(g.completed_quests(), &[1]);
        assert_eq!(w.gold, 25);
    }

    #[test]
    fn local_events_fire_at_points_on_every_check_and_in_buildings_on_entering() {
        let at = Place::Point(3);
        let inn = Place::Building(5);
        let mut g = EventEngine::from_parts(
            vec![many(ev(EventKind::Local)), global(), many(ev(EventKind::Local))],
            vec![(at, vec![1]), (inn, vec![3])],
            0,
            0,
        );
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![2], "the global fires anywhere, the local does not");
        w.place = Some(Place::Point(4));
        assert!(tick_at(&mut g, &mut w, 10).is_empty());
        w.place = Some(at);
        assert_eq!(tick_at(&mut g, &mut w, 20), vec![1]);
        assert_eq!(tick_at(&mut g, &mut w, 30), vec![1], "standing on the point: every check");
        w.place = Some(inn);
        assert_eq!(tick_at(&mut g, &mut w, 40), vec![3], "on entering");
        assert!(tick_at(&mut g, &mut w, 50).is_empty(), "not again while he is inside");
        w.place = None;
        tick_at(&mut g, &mut w, 60);
        w.place = Some(inn);
        assert_eq!(tick_at(&mut g, &mut w, 70), vec![3], "a new visit");
        g.visit(inn);
        assert_eq!(tick_at(&mut g, &mut w, 80), vec![3], "re-entered without leaving the cell");
    }

    #[test]
    fn rumours_fire_only_on_request() {
        let mut rumour = with_message(ev(EventKind::Rumour));
        rumour.results.gold = 75;
        let mut pricey = ev(EventKind::Rumour);
        (pricey.conditions.stats_check, pricey.conditions.gold) = (1, 1000);
        let inn = Place::Building(1);
        let mut g = EventEngine::from_parts(vec![rumour, pricey], vec![(inn, vec![1, 2])], 0, 0);
        let mut w = MockWorld { place: Some(inn), ..MockWorld::new() };
        assert!(g.tick(&mut w).is_empty());
        assert_eq!(g.rumours(&w), vec![1]);
        assert!(g.hear_rumour(&mut w, 2).is_empty(), "not on offer");
        let out = g.hear_rumour(&mut w, 1);
        assert_eq!(fired(&out), vec![1]);
        assert_eq!(w.gold, 75);
        assert!(g.rumours(&w).is_empty(), "a once-rumour is heard once");
        w.place = None;
        assert!(g.rumours(&w).is_empty());
    }

    #[test]
    fn a_flag_ping_pong_fires_each_once_a_minute_and_the_guard_stops_long_runs() {
        let mut g = engine(vec![many(titled(global(), "+A=/A")), many(titled(global(), "-A=A"))]);
        let mut w = MockWorld::new();
        assert_eq!(fired(&g.tick(&mut w)), vec![1, 2]);
        assert_eq!(tick_at(&mut g, &mut w, 1), vec![1, 2]);
        let mut g = engine(vec![many(global()); LOOP_GUARD + 10]);
        let out = g.tick(&mut w);
        assert_eq!(fired(&out).len(), LOOP_GUARD);
        assert_eq!(out.last(), Some(&EventOutcome::LoopGuard));
    }

    #[test]
    fn flags_are_counters() {
        let mut g = engine(vec![
            many(titled(global(), "+Foo")),  // 1
            titled(global(), "=Foo2"),       // 2: needs the counter at 2
            many(titled(global(), "-Foo")), // 3
        ]);
        let mut w = MockWorld::new();
        g.raise_flag("Foo");
        assert_eq!(g.flags().collect::<Vec<_>>(), ["Foo1"]);
        g.raise_flag("Foo");
        assert!(g.flag("Foo") && g.flag("Foo2") && !g.flag("Foo1"));
        g.lower_flag("Foo");
        g.lower_flag("Foo");
        assert!(!g.flag("Foo") && g.flags().next().is_none(), "removed at 0");
        // +Foo and -Foo each fire once a check: Foo goes 1 → 0; 2 never sees Foo2.
        tick_at(&mut g, &mut w, 0);
        assert_eq!(g.times_fired(2), 0);
    }

    #[test]
    fn victory_and_defeat_events_end_the_scenario() {
        let mut win = global();
        (win.conditions.defeated_check, win.conditions.defeated_armies) = (1, [2, 0]);
        let mut lose = global();
        let c = &mut lose.conditions;
        (c.units_check, c.units, c.units_named, c.units_owner) = (1, [74, 0, 0], [1, 0, 0], [6, 0, 0]);
        let mut g = EventEngine::from_parts(vec![win.clone(), lose.clone(), global()], Vec::new(), 1, 2);
        let mut w = MockWorld::new();
        w.named.insert((74, 1), SIDE_PLAYER);
        w.defeated.insert(2);
        let out = g.tick(&mut w);
        assert_eq!(out.last(), Some(&EventOutcome::Victory(1)));
        assert_eq!(g.ended(), Some(&EventOutcome::Victory(1)));
        assert!(tick_at(&mut g, &mut w, DAY).is_empty(), "nothing runs after the end");

        let mut g = EventEngine::from_parts(vec![win, lose], Vec::new(), 1, 2);
        let mut w = MockWorld::new();
        assert_eq!(g.tick(&mut w).last(), Some(&EventOutcome::Defeat(2)));
    }

    #[test]
    fn results_go_through_the_world() {
        let mut e = global();
        let r = &mut e.results;
        (r.experience, r.gold, r.mana) = (100, -20, 7);
        (r.patrol_army, r.patrol_delta) = (3, -2);
        r.cast_spell = 13;
        r.spells_learned = [4, 0, 0, 0];
        (r.units_add, r.units_add_named, r.units_from_army) = ([74, 0, 0, 0], [1, 0, 0, 0], 7);
        (r.units_remove, r.units_remove_named, r.removed_units_to_army) = ([0xFE, 0xFF, 42, 0], [0, 0, 1, 0], 19);
        r.move_to_hero = 1;
        (r.artifacts_add, r.artifacts_remove) = ([43, 0, 0, 0], [111, 0, 0, 0]);
        (r.activate_armies, r.deactivate_army, r.show_army) = ([11, 12], 14, 18);
        r.light_lanterns = [2, 13, 0, 0];
        r.new_hero_class = 11;
        r.delay_hours = 2;
        r.hero_one_hp = 1;
        r.start_battle_with = 8;
        let mut g = engine(vec![e]);
        let mut w = MockWorld { gold: 100, ..MockWorld::new() };
        tick_at(&mut g, &mut w, 0);
        let to = Some(19);
        assert_eq!(
            w.log,
            vec![
                Fx::Xp(100),
                Fx::Gold(-20),
                Fx::Mana(7),
                Fx::Patrol(3, -2),
                Fx::Spell(13),
                Fx::Learn(4),
                Fx::AddUnit(74, 1, Some(7)),
                Fx::RemoveUnit(UnitPick::AddedByEvent, 0, to),
                Fx::RemoveUnit(UnitPick::Any, 0, to),
                Fx::RemoveUnit(UnitPick::Type(42), 1, to),
                Fx::GiveItem(43),
                Fx::TakeItem(111),
                Fx::Activate(11),
                Fx::Activate(12),
                Fx::Deactivate(14),
                Fx::Show(18),
                Fx::MoveToHero(19),
                Fx::Lantern(2),
                Fx::Lantern(13),
                Fx::Class(11),
                Fx::Delay(120),
                Fx::OneHp,
                Fx::Battle(8),
            ]
        );
    }

    #[test]
    fn meeting_repeats_after_no_meeting_clears_it() {
        let mut talk = many(global());
        talk.conditions.meet_army = 1;
        talk.results.no_meeting = 1;
        let mut g = engine(vec![talk]);
        let mut w = MockWorld::new();
        for n in 1..=3 {
            w.met.insert(1);
            tick_at(&mut g, &mut w, n * 10);
            assert_eq!(g.times_fired(1), n as u32);
        }
    }

    #[test]
    fn community_extensions_are_listed_and_run() {
        let mut edit = global();
        (edit.results.no_meeting, edit.results.patrol_delta, edit.results.gold) = (1, 2, 85);
        let mut unspell = global();
        (unspell.results.no_meeting, unspell.results.cast_spell) = (1, 13);
        let mut vanilla = global();
        (vanilla.results.no_meeting, vanilla.conditions.meet_army) = (1, 1);
        let mut g = engine(vec![edit, unspell, vanilla]);
        assert_eq!(g.extensions(), &[(1, Extension::Opcode(2)), (2, Extension::RemoveSpell)]);
        let mut w = MockWorld::new();
        w.met.insert(1);
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1, 2, 3]);
        assert_eq!(w.log, vec![Fx::Unspell(13), Fx::ForgetMeeting(1)], "the spell is lifted, not cast; no gold");
    }

    /// A Community opcode event: "no meeting", patrol value `code`, resources (XP, gold, mana).
    fn op(code: i8, x: i16, g: i16, m: i16) -> Event {
        let mut e = global();
        (e.results.no_meeting, e.results.patrol_delta) = (1, code);
        (e.results.experience, e.results.gold, e.results.mana) = (x, g, m);
        e
    }

    /// An event only a chain fires, giving 5 gold.
    fn target_event() -> Event {
        let mut e = global();
        e.subordinate = 1;
        e.results.gold = 5;
        e
    }

    #[test]
    fn opcodes_1_and_2_add_to_and_set_other_events_fields() {
        let mut second = op(1, 0, 0, 0);
        // Second setting: action 1 (add), shift −3, field 83 (XP), value 100.
        (second.conditions.squad_count, second.conditions.gold, second.conditions.level, second.conditions.holiness_mana) = (1, -3, 83, 100);
        let events = vec![target_event(), op(1, -1, 85, 2), op(2, -2, 89, 40), second];
        let mut g = engine(events.clone());
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![2, 3, 4]);
        assert_eq!([g.event_field(1, 85), g.event_field(1, 89), g.event_field(1, 83)], [Some(7), Some(40), Some(100)]);
        assert_eq!(g.event_field(1, 86), None, "inside a field");
        assert!(w.log.is_empty() && w.gold == 0, "the resources are arguments: {:?}", w.log);
        // A save keeps the edits.
        let saved: EventEngine = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        let mut loaded = saved;
        loaded.restore_statics(engine(events)).unwrap();
        assert_eq!(loaded.event_field(1, 85), Some(7));
        // The edited event gives 7 gold when chained.
        let mut chain = global();
        chain.results.chained_event = 1;
        let mut g = engine(vec![target_event(), op(1, -1, 85, 2), chain]);
        tick_at(&mut g, &mut w, 0);
        assert_eq!(w.log, vec![Fx::Gold(7)]);
    }

    #[test]
    fn opcodes_3_to_5_compare_other_events_fields() {
        let mut second = op(1, 0, 0, 0);
        (second.conditions.squad_count, second.conditions.gold, second.conditions.level, second.conditions.holiness_mana) = (5, -7, 85, 6);
        let events = vec![
            target_event(),
            op(3, -1, 85, 4), // 5 > 4: no
            op(3, -2, 85, 5), // yes
            op(4, -3, 85, 5), // equal: yes
            op(4, -4, 85, 6), // no
            op(5, -5, 85, 6), // 5 < 6: no
            op(5, -6, 85, 5), // yes
            second,           // second setting: 5 < 6: no
            op(3, 40, 85, 0), // no such event: ignored, fires
        ];
        let mut g = engine(events);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![3, 4, 7, 9]);
    }

    #[test]
    fn opcodes_6_to_20_act_through_the_world() {
        let mut equip = op(6, 3, 1, 0);
        equip.results.artifacts_add = [7, 0, 9, 0];
        let mut spells = op(11, 2, -1, 0);
        spells.results.spells_learned = [4, 5, 0, 0];
        let mut forget = op(16, 0, 0, 0);
        forget.results.spells_learned = [3, 0, 0, 0];
        let events = vec![
            equip,
            op(6, -2, 0, 0),
            op(7, 0, 4, 12),
            op(8, 0, 1, 0),
            op(8, 5, 8, 0),
            op(9, -1, 4, 0),
            op(10, 2, 0, -3),
            spells,
            op(12, 4, 7, 2),
            op(13, 2, -1, 2000),
            forget,
            op(17, 9, 12, 0),
            op(19, 6, 19, 11),
            op(20, 30, 10, 0),
        ];
        let n = events.len() as u16;
        let mut g = engine(events);
        g.set_named_units(vec![0, 42]);
        let mut w = MockWorld::new();
        assert_eq!(tick_at(&mut g, &mut w, 0), (1..=n).collect::<Vec<_>>());
        use Holder::*;
        assert_eq!(
            w.log,
            vec![
                Fx::Equip(Army(3), 1, [7, 0, 9, 0]),
                Fx::Equip(Building(2), 0, [0; 4]),
                Fx::Replace(Player, 4, 12),
                Fx::Speed(Player, 5),
                Fx::Speed(Army(5), -3),
                Fx::Faction(Building(1), 4),
                Fx::Relation(Army(2), 0, -3),
                Fx::SetSpells(Army(2), None, vec![4, 5]),
                Fx::Named(Army(4), 7, 2, 42),
                Fx::UnitXp(Army(2), None, 2000),
                Fx::Forget(3),
                Fx::Model(Army(9), 12),
                Fx::Target(6, 19, 11),
                Fx::Teleport(30, 10),
            ],
            "no items given, spells learned, XP or gold"
        );
    }

    #[test]
    fn opcode_8_speed_codes() {
        assert_eq!([1, 5, 6, 8, 0].map(speed_correction), [5, 1, -1, -3, 0]);
    }

    #[test]
    fn opcode_14_checks_lasting_spells() {
        let mut check = op(14, 2, -1, 0);
        check.results.spells_learned = [4, 5, 0, 0];
        let mut g = engine(vec![many(check)]);
        let mut w = MockWorld::new();
        w.spells_on.insert(Holder::Army(2), vec![4]);
        assert!(tick_at(&mut g, &mut w, 0).is_empty(), "spell 5 missing");
        w.spells_on.insert(Holder::Army(2), vec![5, 1, 4]);
        assert_eq!(tick_at(&mut g, &mut w, 10), vec![1], "order does not matter");
        assert!(w.log.is_empty(), "no spells learned");
    }

    #[test]
    fn opcode_15_records_the_campaign_branch() {
        let mut branch = op(15, 3, 2, 0);
        branch.results.chained_event = 2;
        let mut win = global();
        win.subordinate = 1;
        let mut g = EventEngine::from_parts(vec![branch, win], Vec::new(), 2, 0);
        let mut w = MockWorld::new();
        let out = g.tick(&mut w);
        assert!(out.contains(&EventOutcome::Victory(2)));
        assert_eq!(g.campaign_branch(), Some((3, 2)));
    }

    #[test]
    fn opcode_18_draws_a_random_flag() {
        let roll = many(op(18, 49, 57, 0));
        let wants = titled(global(), "=RAND2");
        let mut g = engine(vec![roll, wants]);
        let mut w = MockWorld::new();
        w.rolls = vec![53, 50];
        assert_eq!(tick_at(&mut g, &mut w, 0), vec![1]);
        assert_eq!(g.flags().collect::<Vec<_>>(), vec!["RAND5"]);
        assert_eq!(tick_at(&mut g, &mut w, DAY), vec![1, 2], "drawn anew: RAND2");
        assert_eq!(g.flags().collect::<Vec<_>>(), vec!["RAND2"]);
    }

    #[test]
    fn opcode_19_checks_an_army_position_and_sets_its_target() {
        let mut e = many(op(19, 6, 19, 11));
        (e.conditions.army_strength, e.conditions.gold, e.conditions.holiness_mana) = (6, 30, 10);
        let mut g = engine(vec![e]);
        let mut w = MockWorld::new();
        w.army_cells.insert(6, (29, 10));
        assert!(tick_at(&mut g, &mut w, 0).is_empty());
        w.army_cells.insert(6, (30, 10));
        assert_eq!(tick_at(&mut g, &mut w, 10), vec![1]);
        assert_eq!(w.log, vec![Fx::Target(6, 19, 11)]);
    }
}

#[cfg(test)]
mod real_maps {
    //! Every shipped map through a simulated timeline; skipped without `RAZDOR_DT_DIR`.
    use super::mock::MockWorld;
    use super::*;
    use crate::dt::install::DtInstall;

    struct Run {
        engine: EventEngine,
        questions: usize,
        rumours: usize,
        loop_guards: usize,
    }

    /// A player who visits every building and point in turn (one every two hours), beats one
    /// active army a day, meets every active army, hears every rumour and answers questions
    /// Yes and No in turn.
    fn play(s: &Scenario, days: u64) -> Run {
        let mut g = EventEngine::new(s);
        let mut w =
            MockWorld { archetype: 1, level: 5, gold: 1000, mana: 100, squads: 6, strength: 3000, ..MockWorld::new() };
        w.active.extend(s.armies.iter().filter(|a| a.is_active()).map(|a| a.id));
        w.home.extend(s.armies.iter().map(|a| a.id));
        for (i, b) in s.buildings.iter().enumerate() {
            if let Some(o) = b.owner() {
                w.building_owner.insert(i as u16 + 1, o);
            }
        }
        let mut places: Vec<Place> = (1..=s.buildings.len() as u16).map(Place::Building).collect();
        places.extend(s.points.iter().map(|p| Place::Point(p.id)));
        let mut run = Run { engine: g.clone(), questions: 0, rumours: 0, loop_guards: 0 };
        let start = s.header.start_time as u64;
        let mut yes = true;
        'days: for day in 0..days {
            if let Some(a) = w.active.iter().copied().filter(|a| !w.defeated.contains(a)).min() {
                w.defeated.insert(a);
                w.beaten.insert(a);
                w.active.remove(&a);
            }
            for slot in 0..12 {
                w.now = start + day * 1440 + slot * 120;
                w.met.extend(w.active.iter().copied());
                w.place = places.get((day * 12 + slot) as usize).copied();
                let mut out = g.tick(&mut w);
                for r in g.rumours(&w) {
                    run.rumours += 1;
                    out.extend(g.hear_rumour(&mut w, r));
                }
                for _ in 0..100 {
                    if g.pending_question().is_none() {
                        break;
                    }
                    run.questions += 1;
                    out.extend(g.answer(&mut w, yes));
                    yes = !yes;
                }
                run.loop_guards += out.iter().filter(|o| **o == EventOutcome::LoopGuard).count();
                if g.ended().is_some() {
                    break 'days;
                }
            }
        }
        run.engine = g;
        run
    }

    #[test]
    fn every_shipped_map_runs_a_simulated_timeline() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let mut extensions = 0;
        for m in &dt.maps {
            let s = m.load().unwrap();
            let run = play(&s, 60);
            let g = &run.engine;
            extensions += g.extensions().len();
            let fired = (1..=s.events.len() as u16).filter(|id| g.times_fired(*id) > 0).count();
            let quests = g.journal().len() + g.completed_quests().len();
            println!(
                "{}: {} events, {fired} fired ({} firings), {} questions, {} rumours, {quests} quests \
                 ({} completed), {} loop guards, ended {:?}, {} Community extensions",
                m.name,
                s.events.len(),
                g.total_fired(),
                run.questions,
                run.rumours,
                g.completed_quests().len(),
                run.loop_guards,
                g.ended(),
                g.extensions().len()
            );
            if m.name.starts_with("РК1") {
                assert!(fired >= 10, "{fired}");
                assert!(quests > 0);
            }
        }
        println!("Community extensions in all maps: {extensions}");
    }
}
