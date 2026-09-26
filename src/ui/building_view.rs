//! The building window (town, castle, fort, village, church, market, tavern …), laid out as
//! the original's (video notes §2): a title bar, a column of tabs on the left, the tab's
//! content on the right, and the building's description at the bottom.

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::battle::Team;
use razdor::trf;
use razdor::rules::clock::{duration_label, MINUTES_PER_DAY};
use razdor::rules::content::{ArtefactType, ItemId, SpellDef};
use razdor::rules::formation::Slot;
use razdor::rules::game::{Currency, Game, HireError, TradeError, PACK_SIZE, SPELL_BOOK_SIZE};
use razdor::rules::items::describe;
use razdor::rules::script::HallEntry;
use razdor::rules::town::{ServiceError, Tab};
use razdor::rules::units::Unit;
use razdor::rules::world::LocationKind;

use super::assets::Assets;
use super::chrome;
use super::audio::{cue, Cue};
use super::dialog::{resource_icon, Dialog, Resource, MANA};
use super::items_view::{level_gains, unit_stat_lines};
use super::screens::stat_lines;
use super::story;
use super::widgets::*;
use super::world_view;
use super::Screen;

const PARCHMENT_INK: Color = Color::new(0.45, 0.28, 0.14, 1.0);
const BOX_INK: Color = Color::new(1.0, 0.86, 0.58, 1.0);
const SILVER: Color = Color::new(0.78, 0.78, 0.82, 1.0);
const TAB_RED: Color = Color::new(0.75, 0.18, 0.12, 1.0);

/// State of the building window between frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildingView {
    pub tab: Tab,
    /// Selected row of the market or sanctuary list.
    pub pick: Option<usize>,
    pub scroll: usize,
    /// The market shows the "sell" shop (the pack) instead of the goods.
    pub selling: bool,
}

impl BuildingView {
    pub fn new(tab: Tab) -> BuildingView {
        BuildingView { tab, pick: None, scroll: 0, selling: false }
    }

    fn switch(&mut self, tab: Tab) {
        *self = BuildingView::new(tab);
    }
}

pub fn tab_label(t: Tab) -> &'static str {
    match t {
        Tab::MainHall => tr("Main hall"),
        Tab::Barracks => tr("Barracks"),
        Tab::Garrison => tr("Garrison"),
        Tab::Market => tr("Market"),
        Tab::Sanctuary => tr("Sanctuary"),
        Tab::Tribute => tr("Tribute"),
        Tab::Shipyard => tr("Ships"),
    }
}

fn title(game: &Game) -> String {
    let Some(l) = game.location.map(|l| &game.world.locations[l]) else { return String::new() };
    if l.name.is_empty() {
        l.kind.label().to_string()
    } else {
        l.name.clone()
    }
}

pub fn service_error(e: ServiceError) -> String {
    match e {
        ServiceError::NotHere => tr("Not offered here.").into(),
        ServiceError::CannotAfford => tr("You cannot afford it.").into(),
        ServiceError::NotWounded => tr("Not wounded.").into(),
        ServiceError::NotDead => tr("Alive and well.").into(),
        ServiceError::TooLate => tr("Too late: the body can only be buried.").into(),
        ServiceError::Dead => tr("The dead cannot stand guard.").into(),
        ServiceError::SquadFull => tr("Your army is full.").into(),
        ServiceError::GarrisonFull => tr("The garrison is full.").into(),
        ServiceError::Hero => tr("The hero stays with his army.").into(),
        ServiceError::AlreadyKnown => tr("Already in your book.").into(),
        ServiceError::BookFull => tr("No room in the book.").into(),
        ServiceError::PackFull => tr("The pack is full.").into(),
        ServiceError::NoSuchUnit => tr("Nobody there.").into(),
    }
}

pub fn trade_error(e: TradeError) -> String {
    match e {
        TradeError::NoMarket => tr("There is no market here.").into(),
        TradeError::NotEnoughGold => tr("Not enough gold.").into(),
        TradeError::PackFull => tr("The pack is full.").into(),
        TradeError::NoSuchItem => tr("Nothing there.").into(),
        TradeError::NotForSale => tr("A personal item: it cannot be sold.").into(),
    }
}

struct Frame {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    /// Content pane.
    cx: f32,
    cy: f32,
    cw: f32,
}

/// The window, as the original's building window (836×600 in the reference video): the tab
/// column on the left, the content pane on the right.
fn window() -> Frame {
    let k = chrome::k();
    let (sw, sh) = (screen_width(), screen_height());
    let w = (836.0 * k).min(sw - 8.0).round();
    let h = (600.0 * k).min(sh - chrome::bar_height() - 4.0).round();
    let (x, y) = (((sw - w) / 2.0).round(), ((sh - chrome::bar_height() - h) / 2.0).max(2.0).round());
    Frame { x, y, w, h, cx: x + 256.0 * k, cy: y + 34.0 * k, cw: w - 264.0 * k }
}

/// The original's picture of a tab (`TB-<n>_RUS`, hovered `TBo`, open `TBd`), if it has one.
fn tab_art(tab: Option<Tab>) -> Option<usize> {
    Some(match tab {
        Some(Tab::MainHall) => 1,
        Some(Tab::Barracks) => 2,
        Some(Tab::Garrison) => 3,
        Some(Tab::Market) => 4,
        Some(Tab::Sanctuary) => 5,
        None => 6,
        Some(Tab::Tribute | Tab::Shipyard) => return None,
    })
}

