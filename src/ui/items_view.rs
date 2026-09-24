//! Squad gear and market screens.
use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::content::{ArtefactType, Content, ItemId, Stat};
use razdor::rules::game::{Game, TradeError, PACK_SIZE};
use razdor::rules::items::{describe, EquipError, SLOTS};
use razdor::rules::units::Unit;

use super::assets::Assets;
use super::screens::{attack_line, message_line, top_bar};
use super::widgets::*;
use super::Screen;

const ICON: f32 = 64.0;
const GAP: f32 = 8.0;

fn item_line(content: &Content, item: ItemId) -> String {
    let d = content.item(item);
    format!("{}: {} ({}g)", d.name, describe(content, item), d.cost)
}

/// The unit's full stat list, as in the original's unit panel.
pub(super) fn unit_stat_lines(content: &Content, u: &Unit, wage: i32) -> Vec<String> {
    let s = u.stats(content);
    let mut lines = vec![
        format!("Level {}   XP {}/{}", u.level, u.xp, u.xp_to_next(content)),
        format!("Hits {}/{}   {}", u.hp, s.max_hp(), attack_line(&s)),
        format!("Defence {} melee / {} ranged", s[Stat::DefenceBlow], s[Stat::DefenceShot]),
        format!("Initiative {}   Actions {}", s[Stat::Initiative], s[Stat::Manevres]),
        format!(
            "Magic prot. life {}% / elem. {}% / death {}%",
            s[Stat::ProtectLife],
            s[Stat::ProtectElemental],
            s[Stat::ProtectDeath]
        ),
    ];
    let mut extra = Vec::new();
    if s[Stat::Regen] > 0 {
        extra.push(format!("regen {}%", s[Stat::Regen]));
    }
    if s[Stat::Vampirizm] > 0 {
        extra.push(format!("vampirism {}%", s[Stat::Vampirizm]));
    }
    extra.extend(s.bonuses.iter().map(|b| b.token().to_string()));
    if !extra.is_empty() {
        lines.push(extra.join(", "));
    }
    if wage > 0 {
        lines.push(format!("Daily wage {wage} gold"));
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
        EquipError::SecondWeapon => "Only one weapon or staff at a time.".into(),
        EquipError::WrongClass => "This unit cannot use that (warrior, shooter or mage only).".into(),
        EquipError::NotWearable => "That cannot be worn.".into(),
        EquipError::Dead => "The dead hold nothing.".into(),
        EquipError::NotAPotion => "That is not a potion.".into(),
        EquipError::PackFull => "The pack is full.".into(),
        EquipError::NoSuchItem => "Nothing there.".into(),
    }
}

