//! The event editor window: the event list (filtered by type, group and title) with new,
//! duplicate and delete, and the selected event's fields in the original editor's grouping
//! (event and player, event and heroes, result 1, result 2), plus the places that list the
//! event and the Community opcode view. The texts are Razdor's own; names of units,
//! artefacts, spells, buildings, armies and events come from the map and the install.
//!
//! All logic is in `razdor::editor::events` (tested there); this module draws and returns
//! the command a change makes.

use macroquad::prelude::*;

use razdor::dt::dtm::{Event, GameDate, Scenario};
use razdor::editor::events::{self as ev, Arg, EventFilter};
use razdor::editor::palette::Names;
use razdor::editor::{Command, Target};
use razdor::rules::events::EventEdit;

use super::form::*;
use crate::ui::widgets::*;

/// The window's state between frames.
#[derive(Default)]
pub struct EventsState {
    pub selected: Option<u16>,
    pub filter: EventFilter,
    list_scroll: usize,
    pub tab: usize,
    scroll: f32,
    /// The picture file to import.
    picture_path: String,
    shown: Option<u16>,
}

impl EventsState {
    /// Opens the window on event `id`.
    pub fn select(&mut self, id: u16) {
        self.selected = Some(id);
        self.filter = EventFilter::default();
    }
}

/// What the window asks for.
pub enum EventsAction {
    None,
    Apply(Command, String),
    /// Delete this event (the caller confirms when it is still referred to).
    Delete(u16),
    /// A message for the status line.
    Status(String),
    Close,
}

const TABS: [&str; 6] = ["Event and player", "Event and heroes", "Result 1", "Result 2", "Places", "Community"];

/// Group colours of the list (our own palette).
const GROUP_COLOURS: [Color; 6] = [
    Color::new(0.55, 0.55, 0.55, 1.0),
    Color::new(0.35, 0.7, 0.35, 1.0),
    Color::new(0.35, 0.5, 0.9, 1.0),
    Color::new(0.9, 0.8, 0.3, 1.0),
    Color::new(0.85, 0.35, 0.3, 1.0),
    Color::new(0.7, 0.4, 0.8, 1.0),
];

fn group_colour(g: u8) -> Color {
    GROUP_COLOURS.get(g as usize).copied().unwrap_or(DIM)
}

fn kind_options() -> Options {
    ev::KIND_LABELS.iter().map(|(k, l)| (*k as i64, l.to_string())).collect()
}

fn group_options() -> Options {
    (0..ev::GROUPS).map(|g| (g as i64, format!("Group {}", g + 1))).collect()
}

fn owner_options() -> Options {
    ev::OWNERS.iter().map(|(k, l)| (*k as i64, l.to_string())).collect()
}

/// Quests, plus the current value if it is not one (so it still shows).
fn quest_options(s: &Scenario, current: u16) -> Options {
    let mut o: Options = vec![(0, "(none)".into())];
    o.extend(event_options(s).into_iter().filter(|(id, _)| ev::is_quest(s, *id as u16) || *id == current as i64));
    o
}

fn removed_unit_options(n: &Names) -> Options {
    let mut o = unit_options(n);
    o.push((ev::REMOVE_ADDED_UNIT as i64, "A unit an event added".into()));
    o.push((ev::REMOVE_ANY_UNIT as i64, "Any unit".into()));
    o
}

fn picture_options(n: &Names) -> Options {
    let mut o: Options = vec![(0, "(none)".into()), (ev::PICTURE_VICTORY as i64, "Victory".into()), (ev::PICTURE_DEFEAT as i64, "Defeat".into())];
    o.extend(n.units.iter().filter(|u| u.id < 200).map(|u| (u.id as i64, format!("Portrait: {} ({})", u.name, u.id))));
    o
}

/// What a Community "holder" argument names.
fn holder_label(s: &Scenario, v: i16) -> String {
    match v {
        0 => "the player's army".into(),
        v if v > 0 => {
            if (v as usize) <= s.armies.len() {
                army_label(s, v as u8)
            } else {
                format!("army {v} (missing)")
            }
        }
        v => building_options(s).into_iter().find(|b| b.0 == v.unsigned_abs() as i64).map_or(format!("building {} (missing)", -v), |b| b.1),
    }
}

