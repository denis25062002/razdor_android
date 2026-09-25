//! Saving and loading (video notes §6): the save window with a name, the load window with
//! the original's two tabs (manual saves and autosaves, newest first, with the scenario, the
//! hero and the in-game date), the Esc menu, and the autosaves before every battle and at the
//! 12:00 report. Files live in the player's data folder ([`save::default_dir`]).

use std::path::PathBuf;

use macroquad::prelude::*;

use razdor::rules::game::{Foe, Game};
use razdor::rules::save::{self, SaveEntry, SaveKind};

use super::assets::Assets;
use super::audio::Settings;
use super::battle_view::BattleView;
use super::widgets::*;
use super::world_view;
use super::Screen;

const ROW: Color = Color::new(0.16, 0.12, 0.10, 1.0);
const ROWS: usize = 10;

/// Where a save or load window returns to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Back {
    Map,
    Menu,
    Title,
}

impl Back {
    fn screen(self) -> Screen {
        match self {
            Back::Map => Screen::WorldMap,
            Back::Menu => Screen::Menu,
            Back::Title => Screen::ScenarioSelect,
        }
    }
}

pub struct SaveView {
    pub name: String,
    pub entries: Vec<SaveEntry>,
    pub back: Back,
}

impl SaveView {
    pub fn new(game: &Game, back: Back) -> SaveView {
        let name = format!("{} {}", game.world.title.trim(), save::date_name(&game.clock));
        SaveView { name, entries: save::default_dir().map_or_else(Vec::new, |d| save::list(&d, SaveKind::Manual)), back }
    }
}

pub struct LoadView {
    pub tab: SaveKind,
    pub entries: Vec<SaveEntry>,
    pub selected: usize,
    pub scroll: usize,
    pub back: Back,
}

impl LoadView {
    pub fn new(back: Back) -> LoadView {
        let mut v = LoadView { tab: SaveKind::Manual, entries: Vec::new(), selected: 0, scroll: 0, back };
        v.refresh();
        if v.entries.is_empty() {
            v.tab = SaveKind::Auto;
            v.refresh();
            if v.entries.is_empty() {
                v.tab = SaveKind::Manual;
            }
        }
        v
    }

    fn refresh(&mut self) {
        self.entries = save::default_dir().map_or_else(Vec::new, |d| save::list(&d, self.tab));
        self.selected = 0;
        self.scroll = 0;
    }
}

/// The autosave name before a battle: "Battle - <foe>" (the footage: "Битва - Замок …").
fn battle_name(game: &Game) -> String {
    let who = match game.foe {
        Some(Foe::Army(i)) => game.world.armies.get(i).map(|a| if a.name.is_empty() { a.leader_name.clone() } else { a.name.clone() }),
        Some(Foe::Garrison(l)) => game.world.locations.get(l).map(|l| l.name.clone()),
        None => None,
    };
    match who.filter(|w| !w.trim().is_empty()) {
        Some(w) => format!("Battle - {}", w.trim()),
        None => "Battle".to_string(),
    }
}

/// Writes an autosave named `name`; a failure is reported on stderr (the game goes on).
pub fn autosave(game: &Game, name: &str) {
    let Some(dir) = save::default_dir() else { return };
    if let Err(e) = save::write(&dir, SaveKind::Auto, name, game) {
        eprintln!("autosave: {e}");
    }
}

/// The battle against the pending foe, after the autosave the original makes before every
/// battle.
pub fn battle(game: &mut Game) -> Screen {
    autosave(game, &battle_name(game));
    Screen::Battle(Box::new(BattleView::new(game.start_battle())))
}

/// "2026-09-25 13:08" (UTC) from seconds since 1970.
fn real_time(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (h, m) = (secs % 86_400 / 3600, secs % 3600 / 60);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y}-{mo:02}-{d:02} {h:02}:{m:02}")
}