/// A parchment tab in the left column (`tab` `None` is the exit). Returns true when clicked.
fn tab_button(label: &str, tab: Option<Tab>, r: Rect, active: bool) -> bool {
    let hover = mouse_in(r.x, r.y, r.w, r.h);
    let state = if active {
        "TBd"
    } else if hover {
        "TBo"
    } else {
        "TB-"
    };
    let art = tab_art(tab).and_then(|n| chrome::win(&format!("{state}{n}_RUS")));
    if let Some(t) = art {
        chrome::tex(&t, r, WHITE);
    } else {
        if active {
            draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.45, 0.4, 0.38, 0.35));
        } else if hover {
            draw_rectangle(r.x, r.y, r.w, r.h, Color::new(1.0, 0.95, 0.8, 0.25));
        }
        let (edge, width) = if active { (SILVER, 4.0) } else { (PARCHMENT_INK, 2.0) };
        draw_rectangle_lines(r.x + 6.0, r.y + 6.0, r.w - 12.0, r.h - 12.0, width, edge);
        draw_rectangle_lines(r.x + 12.0, r.y + 12.0, r.w - 24.0, r.h - 24.0, 1.0, Color { a: 0.6, ..edge });
        let size = fit_size(label, r.w - 28.0, (r.h * 0.36).round());
        let d = measure(label, size);
        let color = if active { TAB_RED } else { PARCHMENT_INK };
        text(label, r.x + (r.w - d.width) / 2.0 + 1.5, r.y + (r.h + d.offset_y) / 2.0 - 1.5, size, Color::new(1.0, 0.95, 0.85, 0.5));
        text(label, r.x + (r.w - d.width) / 2.0, r.y + (r.h + d.offset_y) / 2.0 - 3.0, size, color);
    }
    hover && clicked()
}

/// The red-brown text box with the building's description.
fn description_box(desc: &str, x: f32, y: f32, w: f32, h: f32) {
    chrome::text_box(Rect::new(x, y, w, h));
    let lines = wrap(desc, w - 60.0, 19.0);
    let top = y + (h - lines.len() as f32 * 23.0) / 2.0 + 16.0;
    for (i, line) in lines.iter().enumerate() {
        chrome::shadow_centered(line, x + w / 2.0, top + i as f32 * 23.0, 19.0, BOX_INK);
    }
}

/// Gold / wages / income counters, as under the original's recruit row.
fn counters(game: &Game, x: f32, y: f32, w: f32) {
    draw_rectangle(x, y, w, 50.0, Color::new(0.1, 0.06, 0.03, 0.55));
    chrome::silver_frame(Rect::new(x, y, w, 50.0), 1.0);
    let mut wages = format!("- {}", game.daily_wages());
    if game.daily_mana_wages() > 0 {
        wages += &format!(" / {}", trf!("{mana} mana", mana = game.daily_mana_wages()));
    }
    let cells = [
        (Resource::Gold, tr("Gold"), format!("{}", game.gold)),
        (Resource::Mana, tr("Mana"), format!("{}", game.mana)),
        (Resource::Wages, tr("Army wages"), wages),
        (Resource::Income, tr("Income"), format!("+ {}", game.daily_income())),
    ];
    let step = w / cells.len() as f32;
    for (k, (r, label, value)) in cells.iter().enumerate() {
        let cx = x + step * k as f32;
        resource_icon(*r, cx + 28.0, y + 26.0, 34.0);
        text_fit(label, cx + 52.0, y + 21.0, step - 56.0, 17.0, if *r == Resource::Mana { MANA } else { ACCENT });
        text(value, cx + 52.0, y + 41.0, 18.0, INK);
    }
}