/// Two event pickers under a check box (the "happened" conditions).
fn event_pair(f: &mut Form, k: &str, label: &str, check: &mut u8, ids: &mut [u16; 2], events: &Options) {
    f.flag(&format!("{k}c"), label, check);
    let (mut a, mut b) = (ids[0] as i64, ids[1] as i64);
    f.pick_row(k, &mut [(&mut a, events, 1.0), (&mut b, events, 1.0)]);
    *ids = [a as u16, b as u16];
}

fn army_pair(f: &mut Form, k: &str, ids: &mut [u8; 2], armies: &Options) {
    let (mut a, mut b) = (ids[0] as i64, ids[1] as i64);
    f.pick_row(k, &mut [(&mut a, armies, 1.0), (&mut b, armies, 1.0)]);
    *ids = [a as u8, b as u8];
}

fn four_picks(f: &mut Form, k: &str, v: &mut [u8; 4], options: &Options) {
    let mut x = v.map(|b| b as i64);
    let [a, b, c, d] = &mut x;
    f.pick_row(k, &mut [(a, options, 1.0), (b, options, 1.0)]);
    f.pick_row(&format!("{k}b"), &mut [(c, options, 1.0), (d, options, 1.0)]);
    *v = x.map(|b| b as u8);
}

pub fn window(state: &mut EventsState, s: &Scenario, names: &Names) -> EventsAction {
    let (sw, sh) = (screen_width(), screen_height());
    draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.55));
    let w = (sw - 40.0).clamp(300.0, 1180.0);
    let h = (sh - 60.0).max(300.0);
    let r = Rect::new((sw - w) / 2.0, 30.0, w, h);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.095, 0.09, 0.98));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, ACCENT);
    text("Events", r.x + 14.0, r.y + 28.0, 24.0, ACCENT);
    if button(r.right() - 130.0, r.bottom() - 50.0, 116.0, 38.0, "Close", true) || (!typing() && !popup_open() && is_key_pressed(KeyCode::Escape)) {
        return EventsAction::Close;
    }
    if state.selected.is_some_and(|id| s.event(id).is_none()) {
        state.selected = None;
    }
    let list = Rect::new(r.x + 10.0, r.y + 40.0, 330.0f32.min(r.w * 0.34), r.h - 50.0);
    if let Some(a) = event_list(state, s, list) {
        return a;
    }
    let area = Rect::new(list.right() + 14.0, r.y + 40.0, r.right() - list.right() - 26.0, r.h - 100.0);
    match state.selected {
        Some(id) => properties(state, s, names, id, area),
        None => {
            text("Pick an event from the list, or make a new one.", area.x, area.y + 20.0, 17.0, DIM);
            EventsAction::None
        }
    }
}

