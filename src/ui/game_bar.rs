//! The bottom game bar, as the original's (video notes §1, ref 06): four oval buttons on
//! the left (menu, settings, save, load), the time panel in the middle, four on the right
//! (journal, hero and army, spell book, map), and under them the strip with mana, gold,
//! income and wages. The buttons are blue, grey while a window is open, green for the open
//! screen and orange while the minimap shows.

use macroquad::prelude::*;

use razdor::rules::clock::duration_label;
use razdor::rules::game::Game;

use super::chrome::{self, shadow_centered, shadow_text, tex, three_slice, Fx, CREAM, GOLD};
use super::widgets::{clicked, measure, mouse_in, tooltip};

/// A bar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarButton {
    Menu,
    Settings,
    Save,
    Load,
    Journal,
    Squad,
    Spells,
    Map,
}

impl BarButton {
    const LEFT: [BarButton; 4] = [BarButton::Menu, BarButton::Settings, BarButton::Save, BarButton::Load];
    const RIGHT: [BarButton; 4] = [BarButton::Journal, BarButton::Squad, BarButton::Spells, BarButton::Map];

    fn icon(self) -> usize {
        match self {
            BarButton::Menu => 1,
            BarButton::Settings => 2,
            BarButton::Save => 4,
            BarButton::Load => 3,
            BarButton::Journal => 5,
            BarButton::Squad => 6,
            BarButton::Spells => 7,
            BarButton::Map => 8,
        }
    }

    /// The small ovals are the outer two on each side.
    fn small(self) -> bool {
        matches!(self, BarButton::Menu | BarButton::Settings | BarButton::Spells | BarButton::Map)
    }

    fn hint(self) -> &'static str {
        match self {
            BarButton::Menu => "Main menu (Esc)",
            BarButton::Settings => "Sound settings",
            BarButton::Save => "Save the game",
            BarButton::Load => "Load a saved game",
            BarButton::Journal => "The hero's journal (J)",
            BarButton::Squad => "The hero and his army",
            BarButton::Spells => "The spell book (B)",
            BarButton::Map => "Map of the scenario (M)",
        }
    }
}

/// How a button looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Normal,
    /// A window is open or the button does nothing now.
    Grey,
    /// Its screen is open.
    Lit,
    /// The minimap is open.
    Glow,
}

/// The four resources along the bar's lower strip.
fn resources(game: &Game, y: f32, h: f32) {
    let k = chrome::k();
    let w = screen_width();
    let mut wages = format!("- {}", game.daily_wages());
    if game.daily_mana_wages() > 0 {
        wages += &format!(" / {}", game.daily_mana_wages());
    }
    let items = [
        ("mana", "Res-Magic", game.mana.to_string(), Color::new(0.45, 0.85, 1.0, 1.0)),
        ("gold", "Res-Money", game.gold.to_string(), GOLD),
        ("income", "Res-Income", format!("+ {}", game.daily_income()), GOLD),
        ("wages", "Res-Payment", wages, Color::new(1.0, 0.62, 0.25, 1.0)),
    ];
    let label_size = (11.0 * k).round();
    let value_size = (16.0 * k).round();
    for (i, (label, art, value, color)) in items.iter().enumerate() {
        let cx = w * (0.125 + 0.25 * i as f32);
        let lw = measure(label, label_size).width;
        let base = y + h * 0.5 + value_size * 0.36;
        shadow_text(label, cx - lw - 16.0 * k, base, label_size, Color::new(0.75, 0.75, 0.75, 1.0));
        let icon = 20.0 * k;
        match chrome::win(art) {
            Some(t) => {
                let iw = t.width() * icon / t.height();
                tex(&t, Rect::new(cx - iw / 2.0, y + (h - icon) / 2.0, iw, icon), WHITE);
            }
            None => super::dialog::resource_icon(
                [super::dialog::Resource::Mana, super::dialog::Resource::Gold, super::dialog::Resource::Income, super::dialog::Resource::Wages][i],
                cx,
                y + h / 2.0,
                icon,
            ),
        }
        shadow_text(value, cx + 14.0 * k, base, value_size, *color);
    }
}

