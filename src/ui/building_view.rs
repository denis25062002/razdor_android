//! The building window (town, castle, fort, village, church, market, tavern …), laid out as
//! the original's (video notes §2): a title bar, a column of tabs on the left, the tab's
//! content on the right, and the building's description at the bottom.

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::clock::{duration_label, MINUTES_PER_DAY};
use razdor::rules::content::{ArtefactType, ItemId, SpellDef};
use razdor::rules::formation::Slot;
use razdor::rules::game::{Currency, Game, HireError, TradeError, PACK_SIZE, SPELL_BOOK_SIZE};
use razdor::rules::items::describe;
use razdor::rules::script::{HallEntry, RUMOUR_PRICE};
use razdor::rules::town::{ServiceError, Tab};
use razdor::rules::units::Unit;

use super::assets::Assets;
use super::dialog::{resource_icon, Dialog, Resource, MANA};
use super::items_view::unit_stat_lines;
use super::screens::stat_lines;
use super::story;
use super::widgets::*;
use super::world_view;
use super::Screen;

const MARBLE: Color = Color::new(0.10, 0.19, 0.15, 1.0);
const MARBLE_EDGE: Color = Color::new(0.36, 0.55, 0.44, 1.0);
const PARCHMENT: Color = Color::new(0.85, 0.75, 0.55, 1.0);
const PARCHMENT_INK: Color = Color::new(0.45, 0.28, 0.14, 1.0);
const BOX: Color = Color::new(0.32, 0.15, 0.09, 1.0);
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
        Tab::MainHall => "Main hall",
        Tab::Barracks => "Barracks",
        Tab::Garrison => "Garrison",
        Tab::Market => "Market",
        Tab::Sanctuary => "Sanctuary",
        Tab::Tribute => "Tribute",
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
        ServiceError::NotHere => "Not offered here.".into(),
        ServiceError::CannotAfford => "You cannot afford it.".into(),
        ServiceError::NotWounded => "Not wounded.".into(),
        ServiceError::NotDead => "Alive and well.".into(),
        ServiceError::TooLate => "Too late: the body can only be buried.".into(),
        ServiceError::Dead => "The dead cannot stand guard.".into(),
        ServiceError::SquadFull => "Your army is full.".into(),
        ServiceError::GarrisonFull => "The garrison is full.".into(),
        ServiceError::Hero => "The hero stays with his army.".into(),
        ServiceError::AlreadyKnown => "Already in your book.".into(),
        ServiceError::BookFull => "No room in the book.".into(),
        ServiceError::PackFull => "The pack is full.".into(),
        ServiceError::NoSuchUnit => "Nobody there.".into(),
    }
}

