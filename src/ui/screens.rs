use macroquad::prelude::*;

use std::sync::Arc;

use razdor::rules::battle::Team;
use razdor::rules::content::{Content, HeroClass, Stat, UnitId};
use razdor::rules::formation::Slot;
use razdor::rules::game::Game;
use razdor::rules::units::Stats;

use super::assets::Assets;
use super::widgets::*;
use super::{ScenarioEntry, Screen};

fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
}

/// The attack line of a card: melee `A`, ranged `S` or magic `Pwr` with the school.
pub(super) fn attack_line(s: &Stats) -> String {
    let mut parts = Vec::new();
    if s.is_warrior() {
        parts.push(format!("attack {}", s[Stat::AttackBlow]));
    }
    if s.is_shooter() {
        parts.push(format!("shot {}", s[Stat::AttackShot]));
    }
    if s.is_mage() {
        let school = s.magic.map_or(String::new(), |m| format!("{m:?} "));
        parts.push(format!("{school}magic {}", s[Stat::MagicPower]));
    }
    if parts.is_empty() {
        parts.push("no attack".into());
    }
    parts.join(", ")
}

pub(super) fn stat_lines(content: &Content, kind: UnitId) -> [String; 3] {
    let s = Stats::of_level(content, kind, 1);
    [
        format!("Hits {}   Defence {}/{}", s.max_hp(), s[Stat::DefenceBlow], s[Stat::DefenceShot]),
        format!("{}: {}", s.role(), attack_line(&s)),
        format!("Initiative {}   Actions {}", s[Stat::Initiative], s[Stat::Manevres]),
    ]
}

/// First screen: the built-in demo or one of the install's maps (title and description are
/// read from the player's files at runtime).
pub fn scenario_select(scenarios: &[ScenarioEntry], has_install: bool) -> Option<Screen> {
    clear_background(Color::from_rgba(24, 22, 20, 255));
    text_centered("RAZDOR", screen_width() / 2.0, 80.0, 64.0, ACCENT);
    text_centered("Choose a scenario", screen_width() / 2.0, 114.0, 24.0, DIM);
    let (x, w) = (60.0, screen_width() - 120.0);
    let row_h = 52.0;
    let top = 140.0;
    let cols = if scenarios.len() > 10 { 2 } else { 1 };
    let col_w = (w - 20.0 * (cols - 1) as f32) / cols as f32;
    let mut hovered = None;
    let mut entries: Vec<(Option<usize>, String, String)> =
        vec![(None, "Built-in demo: the bandit kingdom".into(), "Our own small map and units. Clear both bandit camps.".into())];
    entries.extend(scenarios.iter().enumerate().map(|(i, e)| {
        let title = if e.scenario.title.trim().is_empty() { e.file.clone() } else { e.scenario.title.clone() };
        let size = format!("{}×{}", e.scenario.width(), e.scenario.height());
        (Some(i), format!("{title}  ({size})"), e.scenario.description.clone())
    }));
    let per_col = entries.len().div_ceil(cols);
    for (k, (idx, title, desc)) in entries.iter().enumerate() {
        let (c, r) = (k / per_col, k % per_col);
        let (ex, ey) = (x + c as f32 * (col_w + 20.0), top + r as f32 * (row_h + 6.0));
        let hover = mouse_in(ex, ey, col_w, row_h);
        draw_rectangle(ex, ey, col_w, row_h, PANEL);
        draw_rectangle_lines(ex, ey, col_w, row_h, 2.0, if hover { ACCENT } else { DIM });
        text(title, ex + 12.0, ey + 22.0, 20.0, if idx.is_none() { ACCENT } else { INK });
        let first = wrap(desc, col_w - 24.0, 16.0).into_iter().next().unwrap_or_default();
        text(&first, ex + 12.0, ey + 42.0, 16.0, DIM);
        if hover {
            hovered = Some((*idx, desc.clone()));
            if clicked() {
                return Some(Screen::ClassSelect { scenario: *idx });
            }
        }
    }
    let y = screen_height() - 110.0;
    if let Some((Some(_), desc)) = hovered {
        for (i, line) in wrap(&desc, w, 18.0).iter().take(4).enumerate() {
            text(line, x, y + i as f32 * 22.0, 18.0, INK);
        }
    } else if !has_install {
        let hint = "Set RAZDOR_DT_DIR to your Discord Times install to play its scenarios.";
        text_centered(hint, screen_width() / 2.0, y + 20.0, 20.0, DIM);
    }
    None
}