/// One oval button; true when clicked (never while greyed).
fn oval(b: BarButton, r: Rect, look: Look) -> bool {
    let hover = look != Look::Grey && mouse_in(r.x, r.y, r.w, r.h);
    let side = if BarButton::LEFT.contains(&b) { "ML" } else { "MR" };
    let size = if b.small() { 2 } else { 1 };
    let state = if hover && is_mouse_button_down(MouseButton::Left) { "Down" } else { "Up" };
    let name = format!("{side}Btn{size}{state}");
    let (fx, tint) = match look {
        Look::Normal if hover => (Fx::Plain, Color::new(1.15, 1.15, 1.25, 1.0)),
        Look::Normal => (Fx::Plain, WHITE),
        Look::Grey => (Fx::Grey, WHITE),
        Look::Lit => (Fx::Grey, Color::new(0.45, 1.0, 0.45, 1.0)),
        Look::Glow => (Fx::Grey, Color::new(1.0, 0.55, 0.2, 1.0)),
    };
    match chrome::win_fx(&name, fx) {
        Some(t) => tex(&t, r, tint),
        None => {
            let base = match look {
                Look::Normal => Color::new(0.16, 0.30, 0.62, 1.0),
                Look::Grey => Color::new(0.33, 0.33, 0.35, 1.0),
                Look::Lit => Color::new(0.2, 0.55, 0.22, 1.0),
                Look::Glow => Color::new(0.75, 0.38, 0.1, 1.0),
            };
            let c = if hover { Color::new(base.r * 1.3, base.g * 1.3, base.b * 1.3, 1.0) } else { base };
            draw_ellipse(r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0, 0.0, c);
            draw_ellipse_lines(r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0, 0.0, 2.0, chrome::SILVER);
        }
    }
    // The glyph: white-on-black art made to glow, tinted by the button's state.
    let glyph = match look {
        Look::Normal => Color::new(0.7, 0.95, 1.0, 1.0),
        Look::Grey => Color::new(0.85, 0.85, 0.85, 0.9),
        Look::Lit => Color::new(0.75, 1.0, 0.75, 1.0),
        Look::Glow => Color::new(1.0, 0.85, 0.5, 1.0),
    };
    if let Some(t) = chrome::win_fx(&format!("button-icon-{}", b.icon()), Fx::Glow) {
        let k = r.h / 54.0 * if b.small() { 1.15 } else { 1.0 };
        let (w, h) = (t.width() * k, t.height() * k);
        tex(&t, Rect::new(r.x + (r.w - w) / 2.0, r.y + (r.h - h) / 2.0, w, h), glyph);
    } else {
        let label = match b {
            BarButton::Menu => "X",
            BarButton::Settings => "Opt",
            BarButton::Save => "Save",
            BarButton::Load => "Load",
            BarButton::Journal => "Quests",
            BarButton::Squad => "Army",
            BarButton::Spells => "Book",
            BarButton::Map => "Map",
        };
        let s = (r.h * 0.34).round();
        shadow_centered(label, r.x + r.w / 2.0, r.y + r.h / 2.0 + s * 0.36, s, glyph);
    }
    if hover {
        tooltip(&[(b.hint().to_string(), CREAM)]);
    }
    let pressed = hover && clicked();
    if pressed {
        super::audio::cue(super::audio::Cue::Button);
    }
    pressed
}

