//! The original's unit panel (battle and army screens): the unit's full-body sepia figure on
//! parchment, worn items in the top corners, its name, the long stat list and its traits
//! with their icons (video notes §3, refs 09 and 11).

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::battle::{bless_effect, curse_effect, Buff};
use razdor::rules::content::{Bonus, Content, HeroClass, ItemId, MagicDirection, MagicSchool, Stat, UnitId};
use razdor::rules::units::Stats;

use super::assets::Assets;
use super::chrome::{self, k, shadow_centered, shadow_right, shadow_text, BLUE_TEXT, CREAM, ORANGE_TEXT, RED_TEXT};
use super::widgets::{measure, mouse_in, wrap};

/// What the panel shows of one unit.
pub struct Sheet<'a> {
    pub kind: UnitId,
    pub name: &'a str,
    pub level: i32,
    pub xp: i32,
    pub need: i32,
    pub hp: i32,
    /// Stats now (with blessings and curses).
    pub now: &'a Stats,
    /// Stats with items, as the battle began.
    pub start: &'a Stats,
    /// Current magic power (it drains during a battle).
    pub power: i32,
    pub wage: i32,
    pub items: [Option<ItemId>; 4],
    pub back_row: bool,
    pub in_building: bool,
    pub hero: Option<HeroClass>,
    /// Status lines under the name (actions left, poison, this turn's modifiers).
    pub status: Vec<(String, Color)>,
}

/// One-based number of a bonus in the original's list (`Bonus<N>.lit`, `[Army] Bonus<N>`).
fn bonus_number(b: &Bonus) -> Option<usize> {
    Bonus::known().position(|x| x == b).map(|i| i + 1)
}

/// English stand-ins for the trait texts, used without an install.
fn bonus_english(b: &Bonus) -> String {
    let s = match b {
        Bonus::SpearDefense => tr("Long weapon: triple melee defence on the first turn of a battle"),
        Bonus::HorseAtack => tr("Fast attack: +1 action on the first turn of a battle"),
        Bonus::ArmorIgnore => tr("Piercing blow: ignores the enemy's defence (not a building's)"),
        Bonus::ArmyMedic => tr("Healer: the army heals 15% of its wounds every day"),
        Bonus::Merchant => tr("Expert trader: +50% when selling, -30% when buying"),
        Bonus::DeathCurse => tr("Death's curse: whoever kills this unit dies too"),
        Bonus::GodAnger => tr("Wrath of God: +10 damage past any defence"),
        Bonus::GodStrike => tr("Anger of God: +20 damage past any defence"),
        Bonus::Unvulnerabe => tr("Invulnerable: loses only 1 hit per blow"),
        Bonus::VampirsGist => tr("Dark gift: ignores armour and absorbs 30% of a blow"),
        Bonus::OldVampirsGist => tr("Dark art: ignores armour, absorbs 30%, +1 action on the first turn"),
        Bonus::Evasive => tr("Evasive: only 70% of physical damage gets through"),
        Bonus::Ghost => tr("Ghost: immune to weapons; its killer dies"),
        Bonus::Artillery => tr("Barrage: always acts first and ignores defence"),
        Bonus::Garrison => tr("Garrison: double strength inside a castle or fort"),
        Bonus::AddPayment => tr("Quartermaster: the army's wages are 30% lower"),
        Bonus::Poison => tr("Poisoned weapon: the target loses 15% of its life each turn"),
        Bonus::Dead => tr("Undead: arrows do 70% less damage"),
        Bonus::FastDead => tr("Fast undead: arrows do 70% less, +1 action on the first turn"),
        Bonus::Counterblow => tr("Counterblow: strikes back when struck"),
        Bonus::FlankStrike => tr("Flank strike: double attack through an empty cell"),
        other => return razdor::rules::items::bonus_name(other),
    };
    s.to_string()
}

fn trait_line(b: &Bonus) -> (String, String) {
    match bonus_number(b) {
        Some(n) => (format!("Bonus{n}"), chrome::ui_text("Army", &format!("Bonus{n}")).unwrap_or_else(|| bonus_english(b))),
        None => ("Bonus1".to_string(), bonus_english(b)),
    }
}