/// The list column; returns an action for its buttons.
fn event_list(state: &mut EventsState, s: &Scenario, r: Rect) -> Option<EventsAction> {
    let mut y = r.y;
    // Type filter.
    let kinds: [(Option<u8>, &str); 5] = [(None, "All"), (Some(1), "Global"), (Some(2), "Local"), (Some(3), "Quest"), (Some(4), "Rumour")];
    let bw = (r.w - 16.0) / 5.0;
    for (i, (k, l)) in kinds.iter().enumerate() {
        if toggle_button(r.x + i as f32 * (bw + 4.0), y, bw, 26.0, l, state.filter.kind == *k) {
            state.filter.kind = *k;
            state.list_scroll = 0;
        }
    }
    y += 32.0;
    let mut groups: Options = vec![(-1, "All groups".into())];
    groups.extend(group_options());
    let g = state.filter.group.map_or(-1, |g| g as i64);
    if let Some(v) = dropdown("events:group", r.x, y, 120.0, g, &groups) {
        state.filter.group = (v >= 0).then_some(v as u8);
        state.list_scroll = 0;
    }
    text("Find", r.x + 128.0, y + 17.0, 16.0, DIM);
    if text_field("events:search", r.x + 166.0, y, r.w - 166.0, 24.0, &mut state.filter.search, false) {
        state.list_scroll = 0;
    }
    y += 32.0;
    let ids = ev::filtered(s, &state.filter);
    let rows_h = r.bottom() - y - 76.0;
    let rows = (rows_h / 24.0).floor().max(1.0) as usize;
    if mouse_in(r.x, y, r.w, rows_h) && !popup_open() {
        let wh = wheel();
        if wh > 0.0 {
            state.list_scroll = state.list_scroll.saturating_sub(3);
        } else if wh < 0.0 {
            state.list_scroll += 3;
        }
    }
    state.list_scroll = state.list_scroll.min(ids.len().saturating_sub(rows));
    draw_rectangle(r.x, y, r.w, rows_h, FIELD_BG);
    for (i, id) in ids.iter().enumerate().skip(state.list_scroll).take(rows) {
        let ry = y + (i - state.list_scroll) as f32 * 24.0;
        let e = &s.events[*id as usize - 1];
        let on = state.selected == Some(*id);
        let hover = mouse_in(r.x, ry, r.w, 23.0);
        if on || hover {
            draw_rectangle(r.x, ry, r.w, 23.0, if on { Color::new(0.45, 0.34, 0.16, 1.0) } else { Color::new(0.25, 0.2, 0.13, 1.0) });
        }
        draw_rectangle(r.x + 3.0, ry + 5.0, 8.0, 13.0, group_colour(e.group_colour));
        let kind = ev::kind_label(e.kind).chars().next().unwrap_or('?');
        let mut l = format!("{id:>4} {kind}  {}", e.title_text().trim());
        while measure(&l, 16.0).width > r.w - 20.0 && !l.is_empty() {
            l.pop();
        }
        text(&l, r.x + 15.0, ry + 17.0, 16.0, INK);
        if hover && clicked() {
            state.selected = Some(*id);
        }
    }
    text(&format!("{} of {} events", ids.len(), s.events.len()), r.x, r.bottom() - 58.0, 15.0, DIM);
    let by = r.bottom() - 40.0;
    let bw = (r.w - 8.0) / 3.0;
    let full = s.events.len() >= ev::MAX_EVENTS;
    if small_button(r.x, by, bw, 30.0, "New", !full) {
        let kind = state.filter.kind.unwrap_or(1);
        return Some(EventsAction::Apply(Command::NewEvent { kind }, String::new()));
    }
    if small_button(r.x + bw + 4.0, by, bw, 30.0, "Duplicate", state.selected.is_some() && !full) {
        let id = state.selected?;
        return Some(EventsAction::Apply(Command::DuplicateEvent { id }, String::new()));
    }
    if small_button(r.x + 2.0 * (bw + 4.0), by, bw, 30.0, "Delete", state.selected.is_some()) {
        return Some(EventsAction::Delete(state.selected?));
    }
    None
}

fn properties(state: &mut EventsState, s: &Scenario, names: &Names, id: u16, r: Rect) -> EventsAction {
    let orig = &s.events[id as usize - 1];
    let mut e: Event = orig.clone();
    if state.shown != Some(id) {
        state.shown = Some(id);
        state.scroll = 0.0;
    }
    // The title (its name part; the flag fields are on the tabs).
    text(&format!("Event {id}"), r.x, r.y + 17.0, 18.0, ACCENT);
    let mut name = ev::split_title(&e.title).name;
    if text_field(&format!("e{id}:name"), r.x + 90.0, r.y, r.w - 90.0, 24.0, &mut name, false) {
        e.title = ev::with_name(&e.title, &name);
    }
    let before = state.tab;
    tabs(r.x, r.y + 30.0, r.w, &TABS, &mut state.tab);
    if state.tab != before {
        state.scroll = 0.0;
    }
    let area = Rect::new(r.x, r.y + 66.0, r.w, r.h - 66.0);
    let mut f = Form::new(&format!("e{id}"), area, state.scroll);
    f.label_w = (area.w * 0.36).min(230.0);
    let mut action = EventsAction::None;
    match state.tab {
        0 => tab_player(&mut f, s, &mut e),
        1 => tab_heroes(&mut f, s, names, &mut e),
        2 => tab_result1(&mut f, s, names, &mut e),
        3 => action = tab_result2(&mut f, s, names, &mut e, &mut state.picture_path),
        4 => {
            if let Some(a) = tab_places(&mut f, s, id) {
                return a;
            }
        }
        _ => tab_community(&mut f, s, names, id, &mut e),
    }
    let content = f.content_height();
    if mouse_in(area.x, area.y, area.w, area.h) && !popup_open() {
        let wh = wheel();
        if wh != 0.0 {
            state.scroll -= wh * 40.0;
        }
    }
    state.scroll = state.scroll.clamp(0.0, (content - area.h + 20.0).max(0.0));
    if !matches!(action, EventsAction::None) {
        return action;
    }
    if e != *orig {
        let key = f.changed.clone().unwrap_or_else(|| format!("e{id}:title"));
        return EventsAction::Apply(Command::SetEvent { id, event: Box::new(e) }, key);
    }
    EventsAction::None
}