pub fn trade_error(e: TradeError) -> String {
    match e {
        TradeError::NoMarket => "There is no market here.".into(),
        TradeError::NotEnoughGold => "Not enough gold.".into(),
        TradeError::PackFull => "The pack is full.".into(),
        TradeError::NoSuchItem => "Nothing there.".into(),
        TradeError::NotForSale => "A personal item: it cannot be sold.".into(),
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

fn window() -> Frame {
    let (sw, sh) = (screen_width(), screen_height());
    let w = 1060.0f32.min(sw - 20.0);
    let h = 700.0f32.min(sh - 130.0);
    let (x, y) = ((sw - w) / 2.0, ((sh - 84.0 - h) / 2.0 - 16.0).max(8.0));
    Frame { x, y, w, h, cx: x + 262.0, cy: y + 38.0, cw: w - 272.0 }
}

/// A parchment tab in the left column. Returns true when clicked.
fn tab_button(label: &str, x: f32, y: f32, w: f32, h: f32, active: bool) -> bool {
    let hover = mouse_in(x, y, w, h);
    draw_rectangle(x, y, w, h, if hover && !active { Color::new(0.9, 0.81, 0.62, 1.0) } else { PARCHMENT });
    draw_rectangle_lines(x, y, w, h, if active { 4.0 } else { 1.5 }, if active { SILVER } else { PARCHMENT_INK });
    let size = 30.0;
    let d = measure(label, size);
    text(label, x + (w - d.width) / 2.0, y + (h + d.offset_y) / 2.0 - 3.0, size, if active { TAB_RED } else { PARCHMENT_INK });
    hover && clicked()
}

/// The dark text box with the building's description.
fn description_box(desc: &str, x: f32, y: f32, w: f32, h: f32) {
    draw_rectangle(x, y, w, h, BOX);
    draw_rectangle_lines(x, y, w, h, 2.0, Color::new(0.6, 0.42, 0.25, 1.0));
    let lines = wrap(desc, w - 40.0, 19.0);
    let top = y + (h - lines.len() as f32 * 23.0) / 2.0 + 16.0;
    for (i, line) in lines.iter().enumerate() {
        text_centered(line, x + w / 2.0, top + i as f32 * 23.0, 19.0, BOX_INK);
    }
}

/// Gold / wages / income counters, as under the original's recruit row.
fn counters(game: &Game, x: f32, y: f32, w: f32) {
    draw_rectangle(x, y, w, 50.0, Color::new(0.2, 0.12, 0.07, 1.0));
    draw_rectangle_lines(x, y, w, 50.0, 1.5, Color::new(0.55, 0.4, 0.25, 1.0));
    let mut wages = format!("- {}", game.daily_wages());
    if game.daily_mana_wages() > 0 {
        wages += &format!(" / {} mana", game.daily_mana_wages());
    }
    let cells = [
        (Resource::Gold, "Gold", format!("{}", game.gold)),
        (Resource::Mana, "Mana", format!("{}", game.mana)),
        (Resource::Wages, "Army wages", wages),
        (Resource::Income, "Income", format!("+ {}", game.daily_income())),
    ];
    let step = w / cells.len() as f32;
    for (k, (r, label, value)) in cells.iter().enumerate() {
        let cx = x + step * k as f32;
        resource_icon(*r, cx + 28.0, y + 26.0, 34.0);
        text(label, cx + 52.0, y + 21.0, 17.0, if *r == Resource::Mana { MANA } else { ACCENT });
        text(value, cx + 52.0, y + 41.0, 18.0, INK);
    }
}

/// A unit card: portrait, name, HP bar; dimmed with a cross for a corpse.
fn unit_card(game: &Game, assets: &Assets, u: &Unit, x: f32, y: f32, w: f32, h: f32) {
    draw_rectangle(x, y, w, h, Color::new(0.12, 0.2, 0.17, 1.0));
    draw_rectangle_lines(x, y, w, h, 1.0, MARBLE_EDGE);
    let c = &game.content;
    assets.draw_unit(u.def, Team::Player, x + w / 2.0, y + h * 0.42, h * 0.72);
    if !u.alive() {
        draw_rectangle(x, y, w, h, Color::new(0.0, 0.0, 0.0, 0.55));
        draw_line(x + 12.0, y + 12.0, x + w - 12.0, y + h - 30.0, 3.0, RED);
        draw_line(x + w - 12.0, y + 12.0, x + 12.0, y + h - 30.0, 3.0, RED);
    } else if u.unpaid {
        text("unpaid", x + 4.0, y + 16.0, 15.0, RED);
    }
    let name: String = u.name(c).chars().take(14).collect();
    text_centered(&name, x + w / 2.0, y + h - 14.0, 14.0, INK);
    hp_bar(x + 4.0, y + h - 8.0, w - 8.0, u.hp, u.max_hp(c));
}

/// Cell of a formation grid of cards.
fn grid_cell(game: &Game, slot: Slot, x: f32, y: f32, cw: f32, ch: f32, gap: f32) -> (f32, f32) {
    let f = game.content.formation;
    let r = f.rows().iter().position(|&r| r == slot.row).unwrap_or(0);
    (x + slot.col as f32 * (cw + gap), y + r as f32 * (ch + gap))
}

fn empty_cells(game: &Game, x: f32, y: f32, cw: f32, ch: f32, gap: f32) {
    let f = game.content.formation;
    for (r, _) in f.rows().iter().enumerate() {
        for col in 0..f.cols {
            let (cx, cy) = (x + col as f32 * (cw + gap), y + r as f32 * (ch + gap));
            draw_rectangle(cx, cy, cw, ch, Color::new(0.07, 0.13, 0.11, 1.0));
            draw_rectangle_lines(cx, cy, cw, ch, 1.0, Color::new(0.25, 0.38, 0.32, 1.0));
        }
    }
}

/// Main hall: the building's picture, the rumours on offer (heard for a price) and this
/// building's quests, the description.
fn main_hall(game: &mut Game, assets: &Assets, f: &Frame, view: &mut BuildingView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let l = game.location?;
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let pic_h = 250.0;
    {
        let loc = &game.world.locations[l];
        draw_rectangle(x, y, w, pic_h, Color::new(0.35, 0.5, 0.65, 1.0));
        draw_rectangle(x, y + pic_h * 0.62, w, pic_h * 0.38, Color::new(0.35, 0.5, 0.3, 1.0));
        if let Some(tex) = assets.dt.as_ref().and_then(|a| a.building(loc.picture.0, loc.picture.1)) {
            let k = ((pic_h - 20.0) / tex.height()).min((w - 20.0) / tex.width()).min(2.5);
            let (tw, th) = (tex.width() * k, tex.height() * k);
            draw_texture_ex(&tex, x + (w - tw) / 2.0, y + pic_h - th - 6.0, WHITE, DrawTextureParams { dest_size: Some(vec2(tw, th)), ..Default::default() });
        } else {
            text_centered(loc.kind.label(), x + w / 2.0, y + pic_h / 2.0, 40.0, INK);
        }
        draw_rectangle_lines(x, y, w, pic_h, 2.0, MARBLE_EDGE);
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
    draw_rectangle(x, ly, w, 36.0, Color::new(0.14, 0.24, 0.2, 1.0));
    text("Quests and rumours:", x + 14.0, ly + 25.0, 20.0, ACCENT);
    let label = format!("Hear rumour ({RUMOUR_PRICE} gold)");
    let mut next = None;
    if button(x + w - 250.0, ly + 3.0, 240.0, 30.0, &label, rumour.is_some() && game.gold >= RUMOUR_PRICE) {
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
    draw_rectangle(x, list_y, w, list_h, PARCHMENT);
    if entries.is_empty() {
        let none = if game.script().is_some() { "Nothing is on offer here." } else { "No quests in the demo." };
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
            HallEntry::Rumour(id) => (id, "rumour", Color::new(0.55, 0.1, 0.1, 1.0)),
            HallEntry::Quest(id) => (id, "in your journal", Color::new(0.1, 0.3, 0.55, 1.0)),
            HallEntry::Done(id) => (id, "done", PARCHMENT_INK),
        };
        if view.pick == Some(k) {
            draw_rectangle(x + 4.0, ry, w - 8.0, row_h - 2.0, Color::new(0.72, 0.6, 0.4, 1.0));
        }
        let title: String = story::event_title(game, id).chars().take(60).collect();
        text(&title, x + 16.0, ry + 18.0, 19.0, color);
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
    let (rw, rh) = (((w - 10.0) / 6.0 - 8.0).min(118.0), 112.0);
    draw_rectangle(x, y, w, rh + 76.0, Color::new(0.3, 0.2, 0.12, 1.0));
    let mut hover_lines = Vec::new();
    if recruits.is_empty() {
        text_centered("No recruits here.", x + w / 2.0, y + 70.0, 22.0, DIM);
    }
    for (k, r) in recruits.iter().take(6).enumerate() {
        let (cx, cy) = (x + 8.0 + k as f32 * (rw + 8.0), y + 8.0);
        draw_rectangle(cx, cy, rw, rh, Color::new(0.4, 0.55, 0.7, 1.0));
        assets.draw_unit(r.unit, Team::Player, cx + rw / 2.0, cy + rh / 2.0, rh - 6.0);
        draw_rectangle_lines(cx, cy, rw, rh, 2.0, Color::new(0.6, 0.45, 0.3, 1.0));
        if mouse_in(cx, cy, rw, rh) {
            let def = c.unit(r.unit);
            hover_lines.push((def.name.clone(), ACCENT));
            hover_lines.extend(stat_lines(&c, r.unit).into_iter().map(|s| (s, INK)));
            hover_lines.push((format!("Daily wage {}", c.wage_for(r.unit, razdor::rules::content::WageKind::of(def))), Color::new(0.95, 0.6, 0.25, 1.0)));
        }
        let price = game.hire_price(r.unit);
        let stock_left = r.stock != Some(0);
        let can = hires && stock_left && game.can_afford(price) && game.squad.len() < game.max_squad();
        if button(cx + 6.0, cy + rh + 4.0, rw - 12.0, 24.0, "Hire", can) {
            let name = c.unit(r.unit).name.clone();
            *message = Some(match game.hire(r.unit) {
                Ok(()) => format!("{name} joins your army."),
                Err(HireError::NotEnoughGold) => "You cannot afford it.".into(),
                Err(HireError::SquadFull) => "Your army is full.".into(),
                Err(HireError::NotOffered) => "Not offered here.".into(),
            });
        }
        text_centered(&format!("Price = {}", price.amount), cx + rw / 2.0, cy + rh + 46.0, 16.0, if price.currency == Currency::Mana { MANA } else { ACCENT });
        let left = match r.stock {
            Some(n) => format!("{n} of {} left", r.max),
            None => "always".to_string(),
        };
        text_centered(&left, cx + rw / 2.0, cy + rh + 64.0, 14.0, DIM);
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
            if button(bx, by, bw, 22.0, "Heal", game.can_afford(p)) {
                action = Some((i, false));
            }
            text_centered(&format!("Price = {}", p.amount), ux + cw / 2.0, by + 38.0, 14.0, ACCENT);
        } else if let Some(p) = game.resurrect_price(i).filter(|_| raises) {
            if button(bx, by, bw, 22.0, "Raise", game.can_afford(p)) {
                action = Some((i, true));
            }
            text_centered(&format!("Price = {}", p.amount), ux + cw / 2.0, by + 38.0, 14.0, ACCENT);
        } else if !u.alive() {
            let left = game.resurrection_minutes_left(i).map_or("to be buried".into(), |m| duration_label(m as f64));
            text_centered(&left, ux + cw / 2.0, by + 16.0, 14.0, DIM);
        }
    }
    let mut next = None;
    if let Some((i, raise)) = action {
        let name = game.squad[i].name(&c).to_string();
        let r = if raise { game.resurrect(i) } else { game.heal(i) };
        match r {
            Ok(events) => {
                *message = Some(if raise { format!("{name} rises again.") } else { format!("{name} is healed.") });
                // What happened meanwhile: a noon report, the scenario's events.
                next = world_view::handle_events(game, events, message, dialogs);
            }
            Err(e) => *message = Some(service_error(e)),
        }
    }
    let note = match (heals, raises) {
        (true, true) => format!("Healing and raising the dead take {} min each.", c.options.healing_time),
        (true, false) => format!("Healing takes {} min. The dead are raised in towns and churches.", c.options.healing_time),
        _ => "No healing here.".into(),
    };
    text(&note, x, f.y + f.h - 6.0, 15.0, DIM);
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
    text("Garrison: click a unit to take it back.", x, y + 20.0, 19.0, ACCENT);
    let gy = y + 30.0;
    empty_cells(game, x, gy, cw, ch, gap);
    let mut take = None;
    let mut hover = Vec::new();
    let now = game.clock.total_minutes() as u64;
    for (j, s) in game.garrison_here().iter().enumerate() {
        let (ux, uy) = grid_cell(game, s.unit.slot, x, gy, cw, ch, gap);
        unit_card(game, assets, &s.unit, ux, uy, cw, ch);
        if mouse_in(ux, uy, cw, ch) {
            let paid = if now.saturating_sub(s.since) < MINUTES_PER_DAY { "paid until the next noon" } else { "no wage while on guard" };
            hover = vec![(s.unit.name(&c).to_string(), ACCENT), (format!("{}/{} HP, {paid}", s.unit.hp, s.unit.max_hp(&c)), INK)];
            if clicked() {
                take = Some(j);
            }
        }
    }
    let ay = gy + 2.0 * (ch + gap) + 40.0;
    text("Your army: click a unit to leave it here.", x, ay - 10.0, 19.0, ACCENT);
    empty_cells(game, x, ay, cw, ch, gap);
    let mut leave = None;
    for i in 0..game.squad.len() {
        let u = &game.squad[i];
        let (ux, uy) = grid_cell(game, u.slot, x, ay, cw, ch, gap);
        unit_card(game, assets, u, ux, uy, cw, ch);
        if mouse_in(ux, uy, cw, ch) {
            hover = vec![(u.name(&c).to_string(), ACCENT), (format!("{}/{} HP, wage {}", u.hp, u.max_hp(&c), game.wage(i)), INK)];
            if clicked() {
                leave = Some(i);
            }
        }
    }
    if let Some(j) = take {
        *message = Some(match game.take_from_garrison(j) {
            Ok(()) => "Back in your army.".into(),
            Err(e) => service_error(e),
        });
    }
    if let Some(i) = leave {
        let name = game.squad[i].name(&c).to_string();
        *message = Some(match game.leave_in_garrison(i) {
            Ok(()) => format!("{name} stays on guard."),
            Err(e) => service_error(e),
        });
    }
    let note = format!("Units on guard are paid for their first day only and heal {}% a day.", c.options.garrison_auto_heal);
    text(&note, x, f.y + f.h - 6.0, 15.0, DIM);
    tooltip(&hover);
}

/// A list with a selection and a scroll bar. Rows are (icon item, name, price). Returns the
/// clicked row.
#[allow(clippy::too_many_arguments)]
fn price_list(assets: Option<&Assets>, rows: &[(Option<ItemId>, String, String)], pick: Option<usize>, scroll: &mut usize, x: f32, y: f32, w: f32, visible: usize) -> Option<usize> {
    let row_h = 30.0;
    draw_rectangle(x, y, w, 26.0 + visible as f32 * row_h, Color::new(0.12, 0.2, 0.16, 1.0));
    text("Name", x + 50.0, y + 19.0, 17.0, ACCENT);
    text("Price", x + w - 80.0, y + 19.0, 17.0, ACCENT);
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

fn item_description(game: &Game, assets: &Assets, item: ItemId, x: f32, y: f32, w: f32, h: f32) {
    let c = &game.content;
    let d = c.item(item);
    draw_rectangle(x, y, w, h, BOX);
    draw_rectangle_lines(x, y, w, h, 2.0, Color::new(0.6, 0.42, 0.25, 1.0));
    assets.draw_item(item, x + w / 2.0 - 28.0, y + 10.0, 56.0);
    text_centered(&d.name, x + w / 2.0, y + 90.0, 20.0, BOX_INK);
    let limit = match d.kind {
        ArtefactType::BlowWeapon => Some("warriors only"),
        ArtefactType::ShotWeapon => Some("shooters only"),
        ArtefactType::Staff => Some("mages only"),
        ArtefactType::Item => Some("trade goods: cannot be worn"),
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
    text_centered(if view.selling { "Your pack: sell for a quarter of the price" } else { "Goods for sale" }, lx + lw / 2.0, y + 18.0, 18.0, ACCENT);
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
            draw_rectangle(x, y, dw, dh, BOX);
            text_centered("Pick an item from the list.", x + dw / 2.0, y + dh / 2.0, 18.0, BOX_INK);
        }
    }
    let by = y + dh + 10.0;
    let mut next = None;
    if button(x, by, 150.0, 40.0, "Inventory", true) {
        next = Some(Screen::Squad { selected: 0, scroll: 0, back: Some(view.clone()) });
    }
    resource_icon(Resource::Gold, x + 180.0, by + 20.0, 34.0);
    text(&format!("Gold {}", game.gold), x + 202.0, by + 27.0, 20.0, ACCENT);
    let label = if view.selling { "Sell" } else { "Buy" };
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
                Ok(g) => format!("Sold {name} for {g} gold."),
                Err(e) => trade_error(e),
            }
        } else {
            match game.buy(k) {
                Ok(item) => format!("Bought {}. It is in your pack.", c.item(item).name),
                Err(e) => trade_error(e),
            }
        });
        view.pick = None;
    }
    let toggle = if view.selling { "Back to the goods" } else { "Sell shop" };
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