fn hero_trait(h: HeroClass) -> (String, String) {
    let n = HeroClass::ALL.iter().position(|&c| c == h).unwrap_or(0) + 1;
    let english = match h {
        HeroClass::Knight => tr("The army of this hero takes 10% less damage from enemy attacks (magic excepted)."),
        HeroClass::Archmage => tr("The Archmage casts spells twice as fast for 50% less mana, but his army gets no bonuses."),
        HeroClass::Ranger => tr("The army of this hero travels 20% faster, and the wounded heal 20% of their hits every day."),
    };
    (format!("HeroBonus{n}"), chrome::ui_text("NewHero", &format!("Bonus{n}")).unwrap_or_else(|| english.to_string()))
}

/// A stat line: label, value, colour.
type Line = (String, String, Color);

fn signed(v: i32) -> String {
    if v > 0 {
        format!("+{v}")
    } else {
        v.to_string()
    }
}

/// Colour of a value against its start-of-battle value: blue raised, red lowered.
fn cmp_color(now: i32, start: i32) -> Color {
    match now.cmp(&start) {
        std::cmp::Ordering::Greater => BLUE_TEXT,
        std::cmp::Ordering::Less => RED_TEXT,
        std::cmp::Ordering::Equal => CREAM,
    }
}

fn buff_lines(lines: &mut Vec<Line>, b: Buff, raise: bool) {
    let (ad, ini, act) = if raise {
        (tr("Adds to attack/defence"), tr("Adds to initiative"), tr("Hastens (+ actions)"))
    } else {
        (tr("Lowers attack/defence"), tr("Lowers initiative"), tr("Slows (- actions)"))
    };
    if b.attack != 0 || b.defence != 0 {
        lines.push((ad.into(), format!("{}/{}", signed(b.attack), signed(b.defence)), CREAM));
    }
    if b.initiative != 0 {
        lines.push((ini.into(), signed(b.initiative), CREAM));
    }
    if b.actions != 0 {
        lines.push((act.into(), signed(b.actions), CREAM));
    }
}

/// The stat list of `s`, as the original's panel orders it.
fn stat_lines(content: &Content, s: &Sheet) -> Vec<Line> {
    let (now, start) = (s.now, s.start);
    let naked = Stats::of_level(content, s.kind, s.level);
    let mut lines: Vec<Line> = Vec::new();
    let max = now.max_hp();
    let hits = if s.hp < max { format!("{} / {max}", s.hp.max(0)) } else { max.to_string() };
    lines.push((tr("Hits").into(), hits, if s.hp < max { RED_TEXT } else { cmp_color(max, start.max_hp()) }));
    // "base + bonus" for what items and traits add, as the original writes it.
    let split = |st: Stat| -> (String, Color) {
        let (n, b) = (naked[st], start[st] - naked[st]);
        let v = if b != 0 && now[st] == start[st] { format!("{n} {} {}", if b > 0 { "+" } else { "-" }, b.abs()) } else { now[st].to_string() };
        (v, cmp_color(now[st], start[st]))
    };
    if start[Stat::AttackBlow] > 0 || now[Stat::AttackBlow] > 0 {
        let (v, c) = split(Stat::AttackBlow);
        lines.push((tr("Melee attack").into(), v, c));
    }
    if start[Stat::AttackShot] > 0 || now[Stat::AttackShot] > 0 {
        let (v, c) = split(Stat::AttackShot);
        lines.push((tr("Ranged attack").into(), v, c));
    }
    let (v, c) = split(Stat::DefenceBlow);
    lines.push((tr("Melee defence").into(), v, c));
    let (v, c) = split(Stat::DefenceShot);
    lines.push((tr("Ranged defence").into(), v, c));
    if now.is_mage() || start.is_mage() {
        let school = now.magic.unwrap_or(MagicSchool::Elemental);
        let p = s.power;
        let o = &content.options;
        let pc = cmp_color(p, start[Stat::MagicPower]);
        let dir = now.magic_direction();
        if matches!(dir, MagicDirection::ToEnemy | MagicDirection::ToAll) {
            lines.push((tr("Magic strike (- hits)").into(), format!("-{p}"), pc));
            buff_lines(&mut lines, curse_effect(o, school, p), false);
        }
        if matches!(dir, MagicDirection::ToAlly | MagicDirection::ToAll) {
            let heal = match school {
                MagicSchool::Life => p,
                MagicSchool::Elemental => p / 2,
                MagicSchool::Death => 0,
            };
            if heal > 0 {
                lines.push((tr("Heals (+ hits)").into(), format!("+{heal}"), pc));
            }
            buff_lines(&mut lines, bless_effect(o, school, p), true);
        }
    }
    for (label, st) in [
        (tr("Life magic protection"), Stat::ProtectLife),
        (tr("Elemental magic protection"), Stat::ProtectElemental),
        (tr("Death magic protection"), Stat::ProtectDeath),
        (tr("Regeneration"), Stat::Regen),
        (tr("Vampirism"), Stat::Vampirizm),
    ] {
        if now[st] != 0 || start[st] != 0 {
            lines.push((label.into(), format!("{}%", now[st]), cmp_color(now[st], start[st])));
        }
    }
    lines.push((tr("Initiative").into(), now[Stat::Initiative].to_string(), cmp_color(now[Stat::Initiative], start[Stat::Initiative])));
    lines.push((tr("Actions").into(), now[Stat::Manevres].to_string(), cmp_color(now[Stat::Manevres], start[Stat::Manevres])));
    if s.wage > 0 {
        lines.push((tr("Daily wage").into(), s.wage.to_string(), ORANGE_TEXT));
    }
    lines
}