/// Tab 1: type, group, time window, archetype, event and flag conditions, the question,
/// defeated and met armies, the hero's stats.
fn tab_player(f: &mut Form, s: &Scenario, e: &mut Event) {
    f.heading("Event");
    f.pick("kind", "Type", &mut e.kind, &kind_options());
    f.pick("group", "Group (list colour)", &mut e.group_colour, &group_options());
    f.pick("archetype", "For which hero", &mut e.archetype, &list_options(&ev::ARCHETYPES, 0));
    f.heading("When");
    let mut relative = ev::is_relative(e) as u8;
    f.flag("relative", "Relative time only (another event sets its start)", &mut relative);
    if (relative != 0) != ev::is_relative(e) {
        ev::set_relative(e, relative != 0, s.header.start_time);
    }
    if !ev::is_relative(e) {
        let mut d = GameDate::from_minutes(e.start_time);
        f.num("year", "Start: year", &mut d.year, 0, 2999);
        f.num("month", "Month", &mut d.month, 1, 12);
        let mut day = d.day - 1;
        f.num("day", "Day (0-29)", &mut day, 0, 29);
        d.day = day + 1;
        f.num("hour", "Hour", &mut d.hour, 0, 23);
        e.start_time = d.to_minutes();
    }
    let mut hours = ev::duration_hours(e);
    f.num("duration", "Open for (hours)", &mut hours, 0, ev::MAX_DURATION_HOURS as i64);
    if hours != ev::duration_hours(e) {
        ev::set_duration_hours(e, hours);
    }
    if e.duration > 0 {
        f.note(&format!("As the game reads it: {} hours (it takes the stored minutes as hours).", e.duration), DIM);
    }
    let mut days = ev::repeat_days(e);
    f.num("repeat", "Repeat every (days, 0 = no)", &mut days, 0, ev::MAX_REPEAT_DAYS as i64);
    if days != ev::repeat_days(e) {
        ev::set_repeat_days(e, days);
    }
    let mut many = ev::repeatable(e) as u8;
    f.flag("many", "Can happen many times", &mut many);
    if (many != 0) != ev::repeatable(e) {
        ev::set_repeatable(e, many != 0);
    }
    f.flag("subordinate", "Subordinate (runs only when another event chains it)", &mut e.subordinate);

    f.heading("Other events");
    let events = event_options_none(s);
    let c = &mut e.conditions;
    event_pair(f, "yes", "Happened, answered yes", &mut c.happened_yes_check, &mut c.happened_yes, &events);
    event_pair(f, "no", "Happened, answered no", &mut c.happened_no_check, &mut c.happened_no, &events);
    event_pair(f, "not", "Not happened", &mut c.not_happened_check, &mut c.not_happened, &events);
    let parts = ev::split_title(&e.title);
    let mut test = parts.test.clone();
    f.text("test_flag", "Check flag (X, or /X = not set)", &mut test);
    if test != parts.test {
        e.title = ev::with_flags(&e.title, &parts.set, &test);
    }
    for m in ev::flag_problems(&e.title) {
        f.note(&format!("Flag script: {m}"), RED);
    }

    f.heading("Question");
    let c = &mut e.conditions;
    f.flag("ask", "Ask a yes/no question first", &mut c.confirm_question);
    f.flag("repeat_yes", "Ask again after a yes", &mut e.results.repeat_after_yes);
    f.memo("question", "Question text", &mut e.question, 3);

    f.heading("Armies");
    let armies = army_options(s);
    let c = &mut e.conditions;
    f.flag("defeated_c", "The player beat these armies", &mut c.defeated_check);
    army_pair(f, "defeated", &mut c.defeated_armies, &armies);
    f.pick("meet", "Meeting this army", &mut c.meet_army, &armies);
    f.note("Needed to talk to a friendly army instead of fighting it.", DIM);

    f.heading("The hero");
    f.flag("stats", "Check the current stats", &mut c.stats_check);
    f.threshold("level", "Level (from 0)", &mut c.level, 99);
    f.threshold("gold", "Gold", &mut c.gold, i16::MAX as i64);
    f.threshold("mana", "Holiness and mana", &mut c.holiness_mana, i16::MAX as i64);
    f.threshold("squads", "Squads in the army", &mut c.squad_count, 12);
    f.threshold("strength", "Army strength", &mut c.army_strength, i16::MAX as i64);
    f.note("A value of 0 is not checked; the switch picks at least or at most.", DIM);
    f.flag("one_hp", "Result: the hero is left with 1 HP", &mut e.results.hero_one_hp);
}

