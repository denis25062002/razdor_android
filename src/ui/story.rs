//! What the scenario's event engine did, on screen: story and quest dialogs (video notes
//! §5), the "added to the journal" / "completed" notices, and the quest journal.
//!
//! Every text comes from the loaded scenario at runtime (with `#HERONAME` filled in).

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::rules::content::{ItemId, UnitId};
use razdor::rules::events::{extension, EventId, EventOutcome, Extension, PICTURE_DEFEAT, PICTURE_VICTORY};
use razdor::rules::game::Game;

use super::dialog::{Dialog, Picture, Resource};
use super::widgets::*;
use super::world_view;
use super::Screen;

pub const QUEST_ADDED: &str = "Quest added to the journal";
pub const QUEST_COMPLETED: &str = "Quest completed";

/// An event's title as shown: without its flag script, escapes filled in.
pub fn event_title(game: &Game, id: EventId) -> String {
    let t = game.script().and_then(|s| s.event(id)).map_or("", |e| e.title_text().trim());
    if t.is_empty() {
        "Event".to_string()
    } else {
        game.fill_text(t)
    }
}

/// Paragraphs of a scenario text.
fn paragraphs(game: &Game, s: &str) -> Vec<String> {
    game.fill_text(s).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// An event's own picture: `width`, `height` (u16 each) and 16-bit pixels, taken as RGB565
/// *(guess, dtm-format.md §9)*.
fn custom_picture(data: &[u8]) -> Option<Texture2D> {
    let w = u16::from_le_bytes([*data.first()?, *data.get(1)?]);
    let h = u16::from_le_bytes([*data.get(2)?, *data.get(3)?]);
    let px = data.get(4..4 + 2 * w as usize * h as usize)?;
    if w == 0 || h == 0 {
        return None;
    }
    let rgba: Vec<u8> = px
        .chunks_exact(2)
        .flat_map(|p| {
            let v = u16::from_le_bytes([p[0], p[1]]);
            let (r, g, b) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
            [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8, 255]
        })
        .collect();
    let tex = Texture2D::from_rgba8(w, h, &rgba);
    tex.set_filter(FilterMode::Nearest);
    Some(tex)
}

/// The window of event `id`: its question (`asking`), or its message with what it gave.
pub fn event_dialog(game: &Game, id: EventId, asking: bool) -> Dialog {
    let mut d = Dialog::new(event_title(game, id));
    d.event = Some(id);
    d.question = asking;
    let Some(e) = game.script().and_then(|s| s.event(id)) else { return d };
    let body = if asking && !e.question.trim().is_empty() { &e.question } else { &e.message };
    d.text = paragraphs(game, body);
    let r = &e.results;
    d.picture = match (&e.custom_picture, r.picture) {
        (Some(data), _) if custom_picture(data).is_some() => custom_picture(data).map(Picture::Image),
        (_, 0 | PICTURE_DEFEAT | PICTURE_VICTORY) => None,
        (_, u) => game.content.try_unit(UnitId(u as u32)).map(|_| Picture::Unit(UnitId(u as u32))),
    };
    // Opcodes use the resource fields as arguments: nothing was given.
    if asking || matches!(extension(e), Some(Extension::Opcode(_))) {
        return d;
    }
    let signed = |v: i16| if v < 0 { format!("- {}", -(v as i32)) } else { format!("+ {v}") };
    if r.gold != 0 {
        d.resources.push((Resource::Gold, format!("Gold {}", signed(r.gold))));
    }
    if r.mana != 0 {
        d.resources.push((Resource::Mana, format!("Mana {}", signed(r.mana))));
    }
    if r.experience != 0 {
        d.resources.push((Resource::Experience, format!("Experience {}", signed(r.experience))));
    }
    d.items = r.artifacts_add.iter().filter(|&&a| a != 0).map(|&a| ItemId(a as u32)).filter(|&i| game.content.try_item(i).is_some()).collect();
    let known = |u: u8| game.content.try_unit(UnitId(u as u32)).map(|_| UnitId(u as u32));
    d.joined = r.units_add.iter().filter_map(|&u| known(u)).collect();
    d.left = r.units_remove.iter().filter(|&&u| u != 0xFE && u != 0xFF).filter_map(|&u| known(u)).collect();
    d
}

/// Puts an outcome on screen: a dialog, a question, a notice on the dialog just queued (or
/// the message line).
pub fn show(game: &Game, o: &EventOutcome, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) {
    match *o {
        EventOutcome::Fired { event, message: true } => dialogs.push_back(event_dialog(game, event, false)),
        EventOutcome::Question(id) => dialogs.push_back(event_dialog(game, id, true)),
        EventOutcome::QuestAdded(id) => notice(QUEST_ADDED, Some(id), message, dialogs),
        EventOutcome::QuestCompleted(_) => notice(QUEST_COMPLETED, None, message, dialogs),
        EventOutcome::LoopGuard => eprintln!("scenario events: loop guard reached"),
        EventOutcome::Fired { .. } | EventOutcome::Declined(_) | EventOutcome::Victory(_) | EventOutcome::Defeat(_) => {}
    }
}

/// The notice goes on the last event dialog queued (the event that gave or closed the
/// quest), if it is that one; the message line shows it too.
fn notice(line: &str, event: Option<EventId>, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) {
    if let Some(d) = dialogs.back_mut().filter(|d| d.event.is_some() && !d.question && (event.is_none() || d.event == event)) {
        d.add_notice(line);
    }
    *message = Some(line.to_string());
}

/// The journal: active quests, then those completed; the selected one's text on the right.
pub fn journal(game: &Game, assets: &super::assets::Assets, selected: &mut usize) -> Option<Screen> {
    world_view::backdrop_lit(game, assets, Some(super::game_bar::BarButton::Journal));
    let (sw, sh) = (screen_width(), screen_height());
    let bar = super::chrome::bar_height();
    let (w, h) = (1000.0f32.min(sw - 20.0), 640.0f32.min(sh - bar - 8.0));
    let (x, y) = ((sw - w) / 2.0, ((sh - bar - h) / 2.0).max(4.0));
    super::chrome::window(Rect::new(x, y, w, h), "The hero's journal", super::chrome::Skin::Marble, false);

    let (active, done): (Vec<EventId>, Vec<EventId>) =
        game.script().map_or((Vec::new(), Vec::new()), |s| (s.journal().to_vec(), s.completed_quests().to_vec()));
    let rows: Vec<(EventId, bool)> = active.iter().map(|&q| (q, false)).chain(done.iter().rev().map(|&q| (q, true))).collect();
    *selected = (*selected).min(rows.len().saturating_sub(1));

    // The list.
    let (lx, ly, lw) = (x + 16.0, y + 44.0, 360.0);
    let lh = h - 110.0;
    super::chrome::parchment(Rect::new(lx, ly, lw, lh), false);
    let ink = Color::new(0.45, 0.28, 0.14, 1.0);
    let mut ry = ly + 8.0;
    let header = |label: &str, ry: &mut f32| {
        text(label, lx + 10.0, *ry + 18.0, 19.0, Color::new(0.55, 0.1, 0.1, 1.0));
        *ry += 28.0;
    };
    header(&format!("Active quests ({})", active.len()), &mut ry);
    if active.is_empty() {
        text("None yet.", lx + 20.0, ry + 16.0, 17.0, ink);
        ry += 26.0;
    }
    for (k, &(q, finished)) in rows.iter().enumerate() {
        if finished && (k == 0 || !rows[k - 1].1) {
            ry += 8.0;
            header(&format!("Completed ({})", done.len()), &mut ry);
        }
        if ry > ly + lh - 26.0 {
            break;
        }
        let sel = *selected == k;
        if sel {
            draw_rectangle(lx + 4.0, ry - 2.0, lw - 8.0, 26.0, Color::new(0.72, 0.6, 0.4, 1.0));
        }
        let title: String = event_title(game, q).chars().take(34).collect();
        text(&title, lx + 20.0, ry + 17.0, 18.0, if finished { Color::new(0.4, 0.36, 0.3, 1.0) } else { ink });
        if mouse_in(lx, ry - 2.0, lw, 26.0) && clicked() {
            *selected = k;
        }
        ry += 26.0;
    }

    // The selected quest's text.
    let (tx, tw) = (lx + lw + 16.0, w - lw - 48.0);
    super::chrome::text_box(Rect::new(tx, ly, tw, lh));
    let box_ink = Color::new(1.0, 0.86, 0.58, 1.0);
    match rows.get(*selected) {
        Some(&(q, finished)) => {
            text_centered(&event_title(game, q), tx + tw / 2.0, ly + 30.0, 21.0, box_ink);
            if finished {
                text_centered(QUEST_COMPLETED, tx + tw / 2.0, ly + 54.0, 17.0, super::dialog::MANA);
            }
            let body = game.script().and_then(|s| s.event(q)).map_or(String::new(), |e| e.message.clone());
            let mut ty = ly + 84.0;
            for para in paragraphs(game, &body) {
                for line in wrap(&para, tw - 40.0, 18.0) {
                    if ty > ly + lh - 10.0 {
                        break;
                    }
                    text(&line, tx + 20.0, ty, 18.0, box_ink);
                    ty += 22.0;
                }
                ty += 6.0;
            }
        }
        None => text_centered("Quests you take appear here.", tx + tw / 2.0, ly + lh / 2.0, 19.0, box_ink),
    }

    let back = button(x + w / 2.0 - 70.0, y + h - 54.0, 140.0, 40.0, "Back", true);
    if back || key(KeyCode::Escape) || key(KeyCode::J) {
        return Some(Screen::WorldMap);
    }
    if key(KeyCode::Down) {
        *selected += 1;
    }
    if key(KeyCode::Up) {
        *selected = selected.saturating_sub(1);
    }
    None
}
