//! The spell book on the world map (key B): the hero's learned spells in the original's
//! 3 × 5 cells, what each costs this hero (mana and casting time), how long it lasts and what
//! it does; cast on the own army or on an enemy army within reach; the spells on the army
//! now, with the time they have left.

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::dt::data::MagicSchool;
use razdor::i18n::{n_, tr};
use razdor::trf;

use razdor::rules::clock::duration_label;
use razdor::rules::content::SpellDef;
use razdor::rules::game::{Game, SPELL_BOOK_SIZE};
use razdor::rules::magic::{self, CastError, CastOutcome, CastTarget, Duration, CAST_RANGE};

use super::audio::{cue, Cue};
use super::dialog::Dialog;
use super::widgets::*;
use super::world_view;
use super::Screen;

const COLS: usize = 3;

/// "heals 30 hits", "defence +5, hits +20%", "-15 hits".
pub fn effect_summary(s: &SpellDef) -> String {
    use razdor::rules::items::stat_label;
    let mut effects = Vec::new();
    if let Some(h) = s.delta_fixed_hits {
        effects.push(if h >= 0 { trf!("heals {h} hits", h) } else { trf!("{h} hits", h) });
    }
    if let Some(p) = s.delta_percent_hits {
        effects.push(trf!("hits {p}% at once", p = format!("{p:+}")));
    }
    for (&st, &v) in &s.add {
        effects.push(format!("{} {v:+}", stat_label(st)));
    }
    for (&st, &v) in &s.percent {
        effects.push(format!("{} {v:+}%", stat_label(st)));
    }
    match s.life_lose_percent {
        Some(p) if p < 0 => effects.push(trf!("max hits {p}%", p)),
        Some(_) => effects.push(tr("lifts life-draining curses").into()),
        None => {}
    }
    if effects.is_empty() {
        tr("no effect").into()
    } else {
        effects.join(", ")
    }
}

pub fn duration_text(s: &SpellDef) -> String {
    match Duration::of(s) {
        Duration::Instant => tr("instant").into(),
        Duration::Minutes(m) => trf!("lasts {time}", time = duration_label(m as f64)),
    }
}

/// The lines describing a spell: effect, school and cost for this hero, duration, target.
pub fn spell_lines(game: &Game, s: &SpellDef) -> Vec<String> {
    let cost = game.cast_cost(s);
    let school = s.school.map_or(String::new(), |m| format!("{} ", school_label(m)));
    let base = if cost.mana != s.cost_mana.max(0) { format!(" {}", trf!("(base {base})", base = s.cost_mana)) } else { String::new() };
    let target = if magic::targets_enemy(s) { tr("an enemy army") } else { tr("your army") };
    vec![
        effect_summary(s),
        format!("{school}{}{base}, {}", trf!("Mana {mana}", mana = cost.mana), trf!("casting {time}", time = duration_label(cost.minutes as f64))),
        trf!("{duration}, cast on {target}", duration = duration_text(s), target),
    ]
}

/// "Life magic.", "Death magic.", "Elemental magic.".
pub fn school_label(m: MagicSchool) -> &'static str {
    match m {
        MagicSchool::Life => tr("Life magic."),
        MagicSchool::Elemental => tr("Elemental magic."),
        MagicSchool::Death => tr("Death magic."),
    }
}

fn cast_error(e: CastError) -> &'static str {
    match e {
        CastError::NotInBook | CastError::NoSuchSpell => tr("That spell is not in your book."),
        CastError::NotEnoughMana => tr("Not enough mana."),
        CastError::WrongTarget => tr("That spell does not work on that target."),
        CastError::OutOfRange => tr("The enemy is out of reach."),
        CastError::Busy => tr("Not now: a battle is coming."),
    }
}

