use macroquad::prelude::*;

use razdor::rules::battle::Team;
use razdor::rules::formation::{Row, Slot, COLS};
use razdor::rules::game::{Arrival, Game, HireError, MAX_SQUAD};
use razdor::rules::units::UnitKind;
use razdor::rules::world::LocationKind;

use super::assets::Assets;
use super::battle_view::BattleView;
use super::widgets::*;
use super::Screen;

fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
}

fn stat_lines(kind: UnitKind) -> [String; 3] {
    let s = kind.stats();
    [
        format!("HP {}   Armor {}", s.max_hp, s.armor),
        kind.describe_attack(),
        format!("Initiative {}", s.initiative),
    ]
}

pub fn class_select(game: &mut Option<Game>, assets: &Assets) -> Option<Screen> {
    clear_background(Color::from_rgba(24, 22, 20, 255));
    text_centered("RAZDOR", screen_width() / 2.0, 110.0, 72.0, ACCENT);
    text_centered("A time of discord. Choose who you are.", screen_width() / 2.0, 150.0, 26.0, DIM);

    let (w, h, gap) = (280.0, 330.0, 30.0);
    let x0 = (screen_width() - (3.0 * w + 2.0 * gap)) / 2.0;
    for (i, kind) in UnitKind::HEROES.into_iter().enumerate() {
        let x = x0 + i as f32 * (w + gap);
        let y = 200.0;
        let hover = mouse_in(x, y, w, h);
        draw_rectangle(x, y, w, h, PANEL);
        draw_rectangle_lines(x, y, w, h, 2.0, if hover { ACCENT } else { DIM });
        assets.draw_unit(kind, Team::Player, x + w / 2.0, y + 80.0, 96.0);
        text_centered(kind.name(), x + w / 2.0, y + 170.0, 34.0, INK);
        for (j, line) in stat_lines(kind).iter().enumerate() {
            text_centered(line, x + w / 2.0, y + 210.0 + j as f32 * 26.0, 21.0, DIM);
        }
        text_centered(&format!("{} gold", kind.starting_gold()), x + w / 2.0, y + 300.0, 24.0, ACCENT);
        if hover && clicked() {
            *game = Some(Game::new(kind, seed()));
            return Some(Screen::WorldMap);
        }
    }
    None
}

fn top_bar(game: &Game) {
    draw_rectangle(0.0, 0.0, screen_width(), 44.0, PANEL);
    let loc = &game.world.locations[game.location];
    text(&format!("Day {}", game.day), 16.0, 29.0, 26.0, INK);
    text(&format!("{} gold", game.gold), 120.0, 29.0, 26.0, ACCENT);
    text(loc.name, 270.0, 29.0, 26.0, INK);
}

/// Squad shown as its 2×6 battle formation (front row on top). Returns the panel height.
fn squad_panel(game: &Game, assets: &Assets, x: f32, y: f32) -> f32 {
    const CELL: f32 = 36.0;
    let h = 40.0 + 2.0 * (CELL + 14.0) + 30.0;
    draw_rectangle(x, y, 240.0, h, PANEL);
    text(&format!("Squad {}/{}", game.squad.len(), MAX_SQUAD), x + 12.0, y + 28.0, 24.0, INK);
    let mut hovered = None;
    for (r, row) in [Row::Front, Row::Back].into_iter().enumerate() {
        for col in 0..COLS {
            let (cx, cy) = (x + 6.0 + col as f32 * (CELL + 1.0), y + 40.0 + r as f32 * (CELL + 14.0));
            draw_rectangle_lines(cx, cy, CELL, CELL, 1.0, DIM);
            let Some(u) = game.squad.iter().find(|u| u.slot == Slot::new(row, col)) else { continue };
            assets.draw_unit(u.kind, Team::Player, cx + CELL / 2.0, cy + CELL / 2.0, CELL);
            hp_bar(cx + 2.0, cy + CELL + 3.0, CELL - 4.0, u.hp, u.kind.stats().max_hp);
            if mouse_in(cx, cy, CELL, CELL) {
                hovered = Some(u);
            }
        }
    }
    let info = match hovered {
        Some(u) => format!("{} {}/{}", u.kind.name(), u.hp, u.kind.stats().max_hp),
        None => "front row / back row".to_string(),
    };
    text(&info, x + 12.0, y + h - 10.0, 18.0, DIM);
    h
}

fn message_line(message: &Option<String>) {
    if let Some(m) = message {
        let w = measure_text(m, None, 24, 1.0).width + 40.0;
        let x = (screen_width() - w) / 2.0;
        draw_rectangle(x, screen_height() - 60.0, w, 40.0, PANEL);
        text_centered(m, screen_width() / 2.0, screen_height() - 33.0, 24.0, ACCENT);
    }
}