/// Hero class: for the demo its own classes, for a scenario the map's three presets.
pub fn class_select(
    game: &mut Option<Game>,
    demo: &Arc<Content>,
    scenario: Option<(&ScenarioEntry, Arc<Content>)>,
    assets: &Assets,
) -> Option<Screen> {
    clear_background(Color::from_rgba(24, 22, 20, 255));
    let content = scenario.as_ref().map_or(demo.clone(), |(_, c)| c.clone());
    let title = scenario.as_ref().map_or("A time of discord".to_string(), |(e, _)| e.scenario.title.clone());
    text_centered(&title, screen_width() / 2.0, 100.0, 44.0, ACCENT);
    text_centered("Choose who you are.", screen_width() / 2.0, 140.0, 26.0, DIM);
    if let Some((e, _)) = &scenario {
        if e.scenario.header.scenario_kind == 2 {
            let note = "A later campaign map: the original carries gold and army over from the previous one.";
            text_centered(note, screen_width() / 2.0, 168.0, 18.0, DIM);
        }
    }

    let (w, h, gap) = (300.0, 380.0, 30.0);
    let x0 = (screen_width() - (3.0 * w + 2.0 * gap)) / 2.0;
    for (i, hero) in HeroClass::ALL.into_iter().enumerate() {
        let kind = hero.unit();
        let x = x0 + i as f32 * (w + gap);
        let y = 180.0;
        let hover = mouse_in(x, y, w, h);
        draw_rectangle(x, y, w, h, PANEL);
        draw_rectangle_lines(x, y, w, h, 2.0, if hover { ACCENT } else { DIM });
        assets.draw_unit(kind, Team::Player, x + w / 2.0, y + 80.0, 96.0);
        text_centered(&content.unit(kind).name, x + w / 2.0, y + 170.0, 30.0, INK);
        for (j, line) in stat_lines(&content, kind).iter().enumerate() {
            text_centered(line, x + w / 2.0, y + 205.0 + j as f32 * 24.0, 18.0, DIM);
        }
        match &scenario {
            Some((e, c)) => {
                let preset = &e.scenario.header.heroes[i];
                text_centered(&format!("{} gold", preset.gold), x + w / 2.0, y + 290.0, 24.0, ACCENT);
                let army: Vec<String> = preset
                    .troops
                    .iter()
                    .filter(|t| t.unit != 0 && t.count > 0)
                    .filter_map(|t| c.try_unit(UnitId(t.unit as u32)).map(|u| format!("{} {}", t.count, u.name)))
                    .collect();
                let army = if army.is_empty() { "alone".to_string() } else { army.join(", ") };
                for (j, line) in wrap(&army, w - 20.0, 17.0).iter().take(3).enumerate() {
                    text_centered(line, x + w / 2.0, y + 318.0 + j as f32 * 20.0, 17.0, INK);
                }
            }
            None => text_centered(&format!("{} gold", content.start_gold(hero)), x + w / 2.0, y + 300.0, 24.0, ACCENT),
        }
        if hover && clicked() {
            *game = Some(match &scenario {
                Some((e, c)) => Game::from_scenario(c.clone(), &e.scenario, hero, seed()),
                None => Game::new(content.clone(), hero, seed()),
            });
            return Some(Screen::WorldMap);
        }
    }
    if button(30.0, screen_height() - 70.0, 160.0, 44.0, "Back", true) {
        return Some(Screen::ScenarioSelect);
    }
    None
}

pub(super) fn top_bar(game: &Game) {
    draw_rectangle(0.0, 0.0, screen_width(), 44.0, PANEL);
    text(&game.clock.label(), 16.0, 29.0, 24.0, INK);
    text(&format!("{} gold", game.gold), 330.0, 29.0, 24.0, ACCENT);
    text(&format!("{} mana", game.mana), 470.0, 29.0, 24.0, Color::new(0.5, 0.7, 1.0, 1.0));
}