/// The stat strip's text.
const STRIP_INK: Color = Color::new(0.98, 0.92, 0.72, 1.0);
const HITS_INK: Color = Color::new(1.0, 0.78, 0.35, 1.0);

/// Blue for a stat above its start value, red below.
fn strip_color(cur: i32, base: i32) -> Color {
    match cur.cmp(&base) {
        std::cmp::Ordering::Greater => BLUE_TEXT,
        std::cmp::Ordering::Less => RED_TEXT,
        std::cmp::Ordering::Equal => STRIP_INK,
    }
}

/// The card's attack piece: `A:` melee, `S:` ranged, `Pwr:` magic.
fn attack_piece(s: &Stats, base: &Stats, power: i32) -> (String, Color) {
    if s.is_warrior() || base.is_warrior() {
        (razdor::trf!("A: {v}", v = s[Stat::AttackBlow]), strip_color(s[Stat::AttackBlow], base[Stat::AttackBlow]))
    } else if s.is_shooter() || base.is_shooter() {
        (razdor::trf!("S: {v}", v = s[Stat::AttackShot]), strip_color(s[Stat::AttackShot], base[Stat::AttackShot]))
    } else {
        (razdor::trf!("Pwr: {power}", power), strip_color(power, base[Stat::MagicPower]))
    }
}

