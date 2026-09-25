//! The spell book on the world map (key B): the hero's learned spells in the original's
//! 3 × 5 cells, what each costs this hero (mana and casting time), how long it lasts and what
//! it does; cast on the own army or on an enemy army within reach; the spells on the army
//! now, with the time they have left.

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::rules::clock::duration_label;
use razdor::rules::content::SpellDef;
use razdor::rules::game::{Game, SPELL_BOOK_SIZE};
use razdor::rules::magic::{self, CastError, CastOutcome, CastTarget, Duration, CAST_RANGE};

use super::audio::{cue, Cue};
use super::dialog::{Dialog, MANA};
use super::widgets::*;
use super::world_view;
use super::Screen;

const MARBLE: Color = Color::new(0.10, 0.20, 0.16, 0.98);
const MARBLE_EDGE: Color = Color::new(0.38, 0.58, 0.46, 1.0);
const CELL: Color = Color::new(0.16, 0.12, 0.20, 1.0);
const COLS: usize = 3;

/// "heals 30 hits", "defence +5, hits +20%", "-15 hits".
pub fn effect_summary(s: &SpellDef) -> String {
    use razdor::rules::items::stat_label;
    let mut effects = Vec::new();
    if let Some(h) = s.delta_fixed_hits {
        effects.push(if h >= 0 { format!("heals {h} hits") } else { format!("{h} hits") });
    }
    if let Some(p) = s.delta_percent_hits {
        effects.push(format!("hits {p:+}% at once"));
    }
    for (&st, &v) in &s.add {
        effects.push(format!("{} {v:+}", stat_label(st)));
    }
    for (&st, &v) in &s.percent {
        effects.push(format!("{} {v:+}%", stat_label(st)));
    }
    match s.life_lose_percent {
        Some(p) if p < 0 => effects.push(format!("max hits {p}%")),
        Some(_) => effects.push("lifts life-draining curses".into()),
        None => {}
    }
    if effects.is_empty() {
        "no effect".into()
    } else {
        effects.join(", ")
    }
}

pub fn duration_text(s: &SpellDef) -> String {
    match Duration::of(s) {
        Duration::Instant => "instant".into(),
        Duration::Permanent => "lasts for good".into(),
        Duration::Minutes(m) => format!("lasts {}", duration_label(m as f64)),
    }
}

/// The lines describing a spell: effect, school and cost for this hero, duration, target.
pub fn spell_lines(game: &Game, s: &SpellDef) -> Vec<String> {
    let cost = game.cast_cost(s);
    let school = s.school.map_or(String::new(), |m| format!("{m:?} magic. "));
    let base = if cost.mana != s.cost_mana.max(0) { format!(" (base {})", s.cost_mana) } else { String::new() };
    let target = if magic::targets_enemy(s) { "an enemy army" } else { "your army" };
    vec![
        effect_summary(s),
        format!("{school}Mana {}{base}, casting {}", cost.mana, duration_label(cost.minutes as f64)),
        format!("{}, cast on {target}", duration_text(s)),
    ]
}

fn cast_error(e: CastError) -> &'static str {
    match e {
        CastError::NotInBook | CastError::NoSuchSpell => "That spell is not in your book.",
        CastError::NotEnoughMana => "Not enough mana.",
        CastError::WrongTarget => "That spell does not work on that target.",
        CastError::OutOfRange => "The enemy is out of reach.",
        CastError::Busy => "Not now: a battle is coming.",
    }
}

