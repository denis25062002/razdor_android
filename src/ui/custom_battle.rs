//! "Custom battle" (a Razdor extra, issue #1): the setup window over the main menu's ruins.
//! On the left the unit types of the install (or the demo's), a click adds one to the army
//! picked; in the middle the player's army (below on the battle screen) and the enemy's,
//! each unit with its level and items; under them the picked unit's items, the formation
//! (wide row or vanilla), the battle AI's level (the settings' own) and who plays each side.
//! "Fight!" starts the round on the normal battle screen (`BattleView::custom`). The setup
//! is kept for the whole run of the program ([`razdor::rules::custom::Session`]). The layout
//! is ours: the original has no such window.

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::battle::Team;
use razdor::rules::content::Content;
use razdor::rules::custom::{self, Control, Session, MAX_LEVEL};
use razdor::trf;

use super::assets::Assets;
use super::audio::{cue, Cue, Settings};
use super::chrome::{self, CREAM, GOLD};
use super::widgets::*;

/// What the setup window chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Fight,
    Back,
}

/// The setup and what the window shows of it: the army new units go to, the unit picked in
/// it, the unit list's scroll (rows) and the item offered to the picked unit.
pub struct SetupView {
    pub session: Session,
    side: Team,
    picked: Option<usize>,
    scroll: usize,
    offer: usize,
}

impl SetupView {
    pub fn new(content: &Content) -> SetupView {
        SetupView { session: Session::new(content), side: Team::Player, picked: None, scroll: 0, offer: 0 }
    }
}

/// Rows of the unit list and of each army.
const ROWS: usize = 12;

fn side_name(team: Team) -> &'static str {
    match team {
        Team::Player => tr("Your army (below)"),
        Team::Enemy => tr("Enemy army (above)"),
    }
}

fn control_label(c: Control) -> &'static str {
    match c {
        Control::Player => tr("Played by: you"),
        Control::Ai => tr("Played by: the AI"),
    }
}

/// A row of a list that can be picked: the blue bar when picked, a warm wash under the
/// mouse. True when clicked.
fn row(r: Rect, picked: bool) -> bool {
    let over = mouse_in(r.x, r.y, r.w, r.h);
    if picked {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.12, 0.2, 0.62, 0.75));
    } else if over {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(1.0, 0.85, 0.5, 0.12));
    }
    over && clicked()
}