fn window(title: &str) -> (f32, f32, f32, f32) {
    let (sw, sh) = (screen_width(), screen_height());
    let (w, h) = (1000.0f32.min(sw - 20.0), 600.0f32.min(sh - 40.0));
    let (x, y) = ((sw - w) / 2.0, (sh - h) / 2.0);
    draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.3));
    super::chrome::window(Rect::new(x, y, w, h), title, super::chrome::Skin::Marble, false);
    (x, y, w, h)
}

/// "Saves: <folder>", shortened from the left to fit `width`.
fn folder_line(dir: &std::path::Path, x: f32, y: f32, width: f32) {
    let mut s = dir.display().to_string();
    while measure(&format!("Saves: ...{s}"), 14.0).width > width && s.chars().count() > 10 {
        s.remove(0);
    }
    let full = dir.display().to_string();
    let shown = if s == full { format!("Saves: {s}") } else { format!("Saves: ...{s}") };
    text(&shown, x, y, 14.0, DIM);
}

/// The list of saves; returns the row clicked (index into `entries`).
fn save_list(entries: &[SaveEntry], selected: Option<usize>, scroll: &mut usize, x: f32, y: f32, w: f32) -> Option<usize> {
    let cols = [(0.0, "Name"), (0.38, "Scenario"), (0.62, "Hero"), (0.76, "In the game")];
    for (f, label) in cols {
        text(label, x + 14.0 + f * w, y + 16.0, 17.0, ACCENT);
    }
    let top = y + 26.0;
    let rh = 38.0;
    if mouse_in(x, top, w, rh * ROWS as f32) {
        let wh = wheel();
        if wh < 0.0 && *scroll + ROWS < entries.len() {
            *scroll += 1;
        } else if wh > 0.0 && *scroll > 0 {
            *scroll -= 1;
        }
    }
    let mut picked = None;
    for (k, e) in entries.iter().enumerate().skip(*scroll).take(ROWS) {
        let ry = top + (k - *scroll) as f32 * rh;
        let hover = mouse_in(x, ry, w, rh - 4.0);
        draw_rectangle(x, ry, w, rh - 4.0, ROW);
        let edge = if selected == Some(k) { ACCENT } else if hover { INK } else { Color::new(0.3, 0.25, 0.2, 1.0) };
        draw_rectangle_lines(x, ry, w, rh - 4.0, if selected == Some(k) { 2.5 } else { 1.0 }, edge);
        if selected == Some(k) {
            text("►", x + 2.0, ry + 23.0, 16.0, ACCENT);
        }
        let m = &e.meta;
        let fit = |s: &str, width: f32| {
            let mut s = s.to_string();
            while measure(&s, 17.0).width > width && s.chars().count() > 3 {
                s.pop();
            }
            s
        };
        text(&fit(&m.name, 0.37 * w - 20.0), x + 14.0, ry + 16.0, 17.0, INK);
        text(&format!("saved {} UTC", real_time(m.saved_at)), x + 14.0, ry + 31.0, 13.0, DIM);
        text(&fit(&m.title, 0.23 * w), x + 14.0 + 0.38 * w, ry + 22.0, 17.0, INK);
        text(&fit(&m.hero, 0.13 * w), x + 14.0 + 0.62 * w, ry + 22.0, 17.0, INK);
        text(&fit(&m.date, 0.23 * w), x + 14.0 + 0.76 * w, ry + 22.0, 16.0, DIM);
        if hover && clicked() {
            picked = Some(k);
        }
    }
    if entries.len() > ROWS {
        let line = format!("{}–{} of {} (wheel to scroll)", *scroll + 1, (*scroll + ROWS).min(entries.len()), entries.len());
        text(&line, x + w - 260.0, top + rh * ROWS as f32 + 14.0, 15.0, DIM);
    }
    picked
}