/// Draws the bar. `look` says how each button looks; returns the button clicked.
pub fn draw(game: &Game, look: impl Fn(BarButton) -> Look) -> Option<BarButton> {
    let k = chrome::k();
    let (w, h) = (screen_width(), screen_height());
    let bar = chrome::bar_height();
    let y = h - bar;
    let row = (60.0 * k).round();
    // The top row: the original's ribbon (`Win2a`), its ends kept, the wooden middle stretched.
    match chrome::win("Win2a") {
        Some(t) => {
            let s = row / t.height();
            let end = 370.0;
            let ew = end * s;
            chrome::tex_src(&t, Rect::new(0.0, 0.0, end, t.height()), Rect::new(0.0, y, ew, row), WHITE);
            chrome::tex_src(&t, Rect::new(t.width() - end, 0.0, end, t.height()), Rect::new(w - ew, y, ew, row), WHITE);
            chrome::tex_src(&t, Rect::new(end, 0.0, t.width() - 2.0 * end, t.height()), Rect::new(ew, y, w - 2.0 * ew, row), WHITE);
        }
        None => {
            chrome::surface(Rect::new(0.0, y, w, row), chrome::Skin::Marble);
            let ew = 370.0 * row / 64.0;
            draw_rectangle(ew, y + 3.0, w - 2.0 * ew, row - 6.0, Color::new(0.42, 0.22, 0.10, 1.0));
            draw_rectangle_lines(ew, y + 3.0, w - 2.0 * ew, row - 6.0, 2.0, chrome::SILVER);
            draw_rectangle_lines(0.0, y, w, row, 2.0, chrome::SILVER);
        }
    }
    if let Some(t) = chrome::win("SteelLine") {
        three_slice(&t, Rect::new(0.0, y - 2.0, w, 4.0 * k), 40.0, WHITE);
    }
    // The lower strip.
    let sy = y + row;
    let sh = bar - row;
    draw_rectangle(0.0, sy, w, sh, Color::new(0.13, 0.13, 0.14, 1.0));
    if let Some(t) = chrome::win_fx("DownCorner", Fx::KeyBlack) {
        let ow = t.width() * sh / t.height();
        let mut x = w * 0.25 - ow / 2.0;
        while x < w {
            tex(&t, Rect::new(x, sy, ow, sh), Color::new(1.0, 1.0, 1.0, 0.5));
            x += w * 0.25;
        }
    }
    resources(game, sy, sh);

    // The time panel.
    let ew = 370.0 * row / 64.0;
    let cx = w / 2.0;
    let size = (13.0 * k).round();
    let (line1, line2) = if game.moving() {
        (format!("Time: {}", game.clock.label()), Some(format!("Path left: {}", duration_label(game.minutes_left() as f64))))
    } else if game.waiting() {
        (format!("Time: {}", game.clock.label()), Some("waiting…".to_string()))
    } else {
        ("Time:".to_string(), Some(game.clock.label()))
    };
    let top = y + row * 0.5 - size * 0.25;
    shadow_centered(&line1, cx, top, size, if game.moving() || game.waiting() { CREAM } else { GOLD });
    if let Some(l2) = line2 {
        shadow_centered(&l2, cx, top + size * 1.35, size, if game.moving() || game.waiting() { GOLD } else { CREAM });
    }
    let _ = ew;

    // The buttons.
    let mut pressed = None;
    let bh = |b: BarButton| if b.small() { 44.0 } else { 54.0 } * row / 64.0;
    let bw = |b: BarButton| if b.small() { 80.0 } else { 98.0 } * row / 64.0;
    let mut x = 8.0 * k;
    for b in BarButton::LEFT {
        let r = Rect::new(x, y + (row - bh(b)) / 2.0, bw(b), bh(b));
        if oval(b, r, look(b)) {
            pressed = Some(b);
        }
        x += bw(b) - 4.0 * k;
    }
    let mut x = w - 8.0 * k;
    for b in BarButton::RIGHT.iter().rev().copied() {
        x -= bw(b);
        let r = Rect::new(x, y + (row - bh(b)) / 2.0, bw(b), bh(b));
        if oval(b, r, look(b)) {
            pressed = Some(b);
        }
        x += 4.0 * k;
    }
    pressed
}