/// Squad shown as its battle formation (front row on top). Returns the panel height.
pub(super) fn squad_panel(game: &Game, assets: &Assets, x: f32, y: f32) -> f32 {
    const CELL: f32 = 36.0;
    let formation = game.content.formation;
    let rows = formation.rows();
    let h = 40.0 + rows.len() as f32 * (CELL + 14.0) + 30.0;
    draw_rectangle(x, y, 240.0, h, PANEL);
    text(&format!("Squad {}/{}", game.squad.len(), game.max_squad()), x + 12.0, y + 28.0, 24.0, INK);
    let mut hovered = None;
    let cell = (228.0 / formation.cols as f32 - 1.0).min(CELL);
    for (r, &row) in rows.iter().enumerate() {
        for col in 0..formation.cols {
            let (cx, cy) = (x + 6.0 + col as f32 * (cell + 1.0), y + 40.0 + r as f32 * (CELL + 14.0));
            draw_rectangle_lines(cx, cy, cell, CELL, 1.0, DIM);
            let Some(i) = game.squad.iter().position(|u| u.slot == Slot::new(row, col)) else { continue };
            let u = &game.squad[i];
            assets.draw_unit(u.def, Team::Player, cx + cell / 2.0, cy + CELL / 2.0, CELL);
            if !u.alive() {
                draw_rectangle(cx, cy, cell, CELL, Color::new(0.0, 0.0, 0.0, 0.6));
                text_centered("+", cx + cell / 2.0, cy + CELL / 2.0 + 7.0, 26.0, RED);
            } else if u.unpaid {
                draw_rectangle(cx, cy, cell, CELL, Color::new(0.0, 0.0, 0.0, 0.6));
                text_centered("$", cx + cell / 2.0, cy + CELL / 2.0 + 7.0, 22.0, RED);
            }
            hp_bar(cx + 2.0, cy + CELL + 3.0, cell - 4.0, u.hp, u.max_hp(&game.content));
            if mouse_in(cx, cy, cell, CELL) {
                hovered = Some(i);
            }
        }
    }
    let c = &game.content;
    let info = match hovered.map(|i| (i, &game.squad[i])) {
        Some((_, u)) if !u.alive() => format!("{} (dead)", u.name(c)),
        Some((_, u)) if u.unpaid => format!("{} unpaid!", u.name(c)),
        Some((i, u)) => format!("{} L{} {}/{}  {}g/day", u.name(c), u.level, u.hp, u.max_hp(c), game.wage(i)),
        None if formation.reserve => "front / back / reserve".to_string(),
        None => "front row / back row".to_string(),
    };
    text(&info, x + 12.0, y + h - 10.0, 18.0, DIM);
    h
}

pub(super) fn message_line(message: &Option<String>) {
    if let Some(m) = message {
        let w = measure(m, 24.0).width + 40.0;
        let x = (screen_width() - w) / 2.0;
        draw_rectangle(x, screen_height() - 60.0, w, 40.0, PANEL);
        text_centered(m, screen_width() / 2.0, screen_height() - 33.0, 24.0, ACCENT);
    }
}

fn end_screen(title: &str, subtitle: &str, color: Color, game: &mut Option<Game>) -> Option<Screen> {
    clear_background(Color::from_rgba(20, 18, 16, 255));
    text_centered(title, screen_width() / 2.0, 260.0, 64.0, color);
    text_centered(subtitle, screen_width() / 2.0, 310.0, 26.0, DIM);
    if button(screen_width() / 2.0 - 110.0, 380.0, 220.0, 50.0, "New game", true) {
        *game = None;
        return Some(Screen::ScenarioSelect);
    }
    None
}

/// Whole days since the game started.
fn days_played(game: &Option<Game>) -> u64 {
    game.as_ref().map_or(0, |g| g.clock.day_index().saturating_sub(g.world.start.day_index()))
}

pub fn game_over(game: &mut Option<Game>) -> Option<Screen> {
    let days = days_played(game);
    end_screen("Your hero has fallen", &format!("The discord goes on. You lasted {days} days."), RED, game)
}

pub fn victory(game: &mut Option<Game>) -> Option<Screen> {
    let days = days_played(game);
    end_screen("The bandits are broken", &format!("Peace returns to the land after {days} days."), ACCENT, game)
}