/// The strip under a card's portrait, as the original's: "A: 45  D: 35/40", "Mnvr: 1
/// Ini: 12", "Hits: 70" (or "Hits: 45/70"). `lit` reddens it (the unit acting, or the one
/// selected on the army screen).
pub fn stat_strip(strip: Rect, now: &Stats, base: &Stats, power: i32, hp: i32, lit: bool) {
    let k = k();
    chrome::surface(strip, chrome::Skin::Strip);
    if lit {
        draw_rectangle(strip.x, strip.y, strip.w, strip.h, Color::new(0.75, 0.25, 0.0, 0.4));
    }
    draw_rectangle_lines(strip.x, strip.y, strip.w, strip.h, 1.0, Color::new(0.0, 0.0, 0.0, 0.5));
    let fs = (strip.h * 0.30).round().max(9.0);
    let lh = strip.h / 3.0;
    let (x0, x1) = (strip.x + 3.0 * k, strip.x + strip.w - 3.0 * k);
    let (att, ac) = attack_piece(now, base, power);
    shadow_text(&att, x0, strip.y + lh - 2.0 * k, fs, ac);
    let d = razdor::trf!("D: {blow}/{shot}", blow = now[Stat::DefenceBlow], shot = now[Stat::DefenceShot]);
    let dc = strip_color(now[Stat::DefenceBlow] + now[Stat::DefenceShot], base[Stat::DefenceBlow] + base[Stat::DefenceShot]);
    shadow_right(&d, x1, strip.y + lh - 2.0 * k, fs, dc);
    let (mn, ini) = (now[Stat::Manevres], now[Stat::Initiative]);
    shadow_text(&razdor::trf!("Mnvr: {mn}", mn), x0, strip.y + 2.0 * lh - 2.0 * k, fs, strip_color(mn, base[Stat::Manevres]));
    shadow_right(&razdor::trf!("Ini: {ini}", ini), x1, strip.y + 2.0 * lh - 2.0 * k, fs, strip_color(ini, base[Stat::Initiative]));
    let max = now.max_hp();
    let hits = if hp < max { razdor::trf!("Hits: {hp}/{max}", hp = hp.max(0), max) } else { razdor::trf!("Hits: {max}", max) };
    shadow_centered(&hits, strip.x + strip.w / 2.0, strip.y + 3.0 * lh - 2.5 * k, fs, if hp < max { Color::new(1.0, 0.6, 0.4, 1.0) } else { HITS_INK });
}

/// Rects of the four item slots around the figure: two at the top left, two at the top
/// right (as in the army screen).
pub fn slot_rects(r: Rect) -> [Rect; 4] {
    let s = 44.0 * k();
    let m = 8.0 * k();
    [
        Rect::new(r.x + m, r.y + m, s, s),
        Rect::new(r.x + m, r.y + m + s + 6.0 * k(), s, s),
        Rect::new(r.x + r.w - m - s, r.y + m, s, s),
        Rect::new(r.x + r.w - m - s, r.y + m + s + 6.0 * k(), s, s),
    ]
}

/// A placeholder figure: a dark sepia silhouette.
fn silhouette(r: Rect) {
    let c = Color::new(0.30, 0.18, 0.08, 0.55);
    let (cx, top) = (r.x + r.w / 2.0, r.y);
    let h = r.h;
    draw_circle(cx, top + h * 0.12, h * 0.075, c);
    let (a, b) = (vec2(cx - h * 0.09, top + h * 0.22), vec2(cx + h * 0.09, top + h * 0.22));
    let (cc, d) = (vec2(cx + h * 0.16, top + h * 0.95), vec2(cx - h * 0.16, top + h * 0.95));
    draw_triangle(a, b, cc, c);
    draw_triangle(a, cc, d, c);
    draw_triangle(a, vec2(cx - h * 0.2, top + h * 0.5), vec2(cx - h * 0.1, top + h * 0.3), c);
    draw_triangle(b, vec2(cx + h * 0.2, top + h * 0.5), vec2(cx + h * 0.1, top + h * 0.3), c);
}