/// The save window: a name (type to edit; a click on a save takes its name, to replace it).
pub fn save_screen(game: &Game, assets: &Assets, view: &mut SaveView, message: &mut Option<String>) -> Option<Screen> {
    world_view::backdrop(game, assets);
    let (x, y, w, h) = window("Save the game");
    while let Some(c) = get_char_pressed() {
        if !c.is_control() && view.name.chars().count() < 60 {
            view.name.push(c);
        }
    }
    if is_key_pressed(KeyCode::Backspace) {
        view.name.pop();
    }
    text("Name:", x + 20.0, y + 64.0, 20.0, INK);
    draw_rectangle(x + 90.0, y + 42.0, w - 110.0, 32.0, ROW);
    draw_rectangle_lines(x + 90.0, y + 42.0, w - 110.0, 32.0, 2.0, ACCENT);
    let caret = if (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" };
    text(&format!("{}{caret}", view.name), x + 98.0, y + 64.0, 20.0, INK);
    let mut scroll = 0;
    let same = view.entries.iter().position(|e| e.meta.name == view.name);
    if let Some(k) = save_list(&view.entries, same, &mut scroll, x + 20.0, y + 90.0, w - 40.0) {
        view.name = view.entries[k].meta.name.clone();
    }
    let dir = save::default_dir();
    if dir.is_none() {
        text("No data folder for saves: set RAZDOR_SAVE_DIR.", x + 20.0, y + h - 30.0, 18.0, RED);
    } else if let Some(d) = &dir {
        folder_line(d, x + 20.0, y + h - 22.0, w - 320.0);
    }
    let label = if same.is_some() { "Replace" } else { "Save" };
    let ok = !view.name.trim().is_empty() && dir.is_some();
    if button(x + w - 280.0, y + h - 60.0, 120.0, 40.0, label, ok) || (ok && key(KeyCode::Enter)) {
        let dir = dir.expect("checked");
        *message = Some(match save::write(&dir, SaveKind::Manual, view.name.trim(), game) {
            Ok(_) => format!("Saved: {}", view.name.trim()),
            Err(e) => format!("Not saved: {e}"),
        });
        return Some(Screen::WorldMap);
    }
    if button(x + w - 150.0, y + h - 60.0, 130.0, 40.0, "Cancel", true) || key(KeyCode::Escape) {
        return Some(view.back.screen());
    }
    None
}

/// The load window: the manual and autosave tabs. Loading itself is the app's
/// (`pending` receives the file).
pub fn load_screen(game: Option<&Game>, assets: &Assets, view: &mut LoadView, pending: &mut Option<PathBuf>, error: &Option<String>) -> Option<Screen> {
    match game {
        Some(g) if view.back != Back::Title => world_view::backdrop(g, assets),
        _ => clear_background(Color::from_rgba(24, 22, 20, 255)),
    }
    let (x, y, w, h) = window("Load a game");
    for (k, (tab, label)) in [(SaveKind::Manual, "Saved"), (SaveKind::Auto, "Autosaves")].into_iter().enumerate() {
        let bx = x + 20.0 + k as f32 * 170.0;
        if button(bx, y + 40.0, 160.0, 36.0, label, true) && view.tab != tab {
            view.tab = tab;
            view.refresh();
        }
        if view.tab == tab {
            draw_rectangle_lines(bx - 3.0, y + 37.0, 166.0, 42.0, 2.0, Color::new(1.0, 0.6, 0.2, 1.0));
        }
    }
    if view.entries.is_empty() {
        text_centered("No saves here yet.", x + w / 2.0, y + 200.0, 20.0, DIM);
    }
    if let Some(k) = save_list(&view.entries, Some(view.selected), &mut view.scroll, x + 20.0, y + 90.0, w - 40.0) {
        view.selected = k;
    }
    if let Some(e) = error {
        for (i, line) in wrap(e, w - 340.0, 18.0).iter().take(2).enumerate() {
            text(line, x + 20.0, y + h - 44.0 + i as f32 * 20.0, 18.0, Color::new(1.0, 0.45, 0.4, 1.0));
        }
    } else if let Some(d) = save::default_dir() {
        folder_line(&d, x + 20.0, y + h - 22.0, w - 320.0);
    }
    let chosen = view.entries.get(view.selected);
    if (button(x + w - 280.0, y + h - 60.0, 120.0, 40.0, "Load", chosen.is_some()) || key(KeyCode::Enter)) && chosen.is_some() {
        *pending = chosen.map(|e| e.path.clone());
    }
    if button(x + w - 150.0, y + h - 60.0, 130.0, 40.0, "Cancel", true) || key(KeyCode::Escape) {
        return Some(view.back.screen());
    }
    None
}

/// The Esc menu over the map: resume, save, load, the main menu.
/// One volume row of the menu: label, value, `-` / `+` and a mute toggle.
fn volume_row(label: &str, volume: f32, muted: bool, x: f32, y: f32, w: f32) -> (i32, bool) {
    let value = if muted { "off".to_string() } else { format!("{:.0}%", volume * 100.0) };
    text(&format!("{label} {value}"), x, y + 26.0, 20.0, if muted { DIM } else { INK });
    let bx = x + w - 170.0;
    let mut steps = 0;
    if button(bx, y, 40.0, 38.0, "-", !muted && volume > 0.0) {
        steps -= 1;
    }
    if button(bx + 46.0, y, 40.0, 38.0, "+", !muted && volume < 1.0) {
        steps += 1;
    }
    let toggle = button(bx + 92.0, y, 78.0, 38.0, if muted { "On" } else { "Off" }, true);
    (steps, toggle)
}

/// The Esc menu: back, save, load, main menu, and the music and sound volumes (+/- keys
/// change the music volume; N anywhere turns the music off and on).
pub fn menu(game: &Game, assets: &Assets, audio: &mut Settings) -> Option<Screen> {
    world_view::backdrop_lit(game, assets, Some(super::game_bar::BarButton::Menu));
    let (sw, sh) = (screen_width(), screen_height());
    let (w, h) = (380.0, 440.0);
    let (x, y) = ((sw - w) / 2.0, (sh - h) / 2.0 - 30.0);
    super::chrome::window(Rect::new(x, y, w, h), "Game menu", super::chrome::Skin::Marble, false);
    let bx = x + 40.0;
    let bw = w - 80.0;
    if button(bx, y + 60.0, bw, 44.0, "Back to the game", true) || key(KeyCode::Escape) {
        return Some(Screen::WorldMap);
    }
    if button(bx, y + 120.0, bw, 44.0, "Save the game", game.foe.is_none()) {
        return Some(Screen::Save(SaveView::new(game, Back::Menu)));
    }
    if button(bx, y + 180.0, bw, 44.0, "Load a game", true) {
        return Some(Screen::Load(LoadView::new(Back::Menu)));
    }
    if button(bx, y + 250.0, bw, 44.0, "Main menu", true) {
        return Some(Screen::ScenarioSelect);
    }
    let (steps, toggle) = volume_row("Music", audio.music_volume, audio.music_muted, bx, y + 320.0, bw);
    let keys = [KeyCode::Equal, KeyCode::KpAdd].iter().any(|&k| key(k)) as i32
        - [KeyCode::Minus, KeyCode::KpSubtract].iter().any(|&k| key(k)) as i32;
    if !audio.music_muted {
        audio.step_music(steps + keys);
    }
    audio.music_muted ^= toggle;
    let (steps, toggle) = volume_row("Sounds", audio.sfx_volume, audio.sfx_muted, bx, y + 370.0, bw);
    audio.step_sfx(steps);
    audio.sfx_muted ^= toggle;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_time_labels() {
        assert_eq!(real_time(0), "1970-01-01 00:00");
        assert_eq!(real_time(1_790_000_000), "2026-09-21 14:13");
    }
}
