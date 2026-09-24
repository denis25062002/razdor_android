use macroquad::prelude::*;

use razdor::rules::battle::{Battle, Outcome, Pos, Team, GRID_H, GRID_W};
use razdor::rules::game::{BattleResult, Game};
use razdor::rules::units::AttackKind;

use super::assets::{team_color, Assets};
use super::widgets::*;
use super::Screen;

const CELL: f32 = 64.0;
const OX: f32 = 24.0;
const OY: f32 = 72.0;
const AI_DELAY: f32 = 0.35;
const WALK_SPEED: f32 = 9.0; // cells per second

struct Walk {
    id: usize,
    path: Vec<Pos>,
    t: f32,
}

pub struct BattleView {
    battle: Battle,
    walk: Option<Walk>,
    ai_timer: f32,
}

fn cell_center(p: Pos) -> Vec2 {
    vec2(OX + (p.0 as f32 + 0.5) * CELL, OY + (p.1 as f32 + 0.5) * CELL)
}

fn cell_under_mouse() -> Option<Pos> {
    let (mx, my) = mouse_position();
    let (cx, cy) = (((mx - OX) / CELL).floor() as i32, ((my - OY) / CELL).floor() as i32);
    (cx >= 0 && cy >= 0 && cx < GRID_W && cy < GRID_H).then_some((cx, cy))
}

impl BattleView {
    pub fn new(battle: Battle) -> Self {
        BattleView { battle, walk: None, ai_timer: 0.0 }
    }

    fn start_walk(&mut self, id: usize, path: Vec<Pos>) {
        if path.len() > 1 {
            self.walk = Some(Walk { id, path, t: 0.0 });
        }
    }

    fn drawn_pos(&self, id: usize) -> Vec2 {
        match &self.walk {
            Some(w) if w.id == id => {
                let seg = (w.t.floor() as usize).min(w.path.len() - 2);
                let frac = (w.t - seg as f32).clamp(0.0, 1.0);
                cell_center(w.path[seg]).lerp(cell_center(w.path[seg + 1]), frac)
            }
            _ => cell_center(self.battle.fighters[id].pos),
        }
    }

    pub fn frame(&mut self, game: &mut Game, assets: &Assets, message: &mut Option<String>) -> Option<Screen> {
        let dt = get_frame_time();
        if let Some(w) = &mut self.walk {
            w.t += dt * WALK_SPEED;
            if w.t >= (w.path.len() - 1) as f32 {
                self.walk = None;
            }
        }

        let outcome = self.battle.outcome();
        let busy = self.walk.is_some() || outcome != Outcome::Ongoing;
        let active = self.battle.active();
        let player_turn = self.battle.fighters[active].team == Team::Player;

        if !busy {
            if player_turn {
                self.player_input(active);
            } else {
                self.ai_timer += dt;
                if self.ai_timer >= AI_DELAY {
                    self.ai_timer = 0.0;
                    let path = self.battle.ai_turn();
                    self.start_walk(active, path);
                }
            }
        }

        self.draw(assets, player_turn && !busy);

        if outcome != Outcome::Ongoing && self.walk.is_none() {
            return self.result_overlay(game, message, outcome);
        }
        None
    }

    fn player_input(&mut self, active: usize) {
        if is_key_pressed(KeyCode::Space) {
            self.battle.skip();
            return;
        }
        if !clicked() {
            return;
        }
        let Some(cell) = cell_under_mouse() else { return };
        if let Some(t) = self.battle.occupant(cell) {
            if self.battle.can_target(active, t) {
                let _ = self.battle.act(t);
            }
        } else if let Ok(path) = self.battle.move_active(cell) {
            self.start_walk(active, path);
        }
    }

    fn draw(&self, assets: &Assets, show_hints: bool) {
        clear_background(Color::from_rgba(30, 34, 28, 255));
        let active = self.battle.active();

        // Grid.
        for x in 0..GRID_W {
            for y in 0..GRID_H {
                let shade = if (x + y) % 2 == 0 { 78 } else { 70 };
                let (px, py) = (OX + x as f32 * CELL, OY + y as f32 * CELL);
                draw_rectangle(px, py, CELL, CELL, Color::from_rgba(shade, shade + 20, shade - 10, 255));
            }
        }

        if show_hints {
            for (&p, _) in self.battle.reachable(active).iter() {
                let (px, py) = (OX + p.0 as f32 * CELL, OY + p.1 as f32 * CELL);
                draw_rectangle(px, py, CELL, CELL, Color::new(1.0, 1.0, 0.6, 0.18));
            }
            for t in self.battle.targets(active) {
                let p = self.battle.fighters[t].pos;
                let (px, py) = (OX + p.0 as f32 * CELL, OY + p.1 as f32 * CELL);
                let c = if self.battle.fighters[t].team == Team::Player { GREEN } else { RED };
                draw_rectangle_lines(px + 2.0, py + 2.0, CELL - 4.0, CELL - 4.0, 4.0, c);
            }
            if let Some(p) = cell_under_mouse() {
                let (px, py) = (OX + p.0 as f32 * CELL, OY + p.1 as f32 * CELL);
                draw_rectangle_lines(px, py, CELL, CELL, 2.0, WHITE);
            }
        }

        // Units.
        for (i, f) in self.battle.fighters.iter().enumerate() {
            if !f.alive() {
                continue;
            }
            let c = self.drawn_pos(i);
            if i == active {
                draw_circle_lines(c.x, c.y, CELL * 0.47, 3.0, ACCENT);
            }
            assets.draw_unit(f.kind, f.team, c.x, c.y - 3.0, CELL * 0.9);
            hp_bar(c.x - CELL * 0.4, c.y + CELL * 0.36, CELL * 0.8, f.hp, f.kind.stats().max_hp);
        }

        self.draw_panel(show_hints);
    }