/// Draws the panel in `r`. `slots` shows the four item slots (empty ones too) and returns
/// the one clicked; the item under the mouse goes to `hover`.
pub fn draw(assets: &Assets, content: &Content, r: Rect, s: &Sheet, slots: bool, hover: &mut Option<ItemId>) -> Option<usize> {
    let k = k();
    chrome::parchment(r, true);
    // The figure, behind the text.
    let fig_h = r.h * 0.66;
    match assets.figure(s.kind) {
        Some(t) => {
            let h = (t.height() * 0.9375 * k).min(fig_h);
            let w = t.width() * h / t.height();
            let top = r.y + 10.0 * k;
            chrome::tex(&t, Rect::new(r.x + (r.w - w) / 2.0, top, w, h), Color::new(1.0, 1.0, 1.0, 0.92));
        }
        None => silhouette(Rect::new(r.x + r.w * 0.2, r.y + 12.0 * k, r.w * 0.6, fig_h * 0.95)),
    }
    // Items worn.
    let mut clicked = None;
    for (i, sr) in slot_rects(r).iter().enumerate() {
        let item = s.items[i];
        if !slots && item.is_none() {
            continue;
        }
        if slots {
            draw_rectangle(sr.x, sr.y, sr.w, sr.h, Color::new(0.2, 0.12, 0.05, 0.35));
            draw_rectangle_lines(sr.x, sr.y, sr.w, sr.h, 1.0, Color::new(0.45, 0.3, 0.15, 0.8));
        }
        if let Some(it) = item {
            assets.draw_item(it, sr.x, sr.y, sr.w);
        }
        if mouse_in(sr.x, sr.y, sr.w, sr.h) {
            if item.is_some() {
                *hover = item;
            }
            if super::widgets::clicked() {
                clicked = Some(i);
            }
        }
    }
    let x0 = r.x + 24.0 * k;
    let x1 = r.x + r.w - 16.0 * k;
    let name_size = (17.0 * k).round();
    // The name and the stats run over the figure's lower part, as in the original.
    let mut y = r.y + r.h * 0.375;
    chrome::strong_centered(s.name, r.x + r.w / 2.0, y, name_size, CREAM);
    y += 17.0 * k;
    let size = (12.0 * k).round();
    let lh = 13.6 * k;
    // Level and experience on one line.
    chrome::strong_text(&razdor::trf!("Level {level}", level = s.level), x0, y, size, CREAM);
    chrome::strong_right(&razdor::trf!("XP {xp} / {need}", xp = s.xp, need = s.need), x1, y, size, CREAM);
    y += lh;
    for (label, value, color) in stat_lines(content, s) {
        let c = if color == CREAM { CREAM } else { color };
        let room = x1 - x0 - measure(&value, size).width - 6.0 * k;
        chrome::strong_text(&label, x0, y, super::widgets::fit_size(&label, room, size), c);
        chrome::strong_right(&value, x1, y, size, c);
        y += lh;
    }
    for (line, color) in &s.status {
        chrome::strong_text(line, x0, y, super::widgets::fit_size(line, x1 - x0, size), *color);
        y += lh;
    }
    // The description, then the traits with their icons.
    y += 6.0 * k;
    let bottom = r.y + r.h - 6.0 * k;
    let desc = &content.unit(s.kind).description;
    let small = (12.0 * k).round();
    let slh = 13.2 * k;
    for line in wrap(desc, r.w - 44.0 * k, small) {
        if y > bottom {
            return clicked;
        }
        shadow_text(&line, x0 - 4.0 * k, y, small, CREAM);
        y += slh;
    }
    let mut traits: Vec<(String, String)> = s.start.bonuses.iter().map(trait_line).collect();
    if let Some(h) = s.hero {
        traits.insert(0, hero_trait(h));
    }
    if s.back_row {
        traits.push(("Bonus-2Row".into(), chrome::ui_text("Army", "Hint1").unwrap_or_else(|| tr("In the second row the unit gets a bonus to its ranged defence!").into())));
    }
    if s.in_building {
        traits.push(("Bonus-InCastle".into(), chrome::ui_text("Army", "Hint2").unwrap_or_else(|| tr("In its own building the unit gets a bonus to all defences!").into())));
    }
    let icon = 24.0 * k;
    for (art, line) in traits {
        y += 5.0 * k;
        if y + slh > bottom {
            break;
        }
        chrome::trait_icon(&art, x0 - 8.0 * k, y - slh + 2.0 * k, icon);
        let lines = wrap(&line, r.w - 44.0 * k - icon, small);
        for (i, l) in lines.iter().enumerate() {
            if y > bottom {
                break;
            }
            let lx = if i < 2 { x0 + icon - 4.0 * k } else { x0 - 8.0 * k };
            shadow_text(l, lx, y, small, BLUE_TEXT);
            y += slh;
        }
        if lines.len() == 1 {
            y += slh * 0.6;
        }
    }
    let _ = measure;
    clicked
}
