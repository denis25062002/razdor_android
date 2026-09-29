//! The original's new-game windows over the main menu's ruins: "Сценарий для Новой Игры"
//! (the scenario list with its round icons, the map in `MapImageFrame`, the name, status,
//! size and description) and "Стартовые характеристики Героя" (the three heroes' portraits,
//! the name field, and the chosen hero's description and bonus in the open book of
//! `NewHero_Paper`). The texts are the install's (`[NewGame]`, `[NewHero]`, `[Buttons]`).
//! No footage shows these two windows: their layout is ours, built from the original's art
//! (L). Without an install the plain screens of `ui::screens` stand in.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use macroquad::prelude::*;

use razdor::i18n::{self, n_, tr, Lang};
use razdor::rules::content::{Content, HeroClass, UnitId};
use razdor::rules::game::Game;
use razdor::trf;

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::chrome::{self, CREAM, GOLD};
use super::dt_font::{with_face, Face};
use super::widgets::*;
use super::{screens, ScenarioEntry, Screen};

thread_local! {
    /// The scenario row picked (index into the list shown) and the hero picked.
    static PICKED: Cell<usize> = const { Cell::new(0) };
    static HERO: Cell<usize> = const { Cell::new(0) };
    /// The map preview of the scenario shown: (index into `scenarios`, texture).
    static PREVIEW: RefCell<Option<(usize, Texture2D)>> = const { RefCell::new(None) };
}