/// A unit card: the portrait with a thin light frame, the level in the corner, HP and XP
/// bars along the bottom; dimmed with a cross for a corpse.
fn unit_card(game: &Game, assets: &Assets, u: &Unit, x: f32, y: f32, w: f32, h: f32) {
    let c = &game.content;
    draw_rectangle(x + 3.0, y + 3.0, w, h, Color::new(0.0, 0.0, 0.0, 0.45));
    let s = w.min(h);
    let sq = Rect::new(x + (w - s) / 2.0, y, s, s);
    draw_rectangle(x, y, w, h, Color::new(0.05, 0.08, 0.07, 1.0));
    assets.draw_portrait(u.def, Team::Player, sq);
    draw_rectangle_lines(x, y, w, h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
    if !u.alive() {
        draw_rectangle(x, y, w, h, Color::new(0.0, 0.0, 0.0, 0.55));
        draw_line(x + 12.0, y + 12.0, x + w - 12.0, y + h - 12.0, 3.0, RED);
        draw_line(x + w - 12.0, y + 12.0, x + 12.0, y + h - 12.0, 3.0, RED);
    } else if u.unpaid {
        chrome::badge("sign-payment", x + 12.0, y + 12.0, 20.0, RED);
    }
    let lv = trf!("Lv {level}", level = u.level);
    let lw = measure(&lv, 14.0).width;
    draw_rectangle(x + w - lw - 6.0, y + 1.0, lw + 5.0, 16.0, Color::new(0.0, 0.0, 0.0, 0.55));
    chrome::shadow_text(&lv, x + w - lw - 3.0, y + 14.0, 14.0, XP_COLOR);
    let name: String = u.name(c).chars().take(16).collect();
    draw_rectangle(x + 1.0, y + h - 24.0, w - 2.0, 23.0, Color::new(0.0, 0.0, 0.0, 0.5));
    chrome::shadow_centered(&name, x + w / 2.0, y + h - 11.0, 13.0, chrome::CREAM);
    hp_bar(x + 3.0, y + h - 7.0, w - 6.0, u.hp, u.max_hp(c));
    xp_bar(x + 3.0, y + h - 2.5, w - 6.0, 2.0, u.xp, u.xp_to_next(c));
}

/// Cell of a formation grid of cards.
fn grid_cell(game: &Game, slot: Slot, x: f32, y: f32, cw: f32, ch: f32, gap: f32) -> (f32, f32) {
    let (r, col) = game.content.formation.display(slot);
    (x + col as f32 * (cw + gap), y + r as f32 * (ch + gap))
}

fn empty_cells(game: &Game, x: f32, y: f32, cw: f32, ch: f32, gap: f32) {
    let f = game.content.formation;
    for r in 0..f.display_lines() {
        for col in (0..f.cols).filter(|&c| f.at_display(r, c).is_some()) {
            let (cx, cy) = (x + col as f32 * (cw + gap), y + r as f32 * (ch + gap));
            let Some(slot) = f.at_display(r, col) else { continue };
            // The ornament hangs under the cell when there is room for it.
            let ornament = ch + gap >= cw * 1.45;
            chrome::empty_cell(Rect::new(cx, cy, cw, ch), chrome::CellIcon::of(f, slot), ornament);
        }
    }
}

/// Main hall: the building's picture, the rumours on offer (heard for free; a rumour's own event may cost gold) and this
/// building's quests, the description.
fn main_hall(game: &mut Game, assets: &Assets, f: &Frame, view: &mut BuildingView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let l = game.location?;
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let pic_h = 250.0;
    {
        let loc = &game.world.locations[l];
        draw_rectangle(x, y, w, pic_h, Color::new(0.35, 0.5, 0.65, 1.0));
        draw_rectangle(x, y + pic_h * 0.62, w, pic_h * 0.38, Color::new(0.35, 0.5, 0.3, 1.0));
        // The original's picture of this kind of building, else its map sprite.
        let scene = match loc.kind {
            LocationKind::Town | LocationKind::Palace => Some("S_Town"),
            LocationKind::Castle | LocationKind::Fort => Some("S_Castle"),
            LocationKind::Church => Some("S_Church"),
            LocationKind::Market | LocationKind::Smithy => Some("S_Market"),
            LocationKind::Tavern => Some("S_Tavern"),
            LocationKind::Village => Some("S_Village"),
            LocationKind::Shipyard => Some("S_Shipyard"),
            LocationKind::Ruins => Some("S_Ruin"),
            _ => None,
        };
        if let Some(t) = scene.and_then(chrome::win) {
            let src_h = (t.width() * pic_h / w).min(t.height());
            chrome::tex_src(&t, Rect::new(0.0, (t.height() - src_h) / 2.0, t.width(), src_h), Rect::new(x, y, w, pic_h), WHITE);
        } else if let Some(tex) = assets.dt.as_ref().and_then(|a| a.building(loc.picture.0, loc.picture.1)) {
            let k = ((pic_h - 20.0) / tex.height()).min((w - 20.0) / tex.width()).min(2.5);
            let (tw, th) = (tex.width() * k, tex.height() * k);
            draw_texture_ex(&tex, x + (w - tw) / 2.0, y + pic_h - th - 6.0, WHITE, DrawTextureParams { dest_size: Some(vec2(tw, th)), ..Default::default() });
        } else {
            text_centered(loc.kind.label(), x + w / 2.0, y + pic_h / 2.0, 40.0, INK);
        }
        chrome::silver_frame(Rect::new(x, y, w, pic_h), 1.0);
    }
    let entries = game.hall_entries();
    if view.pick.is_some_and(|k| k >= entries.len()) {
        view.pick = None;
    }
    let rumour = view.pick.and_then(|k| match entries.get(k) {
        Some(HallEntry::Rumour(id)) => Some(*id),
        _ => None,
    });
    let ly = y + pic_h + 12.0;
    draw_rectangle(x, ly, w, 36.0, Color::new(0.0, 0.0, 0.0, 0.3));
    chrome::silver_frame(Rect::new(x, ly, w, 36.0), 1.0);
    chrome::shadow_text(tr("Quests and rumours:"), x + 14.0, ly + 25.0, 20.0, chrome::GOLD);
    let mut next = None;
    if button(x + w - 250.0, ly + 3.0, 240.0, 30.0, tr("Hear rumour"), rumour.is_some()) {
        if let Some(id) = rumour {
            match game.hear_rumour(id) {
                Ok(events) => {
                    *message = None;
                    view.pick = None;
                    next = world_view::handle_events(game, events, message, dialogs);
                }
                Err(e) => *message = Some(service_error(e)),
            }
        }
    }
    let list_y = ly + 42.0;
    let (row_h, rows) = (24.0, 5);
    let list_h = rows as f32 * row_h + 12.0;
    chrome::parchment(Rect::new(x, list_y, w, list_h), false);
    if entries.is_empty() {
        let none = if game.script().is_some() { tr("Nothing is on offer here.") } else { tr("No quests in the demo.") };
        text_centered(none, x + w / 2.0, list_y + list_h / 2.0 + 6.0, 19.0, PARCHMENT_INK);
    }
    let max_scroll = entries.len().saturating_sub(rows);
    if mouse_in(x, list_y, w, list_h) {
        let wh = wheel();
        if wh < 0.0 {
            view.scroll = (view.scroll + 1).min(max_scroll);
        } else if wh > 0.0 {
            view.scroll = view.scroll.saturating_sub(1);
        }
    }
    view.scroll = view.scroll.min(max_scroll);
    for (k, entry) in entries.iter().enumerate().skip(view.scroll).take(rows) {
        let ry = list_y + 6.0 + (k - view.scroll) as f32 * row_h;
        let (id, note, color) = match *entry {
            HallEntry::Rumour(id) => (id, tr("rumour"), Color::new(0.55, 0.1, 0.1, 1.0)),
            HallEntry::Quest(id) => (id, tr("in your journal"), Color::new(0.1, 0.3, 0.55, 1.0)),
            HallEntry::Done(id) => (id, tr("done"), PARCHMENT_INK),
        };
        if view.pick == Some(k) {
            draw_rectangle(x + 4.0, ry, w - 8.0, row_h - 2.0, Color::new(0.72, 0.6, 0.4, 1.0));
        }
        let title: String = story::event_title(game, id).chars().take(60).collect();
        text_fit(&title, x + 16.0, ry + 18.0, w - 48.0 - measure(note, 16.0).width, 19.0, color);
        text(note, x + w - 16.0 - measure(note, 16.0).width, ry + 17.0, 16.0, PARCHMENT_INK);
        if mouse_in(x, ry, w, row_h) && clicked() {
            view.pick = Some(k);
        }
    }
    let dy = list_y + list_h + 10.0;
    let desc = game.world.locations[l].description.clone();
    description_box(&desc, x, dy, w, f.y + f.h - dy - 10.0);
    next
}

/// Barracks: recruits for hire along the top, the counters, the army with heal and
/// resurrect buttons below.
fn barracks(game: &mut Game, assets: &Assets, f: &Frame, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let l = game.location?;
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let c = game.content.clone();
    let recruits = game.world.locations[l].recruits.clone();
    let hires = game.world.locations[l].hires();
    let (rw, rh) = (((w - 10.0) / 6.0 - 8.0).min(118.0), ((w - 10.0) / 6.0 - 8.0).min(112.0));
    // The recruits stand before the building's sepia interior, as in the original.
    let back = Rect::new(x, y, w, rh + 76.0);
    let interior = match game.world.locations[l].kind {
        LocationKind::Town | LocationKind::Palace => Some("BI_Town"),
        LocationKind::Castle | LocationKind::Fort => Some("BI_Castle"),
        LocationKind::Church => Some("BI_Church"),
        LocationKind::Ruins => Some("BI_Ruin"),
        _ => None,
    };
    match interior.and_then(chrome::win) {
        Some(t) => {
            let src_h = t.width() * back.h / back.w;
            chrome::tex_src(&t, Rect::new(0.0, (t.height() - src_h).max(0.0) / 2.0, t.width(), src_h.min(t.height())), back, WHITE);
        }
        None => draw_rectangle(back.x, back.y, back.w, back.h, Color::new(0.3, 0.2, 0.12, 1.0)),
    }
    chrome::silver_frame(back, 1.0);
    let mut hover_lines = Vec::new();
    if recruits.is_empty() {
        chrome::shadow_centered(tr("No recruits here."), x + w / 2.0, y + 70.0, 22.0, chrome::CREAM);
    }
    for (k, r) in recruits.iter().take(6).enumerate() {
        let (cx, cy) = (x + 8.0 + k as f32 * (rw + 8.0), y + 8.0);
        assets.draw_portrait(r.unit, Team::Player, Rect::new(cx, cy, rw, rh));
        draw_rectangle_lines(cx, cy, rw, rh, 1.0, Color::new(0.85, 0.85, 0.85, 0.9));
        if mouse_in(cx, cy, rw, rh) {
            let def = c.unit(r.unit);
            hover_lines.push((def.name.clone(), ACCENT));
            hover_lines.push((level_label(1, 0, c.xp_to_next(r.unit, 1)), XP_COLOR));
            hover_lines.extend(stat_lines(&c, r.unit).into_iter().map(|s| (s, INK)));
            hover_lines.push((trf!("Per level: {gains}", gains = level_gains(&c, r.unit)), INK));
            hover_lines.push((trf!("Daily wage {wage}", wage = c.wage_for(r.unit, razdor::rules::content::WageKind::of(def))), Color::new(0.95, 0.6, 0.25, 1.0)));
        }
        let price = game.hire_price(r.unit);
        let stock_left = r.stock != Some(0);
        let can = hires && stock_left && game.can_afford(price) && game.squad.len() < game.max_squad();
        if chrome::pill_button(Rect::new(cx + 4.0, cy + rh + 4.0, rw - 8.0, 22.0), tr("Hire"), can, true) {
            let name = c.unit(r.unit).name.clone();
            *message = Some(match game.hire(r.unit) {
                Ok(()) => trf!("{name} joins your army.", name),
                Err(HireError::NotEnoughGold) => tr("You cannot afford it.").into(),
                Err(HireError::SquadFull) => tr("Your army is full.").into(),
                Err(HireError::NotOffered) => tr("Not offered here.").into(),
            });
        }
        chrome::shadow_centered(&trf!("Price: {price}", price = price.amount), cx + rw / 2.0, cy + rh + 44.0, 15.0, if price.currency == Currency::Mana { MANA } else { chrome::GOLD });
        let left = match r.stock {
            Some(n) => trf!("{n} of {max} left", n, max = r.max),
            None => tr("always").to_string(),
        };
        chrome::shadow_centered(&left, cx + rw / 2.0, cy + rh + 62.0, 13.0, chrome::CREAM);
    }
    let cy = y + rh + 84.0;
    counters(game, x, cy, w);

    // The army: a 2×6 grid of cards with the heal / resurrect buttons.
    let gy = cy + 62.0;
    let gap = 6.0;
    let cw = (w - 5.0 * gap) / 6.0;
    let ch = ((f.y + f.h - gy - 12.0) / 2.0 - gap - 44.0).clamp(70.0, 130.0);
    empty_cells(game, x, gy, cw, ch + 44.0, gap);
    let heals = game.heals_here();
    let raises = game.resurrects_here();
    let mut action = None;
    for i in 0..game.squad.len() {
        let u = game.squad[i].clone();
        let (ux, uy) = grid_cell(game, u.slot, x, gy, cw, ch + 44.0, gap);
        unit_card(game, assets, &u, ux, uy, cw, ch);
        if mouse_in(ux, uy, cw, ch) {
            hover_lines = vec![(u.name(&c).to_string(), ACCENT)];
            hover_lines.extend(unit_stat_lines(&c, &u, game.wage(i)).into_iter().map(|s| (s, INK)));
        }
        let (bx, by, bw) = (ux + 4.0, uy + ch + 3.0, cw - 8.0);
        if let Some(p) = game.heal_price(i).filter(|_| heals) {
            if chrome::pill_button(Rect::new(bx, by, bw, 22.0), tr("Heal"), game.can_afford(p), false) {
                action = Some((i, false));
            }
            chrome::shadow_centered(&trf!("Price: {price}", price = p.amount), ux + cw / 2.0, by + 38.0, 14.0, chrome::GOLD);
        } else if let Some(p) = game.resurrect_price(i).filter(|_| raises) {
            if chrome::pill_button(Rect::new(bx, by, bw, 22.0), tr("Raise"), game.can_afford(p), false) {
                action = Some((i, true));
            }
            chrome::shadow_centered(&trf!("Price: {price}", price = p.amount), ux + cw / 2.0, by + 38.0, 14.0, chrome::GOLD);
        } else if !u.alive() {
            let left = game.resurrection_minutes_left(i).map_or(tr("to be buried").into(), |m| duration_label(m as f64));
            text_centered(&left, ux + cw / 2.0, by + 16.0, 14.0, DIM);
        }
    }
    let mut next = None;
    if let Some((i, raise)) = action {
        let name = game.squad[i].name(&c).to_string();
        let r = if raise { game.resurrect(i) } else { game.heal(i) };
        match r {
            Ok(events) => {
                *message = Some(if raise { trf!("{name} rises again.", name) } else { trf!("{name} is healed.", name) });
                // What happened meanwhile: a noon report, the scenario's events.
                next = world_view::handle_events(game, events, message, dialogs);
            }
            Err(e) => *message = Some(service_error(e)),
        }
    }
    let note = match (heals, raises) {
        (true, true) => tr("Healing and raising the dead are paid at once."),
        (true, false) => tr("Healing is paid at once. The dead are raised in towns and churches."),
        _ => tr("No healing here."),
    };
    let size = fit_size(note, w, 15.0);
    chrome::shadow_text(&ellipsize(note, w, size), x, f.y + f.h - 6.0, size, chrome::CREAM);
    tooltip(&hover_lines);
    next
}

/// Garrison: the player's troops left here (top) and his army (bottom); click to move.
fn garrison(game: &mut Game, assets: &Assets, f: &Frame, message: &mut Option<String>) {
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let c = game.content.clone();
    let gap = 6.0;
    let cw = (w - 5.0 * gap) / 6.0;
    let ch = 110.0;
    text(tr("Garrison: click a unit to take it back."), x, y + 20.0, 19.0, ACCENT);
    let gy = y + 30.0;
    empty_cells(game, x, gy, cw, ch, gap);
    let mut take = None;
    let mut hover = Vec::new();
    let now = game.clock.total_minutes() as u64;
    for (j, s) in game.garrison_here().iter().enumerate() {
        let (ux, uy) = grid_cell(game, s.unit.slot, x, gy, cw, ch, gap);
        unit_card(game, assets, &s.unit, ux, uy, cw, ch);
        if mouse_in(ux, uy, cw, ch) {
            let paid = if now.saturating_sub(s.since) < MINUTES_PER_DAY { tr("paid until the next noon") } else { tr("no wage while on guard") };
            let lv = level_label(s.unit.level, s.unit.xp, s.unit.xp_to_next(&c));
            hover = vec![(s.unit.name(&c).to_string(), ACCENT), (lv, XP_COLOR), (trf!("{hp}/{max} HP, {paid}", hp = s.unit.hp, max = s.unit.max_hp(&c), paid), INK)];
            if clicked() {
                take = Some(j);
            }
        }
    }
    let ay = gy + 2.0 * (ch + gap) + 40.0;
    text(tr("Your army: click a unit to leave it here."), x, ay - 10.0, 19.0, ACCENT);
    empty_cells(game, x, ay, cw, ch, gap);
    let mut leave = None;
    for i in 0..game.squad.len() {
        let u = &game.squad[i];
        let (ux, uy) = grid_cell(game, u.slot, x, ay, cw, ch, gap);
        unit_card(game, assets, u, ux, uy, cw, ch);
        if mouse_in(ux, uy, cw, ch) {
            let lv = level_label(u.level, u.xp, u.xp_to_next(&c));
            hover = vec![(u.name(&c).to_string(), ACCENT), (lv, XP_COLOR), (trf!("{hp}/{max} HP, wage {wage}", hp = u.hp, max = u.max_hp(&c), wage = game.wage(i)), INK)];
            if clicked() {
                leave = Some(i);
            }
        }
    }
    if let Some(j) = take {
        *message = Some(match game.take_from_garrison(j) {
            Ok(()) => tr("Back in your army.").into(),
            Err(e) => service_error(e),
        });
    }
    if let Some(i) = leave {
        let name = game.squad[i].name(&c).to_string();
        *message = Some(match game.leave_in_garrison(i) {
            Ok(()) => trf!("{name} stays on guard.", name),
            Err(e) => service_error(e),
        });
    }
    let note = trf!("Units on guard are paid for their first day only and heal {pct}% a day.", pct = c.options.garrison_auto_heal);
    let size = fit_size(&note, w, 15.0);
    chrome::shadow_text(&ellipsize(&note, w, size), x, f.y + f.h - 6.0, size, chrome::CREAM);
    tooltip(&hover);
}

/// A list with a selection and a scroll bar. Rows are (icon item, name, price). Returns the
/// clicked row.
#[allow(clippy::too_many_arguments)]
fn price_list(assets: Option<&Assets>, rows: &[(Option<ItemId>, String, String)], pick: Option<usize>, scroll: &mut usize, x: f32, y: f32, w: f32, visible: usize) -> Option<usize> {
    let row_h = 30.0;
    draw_rectangle(x, y, w, 26.0 + visible as f32 * row_h, Color::new(0.0, 0.04, 0.02, 0.45));
    chrome::silver_frame(Rect::new(x, y, w, 26.0 + visible as f32 * row_h), 1.0);
    text(tr("Name"), x + 50.0, y + 19.0, 17.0, ACCENT);
    text(tr("Price"), x + w - 80.0, y + 19.0, 17.0, ACCENT);
    let max_scroll = rows.len().saturating_sub(visible);
    let over = mouse_in(x, y, w, 26.0 + visible as f32 * row_h);
    let wheel = if over { wheel() } else { 0.0 };
    if wheel < 0.0 {
        *scroll = (*scroll + 1).min(max_scroll);
    } else if wheel > 0.0 {
        *scroll = scroll.saturating_sub(1);
    }
    *scroll = (*scroll).min(max_scroll);
    let mut hit = None;
    for (k, (icon, name, price)) in rows.iter().enumerate().skip(*scroll).take(visible) {
        let ry = y + 26.0 + (k - *scroll) as f32 * row_h;
        let sel = pick == Some(k);
        if sel {
            draw_rectangle(x + 2.0, ry, w - 20.0, row_h - 2.0, Color::new(0.3, 0.38, 0.3, 1.0));
        }
        if let (Some(item), Some(assets)) = (icon, assets) {
            assets.draw_item(*item, x + 8.0, ry + 1.0, row_h - 4.0);
        }
        let shown: String = name.chars().take(26).collect();
        text(&shown, x + 50.0, ry + 21.0, 18.0, if sel { WHITE } else { ACCENT });
        text(price, x + w - 80.0, ry + 21.0, 18.0, ACCENT);
        if mouse_in(x, ry, w - 18.0, row_h) && clicked() {
            hit = Some(k);
        }
    }
    if rows.len() > visible {
        let bh = visible as f32 * row_h;
        draw_rectangle(x + w - 14.0, y + 26.0, 10.0, bh, Color::new(0.05, 0.08, 0.07, 1.0));
        let th = bh * visible as f32 / rows.len() as f32;
        let ty = y + 26.0 + (bh - th) * *scroll as f32 / max_scroll.max(1) as f32;
        draw_rectangle(x + w - 13.0, ty, 8.0, th, SILVER);
    }
    hit
}

pub(super) fn item_description(game: &Game, assets: &Assets, item: ItemId, x: f32, y: f32, w: f32, h: f32) {
    let c = &game.content;
    let d = c.item(item);
    chrome::text_box(Rect::new(x, y, w, h));
    draw_rectangle_lines(x, y, w, h, 2.0, Color::new(0.6, 0.42, 0.25, 1.0));
    assets.draw_item(item, x + w / 2.0 - 28.0, y + 10.0, 56.0);
    text_centered(&d.name, x + w / 2.0, y + 90.0, 20.0, BOX_INK);
    let limit = match d.kind {
        ArtefactType::BlowWeapon => Some(tr("warriors only")),
        ArtefactType::ShotWeapon => Some(tr("shooters only")),
        ArtefactType::Staff => Some(tr("mages only")),
        ArtefactType::Item => Some(tr("trade goods: cannot be worn")),
        _ => None,
    };
    let mut ly = y + 112.0;
    if let Some(limit) = limit {
        text_centered(limit, x + w / 2.0, ly, 16.0, Color::new(1.0, 0.6, 0.4, 1.0));
        ly += 20.0;
    }
    for line in wrap(&d.description, w - 24.0, 15.0).into_iter().take(4) {
        text_centered(&line, x + w / 2.0, ly, 15.0, BOX_INK);
        ly += 18.0;
    }
    for line in wrap(&describe(c, item), w - 24.0, 16.0).into_iter().take(3) {
        text_centered(&line, x + w / 2.0, ly + 4.0, 16.0, MANA);
        ly += 19.0;
    }
}

/// Market: the goods (or, in the sell shop, the pack) with prices, the selected item's
/// description, and the buy / sell buttons.
fn market(game: &mut Game, assets: &Assets, f: &Frame, view: &mut BuildingView, message: &mut Option<String>) -> Option<Screen> {
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let c = game.content.clone();
    let dw = w * 0.42;
    let (lx, lw) = (x + dw + 10.0, w - dw - 10.0);
    let rows: Vec<(Option<ItemId>, String, String)> = if view.selling {
        game.pack.iter().map(|&i| (Some(i), c.item(i).name.clone(), game.sell_price(i).to_string())).collect()
    } else {
        game.market_here().unwrap_or(&[]).iter().map(|&i| (Some(i), c.item(i).name.clone(), game.buy_price(i).to_string())).collect()
    };
    text_centered(if view.selling { tr("Your pack: what the market pays") } else { tr("Goods for sale") }, lx + lw / 2.0, y + 18.0, 18.0, ACCENT);
    if let Some(k) = price_list(Some(assets), &rows, view.pick, &mut view.scroll, lx, y + 26.0, lw, 8) {
        view.pick = Some(k);
    }
    if view.pick.is_some_and(|k| k >= rows.len()) {
        view.pick = None;
    }
    let dh = 26.0 + 8.0 * 30.0 + 26.0;
    match view.pick.and_then(|k| rows.get(k)).and_then(|r| r.0) {
        Some(item) => item_description(game, assets, item, x, y, dw, dh),
        None => {
            chrome::text_box(Rect::new(x, y, dw, dh));
            text_centered(tr("Pick an item from the list."), x + dw / 2.0, y + dh / 2.0, 18.0, BOX_INK);
        }
    }
    let by = y + dh + 10.0;
    let mut next = None;
    if button(x, by, 150.0, 40.0, tr("Inventory"), true) {
        next = Some(Screen::Squad { selected: 0, scroll: 0, back: Some(view.clone()) });
    }
    resource_icon(Resource::Gold, x + 180.0, by + 20.0, 34.0);
    text(&trf!("Gold {gold}", gold = game.gold), x + 202.0, by + 27.0, 20.0, ACCENT);
    let label = if view.selling { tr("Sell") } else { tr("Buy") };
    let can = match (view.selling, view.pick) {
        (true, Some(k)) => k < game.pack.len(),
        (false, Some(k)) => rows.get(k).and_then(|r| r.0).is_some_and(|i| game.gold >= game.buy_price(i) && game.pack.len() < PACK_SIZE),
        _ => false,
    };
    if button(lx, by, 130.0, 40.0, label, can) {
        let k = view.pick.unwrap_or(0);
        *message = Some(if view.selling {
            let name = c.item(game.pack[k]).name.clone();
            match game.sell(k) {
                Ok(g) => trf!("Sold {name} for {g} gold.", name, g),
                Err(e) => trade_error(e),
            }
        } else {
            match game.buy(k) {
                Ok(item) => {
                    cue(Cue::Item(c.item(item).kind));
                    trf!("Bought {item}. It is in your pack.", item = c.item(item).name)
                }
                Err(e) => trade_error(e),
            }
        });
        view.pick = None;
    }
    let toggle = if view.selling { tr("Back to the goods") } else { tr("Sell shop") };
    if button(lx + lw - 190.0, by, 190.0, 40.0, toggle, true) {
        view.selling = !view.selling;
        view.pick = None;
        view.scroll = 0;
    }
    if let Some(l) = game.location {
        let dy = by + 52.0;
        description_box(&game.world.locations[l].description, x, dy, w, f.y + f.h - dy - 10.0);
    }
    next
}

/// Sanctuary: spells for sale; a spell bought goes into the hero's book.
fn sanctuary(game: &mut Game, f: &Frame, view: &mut BuildingView, message: &mut Option<String>) {
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let spells: Vec<SpellDef> = game.spells_here().into_iter().cloned().collect();
    let dw = w * 0.45;
    let (lx, lw) = (x + dw + 10.0, w - dw - 10.0);
    let rows: Vec<_> = spells.iter().map(|s| (None, s.name.clone(), s.cost_gold.to_string())).collect();
    text_centered(tr("Spells"), lx + lw / 2.0, y + 18.0, 18.0, ACCENT);
    if let Some(k) = price_list(None, &rows, view.pick, &mut view.scroll, lx, y + 26.0, lw, 7) {
        view.pick = Some(k);
    }
    let dh = 150.0;
    chrome::text_box(Rect::new(x, y, dw, dh));
    let chosen = view.pick.and_then(|k| spells.get(k));
    match chosen {
        Some(s) => {
            chrome::spell_icon(&s.icons, Rect::new(x + 14.0, y + 14.0, 96.0, 96.0));
            text_centered(&s.name, x + dw / 2.0, y + 30.0, 21.0, BOX_INK);
            for (i, line) in super::spellbook::spell_lines(game, s).iter().enumerate() {
                text_centered(line, x + dw / 2.0, y + 60.0 + i as f32 * 22.0, 16.0, MANA);
            }
        }
        None => text_centered(tr("Pick a spell from the list."), x + dw / 2.0, y + dh / 2.0, 18.0, BOX_INK),
    }
    let by = y + dh + 14.0;
    if let Some(s) = chosen {
        if game.knows_spell(s.id) {
            text_centered(tr("This spell is already in your book!"), x + dw / 2.0, by + 20.0, 18.0, MANA);
        } else if button(x + dw - 130.0, by + 50.0, 130.0, 40.0, tr("Buy"), game.gold >= s.cost_gold) {
            *message = Some(match game.learn_spell(s.id) {
                Ok(()) => trf!("{spell} is written into your book.", spell = s.name),
                Err(e) => service_error(e),
            });
        }
    }
    resource_icon(Resource::Gold, x + 26.0, by + 70.0, 34.0);
    text(&trf!("Gold {gold}", gold = game.gold), x + 50.0, by + 77.0, 20.0, ACCENT);
    text_fit(&trf!("Book {n}/{max}. Cast from the spell book on the map (B).", n = game.spells.len(), max = SPELL_BOOK_SIZE), x, by + 118.0, w, 16.0, DIM);
    if let Some(l) = game.location {
        let dy = y + 26.0 + 26.0 + 7.0 * 30.0 + 50.0;
        description_box(&game.world.locations[l].description, x, dy, w, f.y + f.h - dy - 10.0);
    }
}

/// A village: its waiting tribute and what may be asked instead.
fn tribute(game: &mut Game, f: &Frame, message: &mut Option<String>) {
    let Some(l) = game.location else { return };
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let v = game.world.locations[l].clone();
    draw_rectangle(x, y, w, 120.0, Color::new(0.2, 0.12, 0.07, 1.0));
    text_fit(tr("The headman keeps the tribute for whoever protects the village."), x + 16.0, y + 28.0, w - 32.0, 19.0, INK);
    resource_icon(Resource::Gold, x + 40.0, y + 76.0, 40.0);
    text(&trf!("Gold {gold} (up to {max})", gold = v.tribute_gold, max = v.gold_max.max(v.gold_income)), x + 70.0, y + 84.0, 20.0, ACCENT);
    resource_icon(Resource::Mana, x + 340.0, y + 76.0, 40.0);
    text(&trf!("Mana {mana} (up to {max})", mana = v.tribute_mana, max = v.mana_max.max(v.mana_income)), x + 370.0, y + 84.0, 20.0, MANA);
    // The tribute is taken on entering (economy.md §3); only an offer waits for an answer.
    let mut by = y + 140.0;
    let status = if game.village_offer().is_some() {
        tr("The villagers ask you something before paying their tribute.")
    } else if v.hostile() {
        tr("The village pays no tribute to you.")
    } else {
        tr("Tribute already collected.")
    };
    text_fit(status, x, by + 26.0, w, 19.0, DIM);
    by += 52.0;
    // The one offer this visit may bring (instead of the tribute: it empties the village).
    if let Some(offer) = game.village_offer() {
        use razdor::rules::economy::{OfferResult, VillageOffer, BLESSING_SPELLS, FURS_ITEM, PRIEST_SPELL};
        let spell_name = |id: u32| game.spell(id).map_or(String::new(), |s| s.name.clone());
        let label = match offer {
            VillageOffer::Innkeeper => tr("Instead: the innkeeper pays your army").to_string(),
            VillageOffer::Priest => trf!("Instead: the priest heals ({spell})", spell = spell_name(PRIEST_SPELL)),
            VillageOffer::Blessing => {
                let names: Vec<String> = BLESSING_SPELLS.iter().map(|&s| spell_name(s)).filter(|n| !n.is_empty()).collect();
                trf!("Instead: a long blessing ({spells})", spells = names.join(" / "))
            }
            VillageOffer::Furs => trf!("Instead: furs ({item})", item = game.content.try_item(razdor::rules::content::ItemId(FURS_ITEM)).map_or("", |i| i.name.as_str())),
            VillageOffer::Witch => tr("Instead: the witch's gift of mana").to_string(),
        };
        if button(x, by, 460.0f32.min(w), 42.0, &label, true) {
            let result = game.accept_offer();
            let spell_name = |id: u32| game.spell(id).map_or(String::new(), |s| s.name.clone());
            *message = result.map(|r| match r {
                OfferResult::Paid(n) => trf!("The innkeeper pays off your {n} men.", n),
                OfferResult::Healed(h) => trf!("The priest tends to your wounded: {h} hits.", h = format!("{h:+}")),
                OfferResult::Blessing(id) => trf!("The villagers pray for you: {spell}.", spell = spell_name(id)),
                OfferResult::Furs(item) => trf!("You get {item}.", item = game.content.item(item).name),
                OfferResult::Mana(m) => trf!("The witch gives {m} mana.", m),
            });
        }
        by += 52.0;
        if button(x, by, 460.0f32.min(w), 42.0, tr("No thanks: take the tribute"), true) {
            let (gold, mana) = (v.tribute_gold, v.tribute_mana);
            *message = game.decline_offer().map(|t| match t {
                razdor::rules::game::Tribute::Gold(_) => trf!("The village pays {gold} gold and {mana} mana.", gold, mana),
                razdor::rules::game::Tribute::Item(item) => trf!("The village pays with a {item}.", item = game.content.item(item).name),
            });
        }
        by += 52.0;
    }
    by += 4.0;
    text_fit(tr("The tribute grows every midnight, slower as it nears the village's maximum."), x, by, w, 16.0, DIM);
    let dy = by + 24.0;
    description_box(&v.description, x, dy, w, f.y + f.h - dy - 10.0);
}

/// A shipyard: rent a ship for `ShipCost` gold. It waits on the water nearby.
fn shipyard(game: &mut Game, f: &Frame, message: &mut Option<String>) {
    let Some(l) = game.location else { return };
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let price = game.ship_price();
    draw_rectangle(x, y, w, 120.0, Color::new(0.2, 0.12, 0.07, 1.0));
    text_fit(tr("The shipwright rents out ships. One ship at a time: a new one sends the old one home."), x + 16.0, y + 28.0, w - 32.0, 18.0, INK);
    resource_icon(Resource::Gold, x + 40.0, y + 76.0, 40.0);
    text(&trf!("A ship: {price} gold", price), x + 70.0, y + 84.0, 20.0, ACCENT);
    let by = y + 140.0;
    let label = if game.ship.is_some() { tr("Rent a new ship") } else { tr("Rent a ship") };
    if button(x, by, 420.0, 42.0, label, game.gold >= price) {
        *message = Some(match game.rent_ship() {
            Ok(_) => tr("The ship waits at the pier. Walk onto it, or click the water.").into(),
            Err(razdor::rules::ships::ShipError::NoWater) => tr("There is no water to sail from here.").into(),
            Err(razdor::rules::ships::ShipError::NotEnoughGold) => tr("Not enough gold.").into(),
            Err(razdor::rules::ships::ShipError::NoShipyard) => tr("The shipwright will not deal with you.").into(),
        });
    }
    let notes = [tr("With a ship, click the water to sail; click the shore to land."), tr("The ship waits where you land; walk back onto it to sail again.")];
    for (i, n) in notes.iter().enumerate() {
        text_fit(n, x, by + 70.0 + i as f32 * 20.0, w, 16.0, DIM);
    }
    let dy = by + 120.0;
    description_box(&game.world.locations[l].description, x, dy, w, f.y + f.h - dy - 10.0);
}

/// The building window. `Exit` (or Escape) returns to the map.
pub fn frame(game: &mut Game, assets: &Assets, view: &mut BuildingView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    world_view::backdrop(game, assets);
    // Events that happened meanwhile (an answer's follow-ups …).
    let pending = game.drain_events();
    let early = world_view::handle_events(game, pending, message, dialogs);
    let tabs = game.tabs_here();
    if tabs.is_empty() {
        return Some(Screen::WorldMap);
    }
    if !tabs.contains(&view.tab) {
        view.switch(tabs[0]);
    }
    let f = window();
    let k = chrome::k();
    let (_, close) = chrome::window(Rect::new(f.x, f.y, f.w, f.h), &title(game), chrome::Skin::Marble, true);

    // Tabs, on light parchment.
    let col = Rect::new(f.x + 2.0 * k, f.y + 27.0 * k, 244.0 * k, f.h - 29.0 * k);
    chrome::surface(col, chrome::Skin::Paper);
    draw_line(col.x + col.w + 1.0, col.y, col.x + col.w + 1.0, col.y + col.h, 1.5 * k, SILVER);
    let (tw, th) = (228.0 * k, 88.0 * k);
    let tx = col.x + (col.w - tw) / 2.0;
    let pitch = (th + 6.0 * k).min((col.h - th - 20.0 * k) / tabs.len().max(1) as f32);
    for (i, &t) in tabs.iter().enumerate() {
        let r = Rect::new(tx, col.y + 12.0 * k + i as f32 * pitch, tw, th);
        if tab_button(tab_label(t), Some(t), r, view.tab == t) && view.tab != t {
            view.switch(t);
            *message = None;
        }
    }
    let exit = tab_button(tr("Exit"), None, Rect::new(tx, col.y + col.h - th - 10.0 * k, tw, th), false);

    let mut next = early;
    match view.tab {
        Tab::MainHall => next = next.or(main_hall(game, assets, &f, view, message, dialogs)),
        Tab::Barracks => next = next.or(barracks(game, assets, &f, message, dialogs)),
        Tab::Garrison => garrison(game, assets, &f, message),
        Tab::Market => next = market(game, assets, &f, view, message),
        Tab::Sanctuary => sanctuary(game, &f, view, message),
        Tab::Tribute => tribute(game, &f, message),
        Tab::Shipyard => shipyard(game, &f, message),
    }
    if let Some(m) = message {
        let w = measure(m, 20.0).width + 40.0;
        let (cx, y) = (f.cx + f.cw / 2.0, f.y + f.h + 6.0);
        draw_rectangle(cx - w / 2.0, y, w, 30.0, PANEL);
        text_centered(m, cx, y + 21.0, 20.0, ACCENT);
    }
    if close || exit || key(KeyCode::Escape) {
        *message = None;
        return Some(Screen::WorldMap);
    }
    next
}