/// Draws the window; returns what was chosen. `audio` holds the battle AI's level.
pub fn frame(view: &mut SetupView, content: &Content, assets: &Assets, audio: &mut Settings) -> Option<Pick> {
    super::main_menu::backdrop();
    view.session.setup.fit(content);
    let k = chrome::k();
    let (w, h) = (900.0 * k, 560.0 * k);
    let r = Rect::new((screen_width() - w) / 2.0, (150.0 * k).min(screen_height() - h).max(0.0), w, h);
    let (inner, closed) = chrome::window(r, tr("Custom battle"), chrome::Skin::Marble, true);
    let size = (14.0 * k).round();
    let row_h = 26.0 * k;
    let list_y = inner.y + 40.0 * k;
    let list_h = ROWS as f32 * row_h;

    // The unit types.
    let types = custom::choices(content);
    let lx = inner.x + 10.0 * k;
    let lw = 270.0 * k;
    text_fit(tr("Unit types: a click adds one"), lx, inner.y + 26.0 * k, lw, size, GOLD);
    let list = Rect::new(lx, list_y, lw, list_h);
    chrome::text_box(list);
    let max_scroll = types.len().saturating_sub(ROWS);
    if mouse_in(list.x, list.y, list.w, list.h) {
        let wh = wheel();
        if wh < 0.0 {
            view.scroll = (view.scroll + 1).min(max_scroll);
        } else if wh > 0.0 {
            view.scroll = view.scroll.saturating_sub(1);
        }
    }
    view.scroll = view.scroll.min(max_scroll);
    let full = view.session.setup.army(view.side).len() >= view.session.setup.capacity();
    for (n, &id) in types.iter().enumerate().skip(view.scroll).take(ROWS) {
        let y = list_y + (n - view.scroll) as f32 * row_h;
        let rr = Rect::new(lx + 2.0 * k, y + 1.0 * k, lw - 4.0 * k, row_h - 2.0 * k);
        if row(rr, false) && !full {
            cue(Cue::Button);
            view.session.setup.add(view.side, id);
            view.picked = Some(view.session.setup.army(view.side).len() - 1);
            view.offer = 0;
        }
        assets.draw_portrait(id, view.side, Rect::new(rr.x + 2.0 * k, rr.y + 1.0 * k, rr.h - 2.0 * k, rr.h - 2.0 * k));
        let def = content.unit(id);
        text_fit(&def.name, rr.x + rr.h + 6.0 * k, rr.y + rr.h * 0.7, rr.w - rr.h - 50.0 * k, size, CREAM);
        let hp = trf!("{hp} HP", hp = def.hits);
        shadow_right_fit(&hp, rr.x + rr.w - 4.0 * k, rr.y + rr.h * 0.7, (12.0 * k).round());
    }
    if types.len() > ROWS {
        let note = trf!("{first}–{last} of {all} (wheel scrolls)", first = view.scroll + 1, last = (view.scroll + ROWS).min(types.len()), all = types.len());
        chrome::shadow_text(&note, lx, list_y + list_h + 16.0 * k, (12.0 * k).round(), CREAM);
    }

    // The two armies.
    let aw = 290.0 * k;
    for (col, team) in Team::BOTH.into_iter().enumerate() {
        let ax = lx + lw + 12.0 * k + col as f32 * (aw + 12.0 * k);
        army_column(view, content, assets, team, Rect::new(ax, list_y, aw, list_h), inner.y, size, row_h);
    }

    // The picked unit's items.
    let iy = list_y + list_h + 28.0 * k;
    items_row(view, content, Rect::new(lx + lw + 12.0 * k, iy, 2.0 * aw + 12.0 * k, 52.0 * k), size);

    // The battle's settings and the way on.
    let by = inner.y + inner.h - 40.0 * k;
    let bh = 30.0 * k;
    let setup = &mut view.session.setup;
    let form = if setup.wide { tr("Formation: wide row (6)") } else { tr("Formation: vanilla (4)") };
    if button(lx, by, 220.0 * k, bh, form, true) {
        setup.wide = !setup.wide;
        setup.fit(content);
        view.picked = None;
    }
    let expert = super::main_menu::expert_ai(audio);
    let ai = if expert { tr("Battle AI: improved") } else { tr("Battle AI: normal") };
    if button(lx + 230.0 * k, by, 200.0 * k, bh, ai, true) {
        audio.expert_ai = Some(!expert);
    }
    let ready = view.session.setup.ready();
    let back = button(inner.x + inner.w - 310.0 * k, by, 140.0 * k, bh, tr("Back"), true);
    let fight = button(inner.x + inner.w - 160.0 * k, by, 150.0 * k, bh, tr("Fight! (Enter)"), ready);
    if !ready {
        chrome::shadow_text(tr("Both armies need a unit."), inner.x + inner.w - 310.0 * k, by - 8.0 * k, (12.0 * k).round(), chrome::RED_TEXT);
    }
    if fight || (ready && (key(KeyCode::Enter) || key(KeyCode::KpEnter))) {
        return Some(Pick::Fight);
    }
    if back || closed || key(KeyCode::Escape) {
        return Some(Pick::Back);
    }
    None
}

/// `s` right-aligned at `rx`, in the dim ink of a detail.
fn shadow_right_fit(s: &str, rx: f32, y: f32, size: f32) {
    chrome::shadow_right(s, rx, y, size, Color::new(0.85, 0.80, 0.68, 1.0));
}