/// A text of the install (`[<section>] <key>`) in Russian, else ours.
fn own(section: &str, key: &str, ours: &'static str) -> String {
    let t = (i18n::lang() == Lang::Ru).then(|| chrome::ui_text(section, key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// The window, 594×482 video pixels as the load window, low on the screen under the logo.
struct Win {
    r: Rect,
    k: f32,
}

impl Win {
    fn open(title: &str) -> (Win, bool) {
        let k = chrome::k();
        let (w, h) = (594.0 * k, 482.0 * k);
        let r = Rect::new((screen_width() - w) / 2.0, (190.0 * k).min(screen_height() - h), w, h);
        let (_, closed) = chrome::window(r, title, chrome::Skin::Marble, true);
        (Win { r, k }, closed)
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(self.r.x + x * self.k, self.r.y + y * self.k, w * self.k, h * self.k)
    }

    fn button(&self, x: f32, w: f32, label: &str, enabled: bool) -> bool {
        let r = self.rect(x, 442.0, w, 28.0);
        let hover = !input_blocked() && r.contains(crate::ui::widgets::pointer().into());
        chrome::marble_button(r, label, enabled, hover);
        enabled && hover && clicked()
    }
}

/// The blue bar of the picked row (as the load window's).
fn picked_bar(r: Rect) {
    let steps = 8;
    for s in 0..steps {
        let t = s as f32 / (steps - 1) as f32;
        let a = 0.85 - 0.5 * (t - 0.5).abs();
        draw_rectangle(r.x, r.y + r.h * s as f32 / steps as f32, r.w, r.h / steps as f32 + 0.5, Color::new(0.12, 0.2, 0.62, a));
    }
}

/// The round scenario icon: `SI_Castle`, `SI_Helm`, `SI_Swords`, `SI_Skull`, `SI_Tutorial`
/// by the map's picture index (header byte 0x120, 1..5), in the silver ring `si-border`.
fn scenario_icon(picture: u8, c: Vec2, size: f32) {
    const ICONS: [&str; 5] = ["SI_Castle", "SI_Helm", "SI_Swords", "SI_Skull", "SI_Tutorial"];
    let Some(name) = (picture as usize).checked_sub(1).and_then(|i| ICONS.get(i)) else { return };
    let r = Rect::new(c.x - size / 2.0, c.y - size / 2.0, size, size);
    draw_circle(c.x, c.y, size * 0.46, Color::new(0.0, 0.0, 0.0, 0.8));
    if let Some(t) = chrome::win(name) {
        let inner = Rect::new(r.x + size * 0.12, r.y + size * 0.12, size * 0.76, size * 0.76);
        chrome::tex(&t, inner, WHITE);
    }
    if let Some(t) = chrome::win_fx("si-border", chrome::Fx::KeyBlack) {
        chrome::tex(&t, r, WHITE);
    }
}

/// The map's terrain, one texel per cell (the minimap's colours).
fn preview(index: usize, e: &ScenarioEntry) -> Option<Texture2D> {
    PREVIEW.with(|p| {
        let mut p = p.borrow_mut();
        if let Some((i, t)) = p.as_ref() {
            if *i == index {
                return Some(t.clone());
            }
        }
        let s = &e.scenario;
        let (w, h) = (u16::try_from(s.width()).ok()?, u16::try_from(s.height()).ok()?);
        if w == 0 || h == 0 || s.terrain.len() != w as usize * h as usize {
            return None;
        }
        let rgba: Vec<u8> = s.terrain.iter().flat_map(|&c| <[u8; 4]>::from(super::world_view::surface_color(c))).collect();
        let t = Texture2D::from_rgba8(w, h, &rgba);
        t.set_filter(FilterMode::Linear);
        *p = Some((index, t.clone()));
        Some(t)
    })
}

/// The maps a new game can start on, as in the original: the single scenarios and the first
/// map of each campaign (a later campaign map is reached by winning the one before).
fn startable(scenarios: &[ScenarioEntry]) -> Vec<usize> {
    scenarios.iter().enumerate().filter(|(_, e)| e.scenario.header.scenario_kind != 2).map(|(i, _)| i).collect()
}

/// "Сценарий для Новой Игры".
pub fn scenario_select(scenarios: &[ScenarioEntry], has_install: bool) -> Option<Screen> {
    if !has_install || chrome::win("Win-marble").is_none() {
        return screens::scenario_select(scenarios, has_install);
    }
    super::main_menu::backdrop();
    let (win, closed) = Win::open(&own("NewGame", "Title", n_("Scenario for a new game")));
    let k = win.k;
    let list = startable(scenarios);
    let mut picked = PICKED.with(|p| p.get()).min(list.len().saturating_sub(1));
    // The list: a round icon and the title per row.
    let rows = win.rect(12.0, 36.0, 270.0, 396.0);
    chrome::text_box(rows);
    let rh = 30.0 * k;
    let mut next = None;
    for (row, &i) in list.iter().enumerate() {
        let r = Rect::new(rows.x + 4.0 * k, rows.y + 6.0 * k + row as f32 * rh, rows.w - 8.0 * k, rh - 2.0 * k);
        if r.y + r.h > rows.y + rows.h {
            break;
        }
        let hover = !input_blocked() && r.contains(crate::ui::widgets::pointer().into());
        if row == picked {
            picked_bar(r);
        } else if hover {
            draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.15, 0.45, 0.35));
        }
        let e = &scenarios[i];
        scenario_icon(e.scenario.header.scenario_picture_index, vec2(r.x + 14.0 * k, r.y + r.h / 2.0), 26.0 * k);
        let title = if e.scenario.title.trim().is_empty() { e.file.as_str() } else { e.scenario.title.as_str() };
        with_face(Face::Title, || {
            let size = fit_size(title, r.w - 40.0 * k, 15.0 * k);
            chrome::shadow_text(&ellipsize(title, r.w - 40.0 * k, size), r.x + 32.0 * k, r.y + r.h * 0.5 + size * 0.36, size, CREAM);
        });
        if hover && clicked() {
            cue(Cue::Button);
            if row == picked {
                next = Some(Screen::ClassSelect { scenario: Some(i) });
            }
            picked = row;
        }
    }
    PICKED.with(|p| p.set(picked));
    // The map and what it is.
    if let Some(&i) = list.get(picked) {
        let e = &scenarios[i];
        let frame = win.rect(290.0, 36.0, 295.0, 162.0);
        draw_rectangle(frame.x, frame.y, frame.w, frame.h, BLACK);
        if let Some(t) = preview(i, e) {
            // Square maps, centred in the frame's opening.
            let inner = Rect::new(frame.x + 8.0 * k, frame.y + 8.0 * k, frame.w - 16.0 * k, frame.h - 16.0 * k);
            let side = inner.w.min(inner.h);
            chrome::tex(&t, Rect::new(inner.center().x - side / 2.0, inner.y + (inner.h - side) / 2.0, side, side), WHITE);
        }
        if let Some(t) = chrome::win_fx("MapImageFrame", chrome::Fx::KeyBlack) {
            chrome::tex(&t, frame, WHITE);
        }
        let status = match e.scenario.header.scenario_kind {
            1 => own("NewGame", "Campaign", n_("Campaign")),
            _ => own("NewGame", "OneScenario", n_("Single scenario")),
        };
        let lines = [
            (own("NewGame", "Name", n_("Name:")), e.scenario.title.clone()),
            (own("NewGame", "Status", n_("Status:")), status),
            (own("NewGame", "MapSize", n_("Map size")), format!("{}×{}", e.scenario.width(), e.scenario.height())),
        ];
        let size = 13.0 * k;
        let mut y = frame.y + frame.h + 20.0 * k;
        for (label, value) in lines {
            let label = label.trim_end_matches(':').to_string() + ":";
            chrome::shadow_text(&label, frame.x + 4.0 * k, y, size, GOLD);
            let lw = measure(&label, size).width + 6.0 * k;
            chrome::shadow_text(&ellipsize(&value, frame.w - lw - 8.0 * k, size), frame.x + 4.0 * k + lw, y, size, CREAM);
            y += 17.0 * k;
        }
        let descript = own("NewGame", "Descript", n_("Description:"));
        chrome::shadow_text(&descript, frame.x + 4.0 * k, y, size, GOLD);
        let bottom = win.rect(0.0, 432.0, 0.0, 0.0).y;
        let desc = Rect::new(frame.x, y + 6.0 * k, frame.w, (bottom - y - 6.0 * k).max(0.0));
        chrome::text_box(desc);
        let small = 12.0 * k;
        for (n, line) in wrap(&e.scenario.description, desc.w - 16.0 * k, small).iter().enumerate() {
            let ly = desc.y + 16.0 * k + n as f32 * 14.0 * k;
            if ly > desc.y + desc.h - 4.0 * k {
                break;
            }
            chrome::shadow_text(line, desc.x + 8.0 * k, ly, small, CREAM);
        }
    }
    let can = !list.is_empty();
    if (win.button(369.0, 116.0, &own("Buttons", "Next", n_("Next")), can) || (can && key(KeyCode::Enter))) && next.is_none() {
        cue(Cue::MenuPress);
        next = list.get(picked).map(|&i| Screen::ClassSelect { scenario: Some(i) });
    }
    if win.button(492.0, 95.0, &own("Buttons", "Cancel", n_("Cancel")), true) || closed || key(KeyCode::Escape) {
        return Some(Screen::MainMenu);
    }
    next
}

/// "Стартовые характеристики Героя".
pub fn class_select(game: &mut Option<Game>, demo: &Arc<Content>, scenario: Option<(&ScenarioEntry, Arc<Content>)>, assets: &Assets) -> Option<Screen> {
    if scenario.is_none() || chrome::win("Win-marble").is_none() {
        return screens::class_select(game, demo, scenario, assets);
    }
    let (e, content) = scenario.expect("checked");
    super::main_menu::backdrop();
    let (win, closed) = Win::open(&own("NewHero", "Title", n_("The hero's starting characteristics")));
    let k = win.k;
    let mut pick = HERO.with(|h| h.get()).min(2);
    // The three heroes.
    for (i, hero) in HeroClass::ALL.into_iter().enumerate() {
        let r = win.rect(27.0 + i as f32 * 185.0, 38.0, 170.0, 150.0);
        let hover = !input_blocked() && r.contains(crate::ui::widgets::pointer().into());
        draw_rectangle(r.x + 3.0 * k, r.y + 3.0 * k, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.5));
        match chrome::win(["Hero0", "hero1", "hero2"][i]) {
            Some(t) => chrome::tex_src(&t, Rect::new(0.0, (t.height() - t.height() * 150.0 / 170.0) / 2.0, t.width(), t.height() * 150.0 / 170.0), r, WHITE),
            None => assets.draw_portrait(hero.unit(), razdor::rules::battle::Team::Player, r),
        }
        if i == pick {
            chrome::glow_frame(r, Color::new(0.35, 1.0, 0.35, 1.0), true);
        } else if hover {
            chrome::glow_frame(r, Color::new(0.35, 0.55, 1.0, 0.9), false);
        }
        let name = &content.unit(hero.unit()).name;
        with_face(Face::Title, || chrome::shadow_centered(name, r.center().x, r.y + r.h + 18.0 * k, 16.0 * k, if i == pick { GOLD } else { CREAM }));
        if hover && clicked() {
            cue(Cue::Button);
            pick = i;
        }
    }
    HERO.with(|h| h.set(pick));
    let hero = HeroClass::ALL[pick];
    // The hero's name, typed.
    let name = screens::HERO_NAME.with(|n| {
        let mut n = n.borrow_mut();
        screens::edit_name(&mut n);
        n.clone()
    });
    // The name plate (`nameframe`, 162×22, silver): the name in dark ink on it.
    let field = win.rect(172.0, 212.0, 250.0, 250.0 * 22.0 / 162.0);
    let caret = if (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" };
    let shown = if name.is_empty() { format!("{}{caret}", own("NewHero", "PrivateHeroName", n_("(no name)"))) } else { format!("{name}{caret}") };
    let size = 14.0 * k;
    match chrome::win("nameframe") {
        Some(t) => {
            chrome::tex(&t, field, WHITE);
            let ink = if name.is_empty() { Color::new(0.25, 0.25, 0.27, 1.0) } else { Color::new(0.05, 0.04, 0.03, 1.0) };
            with_face(Face::Bold, || {
                let w = measure(&shown, size).width;
                let (x, y) = (field.center().x - w / 2.0, field.y + field.h * 0.5 + size * 0.36);
                // A light halo keeps the ink readable on the chain.
                for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                    text(&shown, x + dx * k, y + dy * k, size, Color::new(0.92, 0.92, 0.95, 0.9));
                }
                text(&shown, x, y, size, ink);
            });
        }
        None => {
            chrome::text_box(field);
            chrome::shadow_centered(&shown, field.center().x, field.y + field.h * 0.5 + size * 0.36, size, CREAM);
        }
    }
    // The book: the hero on the left page, his bonus and what the map gives him on the right.
    let book = win.rect(15.0, 250.0, 564.0, 186.0);
    match chrome::win("NewHero_Paper") {
        Some(t) => chrome::tex(&t, book, WHITE),
        None => chrome::text_box(book),
    }
    let n = pick + 1;
    let small = 12.0 * k;
    let page_w = book.w / 2.0 - 44.0 * k;
    let (left, right) = (book.x + 26.0 * k, book.x + book.w / 2.0 + 18.0 * k);
    let ours = [
        n_("A veteran of many cruel battles, the Knight has superb experience of war. Under the Church's protection he can withstand hostile spells."),
        n_("The Archmage can call on the mighty forces of the world that ordinary people cannot command. In battle he binds the enemy."),
        n_("The Ranger is a fast and accurate shot who finds his way anywhere. Watching nature, he has learnt the secrets of healing wounds fast."),
    ];
    let descript = own("NewHero", &format!("Descript{n}"), ours[pick]);
    for (j, line) in wrap(&descript, page_w, small).iter().take(9).enumerate() {
        chrome::shadow_text(line, left, book.y + 28.0 * k + j as f32 * 14.0 * k, small, CREAM);
    }
    let icon = 24.0 * k;
    chrome::trait_icon(&format!("HeroBonus{n}"), right, book.y + 18.0 * k, icon);
    let bonus_ours = [
        n_("The army of this hero takes 10% less damage from enemy attacks (magic excepted)."),
        n_("The Archmage casts spells twice as fast for 50% less mana, but his army gets no bonuses."),
        n_("The army of this hero travels 20% faster, and the wounded heal 20% of their hits every day."),
    ];
    let bonus = own("NewHero", &format!("Bonus{n}"), bonus_ours[pick]);
    let mut y = book.y + 28.0 * k;
    for (j, line) in wrap(&bonus, page_w - icon - 6.0 * k, small).iter().take(5).enumerate() {
        chrome::shadow_text(line, right + icon + 6.0 * k, y, small, chrome::BLUE_TEXT);
        y = book.y + 28.0 * k + (j + 1) as f32 * 14.0 * k;
    }
    let preset = &e.scenario.header.heroes[pick];
    y += 8.0 * k;
    chrome::shadow_text(&trf!("Gold: {gold}", gold = preset.gold), right, y, small, GOLD);
    let army: Vec<String> = preset
        .troops
        .iter()
        .filter(|t| t.unit != 0 && t.count > 0)
        .filter_map(|t| content.try_unit(UnitId(t.unit as u32)).map(|u| format!("{} {}", t.count, u.name)))
        .collect();
    let army = if army.is_empty() { tr("alone").to_string() } else { army.join(", ") };
    for (j, line) in wrap(&army, page_w, small).iter().take(4).enumerate() {
        chrome::shadow_text(line, right, y + (j + 1) as f32 * 14.0 * k, small, CREAM);
    }
    if win.button(369.0, 116.0, &own("NewHero", "Start", n_("Start")), true) || key(KeyCode::Enter) {
        cue(Cue::MenuPress);
        *game = Some(screens::start_game(demo, Some((e, &content)), hero, &name));
        return Some(Screen::WorldMap);
    }
    if win.button(492.0, 95.0, &own("Buttons", "Prev", n_("Back")), true) || closed || key(KeyCode::Escape) {
        return Some(Screen::ScenarioSelect);
    }
    None
}