    fn draw_panel(&self, show_hints: bool) {
        let x = OX + GRID_W as f32 * CELL + 20.0;
        let w = screen_width() - x - 16.0;
        text(&format!("Round {}", self.battle.round), OX, 50.0, 32.0, INK);

        draw_rectangle(x, OY, w, GRID_H as f32 * CELL, PANEL);
        let active = &self.battle.fighters[self.battle.active()];
        let who = if active.team == Team::Player { "Your move" } else { "Enemy move" };
        text(who, x + 12.0, OY + 30.0, 26.0, team_color(active.team));

        // Hovered unit (or the active one) details.
        let shown = cell_under_mouse().and_then(|c| self.battle.occupant(c)).unwrap_or(self.battle.active());
        let f = &self.battle.fighters[shown];
        let s = f.kind.stats();
        text(f.kind.name(), x + 12.0, OY + 62.0, 24.0, INK);
        text(&format!("HP {}/{}  Armor {}", f.hp, s.max_hp, s.armor), x + 12.0, OY + 84.0, 19.0, DIM);
        let attack = match s.attack {
            AttackKind::Heal { .. } => s.attack.describe(),
            _ => format!("Dmg {}-{}  {}", s.dmg_min, s.dmg_max, s.attack.describe()),
        };
        text(&attack, x + 12.0, OY + 104.0, 19.0, DIM);
        text(&format!("Move {}  Init {}", s.moves, s.initiative), x + 12.0, OY + 124.0, 19.0, DIM);

        text("Turn order", x + 12.0, OY + 158.0, 20.0, INK);
        for (n, id) in self.battle.queue().take(6).enumerate() {
            let q = &self.battle.fighters[id];
            let marker = if n == 0 { "> " } else { "  " };
            text(&format!("{marker}{}", q.kind.name()), x + 12.0, OY + 180.0 + n as f32 * 20.0, 18.0, team_color(q.team));
        }

        text("Log (newest first)", x + 12.0, OY + 320.0, 20.0, INK);
        for (n, line) in self.battle.log.iter().rev().take(8).enumerate() {
            text(line, x + 12.0, OY + 342.0 + n as f32 * 20.0, 17.0, DIM);
        }

        if show_hints {
            let hint = if self.battle.has_moved() {
                "Click a target, or Space to end turn"
            } else {
                "Click a cell to move, a target to attack, Space to skip"
            };
            text(hint, OX, OY + GRID_H as f32 * CELL + 30.0, 22.0, DIM);
        }
    }

    fn result_overlay(&self, game: &mut Game, message: &mut Option<String>, outcome: Outcome) -> Option<Screen> {
        let (w, h) = (420.0, 180.0);
        let (x, y) = ((screen_width() - w) / 2.0, (screen_height() - h) / 2.0);
        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.5));
        draw_rectangle(x, y, w, h, PANEL);
        draw_rectangle_lines(x, y, w, h, 2.0, ACCENT);
        let (title, color) = match outcome {
            Outcome::Victory => ("Victory!", ACCENT),
            _ => ("Defeat", RED),
        };
        text_centered(title, x + w / 2.0, y + 64.0, 52.0, color);
        if !button(x + w / 2.0 - 100.0, y + 105.0, 200.0, 48.0, "Continue", true) {
            return None;
        }
        match game.resolve_battle(&self.battle) {
            BattleResult::Defeat => Some(Screen::GameOver),
            BattleResult::Victory { .. } if game.won() => Some(Screen::Victory),
            BattleResult::Victory { reward, lost } => {
                let losses = if lost > 0 { format!(", {lost} fell") } else { String::new() };
                *message = Some(format!("Victory! +{reward} gold{losses}."));
                Some(Screen::WorldMap)
            }
        }
    }
}