/// Tab 2: who owns buildings, artefacts and named squads; armies beaten, active, inactive,
/// at home.
fn tab_heroes(f: &mut Form, s: &Scenario, names: &Names, e: &mut Event) {
    let owners = owner_options();
    let c = &mut e.conditions;
    f.heading("Buildings");
    f.flag("bld_c", "These buildings belong to", &mut c.buildings_check);
    let buildings = building_options(s);
    for k in 0..3 {
        let (mut b, mut o) = (c.buildings[k] as i64, c.buildings_owner[k] as i64);
        f.pick_row(&format!("bld{k}"), &mut [(&mut b, &buildings, 2.0), (&mut o, &owners, 1.0)]);
        (c.buildings[k], c.buildings_owner[k]) = (b as u8, o as u8);
    }
    f.heading("Artefacts");
    f.flag("art_c", "These artefacts are held by", &mut c.artifacts_check);
    let arts = artefact_options(names);
    for k in 0..3 {
        let (mut a, mut o) = (c.artifacts[k] as i64, c.artifacts_owner[k] as i64);
        f.pick_row(&format!("art{k}"), &mut [(&mut a, &arts, 2.0), (&mut o, &owners, 1.0)]);
        (c.artifacts[k], c.artifacts_owner[k]) = (a as u8, o as u8);
    }
    f.heading("Named squads");
    f.flag("unit_c", "Squads of this unit and name are in the army of", &mut c.units_check);
    let units = unit_options(names);
    let named = named_options(s);
    for k in 0..3 {
        let (mut u, mut n, mut o) = (c.units[k] as i64, c.units_named[k] as i64, c.units_owner[k] as i64);
        f.pick_row(&format!("unit{k}"), &mut [(&mut u, &units, 1.4), (&mut n, &named, 1.2), (&mut o, &owners, 1.0)]);
        (c.units[k], c.units_named[k], c.units_owner[k]) = (u as u8, n as u8, o as u8);
    }
    f.heading("Armies");
    let armies = army_options(s);
    f.flag("beaten_c", "Beaten by anyone", &mut c.beaten_check);
    army_pair(f, "beaten", &mut c.beaten_armies, &armies);
    f.pick("active", "Army is active", &mut c.army_active, &armies);
    f.pick("inactive", "Army is not active", &mut c.army_inactive, &armies);
    f.pick("home", "Army is in its home building", &mut c.army_at_home, &armies);
}