/// A text of the install (`[Magic] <key>`) in Russian, else ours.
fn own(key: &str, ours: &'static str) -> String {
    let t = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| super::chrome::ui_text("Magic", key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// Whole hours of `minutes`, as the book writes them ("2 час", "6 часов" in the original).
fn hours(minutes: u64, many: bool) -> String {
    let h = (minutes as f64 / 60.0).round() as u64;
    let word = if many { own("Hours", n_("hours")) } else { own("Hour", n_("h")) };
    format!("{h} {word}")
}

/// The spell book window as the original's (the video, 36:59): 3 × 5 brown cells, each
/// with the spell's round picture in its silver frame, its name, what it does, its mana and
/// reading time and how long it lasts; empty cells are dark with the knot and "Нет изученных
/// заклинаний". A click casts the spell (on an enemy army: pick it in the list that opens);
/// the spells on the army are listed under the book.
pub fn frame(
    game: &mut Game,
    assets: &super::assets::Assets,
    selected: &mut usize,
    message: &mut Option<String>,
    dialogs: &mut VecDeque<Dialog>,
) -> Option<Screen> {
    use super::chrome::{self, CREAM};
    let bar_pick = world_view::window_backdrop(game, assets, Some(super::game_bar::BarButton::Spells));
    let k = chrome::k();
    // The army screen's window: 836 × 600 video pixels over the map.
    let (w, h) = ((836.0 * k).round(), (600.0 * k).round());
    let r = Rect::new(((screen_width() - w) / 2.0).round(), ((screen_height() - chrome::bar_height() - h) / 2.0).max(2.0).round(), w, h);
    let title = own("Title", n_("The hero's spell book"));
    let (inner, closed) = chrome::window(r, &title, chrome::Skin::Marble, true);
    let book: Vec<SpellDef> = game.book().into_iter().cloned().collect();
    let gap = 8.0 * k;
    let (cw, ch) = ((inner.w - 4.0 * gap) / 3.0, (inner.h - 6.0 * gap) / 5.0 + gap);
    let mut cast = None;
    for cell in 0..SPELL_BOOK_SIZE {
        let (c, row) = (cell % COLS, cell / COLS);
        let cr = Rect::new(inner.x + gap + c as f32 * (cw + gap), inner.y + gap + row as f32 * ch, cw, ch - gap);
        let spell = book.get(cell);
        // The cell: brown parchment, dark when empty.
        chrome::surface(cr, chrome::Skin::Brown);
        if spell.is_none() {
            // Olive and darker than a learnt spell's cell, as in the video.
            chrome::multiply(|| draw_rectangle(cr.x, cr.y, cr.w, cr.h, Color::new(0.62, 0.78, 0.5, 1.0)));
        }
        let hover = !input_blocked() && cr.contains(crate::ui::widgets::pointer().into());
        let edge = if spell.is_some() && (*selected == cell || hover) { Color::new(1.0, 0.85, 0.5, 1.0) } else { Color::new(0.55, 0.5, 0.4, 1.0) };
        draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, 1.0, edge);
        let icon = Rect::new(cr.x + 4.0 * k, cr.y + 4.0 * k, cr.h - 8.0 * k, cr.h - 8.0 * k);
        let tx = icon.x + icon.w + (cr.x + cr.w - icon.x - icon.w) / 2.0;
        let room = cr.x + cr.w - icon.x - icon.w - 8.0 * k;
        let Some(s) = spell else {
            // The knot stamped in black, and the words pressed into the leather.
            if let Some(t) = chrome::win_fx("MSign", chrome::Fx::Shade) {
                chrome::tex(&t, icon, WHITE);
            }
            let none = own("NoSpell", n_("No\nspells\nlearnt"));
            let lines: Vec<&str> = none.split('\n').collect();
            let size = 15.0 * k;
            let top = cr.y + cr.h / 2.0 - (lines.len() as f32 - 1.0) * 8.0 * k + size * 0.36;
            super::dt_font::with_face(super::dt_font::Face::Title, || {
                for (i, l) in lines.iter().enumerate() {
                    let y = top + i as f32 * 16.0 * k;
                    text_centered(l, tx + 1.0 * k, y + 1.0 * k, size, Color::new(0.55, 0.42, 0.2, 0.7));
                    text_centered(l, tx, y, size, Color::new(0.08, 0.05, 0.02, 1.0));
                }
            });
            continue;
        };
        chrome::spell_icon(&s.icons, icon);
        if let Some(t) = chrome::win_fx("Spell-Frame", chrome::Fx::KeyBlack) {
            chrome::tex(&t, icon, WHITE);
        }
        let cost = game.cast_cost(s);
        let size = 12.0 * k;
        super::dt_font::with_face(super::dt_font::Face::Title, || {
            let ns = fit_size(&s.name, room, 17.0 * k);
            chrome::shadow_centered(&s.name, tx, cr.y + 26.0 * k, ns, CREAM);
        });
        let effect = effect_summary(s);
        let mana_line = format!("{} {}, {} {}", own("Mana", n_("Mana:")), cost.mana, own("Reading", n_("Reading:")), hours(cost.minutes, false));
        let last = match Duration::of(s) {
            Duration::Instant => own("MomentaryEffect", n_("Instant effect")),
            Duration::Minutes(m) => format!("{} {}", own("TimeOfEffect", n_("Lasts:")), hours(m, true)),
        };
        let mana_color = if game.mana >= cost.mana { Color::new(1.0, 0.72, 0.3, 1.0) } else { chrome::RED_TEXT };
        let mut y = cr.y + 50.0 * k;
        for line in wrap(&effect, room, size).iter().take(2) {
            chrome::shadow_centered(line, tx, y, size, chrome::BLUE_TEXT);
            y += 13.0 * k;
        }
        chrome::shadow_centered(&ellipsize(&mana_line, room, size), tx, y, size, mana_color);
        chrome::shadow_centered(&ellipsize(&last, room, size), tx, y + 13.0 * k, size, Color::new(1.0, 0.72, 0.3, 1.0));
        if hover {
            if game.mana < cost.mana {
                tooltip(&[(own("NoManaForSpell", n_("Not enough mana\nto cast this spell!")).replace('\n', " "), chrome::RED_TEXT)]);
            }
            if clicked() {
                *selected = cell;
                cast = Some(s.clone());
            }
        }
    }
    // An enemy spell: the armies within reach, in a list under the book.
    let mut next = None;
    let picked = book.get(*selected).cloned();
    let mut target = None;
    if let Some(s) = cast.as_ref().filter(|s| !magic::targets_enemy(s)) {
        target = Some((s.clone(), CastTarget::Own));
    }
    if let Some(s) = picked.as_ref().filter(|s| magic::targets_enemy(s)) {
        let armies = game.spell_targets();
        let lh = (28.0 + 30.0 * armies.len().clamp(1, 4) as f32) * k;
        let lr = Rect::new(r.center().x - 180.0 * k, r.y + r.h - lh - 10.0 * k, 360.0 * k, lh);
        chrome::window(lr, &s.name, chrome::Skin::Marble, false);
        if armies.is_empty() {
            let line = trf!("No enemy army in sight within {range} cells.", range = CAST_RANGE);
            chrome::shadow_centered(&line, lr.center().x, lr.y + 44.0 * k, 13.0 * k, CREAM);
        }
        let here = game.tile();
        let can = game.mana >= game.cast_cost(s).mana && game.foe.is_none();
        for (i, &a) in armies.iter().take(4).enumerate() {
            let army = &game.world.armies[a];
            let name = if army.name.is_empty() { tr("an army").to_string() } else { army.name.clone() };
            let d = game.world.map.distance(army.tile(&game.world.map), here);
            let label = trf!("Cast on {name} ({d} cells)", name, d);
            let b = Rect::new(lr.x + 10.0 * k, lr.y + 32.0 * k + i as f32 * 30.0 * k, lr.w - 20.0 * k, 26.0 * k);
            let over = !input_blocked() && b.contains(crate::ui::widgets::pointer().into());
            chrome::marble_button(b, &label, can, over);
            if can && over && clicked() {
                target = Some((s.clone(), CastTarget::Army(army.uid)));
            }
        }
    }
    if let Some((s, t)) = target {
        match game.cast(s.id, t) {
            Ok(done) => {
                cue(Cue::CastSpell);
                if matches!(done.outcome, CastOutcome::Done { .. }) {
                    cue(if matches!(t, CastTarget::Own) { Cue::SpellGood } else { Cue::SpellEvil });
                }
                *message = Some(match done.outcome {
                    CastOutcome::Done { hits, killed, destroyed } => {
                        let mut m = s.name.clone();
                        if hits != 0 {
                            m += &trf!(": {hits} hits", hits = format!("{hits:+}"));
                        }
                        if killed > 0 {
                            m += &trf!(", {killed} fell", killed);
                        }
                        if destroyed {
                            m += tr(". The army is no more.");
                        } else if magic::is_lasting(&s) {
                            m += &format!(" ({})", duration_text(&s));
                        }
                        m
                    }
                    CastOutcome::Interrupted => tr("An enemy fell on you while you were casting: the spell is lost.").into(),
                    CastOutcome::TargetLost => tr("The target got away before the spell was ready.").into(),
                    CastOutcome::OutOfMana => tr("Not enough mana left when the spell was ready.").into(),
                });
                next = world_view::handle_events(game, done.events, message, dialogs).or(Some(Screen::WorldMap));
            }
            Err(e) => *message = Some(cast_error(e).into()),
        }
    }
    if closed || key(KeyCode::Escape) || key(KeyCode::B) {
        next = next.or(Some(Screen::WorldMap));
    }
    if let Some(m) = message.as_ref().filter(|_| next.is_none()) {
        chrome::shadow_centered(m, r.center().x, r.y + r.h - 8.0 * k, 14.0 * k, ACCENT);
    }
    next.or(bar_pick)
}
