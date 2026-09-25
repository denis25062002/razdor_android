//! The scenario settings window: title and description, start date, victory and defeat
//! events, the three hero presets, the faction relations, campaign settings and the named
//! characters.

use macroquad::prelude::*;

use razdor::dt::dtm::{GameDate, Scenario};
use razdor::editor::defaults::DEFAULT_RELATIONS;
use razdor::editor::palette::{self, Names};
use razdor::editor::{Command, Settings};

use super::form::*;
use crate::ui::widgets::*;

#[derive(Default)]
pub struct SettingsState {
    pub tab: usize,
    pub scroll: f32,
}

/// What the window asks for.
pub enum SettingsAction {
    None,
    Apply(Command, String),
    /// Close the window and let the user click the start of hero preset `k`.
    PickStart(usize),
    Close,
}

const TABS: [&str; 7] = ["Scenario", "Knight", "Archmage", "Ranger", "Factions", "Campaign", "Characters"];

/// Relation presets of the factions tab (our own): the default, everyone allied, everyone
/// neutral, everyone at war (a faction always loves itself).
fn preset(k: usize) -> [[i8; 4]; 4] {
    let fill = |v: i8| std::array::from_fn(|r| std::array::from_fn(|c| if r == c { 3 } else { v }));
    match k {
        0 => DEFAULT_RELATIONS,
        1 => fill(2),
        2 => fill(0),
        _ => fill(-3),
    }
}

pub fn window(state: &mut SettingsState, s: &Scenario, names: &Names) -> SettingsAction {
    let (sw, sh) = (screen_width(), screen_height());
    draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.55));
    let w = 720.0f32.min(sw - 40.0);
    let h = (sh - 80.0).max(300.0);
    let r = Rect::new((sw - w) / 2.0, 40.0, w, h);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.095, 0.09, 0.98));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, ACCENT);
    text("Scenario settings", r.x + 14.0, r.y + 28.0, 24.0, ACCENT);
    let before = state.tab;
    tabs(r.x + 10.0, r.y + 40.0, r.w - 20.0, &TABS, &mut state.tab);
    if state.tab != before {
        state.scroll = 0.0;
    }
    let area = Rect::new(r.x + 20.0, r.y + 80.0, r.w - 40.0, r.h - 140.0);
    let orig = Settings {
        header: s.header.clone(),
        title: s.title.clone(),
        description: s.description.clone(),
        campaign_name: s.campaign_name.clone(),
        next_map: s.next_map.clone(),
        named_characters: s.named_characters.clone(),
    };
    let mut st = orig.clone();
    let mut f = Form::new("settings", area, state.scroll);
    let mut action = SettingsAction::None;
    let units = unit_options(names);
    match state.tab {
        0 => {
            f.text("title", "Title", &mut st.title);
            f.memo("description", "Description", &mut st.description, 6);
            f.note(&format!("Map size {} x {} cells.", s.width(), s.height()), DIM);
            f.heading("The clock starts at");
            let mut d = GameDate::from_minutes(st.header.start_time);
            f.num("year", "Year", &mut d.year, 0, 9999);
            f.num("month", "Month", &mut d.month, 1, 12);
            // The game shows days from 0.
            let mut day = d.day - 1;
            f.num("day", "Day (0-29)", &mut day, 0, 29);
            d.day = day + 1;
            f.num("hour", "Hour", &mut d.hour, 0, 23);
            st.header.start_time = d.to_minutes();
            f.heading("End of the scenario");
            let events = event_options_none(s);
            f.pick("victory", "Victory event", &mut st.header.victory_event, &events);
            f.pick("defeat", "Defeat event", &mut st.header.defeat_event, &events);
            f.note("The scenario ends when one of these events fires. Events are edited with the Events button.", DIM);
        }
        k @ 1..=3 => {
            let hero = &mut st.header.heroes[k - 1];
            f.heading(&format!("{} start", palette::HERO_CLASSES[k - 1]));
            f.num("x", "X", &mut hero.x, 0, s.width() as i64 - 1);
            f.num("y", "Y", &mut hero.y, 0, s.height() as i64 - 1);
            if f.button("Pick the start on the map", true) {
                action = SettingsAction::PickStart(k - 1);
            }
            f.num("gold", "Gold", &mut hero.gold, 0, i16::MAX as i64);
            f.num("mana", "Mana", &mut hero.mana, 0, i16::MAX as i64);
            f.pick("building", "Start building", &mut hero.start_building, &building_options(s));
            f.heading("Troops");
            f.troops("troops", &units, &mut hero.troops);
            f.heading("Artefacts");
            let arts = artefact_options(names);
            for i in 0..3 {
                f.pick(&format!("art{i}"), &format!("Artefact {}", i + 1), &mut hero.artifacts[i], &arts);
            }
            f.heading("Spells and prayers");
            let spells = spell_options(names);
            for i in 0..6 {
                f.pick(&format!("spell{i}"), &format!("Spell {}", i + 1), &mut hero.spells[i], &spells);
            }
        }
        4 => {
            f.note("How each faction (row) feels about each faction, -3 (war) to 3 (friends).", DIM);
            if let Some(k) = f.buttons(&["Default", "All allied", "All neutral", "All at war"]) {
                st.header.relations = preset(k);
                f.changed = Some("relations".into());
            }
            for (i, row) in palette::FACTIONS.iter().enumerate() {
                f.heading(row);
                f.relations(&format!("r{i}"), &mut st.header.relations[i]);
            }
        }
        5 => {
            f.pick("kind", "Scenario kind", &mut st.header.scenario_kind, &list_options(&palette::SCENARIO_KINDS, 0));
            f.text("campaign", "Campaign name", &mut st.campaign_name);
            f.text("next", "Next map file", &mut st.next_map);
            f.num("picture", "Built-in picture", &mut st.header.scenario_picture_index, 0, 255);
            f.heading("The hero keeps from the previous map");
            for (i, what) in palette::CARRY_OVER.iter().enumerate() {
                f.flag(&format!("carry{i}"), what, &mut st.header.carry_over[i]);
            }
        }
        _ => {
            f.note("Named characters are unit types with a name; armies, events and the hero's squad can use them.", DIM);
            let mut remove = None;
            for (i, nc) in st.named_characters.iter_mut().enumerate() {
                f.heading(&format!("Character {}", i + 1));
                f.text(&format!("name{i}"), "Name", &mut nc.name);
                f.pick(&format!("unit{i}"), "Unit", &mut nc.unit, &units);
                if f.button("Remove", true) {
                    remove = Some(i);
                }
            }
            if let Some(i) = remove {
                return SettingsAction::Apply(Command::RemoveNamedCharacter { index: i as u8 + 1 }, String::new());
            }
            if f.button("Add a named character", st.named_characters.len() < 32) {
                return SettingsAction::Apply(Command::AddNamedCharacter { unit: 1, name: "New character".into() }, String::new());
            }
        }
    }
    let content = f.content_height();
    if mouse_in(area.x, area.y, area.w, area.h) && !popup_open() {
        let wh = wheel();
        if wh != 0.0 {
            state.scroll -= wh * 40.0;
        }
    }
    state.scroll = state.scroll.clamp(0.0, (content - area.h + 20.0).max(0.0));
    if button(r.right() - 150.0, r.bottom() - 52.0, 130.0, 40.0, "Close", true) || (!typing() && is_key_pressed(KeyCode::Escape)) {
        return SettingsAction::Close;
    }
    if st != orig {
        let key = f.changed.clone().unwrap_or_else(|| "settings".into());
        return SettingsAction::Apply(Command::SetSettings(Box::new(st)), key);
    }
    action
}