/// Tab 3: the message, chained event, quest, resources, relative event, delay, flag,
/// units added, spells, artefacts gained.
fn tab_result1(f: &mut Form, s: &Scenario, names: &Names, e: &mut Event) {
    f.memo("message", "Message (empty: the event happens silently)", &mut e.message, 5);
    let events = event_options_none(s);
    let r = &mut e.results;
    f.pick("chained", "Chained event (runs at once)", &mut r.chained_event, &events);
    let quests = quest_options(s, r.completes_quest);
    f.pick("quest", "Quest completed", &mut r.completes_quest, &quests);
    f.heading("Resources");
    if ev::opcode(e).is_some() {
        f.note("This event runs a Community opcode: these three fields are its arguments (see Community).", ACCENT);
    }
    let r = &mut e.results;
    f.num("xp", "Experience", &mut r.experience, -32000, 32000);
    f.num("gold", "Gold", &mut r.gold, -32000, 32000);
    f.num("mana", "Mana", &mut r.mana, -32000, 32000);
    f.heading("Time");
    f.pick("rel_event", "Relative event", &mut r.relative_event, &events);
    f.num("relative_h", "... starts in (hours)", &mut r.relative_delay_hours, 0, 5000);
    f.num("delay", "The hero waits (hours)", &mut r.delay_hours, 0, 65535);
    let parts = ev::split_title(&e.title);
    let mut set = parts.set.clone();
    f.text("set_flag", "Flag (+X sets, -X clears)", &mut set);
    if set != parts.set {
        e.title = ev::with_flags(&e.title, &set, &parts.test);
    }
    if !set.is_empty() && !set.starts_with(['+', '-']) {
        f.note("Start the flag with + or -, or it is not saved (as in the original).", RED);
    }
    f.heading("Units joining the army");
    let units = unit_options(names);
    let named = named_options(s);
    let r = &mut e.results;
    for k in 0..4 {
        let (mut u, mut n) = (r.units_add[k] as i64, r.units_add_named[k] as i64);
        f.pick_row(&format!("add{k}"), &mut [(&mut u, &units, 1.0), (&mut n, &named, 1.0)]);
        (r.units_add[k], r.units_add_named[k]) = (u as u8, n as u8);
    }
    f.pick("from_army", "Taken from army", &mut r.units_from_army, &army_options(s));
    f.heading("Spells learned");
    four_picks(f, "spells", &mut r.spells_learned, &spell_options(names));
    f.heading("Artefacts gained");
    four_picks(f, "arts_add", &mut r.artifacts_add, &artefact_options(names));
}

/// Tab 4: units removed, artefacts lost, lanterns, armies, patrol, class, battle, spell,
/// "no meeting", picture.
fn tab_result2(f: &mut Form, s: &Scenario, names: &Names, e: &mut Event, picture_path: &mut String) -> EventsAction {
    let armies = army_options(s);
    let named = named_options(s);
    let r = &mut e.results;
    f.heading("Units leaving the army");
    f.note("The last to join leaves first.", DIM);
    let removed = removed_unit_options(names);
    for k in 0..4 {
        let (mut u, mut n) = (r.units_remove[k] as i64, r.units_remove_named[k] as i64);
        f.pick_row(&format!("rem{k}"), &mut [(&mut u, &removed, 1.0), (&mut n, &named, 1.0)]);
        (r.units_remove[k], r.units_remove_named[k]) = (u as u8, n as u8);
    }
    f.pick("to_army", "They go to army", &mut r.removed_units_to_army, &armies);
    f.flag("to_hero", "Move that army to the hero", &mut r.move_to_hero);
    f.heading("Artefacts lost");
    four_picks(f, "arts_rem", &mut r.artifacts_remove, &artefact_options(names));
    f.heading("Lanterns lit");
    let points = point_options(s);
    let mut l = r.light_lanterns.map(|v| v as i64);
    {
        let [a, b, c, d] = &mut l;
        f.pick_row("lamps", &mut [(a, &points, 1.0), (b, &points, 1.0)]);
        f.pick_row("lampsb", &mut [(c, &points, 1.0), (d, &points, 1.0)]);
    }
    r.light_lanterns = l.map(|v| v as u16);
    f.heading("Armies");
    f.pick("show", "Show army", &mut r.show_army, &armies);
    army_pair(f, "activate", &mut r.activate_armies, &armies);
    f.pick("deactivate", "Deactivate army", &mut r.deactivate_army, &armies);
    let opcode = ev::opcode(e).is_some();
    let r = &mut e.results;
    if opcode {
        f.note("Patrol change: this event's patrol value selects a Community opcode (see Community).", ACCENT);
    } else {
        f.pick("patrol_army", "Change the patrol of", &mut r.patrol_army, &armies);
        f.num("patrol", "... by", &mut r.patrol_delta, -120, 120);
    }
    f.pick("battle", "Start a battle with", &mut r.start_battle_with, &armies);
    f.flag("generate", "Generate that army to match the player", &mut e.generate_battle_army);
    let r = &mut e.results;
    f.flag("no_meeting", "No meeting with the army (for later checks)", &mut r.no_meeting);
    if r.no_meeting != 0 && e.conditions.meet_army != 0 {
        f.note("! This event also needs a meeting with an army.", ACCENT);
    }
    f.heading("Hero");
    let r = &mut e.results;
    f.pick("class", "New hero class", &mut r.new_hero_class, &unit_options(names));
    f.pick("spell", "Spell on the player's army", &mut r.cast_spell, &spell_options(names));
    f.heading("Picture");
    f.pick("picture", "Standard picture", &mut r.picture, &picture_options(names));
    match e.custom_picture.as_deref().map(ev::picture_size) {
        Some(Some((w, h))) => f.note(&format!("Own picture: {w} x {h} (shown instead of the standard one)."), INK),
        Some(None) => f.note("Own picture: stored, but its size does not match its data.", RED),
        None => f.note("No own picture.", DIM),
    }
    f.text("pic_path", "Import a PNG (path)", picture_path);
    let mut action = EventsAction::None;
    match f.buttons(&["Import picture", "Remove own picture"]) {
        Some(0) => match std::fs::read(picture_path.trim()).ok().and_then(|b| Image::from_file_with_format(&b, None).ok()) {
            Some(img) => match ev::picture_from_rgba(img.width as u32, img.height as u32, &img.bytes) {
                Some(p) => {
                    e.custom_picture = Some(p);
                    f.changed = Some("picture_file".into());
                }
                None => action = EventsAction::Status("The picture is empty.".into()),
            },
            None => action = EventsAction::Status(format!("Cannot read a picture from \"{}\".", picture_path.trim())),
        },
        Some(_) => {
            e.custom_picture = None;
            f.changed = Some("picture_file".into());
        }
        None => {}
    }
    f.note("Imported pictures are scaled to 128 x 128 and stored as 16-bit colour.", DIM);
    action
}