fn spell_lines(game: &Game, s: &SpellDef) -> Vec<String> {
    use razdor::rules::items::stat_label;
    let mut effects = Vec::new();
    if let Some(h) = s.delta_fixed_hits {
        effects.push(if h >= 0 { format!("heals {h} hits") } else { format!("{} hits", h) });
    }
    for (&st, &v) in &s.add {
        effects.push(format!("{} {v:+}", stat_label(st)));
    }
    for (&st, &v) in &s.percent {
        effects.push(format!("{} {v:+}%", stat_label(st)));
    }
    let school = s.school.map_or(String::new(), |m| format!("{m:?} magic. "));
    let duration = match s.time_work {
        None => "instant".to_string(),
        Some(h) if h >= 9999 => "permanent".to_string(),
        Some(h) => format!("lasts {h} h"),
    };
    let _ = game;
    vec![
        effects.join(", "),
        format!("{school}Mana {}, reading {} h", s.cost_mana, s.time_cast.unwrap_or(0)),
        duration,
    ]
}

/// Sanctuary: spells for sale; a spell bought goes into the hero's book.
fn sanctuary(game: &mut Game, f: &Frame, view: &mut BuildingView, message: &mut Option<String>) {
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let spells: Vec<SpellDef> = game.spells_here().into_iter().cloned().collect();
    let dw = w * 0.45;
    let (lx, lw) = (x + dw + 10.0, w - dw - 10.0);
    let rows: Vec<_> = spells.iter().map(|s| (None, s.name.clone(), s.cost_gold.to_string())).collect();
    text_centered("Spells", lx + lw / 2.0, y + 18.0, 18.0, ACCENT);
    if let Some(k) = price_list(None, &rows, view.pick, &mut view.scroll, lx, y + 26.0, lw, 7) {
        view.pick = Some(k);
    }
    let dh = 150.0;
    draw_rectangle(x, y, dw, dh, BOX);
    let chosen = view.pick.and_then(|k| spells.get(k));
    match chosen {
        Some(s) => {
            text_centered(&s.name, x + dw / 2.0, y + 30.0, 21.0, BOX_INK);
            for (i, line) in spell_lines(game, s).iter().enumerate() {
                text_centered(line, x + dw / 2.0, y + 60.0 + i as f32 * 22.0, 16.0, MANA);
            }
        }
        None => text_centered("Pick a spell from the list.", x + dw / 2.0, y + dh / 2.0, 18.0, BOX_INK),
    }
    let by = y + dh + 14.0;
    if let Some(s) = chosen {
        if game.knows_spell(s.id) {
            text_centered("This spell is already in your book!", x + dw / 2.0, by + 20.0, 18.0, MANA);
        } else if button(x + dw - 130.0, by + 50.0, 130.0, 40.0, "Buy", game.gold >= s.cost_gold) {
            *message = Some(match game.learn_spell(s.id) {
                Ok(()) => format!("{} is written into your book.", s.name),
                Err(e) => service_error(e),
            });
        }
    }
    resource_icon(Resource::Gold, x + 26.0, by + 70.0, 34.0);
    text(&format!("Gold {}", game.gold), x + 50.0, by + 77.0, 20.0, ACCENT);
    text(&format!("Book {}/{SPELL_BOOK_SIZE}. Casting comes later.", game.spells.len()), x, by + 118.0, 16.0, DIM);
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
    text("The headman keeps the tribute for whoever protects the village.", x + 16.0, y + 28.0, 19.0, INK);
    resource_icon(Resource::Gold, x + 40.0, y + 76.0, 40.0);
    text(&format!("Gold {} (up to {})", v.tribute_gold, v.gold_max.max(v.gold_income)), x + 70.0, y + 84.0, 20.0, ACCENT);
    resource_icon(Resource::Mana, x + 340.0, y + 76.0, 40.0);
    text(&format!("Mana {} (up to {})", v.tribute_mana, v.mana_max.max(v.mana_income)), x + 370.0, y + 84.0, 20.0, MANA);
    let ready = game.tribute_available().is_some();
    let mut by = y + 140.0;
    if button(x, by, 420.0, 42.0, "Collect the tribute", ready) {
        let (gold, mana) = (v.tribute_gold, v.tribute_mana);
        *message = game.collect_tribute().map(|t| match t {
            razdor::rules::game::Tribute::Gold(_) => format!("The village pays {gold} gold and {mana} mana."),
            razdor::rules::game::Tribute::Item(item) => format!("The village pays with a {}.", game.content.item(item).name),
        });
    }
    by += 52.0;
    if button(x, by, 420.0, 42.0, "Instead: the priest heals the army", ready) {
        game.priest_heal();
        *message = Some("The priest tends to your wounded.".into());
    }
    by += 52.0;
    let unpaid = game.squad.iter().filter(|u| u.unpaid).count();
    if button(x, by, 420.0, 42.0, &format!("Instead: the innkeeper pays {unpaid} unpaid"), ready && unpaid > 0) {
        *message = game.innkeeper_pay().map(|n| format!("The innkeeper pays off {n} of your men."));
    }
    by += 56.0;
    let notes = [
        "The tribute grows every midnight up to the village's maximum.",
        "Not yet offered: a long blessing, furs to sell, the magic ritual (coming with spells).",
    ];
    for (i, n) in notes.iter().enumerate() {
        text(n, x, by + i as f32 * 20.0, 16.0, DIM);
    }
    let dy = by + 50.0;
    description_box(&v.description, x, dy, w, f.y + f.h - dy - 10.0);
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
    draw_rectangle(f.x, f.y, f.w, f.h, MARBLE);
    draw_rectangle_lines(f.x, f.y, f.w, f.h, 3.0, MARBLE_EDGE);
    draw_rectangle(f.x, f.y, f.w, 28.0, Color::new(0.06, 0.13, 0.10, 1.0));
    text_centered(&title(game), f.x + f.w / 2.0, f.y + 21.0, 20.0, INK);
    let close = button(f.x + f.w - 30.0, f.y + 3.0, 24.0, 22.0, "x", true);

    // Tabs.
    let (tx, tw) = (f.x + 10.0, 240.0);
    draw_rectangle(tx, f.y + 36.0, tw, f.h - 46.0, Color::new(0.8, 0.69, 0.48, 1.0));
    let th = 76.0;
    for (k, &t) in tabs.iter().enumerate() {
        if tab_button(tab_label(t), tx + 10.0, f.y + 48.0 + k as f32 * (th + 12.0), tw - 20.0, th, view.tab == t) && view.tab != t {
            view.switch(t);
            *message = None;
        }
    }
    let exit = tab_button("Exit", tx + 10.0, f.y + f.h - th - 20.0, tw - 20.0, th, false);

    let mut next = early;
    match view.tab {
        Tab::MainHall => next = next.or(main_hall(game, assets, &f, view, message, dialogs)),
        Tab::Barracks => next = next.or(barracks(game, assets, &f, message, dialogs)),
        Tab::Garrison => garrison(game, assets, &f, message),
        Tab::Market => next = market(game, assets, &f, view, message),
        Tab::Sanctuary => sanctuary(game, &f, view, message),
        Tab::Tribute => tribute(game, &f, message),
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