/// The spell book window. `selected` is the book cell picked.
pub fn frame(
    game: &mut Game,
    assets: &super::assets::Assets,
    selected: &mut usize,
    message: &mut Option<String>,
    dialogs: &mut VecDeque<Dialog>,
) -> Option<Screen> {
    world_view::backdrop(game, assets);
    let (sw, sh) = (screen_width(), screen_height());
    let (w, h) = (1080.0f32.min(sw - 20.0), 640.0f32.min(sh - 110.0));
    let (x, y) = ((sw - w) / 2.0, ((sh - 84.0 - h) / 2.0).max(8.0));
    draw_rectangle(x, y, w, h, MARBLE);
    draw_rectangle_lines(x, y, w, h, 3.0, MARBLE_EDGE);
    draw_rectangle(x, y, w, 30.0, Color::new(0.06, 0.13, 0.10, 1.0));
    text_centered("Spell book", x + w / 2.0, y + 22.0, 20.0, INK);
    text(&format!("Mana {}", game.mana), x + w - 140.0, y + 22.0, 20.0, MANA);

    // The book: 3 × 5 cells.
    let book: Vec<SpellDef> = game.book().into_iter().cloned().collect();
    let (gx, gy) = (x + 20.0, y + 46.0);
    let (cw, ch) = (190.0, 76.0);
    for k in 0..SPELL_BOOK_SIZE {
        let (c, r) = (k % COLS, k / COLS);
        let (cx, cy) = (gx + c as f32 * (cw + 8.0), gy + r as f32 * (ch + 8.0));
        draw_rectangle(cx, cy, cw, ch, CELL);
        let Some(s) = book.get(k) else {
            draw_rectangle_lines(cx, cy, cw, ch, 1.0, DIM);
            continue;
        };
        let hover = mouse_in(cx, cy, cw, ch);
        let edge = if *selected == k { ACCENT } else if hover { INK } else { MARBLE_EDGE };
        draw_rectangle_lines(cx, cy, cw, ch, if *selected == k { 3.0 } else { 2.0 }, edge);
        for (i, line) in wrap(&s.name, cw - 16.0, 18.0).iter().take(2).enumerate() {
            text(line, cx + 8.0, cy + 22.0 + i as f32 * 20.0, 18.0, INK);
        }
        let cost = game.cast_cost(s);
        let color = if game.mana >= cost.mana { MANA } else { Color::new(0.9, 0.4, 0.35, 1.0) };
        text(&format!("{} mana, {}", cost.mana, duration_label(cost.minutes as f64)), cx + 8.0, cy + ch - 10.0, 16.0, color);
        if hover && clicked() {
            *selected = k;
        }
    }
    if book.is_empty() {
        text("Your book is empty. Learn spells at a sanctuary.", gx, gy + 5.0 * (ch + 8.0) + 24.0, 18.0, DIM);
    }

    // The chosen spell and its targets.
    let px = gx + COLS as f32 * (cw + 8.0) + 12.0;
    let pw = x + w - 20.0 - px;
    let mut next = None;
    if let Some(s) = book.get(*selected) {
        let mut cy = gy;
        draw_rectangle(px, cy, pw, 150.0, Color::new(0.36, 0.16, 0.10, 1.0));
        text_centered(&s.name, px + pw / 2.0, cy + 28.0, 22.0, Color::new(1.0, 0.85, 0.55, 1.0));
        for (i, line) in spell_lines(game, s).iter().enumerate() {
            for (j, l) in wrap(line, pw - 20.0, 17.0).iter().take(2).enumerate() {
                text_centered(l, px + pw / 2.0, cy + 58.0 + i as f32 * 30.0 + j as f32 * 17.0, 17.0, INK);
            }
        }
        cy += 164.0;
        let cost = game.cast_cost(s);
        let can = game.mana >= cost.mana && game.foe.is_none();
        let mut target = None;
        if magic::targets_enemy(s) {
            let armies = game.spell_targets();
            if armies.is_empty() {
                let line = format!("No enemy army in sight within {CAST_RANGE} cells.");
                text_centered(&line, px + pw / 2.0, cy + 24.0, 18.0, DIM);
            }
            let here = game.tile();
            for (k, &i) in armies.iter().take(4).enumerate() {
                let a = &game.world.armies[i];
                let name = if a.name.is_empty() { "an army".to_string() } else { a.name.clone() };
                let d = game.world.map.distance(a.tile(&game.world.map), here);
                let label = format!("Cast on {name} ({d} cells)");
                if button(px, cy + k as f32 * 50.0, pw, 42.0, &label, can) {
                    target = Some(CastTarget::Army(a.uid));
                }
            }
        } else if button(px, cy, pw, 44.0, "Cast on your army", can) || (can && key(KeyCode::Enter)) {
            target = Some(CastTarget::Own);
        }
        if game.mana < cost.mana {
            text_centered("Not enough mana.", px + pw / 2.0, cy + 230.0, 18.0, Color::new(0.9, 0.4, 0.35, 1.0));
        }
        if let Some(t) = target {
            match game.cast(s.id, t) {
                Ok(cast) => {
                    cue(Cue::CastSpell);
                    if matches!(cast.outcome, CastOutcome::Done { .. }) {
                        cue(if matches!(t, CastTarget::Own) { Cue::SpellGood } else { Cue::SpellEvil });
                    }
                    *message = Some(match cast.outcome {
                        CastOutcome::Done { hits, killed, destroyed } => {
                            let mut m = s.name.clone();
                            if hits != 0 {
                                m += &format!(": {hits:+} hits");
                            }
                            if killed > 0 {
                                m += &format!(", {killed} fell");
                            }
                            if destroyed {
                                m += ". The army is no more.";
                            } else if magic::is_lasting(s) {
                                m += &format!(" ({})", duration_text(s));
                            }
                            m
                        }
                        CastOutcome::Interrupted => "An enemy fell on you while you were casting: the spell is lost.".into(),
                        CastOutcome::TargetLost => "The target got away before the spell was ready.".into(),
                    });
                    next = world_view::handle_events(game, cast.events, message, dialogs).or(Some(Screen::WorldMap));
                }
                Err(e) => *message = Some(cast_error(e).into()),
            }
        }
    }

    // Spells on the army now.
    let ay = y + h - 118.0;
    text("On your army:", px, ay, 18.0, ACCENT);
    let now = game.clock.total_minutes() as u64;
    let active: Vec<String> = game
        .active_spells()
        .iter()
        .filter_map(|e| {
            let name = &game.spell(e.spell)?.name;
            Some(match e.until {
                None => format!("{name}: for good"),
                Some(t) => format!("{name}: {} left", duration_label(t.saturating_sub(now) as f64)),
            })
        })
        .collect();
    if active.is_empty() {
        text("no spells", px, ay + 22.0, 17.0, DIM);
    }
    for (i, line) in active.iter().take(4).enumerate() {
        text(line, px, ay + 22.0 + i as f32 * 20.0, 17.0, MANA);
    }

    text(&format!("Book {}/{SPELL_BOOK_SIZE}. Casting takes game time: armies move meanwhile.", book.len()), gx, y + h - 18.0, 16.0, DIM);
    if button(x + w - 140.0, y + h - 54.0, 120.0, 40.0, "Close", true) || key(KeyCode::Escape) || key(KeyCode::B) {
        next = next.or(Some(Screen::WorldMap));
    }
    if let Some(m) = message.as_ref().filter(|_| next.is_none()) {
        text_centered(m, x + w / 2.0, y + h + 30.0, 20.0, ACCENT);
    }
    next
}