/// Tab 5: the buildings and points that list the event, and what refers to it.
fn tab_places(f: &mut Form, s: &Scenario, id: u16) -> Option<EventsAction> {
    let (buildings, points) = ev::places_of(s, id);
    f.heading("Checked in these buildings");
    let bopts = building_options(s);
    for b in &buildings {
        let label = bopts.iter().find(|o| o.0 == *b as i64).map_or(format!("#{b}"), |o| o.1.clone());
        f.note(&label, INK);
        if f.button("Detach", true) {
            return Some(EventsAction::Apply(Command::DetachEvent { place: Target::Building(*b), event: id }, String::new()));
        }
    }
    let mut add: i64 = 0;
    let mut o: Options = vec![(0, "Attach to a building...".into())];
    o.extend(bopts.into_iter().skip(1).filter(|b| !buildings.contains(&(b.0 as u16))));
    f.pick("attach_b", "", &mut add, &o);
    if add > 0 {
        return Some(EventsAction::Apply(Command::AttachEvent { place: Target::Building(add as u16), event: id }, String::new()));
    }
    f.heading("Checked on these points");
    let popts = point_options(s);
    for p in &points {
        let label = popts.iter().find(|o| o.0 == *p as i64).map_or(format!("#{p}"), |o| o.1.clone());
        f.note(&label, INK);
        if f.button("Detach", true) {
            return Some(EventsAction::Apply(Command::DetachEvent { place: Target::Point(*p), event: id }, String::new()));
        }
    }
    let mut add: i64 = 0;
    let mut o: Options = vec![(0, "Attach to a point...".into())];
    o.extend(popts.into_iter().skip(1).filter(|p| !points.contains(&(p.0 as u8))));
    f.pick("attach_p", "", &mut add, &o);
    if add > 0 {
        return Some(EventsAction::Apply(Command::AttachEvent { place: Target::Point(add as u8), event: id }, String::new()));
    }
    f.note("A point holds at most 5 events. Global events need no place.", DIM);
    f.heading("Referred to by");
    let refs = ev::references_to(s, id);
    if refs.is_empty() {
        f.note("Nothing.", DIM);
    }
    for r in refs {
        f.note(&r.to_string(), INK);
    }
    None
}

