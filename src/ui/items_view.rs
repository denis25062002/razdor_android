//! The hero and army screen: gear, backpack, promotion.
use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::content::{ArtefactType, Content, ItemId, Stat, UnitId};
use razdor::rules::experience::is_percent_stat;
use razdor::rules::game::{Game, PACK_SIZE};
use razdor::rules::items::EquipError;
use razdor::rules::units::Unit;

use super::assets::Assets;
use super::audio::{cue, cued, Cue};
use super::building_view::{service_error, BuildingView};
use super::screens::attack_line;
use super::chrome;
use super::unit_sheet;
use super::widgets::*;
use super::Screen;


/// What a class gains per level: "+5 hits, +2 melee, +5% life prot.". Percent stats close
/// that share of the gap to 100.
pub(super) fn level_gains(content: &Content, kind: UnitId) -> String {
    let def = content.unit(kind);
    let label = |st: Stat| match st {
        Stat::Hits => "hits",
        Stat::AttackBlow => "melee",
        Stat::DefenceBlow => "melee def.",
        Stat::AttackShot => "ranged",
        Stat::DefenceShot => "ranged def.",
        Stat::MagicPower => "magic",
        Stat::Initiative => "initiative",
        Stat::Manevres => "actions",
        Stat::ProtectLife => "life prot.",
        Stat::ProtectDeath => "death prot.",
        Stat::ProtectElemental => "elem. prot.",
        Stat::Regen => "regen",
        Stat::Vampirizm => "vampirism",
    };
    let parts: Vec<String> = Stat::ALL
        .into_iter()
        .filter_map(|st| def.level_up.get(&st).filter(|&&d| d != 0 && (st != Stat::MagicPower || def.magic.is_some())).map(|&d| (st, d)))
        .map(|(st, d)| if is_percent_stat(st) { format!("+{d}% {}", label(st)) } else { format!("{d:+} {}", label(st)) })
        .collect();
    if parts.is_empty() {
        "nothing".into()
    } else {
        parts.join(", ")
    }
}