/// One army: its name and count (a click picks it for new units), who plays it, and its
/// units with their level steppers, items and the × that removes them.
#[allow(clippy::too_many_arguments)]
fn army_column(view: &mut SetupView, content: &Content, assets: &Assets, team: Team, list: Rect, top: f32, size: f32, row_h: f32) {
    let k = chrome::k();
    let setup = &view.session.setup;
    let head = format!("{}: {}/{}", side_name(team), setup.army(team).len(), setup.capacity());
    let chosen = view.side == team;
    let hr = Rect::new(list.x, top + 8.0 * k, list.w * 0.55, 24.0 * k);
    if mouse_in(hr.x, hr.y, hr.w, hr.h) && clicked() {
        view.side = team;
        view.picked = None;
    }
    text_fit(&head, hr.x, hr.y + 18.0 * k, hr.w, size, if chosen { GOLD } else { CREAM });
    let control = setup.control[team.index()];
    let cr = Rect::new(list.x + list.w * 0.57, top + 6.0 * k, list.w * 0.43, 26.0 * k);
    if button(cr.x, cr.y, cr.w, cr.h, control_label(control), true) {
        view.session.setup.control[team.index()] = control.other();
    }
    chrome::text_box(list);
    if chosen {
        chrome::glow_frame(list, GOLD, false);
    }
    let mut remove = None;
    for (i, p) in view.session.setup.army(team).to_vec().into_iter().enumerate() {
        let y = list.y + i as f32 * row_h;
        let rr = Rect::new(list.x + 2.0 * k, y + 1.0 * k, list.w - 4.0 * k, row_h - 2.0 * k);
        let picked = chosen && view.picked == Some(i);
        // The steppers and the × take their clicks before the row does.
        let (bw, bx) = (20.0 * k, rr.x + rr.w - 4.0 * k);
        let minus = Rect::new(bx - 3.0 * bw - 46.0 * k, rr.y + 2.0 * k, bw, rr.h - 4.0 * k);
        let plus = Rect::new(bx - 2.0 * bw - 4.0 * k, minus.y, bw, minus.h);
        let cross = Rect::new(bx - bw, minus.y, bw, minus.h);
        let on_buttons = [minus, plus, cross].iter().any(|b| mouse_in(b.x, b.y, b.w, b.h));
        if !on_buttons && row(rr, picked) {
            view.side = team;
            view.picked = Some(i);
            view.offer = 0;
        } else if on_buttons {
            row(rr, picked);
        }
        assets.draw_portrait(p.unit, team, Rect::new(rr.x + 2.0 * k, rr.y + 1.0 * k, rr.h - 2.0 * k, rr.h - 2.0 * k));
        let name = match p.items.len() {
            0 => content.unit(p.unit).name.clone(),
            n => format!("{} +{n}", content.unit(p.unit).name),
        };
        text_fit(&name, rr.x + rr.h + 6.0 * k, rr.y + rr.h * 0.7, minus.x - rr.x - rr.h - 10.0 * k, size, CREAM);
        let lvl = trf!("Lv {level}", level = p.level);
        text_fit(&lvl, minus.x + bw + 3.0 * k, rr.y + rr.h * 0.7, 40.0 * k, (12.0 * k).round(), GOLD);
        if small_button(minus.x, minus.y, minus.w, minus.h, "-", p.level > 1) {
            view.session.setup.set_level(team, i, p.level - 1);
        }
        if small_button(plus.x, plus.y, plus.w, plus.h, "+", p.level < MAX_LEVEL) {
            view.session.setup.set_level(team, i, p.level + 1);
        }
        if small_button(cross.x, cross.y, cross.w, cross.h, "×", true) {
            remove = Some(i);
        }
    }
    if let Some(i) = remove {
        cue(Cue::Button);
        view.session.setup.remove(team, i);
        if view.side == team {
            view.picked = match view.picked {
                Some(p) if p == i => None,
                Some(p) if p > i => Some(p - 1),
                other => other,
            };
        }
    }
}

/// The picked unit's worn items (a click takes one off) and the next item it may wear,
/// stepped through with ‹ ›, put on with "Put on".
fn items_row(view: &mut SetupView, content: &Content, r: Rect, size: f32) {
    let k = chrome::k();
    let team = view.side;
    let Some(i) = view.picked.filter(|&i| i < view.session.setup.army(team).len()) else {
        chrome::shadow_text(tr("Pick a unit of an army to give it items."), r.x, r.y + 16.0 * k, size, CREAM);
        return;
    };
    let pick = view.session.setup.army(team)[i].clone();
    let title = trf!("Items of «{name}» (a click takes one off):", name = content.unit(pick.unit).name);
    text_fit(&title, r.x, r.y + 14.0 * k, r.w, size, GOLD);
    let mut x = r.x;
    let mut off = None;
    for (n, &item) in pick.items.iter().enumerate() {
        let name = content.try_item(item).map_or_else(|| "?".to_string(), |d| d.name.clone());
        let w = (measure(&name, (12.0 * k).round()).width + 14.0 * k).min(160.0 * k);
        if small_button(x, r.y + 20.0 * k, w, 22.0 * k, &name, true) {
            off = Some(n);
        }
        x += w + 4.0 * k;
    }
    if pick.items.is_empty() {
        chrome::shadow_text(tr("none"), x, r.y + 36.0 * k, (12.0 * k).round(), CREAM);
        x += 50.0 * k;
    }
    if let Some(n) = off {
        view.session.setup.take_off(team, i, n);
    }
    let offers = custom::wearable(content, &pick);
    if offers.is_empty() {
        return;
    }
    view.offer %= offers.len();
    let x = x.max(r.x + r.w * 0.45);
    let y = r.y + 20.0 * k;
    let bw = 22.0 * k;
    if small_button(x, y, bw, 22.0 * k, "‹", true) {
        view.offer = (view.offer + offers.len() - 1) % offers.len();
    }
    let item = offers[view.offer];
    let label = content.try_item(item).map_or_else(String::new, |d| format!("{} ({})", d.name, razdor::rules::items::kind_name(d.kind)));
    let nw = r.x + r.w - x - 2.0 * bw - 90.0 * k;
    text_fit(&label, x + bw + 4.0 * k, y + 16.0 * k, nw - 8.0 * k, (12.0 * k).round(), CREAM);
    if small_button(x + bw + nw, y, bw, 22.0 * k, "›", true) {
        view.offer = (view.offer + 1) % offers.len();
    }
    if small_button(x + 2.0 * bw + nw + 4.0 * k, y, 82.0 * k, 22.0 * k, tr("Put on"), true) && view.session.setup.wear(content, team, i, item).is_ok() {
        cue(Cue::Button);
        view.offer = 0;
    }
}
