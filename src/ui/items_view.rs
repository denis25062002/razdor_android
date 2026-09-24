//! Squad gear and market screens.
use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::game::{Game, TradeError, PACK_SIZE};
use razdor::rules::items::{EquipError, ItemId, SLOTS};
use razdor::rules::units::{AttackKind, Unit};

use super::assets::Assets;
use super::screens::{message_line, top_bar};
use super::widgets::*;
use super::Screen;

const ICON: f32 = 64.0;
const GAP: f32 = 8.0;

fn item_line(item: ItemId) -> String {
    let d = item.def();
    format!("{}: {} ({}g)", d.name, d.describe(), d.price)
}

fn unit_stat_lines(u: &Unit) -> Vec<String> {
    let s = u.stats();
    let attack = match s.attack {
        AttackKind::Heal { amount } => format!("heals {amount}"),
        a => format!("{}, dmg {}-{}", a.role(), s.dmg_min, s.dmg_max),
    };
    let mut lines = vec![
        format!("HP {}/{}   Armor {}", u.hp, s.max_hp, s.armor),
        attack,
        format!("Initiative {}   Actions {}", s.initiative, s.actions),
    ];
    let p = u.passives();
    let mut extra = Vec::new();
    if p.regen > 0 {
        extra.push(format!("regen {}", p.regen));
    }
    if p.no_flank {
        extra.push("no flank damage".to_string());
    }
    if p.magic_strike {
        extra.push("ignores armor".to_string());
    }
    if !extra.is_empty() {
        lines.push(extra.join(", "));
    }
    lines
}

/// An item slot box; returns true when clicked. Sets `hover` to the item under the mouse.
fn slot_box(assets: &Assets, item: Option<ItemId>, x: f32, y: f32, hover: &mut Option<ItemId>) -> bool {
    let over = mouse_in(x, y, ICON, ICON);
    draw_rectangle(x, y, ICON, ICON, Color::new(0.2, 0.19, 0.17, 1.0));
    draw_rectangle_lines(x, y, ICON, ICON, if over { 2.0 } else { 1.0 }, if over { ACCENT } else { DIM });
    if let Some(i) = item {
        assets.draw_item(i, x, y, ICON);
        if over {
            *hover = Some(i);
        }
    }
    over && clicked() && item.is_some()
}

fn equip_error(e: EquipError) -> String {
    match e {
        EquipError::NoFreeSlot => "No free slot.".into(),
        EquipError::SameType => "Already wears an item of that type.".into(),
        EquipError::PackFull => "The pack is full.".into(),
        EquipError::NoSuchItem => "Nothing there.".into(),
    }
}

/// Moves items between the pack and squad members. `from_town` picks where "Back" returns to.
pub fn squad(
    game: &mut Game,
    assets: &Assets,
    selected: &mut usize,
    from_town: bool,
    message: &mut Option<String>,
) -> Option<Screen> {
    clear_background(Color::from_rgba(34, 30, 26, 255));
    top_bar(game);
    *selected = (*selected).min(game.squad.len() - 1);
    text("Squad & gear", 30.0, 88.0, 34.0, INK);
    text("Pick a unit, then click a pack item to equip it, or a worn item to take it off.", 30.0, 114.0, 19.0, DIM);
    let mut hover = None;

    // Unit list.
    for (i, u) in game.squad.iter().enumerate() {
        let (x, y, w, h) = (30.0, 130.0 + i as f32 * 46.0, 290.0, 42.0);
        let over = mouse_in(x, y, w, h);
        draw_rectangle(x, y, w, h, if i == *selected { Color::new(0.3, 0.25, 0.16, 1.0) } else { PANEL });
        if over {
            draw_rectangle_lines(x, y, w, h, 1.0, ACCENT);
        }
        assets.draw_unit(u.kind, Team::Player, x + 22.0, y + 21.0, 34.0);
        let name = if i == 0 { format!("{} (hero)", u.kind.name()) } else { u.kind.name().to_string() };
        text(&name, x + 48.0, y + 20.0, 20.0, INK);
        let worn = u.items.iter().flatten().count();
        text(&format!("{}/{} HP  {worn}/{SLOTS} items", u.hp, u.stats().max_hp), x + 48.0, y + 37.0, 16.0, DIM);
        if over && clicked() {
            *selected = i;
        }
    }

    // Selected unit.
    let x = 350.0;
    let u = &game.squad[*selected];
    draw_rectangle(x, 130.0, 350.0, 330.0, PANEL);
    assets.draw_unit(u.kind, Team::Player, x + 50.0, 180.0, 70.0);
    text(u.kind.name(), x + 100.0, 175.0, 28.0, INK);
    for (j, line) in unit_stat_lines(u).iter().enumerate() {
        text(line, x + 16.0, 245.0 + j as f32 * 22.0, 19.0, DIM);
    }
    text("Worn", x + 16.0, 345.0, 20.0, INK);
    let mut unequip = None;
    for slot in 0..SLOTS {
        let sx = x + 16.0 + slot as f32 * (ICON + GAP);
        if slot_box(assets, u.items[slot], sx, 360.0, &mut hover) {
            unequip = Some(slot);
        }
    }
    if let Some(slot) = unequip {
        *message = game.unequip(*selected, slot).err().map(equip_error);
    }

    // Pack.
    let px = 730.0;
    text(&format!("Pack {}/{PACK_SIZE}", game.pack.len()), px, 150.0, 24.0, INK);
    let mut equip = None;
    for i in 0..PACK_SIZE {
        let (cx, cy) = (px + (i % 4) as f32 * (ICON + GAP), 165.0 + (i / 4) as f32 * (ICON + GAP));
        if slot_box(assets, game.pack.get(i).copied(), cx, cy, &mut hover) {
            equip = Some(i);
        }
    }
    if let Some(i) = equip {
        *message = game.equip(*selected, i).err().map(equip_error);
    }

    let info = hover.map_or_else(|| "Hover an item to see what it does.".to_string(), item_line);
    text(&info, 30.0, screen_height() - 80.0, 20.0, if hover.is_some() { INK } else { DIM });
    message_line(message);
    let label = if from_town { "Back to castle" } else { "Back to map" };
    if button(screen_width() - 260.0, screen_height() - 130.0, 240.0, 44.0, label, true) {
        *message = None;
        return Some(if from_town { Screen::Town } else { Screen::WorldMap });
    }
    None
}