/// The unit's full stat list, as in the original's unit panel.
pub(super) fn unit_stat_lines(content: &Content, u: &Unit, wage: i32) -> Vec<String> {
    let s = u.stats(content);
    let need = u.xp_to_next(content);
    let mut lines = vec![
        format!("{}   next level in {} XP", level_label(u.level, u.xp, need), (need - u.xp).max(0)),
        format!("Per level: {}", level_gains(content, u.def)),
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

thread_local! {
    /// The army screen shows the upgrade tree instead of the backpack.
    static SHOW_TREE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The promotion tree of squad member `sel` in `r`, as the original's: the current class at
/// the bottom, arrows up to its options (portraits; the ones open now glow and promote on a
/// click, free of charge).
fn tree_view(game: &mut Game, assets: &Assets, sel: usize, u: &Unit, r: Rect, message: &mut Option<String>) {
    let c = game.content.clone();
    let k = chrome::k();
    let tree = if sel == 0 { Vec::new() } else { u.upgrade_tree(&c) };
    if let Some(t) = chrome::win_fx("UpgradeTree", chrome::Fx::KeyBlack) {
        chrome::tex(&t, r, WHITE);
    }
    let s = (r.w * 0.28).min(r.h * 0.36);
    let cur = Rect::new(r.x + (r.w - s) / 2.0, r.y + r.h * 0.95 - s, s, s);
    let slots = [0.17, 0.5, 0.83];
    for (i, &(to, level, ok)) in tree.iter().take(3).enumerate() {
        let o = Rect::new(r.x + r.w * slots[i] - s / 2.0, r.y + r.h * 0.04, s, s);
        if chrome::win("UpgradeTree").is_none() {
            draw_line(cur.x + cur.w / 2.0, cur.y, o.x + o.w / 2.0, o.y + o.h, 3.0, if ok { chrome::GOLD } else { DIM });
        }
        draw_rectangle(o.x - 2.0, o.y - 2.0, o.w + 4.0, o.h + 4.0, Color::new(0.0, 0.0, 0.0, 0.6));
        assets.draw_portrait(to, Team::Player, o);
        if !ok {
            draw_rectangle(o.x, o.y, o.w, o.h, Color::new(0.0, 0.0, 0.0, 0.45));
        }
        draw_rectangle_lines(o.x, o.y, o.w, o.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let label = format!("Lv {level}");
        chrome::shadow_centered(&label, o.x + o.w / 2.0, o.y + o.h - 4.0 * k, (12.0 * k).round(), if ok { chrome::GOLD } else { chrome::CREAM });
        let over = mouse_in(o.x, o.y, o.w, o.h);
        if ok {
            chrome::glow_frame(o, Color::new(0.35, 1.0, 0.35, if over { 1.0 } else { 0.6 }), over);
        }
        if over {
            tooltip(&[(c.unit(to).name.clone(), chrome::GOLD), (level_gains(&c, to), chrome::CREAM)]);
            if ok && clicked() {
                *message = Some(match game.promote(sel, to) {
                    Ok(()) => cued(Cue::Upgrade, format!("{} is now a {} (level 1, XP 0).", u.name(&c), c.unit(to).name)),
                    Err(_) => "Not possible.".into(),
                });
            }
        }
    }
    draw_rectangle(cur.x - 2.0, cur.y - 2.0, cur.w + 4.0, cur.h + 4.0, Color::new(0.0, 0.0, 0.0, 0.6));
    assets.draw_portrait(u.def, Team::Player, cur);
    draw_rectangle_lines(cur.x, cur.y, cur.w, cur.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
    let note = if sel == 0 {
        "The hero rises by levels only."
    } else if tree.is_empty() {
        "The final class: it improves by levels only."
    } else if tree.iter().any(|&(_, _, ok)| ok) {
        "Click a lit class to promote (free; back to level 1)."
    } else {
        "Not enough experience to promote yet."
    };
    for (i, line) in wrap(note, r.w - 12.0, (12.0 * k).round()).iter().enumerate() {
        chrome::shadow_centered(line, r.x + r.w / 2.0, r.y + r.h * 0.5 + i as f32 * 14.0 * k, (12.0 * k).round(), chrome::CREAM);
    }
}

/// The backpack: 5 columns of the original's inventory squares, scrolling. Returns the
/// pack index clicked.
fn pack_view(game: &Game, assets: &Assets, r: Rect, scroll: &mut usize, hover: &mut Option<ItemId>) -> Option<usize> {
    let k = chrome::k();
    let cell = ((r.w - 18.0 * k) / PACK_COLS as f32).floor();
    let rows_shown = ((r.h / cell).floor() as usize).max(1);
    let rows = PACK_SIZE.div_ceil(PACK_COLS);
    let max_scroll = rows.saturating_sub(rows_shown);
    if mouse_in(r.x, r.y, r.w, r.h) {
        let w = wheel();
        if w < 0.0 {
            *scroll = (*scroll + 1).min(max_scroll);
        } else if w > 0.0 {
            *scroll = scroll.saturating_sub(1);
        }
    }
    *scroll = (*scroll).min(max_scroll);
    let inv = chrome::win("Inventory");
    let mut hit = None;
    for row in 0..rows_shown {
        for col in 0..PACK_COLS {
            let i = (*scroll + row) * PACK_COLS + col;
            let cr = Rect::new(r.x + col as f32 * cell, r.y + row as f32 * cell, cell, cell);
            match &inv {
                Some(t) => {
                    let s = t.width() / 5.0;
                    chrome::tex_src(t, Rect::new(col as f32 * s, ((row + *scroll) % 5) as f32 * s, s, s), cr, WHITE);
                }
                None => {
                    chrome::surface(cr, chrome::Skin::Paper);
                    draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, 1.0, Color::new(0.5, 0.35, 0.2, 0.8));
                }
            }
            let Some(&item) = game.pack.get(i) else { continue };
            assets.draw_item(item, cr.x + 2.0, cr.y + 2.0, cell - 4.0);
            if mouse_in(cr.x, cr.y, cr.w, cr.h) {
                *hover = Some(item);
                draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, 2.0, chrome::GOLD);
                if clicked() {
                    hit = Some(i);
                }
            }
        }
    }
    // The scroll bar.
    let bx = r.x + PACK_COLS as f32 * cell + 4.0 * k;
    let bh = rows_shown as f32 * cell;
    draw_rectangle(bx, r.y, 12.0 * k, bh, Color::new(0.05, 0.05, 0.05, 0.8));
    let th = bh * rows_shown as f32 / rows.max(1) as f32;
    let ty = r.y + (bh - th) * *scroll as f32 / max_scroll.max(1) as f32;
    draw_rectangle(bx + 1.0, ty, 12.0 * k - 2.0, th, chrome::SILVER);
    hit
}

/// The hero and army screen, as the original's (refs 11 and 12): the selected unit's panel
/// with its four item slots on the left; the backpack (or the upgrade tree) and the item
/// description at the top; the army's 2×6 cards below. Click a card to select it, a pack
/// item to wear or drink it, a worn item to take it off. `back` is the building window to
/// return to, if it was opened from one.
pub fn squad(
    game: &mut Game,
    assets: &Assets,
    selected: &mut usize,
    scroll: &mut usize,
    back: &Option<BuildingView>,
    message: &mut Option<String>,
) -> Option<Screen> {
    super::world_view::backdrop_lit(game, assets, Some(super::game_bar::BarButton::Squad));
    *selected = (*selected).min(game.squad.len() - 1);
    let c = game.content.clone();
    let k = chrome::k();
    let (sw, sh) = (screen_width(), screen_height());
    let (ww, wh) = ((836.0 * k).round(), (600.0 * k).round());
    let win = Rect::new(((sw - ww) / 2.0).round(), ((sh - chrome::bar_height() - wh) / 2.0).max(2.0).round(), ww, wh);
    let (_, close) = chrome::window(win, "The hero's characteristics and army", chrome::Skin::Marble, true);
    let at = |x: f32, y: f32, w: f32, h: f32| Rect::new(win.x + x * k, win.y + y * k, w * k, h * k);
    let mut hover = None;
    let sel = *selected;
    let u = game.squad[sel].clone();

    // The unit's panel; a click on a worn item takes it off.
    let stats = u.stats(&c);
    let hero = (sel == 0).then(|| razdor::rules::content::HeroClass::ALL.into_iter().find(|h| h.unit() == u.def)).flatten();
    let mut status = Vec::new();
    if !u.potions.is_empty() {
        let names: Vec<&str> = u.potions.iter().map(|&p| c.item(p).name.as_str()).collect();
        status.push((format!("Until the next battle: {}", names.join(", ")), chrome::BLUE_TEXT));
    }
    if !u.alive() {
        status.push(("Dead".to_string(), chrome::RED_TEXT));
    } else if u.unpaid {
        status.push(("Unpaid: refuses to fight".to_string(), chrome::RED_TEXT));
    }
    let sheet = unit_sheet::Sheet {
        kind: u.def,
        name: u.name(&c),
        level: u.level,
        xp: u.xp,
        need: u.xp_to_next(&c),
        hp: u.hp,
        now: &stats,
        start: &stats,
        power: stats[Stat::MagicPower],
        wage: game.wage(sel),
        items: u.items,
        back_row: u.slot.row == razdor::rules::formation::Row::Back,
        in_building: false,
        hero,
        status,
    };
    if let Some(slot) = unit_sheet::draw(assets, &c, at(2.0, 27.0, 244.0, 570.0), &sheet, true, &mut hover) {
        if u.items[slot].is_some() {
            *message = game.unequip(sel, slot).err().map(equip_error);
        }
    }
    draw_line(win.x + 247.0 * k, win.y + 27.0 * k, win.x + 247.0 * k, win.y + wh - 2.0, 1.5 * k, chrome::SILVER);

    // Top middle: the backpack or the upgrade tree.
    let show_tree = SHOW_TREE.with(|t| t.get());
    let head = (13.0 * k).round();
    for (i, (label, tree)) in [(format!("Backpack {}/{PACK_SIZE}", game.pack.len()), false), ("Upgrade tree".to_string(), true)].iter().enumerate() {
        let r = at(258.0 + i as f32 * 150.0, 30.0, 146.0, 20.0);
        let on = show_tree == *tree;
        let over = mouse_in(r.x, r.y, r.w, r.h);
        if on {
            draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.35));
            draw_line(r.x, r.y + r.h, r.x + r.w, r.y + r.h, 1.5, chrome::GOLD);
        }
        chrome::shadow_centered(label, r.x + r.w / 2.0, r.y + r.h * 0.5 + head * 0.36, head, if on || over { chrome::GOLD } else { chrome::CREAM });
        if over && clicked() {
            SHOW_TREE.with(|t| t.set(*tree));
        }
    }
    let content = at(258.0, 54.0, 300.0, 242.0);
    if show_tree {
        tree_view(game, assets, sel, &u, content, message);
    } else if let Some(i) = pack_view(game, assets, content, scroll, &mut hover) {
        let item = game.pack[i];
        let kind = c.item(item).kind;
        *message = if kind == ArtefactType::Potion {
            Some(match game.drink(sel, i) {
                Ok(healed) if healed > 0 => cued(Cue::Item(kind), format!("{} drinks it: +{healed} hits.", u.name(&c))),
                Ok(_) => cued(Cue::Item(kind), format!("{} drinks it. The effect lasts until the next battle ends.", u.name(&c))),
                Err(e) => equip_error(e),
            })
        } else {
            let done = game.equip(sel, i);
            if done.is_ok() {
                cue(Cue::Item(kind));
            }
            done.err().map(equip_error)
        };
    }

    // Top right: the item under the mouse, and sending the unit away.
    chrome::shadow_centered("Item description", win.x + 697.0 * k, win.y + 30.0 * k + 10.0 * k + head * 0.36, head, chrome::GOLD);
    let desc = at(570.0, 54.0, 256.0, 172.0);
    match hover {
        Some(item) => super::building_view::item_description(game, assets, item, desc.x, desc.y, desc.w, desc.h),
        None => {
            chrome::text_box(desc);
            let hint = "Hover an item to read about it. Click a pack item to wear or drink it, a worn one to take it off.";
            for (i, line) in wrap(hint, desc.w - 30.0, 14.0).iter().enumerate() {
                chrome::shadow_centered(line, desc.x + desc.w / 2.0, desc.y + desc.h / 2.0 - 16.0 + i as f32 * 17.0, 14.0, Color::new(1.0, 0.9, 0.66, 1.0));
            }
        }
    }
    let row = at(570.0, 234.0, 256.0, 62.0);
    draw_rectangle(row.x, row.y, row.w, row.h, Color::new(0.25, 0.04, 0.02, 0.55));
    chrome::silver_frame(row, 1.0);
    let face = Rect::new(row.x + 4.0 * k, row.y + 4.0 * k, row.h - 8.0 * k, row.h - 8.0 * k);
    assets.draw_portrait(u.def, Team::Player, face);
    if sel > 0 {
        let label = if u.alive() { "Dismiss" } else { "Bury" };
        let b = Rect::new(face.x + face.w + 12.0 * k, row.y + 14.0 * k, row.w - face.w - 24.0 * k, row.h - 28.0 * k);
        if button(b.x, b.y, b.w, b.h, label, true) {
            let name = u.name(&c).to_string();
            *message = Some(match game.dismiss(sel) {
                Ok(()) if u.alive() => format!("{name} leaves your army."),
                Ok(()) => format!("{name} is laid to rest."),
                Err(e) => service_error(e),
            });
            *selected = sel - 1;
        }
    } else {
        chrome::shadow_text("The hero leads the army.", face.x + face.w + 12.0 * k, row.y + row.h / 2.0 + 5.0, 14.0, chrome::CREAM);
    }

    // The strip: the last message, or what to do.
    let strip = at(248.0, 302.0, 586.0, 20.0);
    let (hint, hc) = match message {
        Some(m) => (m.clone(), chrome::GOLD),
        None => ("Click a unit to select it; Esc returns".to_string(), Color::new(1.0, 0.55, 0.25, 1.0)),
    };
    chrome::hint_strip(strip, &hint, hc);

    // The army: the cards as in battle, the selected one lit.
    let f = c.formation;
    let lines = f.display_lines() as f32;
    let cs = 1.0f32.min(2.0 / lines).min(6.0 / f.cols as f32);
    let (card, pitch) = (vec2(88.0 * cs * k, 128.0 * cs * k).round(), vec2(96.0 * cs * k, 133.0 * cs * k));
    let grid_w = f.cols as f32 * pitch.x - 8.0 * cs * k;
    let gx = (strip.x + (strip.w - grid_w) / 2.0).round();
    let cell_at = |slot: razdor::rules::formation::Slot| {
        let (line, col) = f.display(slot);
        vec2(gx + col as f32 * pitch.x, strip.y + strip.h + 10.0 * k + line as f32 * pitch.y).round()
    };
    for slot in f.slots() {
        if game.squad.iter().any(|u| u.slot == slot) {
            continue;
        }
        let p = cell_at(slot);
        chrome::empty_cell(Rect::new(p.x, p.y, card.x, card.y), chrome::CellIcon::of(f, slot), true);
    }
    for (i, v) in game.squad.iter().enumerate() {
        let p = cell_at(v.slot);
        let sq = Rect::new(p.x, p.y, card.x, card.x);
        draw_rectangle(p.x + 4.0 * k, p.y + 4.0 * k, card.x, card.y, Color::new(0.0, 0.0, 0.0, 0.45));
        assets.draw_portrait(v.def, Team::Player, sq);
        draw_rectangle_lines(sq.x, sq.y, sq.w, sq.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let vs = v.stats(&c);
        unit_sheet::stat_strip(Rect::new(p.x, p.y + card.x, card.x, card.y - card.x), &vs, &vs, vs[Stat::MagicPower], v.hp, i == sel);
        if !v.alive() {
            draw_rectangle(sq.x, sq.y, sq.w, sq.h, Color::new(0.0, 0.0, 0.0, 0.55));
            draw_line(sq.x + 10.0, sq.y + 10.0, sq.x + sq.w - 10.0, sq.y + sq.h - 10.0, 3.0, RED);
            draw_line(sq.x + sq.w - 10.0, sq.y + 10.0, sq.x + 10.0, sq.y + sq.h - 10.0, 3.0, RED);
        } else if v.unpaid {
            chrome::badge("sign-payment", sq.x + sq.w - 12.0 * k, sq.y + 12.0 * k, 20.0 * k, RED);
        }
        if i > 0 && v.upgrade_tree(&c).iter().any(|&(_, _, ok)| ok) {
            chrome::badge("Sign-Upgrade", sq.x + 12.0 * k, sq.y + 12.0 * k, 20.0 * k, GREEN);
        }
        if i == 0 {
            chrome::badge("SI_Helm", sq.x + 12.0 * k, sq.y + 12.0 * k, 20.0 * k, chrome::GOLD);
        }
        let over = mouse_in(p.x, p.y, card.x, card.y);
        if i == sel {
            chrome::glow_frame(sq, Color::new(0.35, 1.0, 0.35, 1.0), true);
        } else if over {
            chrome::glow_frame(sq, Color::new(0.35, 0.55, 1.0, 0.9), false);
        }
        if over && clicked() && i != sel {
            *selected = i;
            *message = None;
        }
    }

    if close || key(KeyCode::Escape) || key(KeyCode::A) {
        *message = None;
        return Some(match back {
            Some(v) => Screen::Building(v.clone()),
            None => Screen::WorldMap,
        });
    }
    None
}
