//! The hero and army screen: gear, backpack, promotion.
use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::content::{ArtefactType, Content, ItemId, Stat};
use razdor::rules::game::{Game, PACK_SIZE};
use razdor::rules::items::{describe, EquipError, SLOTS};
use razdor::rules::units::Unit;

use super::assets::Assets;
use super::building_view::{service_error, BuildingView};
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

/// Backpack grid: 5 columns as in the original, scrolling.
const PACK_COLS: usize = 5;
const PACK_ROWS: usize = 6;

/// The hero and army screen: each unit's 4 item slots, the scrolling backpack, promotion
/// through the upgrade tree, potions, and sending a unit away (or burying a corpse).
/// `back` is the building window to return to, if it was opened from one.
pub fn squad(
    game: &mut Game,
    assets: &Assets,
    selected: &mut usize,
    scroll: &mut usize,
    back: &Option<BuildingView>,
    message: &mut Option<String>,
) -> Option<Screen> {
    clear_background(Color::from_rgba(34, 30, 26, 255));
    top_bar(game);
    *selected = (*selected).min(game.squad.len() - 1);
    let c = game.content.clone();
    text("Hero and army", 30.0, 88.0, 34.0, INK);
    text("Pick a unit, then click a pack item to equip or drink it, or a worn item to take it off.", 30.0, 114.0, 19.0, DIM);
    let mut hover = None;

    // Unit list.
    let row_h = (46.0f32).min((screen_height() - 260.0) / game.squad.len().max(1) as f32);
    for (i, u) in game.squad.iter().enumerate() {
        let (x, y, w, h) = (30.0, 130.0 + i as f32 * row_h, 290.0, row_h - 4.0);
        let over = mouse_in(x, y, w, h);
        draw_rectangle(x, y, w, h, if i == *selected { Color::new(0.3, 0.25, 0.16, 1.0) } else { PANEL });
        if over {
            draw_rectangle_lines(x, y, w, h, 1.0, ACCENT);
        }
        assets.draw_unit(u.def, Team::Player, x + 22.0, y + h / 2.0, h - 8.0);
        let name = if i == 0 { format!("{} (hero)", u.name(&c)) } else { u.name(&c).to_string() };
        text(&name, x + 48.0, y + 18.0, 19.0, if u.alive() { INK } else { RED });
        let state = if u.alive() {
            format!("L{}  {}/{} HP  {}/{SLOTS} items", u.level, u.hp, u.max_hp(&c), u.items.iter().flatten().count())
        } else {
            "dead".to_string()
        };
        text(&state, x + 48.0, y + 35.0, 15.0, DIM);
        if over && clicked() {
            *selected = i;
        }
    }

    // Selected unit: portrait with two slots on each side, then the stats.
    let x = 350.0;
    let sel = *selected;
    let u = game.squad[sel].clone();
    draw_rectangle(x, 130.0, 380.0, 470.0, PANEL);
    let mut unequip = None;
    let slot_pos = [(x + 12.0, 142.0), (x + 12.0, 142.0 + ICON + GAP), (x + 380.0 - ICON - 12.0, 142.0), (x + 380.0 - ICON - 12.0, 142.0 + ICON + GAP)];
    for (slot, &(sx, sy)) in slot_pos.iter().enumerate() {
        if slot_box(assets, u.items[slot], sx, sy, &mut hover) {
            unequip = Some(slot);
        }
    }
    assets.draw_unit(u.def, Team::Player, x + 190.0, 208.0, 128.0);
    text_centered(u.name(&c), x + 190.0, 300.0, 26.0, INK);
    for (j, line) in unit_stat_lines(&c, &u, game.wage(sel)).iter().enumerate() {
        text(line, x + 16.0, 330.0 + j as f32 * 21.0, 17.0, DIM);
    }
    if let Some(slot) = unequip {
        *message = game.unequip(sel, slot).err().map(equip_error);
    }
    if !u.potions.is_empty() {
        let names: Vec<&str> = u.potions.iter().map(|&p| c.item(p).name.as_str()).collect();
        text(&format!("Until the next battle ends: {}", names.join(", ")), x + 16.0, 520.0, 16.0, ACCENT);
    }
    // Promotions (upgrade tree).
    let mut by = 610.0;
    for to in u.promotions(&c) {
        let label = format!("Promote to {}", c.unit(to).name);
        if button(x, by, 380.0, 36.0, &label, true) {
            *message = Some(match game.promote(sel, to) {
                Ok(()) => format!("{} is now a {}.", u.name(&c), c.unit(to).name),
                Err(_) => "Not possible.".into(),
            });
        }
        by += 42.0;
    }
    if sel > 0 {
        let label = if u.alive() { "Dismiss" } else { "Bury" };
        if button(x, by, 180.0, 36.0, label, true) {
            let name = u.name(&c).to_string();
            *message = Some(match game.dismiss(sel) {
                Ok(()) if u.alive() => format!("{name} leaves your army."),
                Ok(()) => format!("{name} is laid to rest."),
                Err(e) => service_error(e),
            });
            *selected = sel - 1;
        }
    }

    // Backpack.
    let px = 760.0;
    let rows = PACK_SIZE.div_ceil(PACK_COLS);
    let max_scroll = rows.saturating_sub(PACK_ROWS);
    let grid_h = PACK_ROWS as f32 * (ICON + GAP);
    let grid_w = PACK_COLS as f32 * (ICON + GAP);
    text(&format!("Backpack {}/{PACK_SIZE}", game.pack.len()), px, 150.0, 24.0, INK);
    if mouse_in(px, 165.0, grid_w + 20.0, grid_h) {
        let w = wheel();
        if w < 0.0 {
            *scroll = (*scroll + 1).min(max_scroll);
        } else if w > 0.0 {
            *scroll = scroll.saturating_sub(1);
        }
    }
    *scroll = (*scroll).min(max_scroll);
    let mut use_item = None;
    for r in 0..PACK_ROWS {
        for col in 0..PACK_COLS {
            let i = (*scroll + r) * PACK_COLS + col;
            let (cx, cy) = (px + col as f32 * (ICON + GAP), 165.0 + r as f32 * (ICON + GAP));
            if slot_box(assets, game.pack.get(i).copied(), cx, cy, &mut hover) {
                use_item = Some(i);
            }
        }
    }
    let bx = px + grid_w + 4.0;
    draw_rectangle(bx, 165.0, 12.0, grid_h - GAP, Color::new(0.1, 0.1, 0.1, 1.0));
    let th = (grid_h - GAP) * PACK_ROWS as f32 / rows as f32;
    draw_rectangle(bx + 1.0, 165.0 + (grid_h - GAP - th) * *scroll as f32 / max_scroll.max(1) as f32, 10.0, th, DIM);
    if button(px, 170.0 + grid_h, 80.0, 30.0, "Up", *scroll > 0) {
        *scroll -= 1;
    }
    if button(px + 90.0, 170.0 + grid_h, 80.0, 30.0, "Down", *scroll < max_scroll) {
        *scroll += 1;
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
    let label = if back.is_some() { "Back" } else { "Back to map" };
    if button(screen_width() - 260.0, screen_height() - 130.0, 240.0, 44.0, label, true) || key(KeyCode::Escape) {
        *message = None;
        return Some(match back {
            Some(v) => Screen::Building(v.clone()),
            None => Screen::WorldMap,
        });
    }
    None
}