/// Moves items between the pack and squad members, drinks potions and promotes units.
/// `from_town` picks where "Back" returns to.
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
    let c = game.content.clone();
    text("Squad & gear", 30.0, 88.0, 34.0, INK);
    text("Pick a unit, then click a pack item to equip or drink it, or a worn item to take it off.", 30.0, 114.0, 19.0, DIM);
    let mut hover = None;

    // Unit list.
    for (i, u) in game.squad.iter().enumerate() {
        let (x, y, w, h) = (30.0, 130.0 + i as f32 * 46.0, 290.0, 42.0);
        let over = mouse_in(x, y, w, h);
        draw_rectangle(x, y, w, h, if i == *selected { Color::new(0.3, 0.25, 0.16, 1.0) } else { PANEL });
        if over {
            draw_rectangle_lines(x, y, w, h, 1.0, ACCENT);
        }
        assets.draw_unit(u.def, Team::Player, x + 22.0, y + 21.0, 34.0);
        let name = if i == 0 { format!("{} (hero)", u.name(&c)) } else { u.name(&c).to_string() };
        text(&name, x + 48.0, y + 20.0, 20.0, INK);
        let worn = u.items.iter().flatten().count();
        text(&format!("L{}  {}/{} HP  {worn}/{SLOTS} items", u.level, u.hp, u.max_hp(&c)), x + 48.0, y + 37.0, 16.0, DIM);
        if over && clicked() {
            *selected = i;
        }
    }

    // Selected unit.
    let x = 350.0;
    let sel = *selected;
    let u = game.squad[sel].clone();
    draw_rectangle(x, 130.0, 360.0, 420.0, PANEL);
    assets.draw_unit(u.def, Team::Player, x + 50.0, 180.0, 70.0);
    text(u.name(&c), x + 100.0, 175.0, 28.0, INK);
    for (j, line) in unit_stat_lines(&c, &u, game.wage(sel)).iter().enumerate() {
        text(line, x + 16.0, 238.0 + j as f32 * 21.0, 17.0, DIM);
    }
    text("Worn", x + 16.0, 410.0, 20.0, INK);
    let mut unequip = None;
    for slot in 0..SLOTS {
        let sx = x + 16.0 + slot as f32 * (ICON + GAP);
        if slot_box(assets, u.items[slot], sx, 420.0, &mut hover) {
            unequip = Some(slot);
        }
    }
    if let Some(slot) = unequip {
        *message = game.unequip(sel, slot).err().map(equip_error);
    }
    if !u.potions.is_empty() {
        let names: Vec<&str> = u.potions.iter().map(|&p| c.item(p).name.as_str()).collect();
        text(&format!("Until the next battle ends: {}", names.join(", ")), x + 16.0, 506.0, 16.0, ACCENT);
    }
    // Promotions (upgrade tree).
    for (n, to) in u.promotions(&c).into_iter().enumerate() {
        let label = format!("Promote to {}", c.unit(to).name);
        if button(x, 560.0 + n as f32 * 44.0, 360.0, 38.0, &label, true) {
            *message = Some(match game.promote(sel, to) {
                Ok(()) => format!("{} is now a {}.", u.name(&c), c.unit(to).name),
                Err(_) => "Not possible.".into(),
            });
        }
    }

    // Pack.
    let px = 740.0;
    text(&format!("Pack {}/{PACK_SIZE}", game.pack.len()), px, 150.0, 24.0, INK);
    let mut use_item = None;
    for i in 0..PACK_SIZE {
        let (cx, cy) = (px + (i % 4) as f32 * (ICON + GAP), 165.0 + (i / 4) as f32 * (ICON + GAP));
        if slot_box(assets, game.pack.get(i).copied(), cx, cy, &mut hover) {
            use_item = Some(i);
        }
    }
    if let Some(i) = use_item {
        let item = game.pack[i];
        *message = if c.item(item).kind == ArtefactType::Potion {
            Some(match game.drink(sel, i) {
                Ok(healed) if healed > 0 => format!("{} drinks it: +{healed} hits.", u.name(&c)),
                Ok(_) => format!("{} drinks it. The effect lasts until the next battle ends.", u.name(&c)),
                Err(e) => equip_error(e),
            })
        } else {
            game.equip(sel, i).err().map(equip_error)
        };
    }

    let info = hover.map_or_else(|| "Hover an item to see what it does.".to_string(), |i| item_line(&c, i));
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
        TradeError::NotForSale => "A personal item: it cannot be sold.".into(),
    }
}

/// Buy from the castle market, sell from the pack.
pub fn market(game: &mut Game, assets: &Assets, message: &mut Option<String>) -> Option<Screen> {
    clear_background(Color::from_rgba(40, 32, 26, 255));
    top_bar(game);
    let c = game.content.clone();
    let name = game.location.map_or("Castle", |l| game.world.locations[l].name);
    text(&format!("{name}: market"), 30.0, 88.0, 34.0, INK);
    text("New stock arrives every Monday. Items sell for a quarter of their price.", 30.0, 114.0, 19.0, DIM);

    let stock = game.market_here().unwrap_or(&[]).to_vec();
    if stock.is_empty() {
        text("Sold out until Monday.", 30.0, 160.0, 22.0, DIM);
    }
    let mut buy = None;
    for (i, &item) in stock.iter().enumerate() {
        let (x, y) = (30.0, 130.0 + i as f32 * 76.0);
        let price = game.buy_price(item);
        draw_rectangle(x, y, 600.0, 70.0, PANEL);
        assets.draw_item(item, x + 4.0, y + 3.0, ICON);
        text(&c.item(item).name, x + 80.0, y + 28.0, 24.0, INK);
        text(&describe(&c, item), x + 80.0, y + 52.0, 18.0, DIM);
        let affordable = game.gold >= price && game.pack.len() < PACK_SIZE;
        if button(x + 460.0, y + 14.0, 124.0, 42.0, &format!("Buy {price}g"), affordable) {
            buy = Some(i);
        }
    }
    if let Some(i) = buy {
        *message = Some(match game.buy(i) {
            Ok(item) => format!("Bought {}. Use it from Squad & gear.", c.item(item).name),
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
        text(&c.item(item).name, px + 38.0, y + 21.0, 19.0, INK);
        if button(px + 280.0, y + 1.0, 120.0, 28.0, &format!("Sell {}g", game.sell_price(item)), true) {
            sell = Some(i);
        }
    }
    if let Some(i) = sell {
        let what = format!("Sold {}", c.item(game.pack[i]).name);
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