/// Tab 6: the Community opcode ("no meeting" + patrol value) and its arguments.
fn tab_community(f: &mut Form, s: &Scenario, names: &Names, id: u16, e: &mut Event) {
    f.note("The Community Update reads \"no meeting\" with a patrol change of 1-20 as an opcode; its arguments are the experience, gold and mana changes of Result 1, which are then not given.", DIM);
    let mut ops: Options = vec![(0, "(none)".into())];
    ops.extend(ev::OPCODES.iter().map(|o| (o.code as i64, format!("{} {}", o.code, o.name))));
    let mut op = ev::opcode(e).unwrap_or(0);
    f.pick("opcode", "Opcode", &mut op, &ops);
    if Some(op) != ev::opcode(e).or(Some(0)) {
        ev::set_opcode(e, (op > 0).then_some(op));
    }
    if ev::opcode(e).is_none() {
        let r = &e.results;
        if r.no_meeting != 0 && r.cast_spell != 0 {
            f.note("\"No meeting\" + a spell: the spell is lifted from the player instead of cast.", INK);
        }
        if r.no_meeting != 0 && e.conditions.units_check != 0 {
            f.note("\"No meeting\" + named squads: the named unit's class is checked too.", INK);
        }
        return;
    }
    let Some(info) = ev::opcode_info(op) else { return };
    f.heading(info.name);
    if info.condition {
        f.note("A condition: the event fires only when the check holds.", DIM);
    }
    let mut args = ev::opcode_args(e);
    let fields: Options = ev::EVENT_FIELDS.iter().map(|(o, l)| (*o as i64, format!("{l} ({o})"))).collect();
    for (k, arg) in info.args.iter().enumerate() {
        let Some((label, kind)) = arg else { continue };
        let key = format!("arg{k}");
        match kind {
            Arg::EventField => f.pick(&key, label, &mut args[k], &fields),
            Arg::Named => f.pick(&key, label, &mut args[k], &named_options(s)),
            Arg::Unit => f.pick(&key, label, &mut args[k], &unit_options(names)),
            _ => f.num(&key, label, &mut args[k], -32000, 32000),
        }
        match kind {
            Arg::Holder => f.note(&format!("= {}", holder_label(s, args[k])), DIM),
            Arg::Army if args[k] > 0 => f.note(&format!("= {}", holder_label(s, args[k])), DIM),
            Arg::EventShift => f.note(&format!("= {}", ev::event_label(s, (id as i64 + args[k] as i64).clamp(0, u16::MAX as i64) as u16)), DIM),
            _ => {}
        }
    }
    ev::set_opcode_args(e, args);
    if let Some(uses) = info.uses {
        f.note(&format!("Also uses {uses}."), INK);
    }
    if (1..=5).contains(&op) {
        f.heading("Second setting");
        let mut on = ev::second_edit(e).is_some() as u8;
        f.flag("second", "Also a second edit (in the conditions)", &mut on);
        let mut x = ev::second_edit(e).unwrap_or(EventEdit { action: 1, shift: 0, field: 85, value: 0 });
        if on != 0 {
            let actions: Options = ev::OPCODES[..5].iter().map(|o| (o.code as i64, o.name.to_string())).collect();
            f.pick("second_action", "Action (squad count)", &mut x.action, &actions);
            f.num("second_shift", "Target event (gold)", &mut x.shift, -32000, 32000);
            f.note(&format!("= {}", ev::event_label(s, (id as i64 + x.shift as i64).clamp(0, u16::MAX as i64) as u16)), DIM);
            f.pick("second_field", "Field (level)", &mut x.field, &fields);
            f.num("second_value", "Value (holiness and mana)", &mut x.value, -32000, 32000);
        }
        ev::set_second_edit(e, (on != 0).then_some(x));
    }
    if op == 19 {
        f.heading("Position check");
        let c = &mut e.conditions;
        f.num("pos_army", "Army (strength field)", &mut c.army_strength, 0, 255);
        f.num("pos_x", "X (gold field)", &mut c.gold, 0, 32000);
        f.num("pos_y", "Y (holiness field)", &mut c.holiness_mana, 0, 32000);
        f.note("0 = no check.", DIM);
    }
}
