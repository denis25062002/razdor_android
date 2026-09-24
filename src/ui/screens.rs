use macroquad::prelude::*;

use std::sync::Arc;

use razdor::rules::battle::Team;
use razdor::rules::content::{Content, HeroClass, Stat, UnitId};
use razdor::rules::formation::Slot;
use razdor::rules::game::{Game, HireError};
use razdor::rules::units::Stats;

use super::assets::Assets;
use super::widgets::*;
use super::Screen;

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

fn stat_lines(content: &Content, kind: UnitId) -> [String; 3] {
    let s = Stats::of_level(content, kind, 1);
    [
        format!("Hits {}   Defence {}/{}", s.max_hp(), s[Stat::DefenceBlow], s[Stat::DefenceShot]),
        format!("{}: {}", s.role(), attack_line(&s)),
        format!("Initiative {}   Actions {}", s[Stat::Initiative], s[Stat::Manevres]),
    ]
}

pub fn class_select(game: &mut Option<Game>, content: &Arc<Content>, assets: &Assets) -> Option<Screen> {
    clear_background(Color::from_rgba(24, 22, 20, 255));
    text_centered("RAZDOR", screen_width() / 2.0, 110.0, 72.0, ACCENT);
    text_centered("A time of discord. Choose who you are.", screen_width() / 2.0, 150.0, 26.0, DIM);

    let (w, h, gap) = (280.0, 330.0, 30.0);
    let x0 = (screen_width() - (3.0 * w + 2.0 * gap)) / 2.0;
    for (i, hero) in HeroClass::ALL.into_iter().enumerate() {
        let kind = hero.unit();
        let x = x0 + i as f32 * (w + gap);
        let y = 200.0;
        let hover = mouse_in(x, y, w, h);
        draw_rectangle(x, y, w, h, PANEL);
        draw_rectangle_lines(x, y, w, h, 2.0, if hover { ACCENT } else { DIM });
        assets.draw_unit(kind, Team::Player, x + w / 2.0, y + 80.0, 96.0);
        text_centered(&content.unit(kind).name, x + w / 2.0, y + 170.0, 34.0, INK);
        for (j, line) in stat_lines(content, kind).iter().enumerate() {
            text_centered(line, x + w / 2.0, y + 210.0 + j as f32 * 26.0, 19.0, DIM);
        }
        text_centered(&format!("{} gold", content.start_gold(hero)), x + w / 2.0, y + 300.0, 24.0, ACCENT);
        if hover && clicked() {
            *game = Some(Game::new(content.clone(), hero, seed()));
            return Some(Screen::WorldMap);
        }
    }
    None
}

pub(super) fn top_bar(game: &Game) {
    draw_rectangle(0.0, 0.0, screen_width(), 44.0, PANEL);
    text(&game.clock.label(), 16.0, 29.0, 26.0, INK);
    text(&format!("{} gold", game.gold), 330.0, 29.0, 26.0, ACCENT);
    let (state, color) = if game.moving() { ("travelling", INK) } else { ("time stands still", DIM) };
    text(state, 480.0, 29.0, 20.0, color);
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
            if u.unpaid {
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
        let w = measure_text(m, None, 24, 1.0).width + 40.0;
        let x = (screen_width() - w) / 2.0;
        draw_rectangle(x, screen_height() - 60.0, w, 40.0, PANEL);
        text_centered(m, screen_width() / 2.0, screen_height() - 33.0, 24.0, ACCENT);
    }
}

pub fn town(game: &mut Game, assets: &Assets, message: &mut Option<String>) -> Option<Screen> {
    clear_background(Color::from_rgba(40, 32, 26, 255));
    top_bar(game);
    let name = game.location.map_or("Castle", |l| game.world.locations[l].name);
    text(&format!("{name}: recruits"), 30.0, 90.0, 34.0, INK);
    text("Your squad rests here and is fully healed. Recruits need daily pay.", 30.0, 118.0, 20.0, DIM);

    let recruits = game.recruits_here().to_vec();
    for (i, kind) in recruits.into_iter().enumerate() {
        let (x, y) = (30.0, 140.0 + i as f32 * 130.0);
        let c = game.content.clone();
        let name = &c.unit(kind).name;
        draw_rectangle(x, y, 660.0, 116.0, PANEL);
        assets.draw_unit(kind, Team::Player, x + 58.0, y + 58.0, 80.0);
        text(name, x + 115.0, y + 34.0, 28.0, INK);
        for (j, line) in stat_lines(&c, kind).iter().enumerate() {
            text(line, x + 115.0, y + 60.0 + j as f32 * 22.0, 19.0, DIM);
        }
        text(&format!("Wage {} gold/day", c.wage(kind)), x + 330.0, y + 34.0, 20.0, ACCENT);
        let label = format!("Hire {}g", c.unit(kind).cost);
        if button(x + 510.0, y + 36.0, 130.0, 44.0, &label, true) {
            *message = Some(match game.hire(kind) {
                Ok(()) => format!("{name} joins your squad."),
                Err(HireError::NotEnoughGold) => "Not enough gold.".into(),
                Err(HireError::SquadFull) => "Your squad is full.".into(),
                Err(HireError::NotOffered) => "Not offered here.".into(),
            });
        }
    }

    let by = 60.0 + squad_panel(game, assets, screen_width() - 260.0, 60.0) + 16.0;
    message_line(message);
    if button(screen_width() - 260.0, by, 240.0, 44.0, "Market", game.market_here().is_some()) {
        *message = None;
        return Some(Screen::Market);
    }
    if button(screen_width() - 260.0, by + 52.0, 240.0, 44.0, "Squad & gear", true) {
        *message = None;
        return Some(Screen::Squad { selected: 0, from_town: true });
    }
    if button(screen_width() - 260.0, by + 104.0, 240.0, 44.0, "Leave castle", true) {
        *message = None;
        return Some(Screen::WorldMap);
    }
    None
}

fn end_screen(title: &str, subtitle: &str, color: Color, game: &mut Option<Game>) -> Option<Screen> {
    clear_background(Color::from_rgba(20, 18, 16, 255));
    text_centered(title, screen_width() / 2.0, 260.0, 64.0, color);
    text_centered(subtitle, screen_width() / 2.0, 310.0, 26.0, DIM);
    if button(screen_width() / 2.0 - 110.0, 380.0, 220.0, 50.0, "New game", true) {
        *game = None;
        return Some(Screen::ClassSelect);
    }
    None
}

pub fn game_over(game: &mut Option<Game>) -> Option<Screen> {
    let day = game.as_ref().map_or(0, |g| g.clock.day());
    end_screen("Your hero has fallen", &format!("The discord goes on. You lasted {day} days."), RED, game)
}

pub fn victory(game: &mut Option<Game>) -> Option<Screen> {
    let day = game.as_ref().map_or(0, |g| g.clock.day());
    end_screen("The bandits are broken", &format!("Peace returns to the land on day {day}."), ACCENT, game)
}