pub fn world_map(game: &mut Game, assets: &Assets, message: &mut Option<String>) -> Option<Screen> {
    clear_background(Color::from_rgba(52, 70, 44, 255));
    let (mx0, my0, mw, mh) = (20.0, 60.0, screen_width() - 290.0, screen_height() - 140.0);
    let to_screen = |p: (f32, f32)| (mx0 + p.0 * mw, my0 + p.1 * mh);

    for &(a, b) in &game.world.roads {
        let (ax, ay) = to_screen(game.world.locations[a].pos);
        let (bx, by) = to_screen(game.world.locations[b].pos);
        draw_line(ax, ay, bx, by, 6.0, Color::from_rgba(150, 125, 80, 255));
    }

    let mut next = None;
    for (i, loc) in game.world.locations.iter().enumerate() {
        let (x, y) = to_screen(loc.pos);
        let reachable = game.world.connected(game.location, i);
        let hover = reachable && (vec2(x, y) - Vec2::from(mouse_position())).length() < 34.0;
        let fill = match (&loc.kind, loc.cleared) {
            (LocationKind::Town { .. }, _) => Color::from_rgba(200, 170, 110, 255),
            (LocationKind::Camp { .. }, false) => Color::from_rgba(170, 50, 40, 255),
            (LocationKind::Camp { .. }, true) => Color::from_rgba(100, 100, 100, 255),
        };
        if hover {
            draw_circle(x, y, 38.0, ACCENT);
        } else if reachable {
            draw_circle_lines(x, y, 36.0, 3.0, ACCENT);
        }
        match loc.kind {
            LocationKind::Town { .. } => draw_rectangle(x - 26.0, y - 26.0, 52.0, 52.0, fill),
            LocationKind::Camp { .. } => draw_poly(x, y, 3, 30.0, -90.0, fill),
        }
        let label = if loc.cleared { format!("{} (cleared)", loc.name) } else { loc.name.to_string() };
        text_centered(&label, x, y + 56.0, 24.0, INK);
        if hover && clicked() {
            next = Some(i);
        }
    }

    let (hx, hy) = to_screen(game.world.locations[game.location].pos);
    assets.draw_unit(game.hero().kind, Team::Player, hx + 30.0, hy - 30.0, 40.0);

    top_bar(game);
    let by = 60.0 + squad_panel(game, assets, screen_width() - 260.0, 60.0) + 16.0;
    let in_town = matches!(game.world.locations[game.location].kind, LocationKind::Town { .. });
    if button(screen_width() - 260.0, by, 240.0, 44.0, "Enter town", in_town) {
        *message = None;
        return Some(Screen::Town);
    }
    text("Click a highlighted", screen_width() - 260.0, by + 70.0, 18.0, DIM);
    text("location to travel.", screen_width() - 260.0, by + 90.0, 18.0, DIM);
    message_line(message);

    let dest = next?;
    *message = None;
    match game.travel(dest) {
        Ok(Arrival::Town) => Some(Screen::Town),
        Ok(Arrival::Battle) => Some(Screen::Battle(Box::new(BattleView::new(game.start_battle())))),
        Ok(Arrival::Cleared) => {
            *message = Some("Only ashes remain here.".into());
            None
        }
        Err(_) => None,
    }
}

pub fn town(game: &mut Game, assets: &Assets, message: &mut Option<String>) -> Option<Screen> {
    clear_background(Color::from_rgba(40, 32, 26, 255));
    top_bar(game);
    let name = game.world.locations[game.location].name;
    text(&format!("{name}: recruits"), 30.0, 90.0, 34.0, INK);
    text("Your squad rests here and is fully healed.", 30.0, 118.0, 20.0, DIM);

    let recruits = game.recruits_here().to_vec();
    for (i, kind) in recruits.into_iter().enumerate() {
        let (x, y) = (30.0, 140.0 + i as f32 * 130.0);
        draw_rectangle(x, y, 620.0, 116.0, PANEL);
        assets.draw_unit(kind, Team::Player, x + 58.0, y + 58.0, 80.0);
        text(kind.name(), x + 115.0, y + 34.0, 28.0, INK);
        for (j, line) in stat_lines(kind).iter().enumerate() {
            text(line, x + 115.0, y + 60.0 + j as f32 * 22.0, 20.0, DIM);
        }
        let label = format!("Hire {}g", kind.cost());
        if button(x + 470.0, y + 36.0, 130.0, 44.0, &label, true) {
            *message = Some(match game.hire(kind) {
                Ok(()) => format!("{} joins your squad.", kind.name()),
                Err(HireError::NotEnoughGold) => "Not enough gold.".into(),
                Err(HireError::SquadFull) => "Your squad is full.".into(),
                Err(HireError::NotOffered) => "Not offered here.".into(),
            });
        }
    }

    let by = 60.0 + squad_panel(game, assets, screen_width() - 260.0, 60.0) + 16.0;
    message_line(message);
    if button(screen_width() - 260.0, by, 240.0, 44.0, "Leave town", true) {
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
    let day = game.as_ref().map_or(0, |g| g.day);
    end_screen("Your hero has fallen", &format!("The discord goes on. You lasted {day} days."), RED, game)
}

pub fn victory(game: &mut Option<Game>) -> Option<Screen> {
    let day = game.as_ref().map_or(0, |g| g.day);
    end_screen("The bandits are broken", &format!("Peace returns to the land on day {day}."), ACCENT, game)
}