fn trade_error(e: TradeError) -> String {
    match e {
        TradeError::NoMarket => "There is no market here.".into(),
        TradeError::NotEnoughGold => "Not enough gold.".into(),
        TradeError::PackFull => "The pack is full.".into(),
        TradeError::NoSuchItem => "Nothing there.".into(),
    }
}

/// Buy from the castle market, sell from the pack.
pub fn market(game: &mut Game, assets: &Assets, message: &mut Option<String>) -> Option<Screen> {
    clear_background(Color::from_rgba(40, 32, 26, 255));
    top_bar(game);
    let name = game.location.map_or("Castle", |l| game.world.locations[l].name);
    text(&format!("{name}: market"), 30.0, 88.0, 34.0, INK);
    text("New stock arrives every Monday. Items sell for half their price.", 30.0, 114.0, 19.0, DIM);

    let stock = game.market_here().unwrap_or(&[]).to_vec();
    if stock.is_empty() {
        text("Sold out until Monday.", 30.0, 160.0, 22.0, DIM);
    }
    let mut buy = None;
    for (i, &item) in stock.iter().enumerate() {
        let (x, y) = (30.0, 130.0 + i as f32 * 76.0);
        let d = item.def();
        draw_rectangle(x, y, 600.0, 70.0, PANEL);
        assets.draw_item(item, x + 4.0, y + 3.0, ICON);
        text(&d.name, x + 80.0, y + 28.0, 24.0, INK);
        text(&d.describe(), x + 80.0, y + 52.0, 18.0, DIM);
        let affordable = game.gold >= d.price && game.pack.len() < PACK_SIZE;
        if button(x + 460.0, y + 14.0, 124.0, 42.0, &format!("Buy {}g", d.price), affordable) {
            buy = Some(i);
        }
    }
    if let Some(i) = buy {
        *message = Some(match game.buy(i) {
            Ok(item) => format!("Bought {}. Equip it from Squad & gear.", item.def().name),
            Err(e) => trade_error(e),
        });
    }

    let px = 660.0;
    text(&format!("Your pack {}/{PACK_SIZE}", game.pack.len()), px, 150.0, 24.0, INK);
    if game.pack.is_empty() {
        text("Empty. Worn items must be taken off to sell.", px, 180.0, 18.0, DIM);
    }
    let mut sell = None;
    for (i, &item) in game.pack.iter().enumerate() {
        let y = 162.0 + i as f32 * 30.0;
        assets.draw_item(item, px, y, 28.0);
        text(&item.def().name, px + 38.0, y + 21.0, 19.0, INK);
        if button(px + 280.0, y + 1.0, 120.0, 28.0, &format!("Sell {}g", item.def().sell_price()), true) {
            sell = Some(i);
        }
    }
    if let Some(i) = sell {
        let name = &game.pack[i].def().name;
        let what = format!("Sold {name}");
        *message = Some(match game.sell(i) {
            Ok(g) => format!("{what} for {g} gold."),
            Err(e) => trade_error(e),
        });
    }

    message_line(message);
    if button(30.0, screen_height() - 110.0, 240.0, 44.0, "Back to castle", true) {
        *message = None;
        return Some(Screen::Town);
    }
    None
}
