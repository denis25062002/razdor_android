use std::f32::consts::PI;

use macroquad::prelude::*;

use razdor::rules::battle::{Battle, Hit, Outcome, Team, MAX_ROUNDS};
use razdor::rules::formation::{Row, Slot, COLS};
use razdor::rules::game::{BattleResult, Game};

use super::assets::{team_color, Assets};
use super::widgets::*;
use super::Screen;

const CARD_W: f32 = 112.0;
const CARD_H: f32 = 132.0;
const PITCH_X: f32 = 122.0;
const PITCH_Y: f32 = 142.0;
/// Empty strip between the two formations.
const MID_GAP: f32 = 40.0;
const OX: f32 = 24.0;
const OY: f32 = 64.0;
const PANEL_X: f32 = OX + COLS as f32 * PITCH_X + 16.0;
const PANEL_H: f32 = 4.0 * PITCH_Y + MID_GAP - 10.0;
const AI_DELAY: f32 = 0.45;
const FX_TIME: f32 = 0.6;

/// A strike being animated: the actor lunges, targets flash and show numbers.
struct Fx {
    actor: usize,
    hits: Vec<Hit>,
    t: f32,
}

pub struct BattleView {
    battle: Battle,
    fx: Option<Fx>,
    ai_timer: f32,
    /// Deploy phase: card picked up to move.
    selected: Option<Slot>,
}

/// Screen rows top to bottom: enemy back, enemy front, player front, player back.
fn cell_pos(team: Team, slot: Slot) -> Vec2 {
    let line = match (team, slot.row) {
        (Team::Enemy, Row::Back) => 0,
        (Team::Enemy, Row::Front) => 1,
        (Team::Player, Row::Front) => 2,
        (Team::Player, Row::Back) => 3,
    };
    let gap = if line >= 2 { MID_GAP } else { 0.0 };
    vec2(OX + slot.col as f32 * PITCH_X, OY + line as f32 * PITCH_Y + gap)
}

fn all_cells() -> impl Iterator<Item = (Team, Slot)> {
    [Team::Enemy, Team::Player].into_iter().flat_map(|t| Slot::all().map(move |s| (t, s)))
}

fn cell_under_mouse() -> Option<(Team, Slot)> {
    all_cells().find(|&(t, s)| {
        let p = cell_pos(t, s);
        mouse_in(p.x, p.y, CARD_W, CARD_H)
    })
}

impl BattleView {
    pub fn new(battle: Battle) -> Self {
        BattleView { battle, fx: None, ai_timer: 0.0, selected: None }
    }

    fn fighter_under_mouse(&self) -> Option<usize> {
        let (team, slot) = cell_under_mouse()?;
        self.battle.at(team, slot)
    }

    pub fn frame(&mut self, game: &mut Game, assets: &Assets, message: &mut Option<String>) -> Option<Screen> {
        let dt = get_frame_time();
        if let Some(fx) = &mut self.fx {
            fx.t += dt;
            if fx.t >= FX_TIME {
                self.fx = None;
            }
        }

        if self.battle.is_deploying() {
            self.deploy_input();
        } else if self.fx.is_none() {
            if let Some(active) = self.battle.active() {
                if self.battle.fighters[active].team == Team::Player {
                    self.player_input(active);
                } else {
                    self.ai_timer += dt;
                    if self.ai_timer >= AI_DELAY {
                        self.ai_timer = 0.0;
                        if let Some((actor, hits)) = self.battle.ai_turn() {
                            self.fx = Some(Fx { actor, hits, t: 0.0 });
                        }
                    }
                }
            }
        }

        self.draw(assets);

        if self.battle.is_deploying() && button(PANEL_X + 12.0, OY + 290.0, 200.0, 48.0, "Fight!", true) {
            self.selected = None;
            self.battle.begin();
        }

        let outcome = self.battle.outcome();
        if !self.battle.is_deploying() && outcome != Outcome::Ongoing && self.fx.is_none() {
            return self.result_overlay(game, message, outcome);
        }
        None
    }

    fn deploy_input(&mut self) {
        if is_key_pressed(KeyCode::Enter) {
            self.battle.begin();
            return;
        }
        if !clicked() {
            return;
        }
        let Some((Team::Player, slot)) = cell_under_mouse() else {
            self.selected = None;
            return;
        };
        match self.selected.take() {
            Some(from) if from != slot => {
                let _ = self.battle.move_card(from, slot);
            }
            Some(_) => {}
            None if self.battle.at(Team::Player, slot).is_some() => self.selected = Some(slot),
            None => {}
        }
    }

    fn player_input(&mut self, active: usize) {
        if is_key_pressed(KeyCode::Space) {
            self.battle.skip();
            return;
        }
        if !clicked() {
            return;
        }
        if let Some(t) = self.fighter_under_mouse() {
            if let Ok(hits) = self.battle.act(t) {
                self.fx = Some(Fx { actor: active, hits, t: 0.0 });
            }
        }
    }

    fn draw(&self, assets: &Assets) {
        clear_background(Color::from_rgba(30, 32, 28, 255));
        let active = self.battle.active();
        let player_turn = active.is_some_and(|a| self.battle.fighters[a].team == Team::Player) && self.fx.is_none();
        let targets = match (player_turn, active) {
            (true, Some(a)) => self.battle.targets(a),
            _ => Vec::new(),
        };
        let hovered_cell = cell_under_mouse();

        // Empty cells.
        for (team, slot) in all_cells() {
            let p = cell_pos(team, slot);
            draw_rectangle(p.x, p.y, CARD_W, CARD_H, Color::new(0.2, 0.22, 0.19, 1.0));
            let lit = self.battle.is_deploying() && team == Team::Player && hovered_cell == Some((team, slot));
            draw_rectangle_lines(p.x, p.y, CARD_W, CARD_H, 1.0, if lit { INK } else { Color::new(0.35, 0.37, 0.32, 1.0) });
        }
        let mid = cell_pos(Team::Player, Slot::new(Row::Front, 0)).y - MID_GAP / 2.0;
        text_centered("vs", OX + (COLS as f32 * PITCH_X) / 2.0, mid + 8.0, 22.0, DIM);

        for (i, f) in self.battle.fighters.iter().enumerate() {
            let in_fx = self.fx.as_ref().is_some_and(|fx| fx.hits.iter().any(|h| h.target == i));
            if !f.alive() && !in_fx {
                continue;
            }
            let mut p = cell_pos(f.team, f.slot);
            if let Some(fx) = self.fx.as_ref().filter(|fx| fx.actor == i) {
                let lunge = (fx.t / (FX_TIME * 0.5)).min(1.0);
                let dir = if f.team == Team::Player { -1.0 } else { 1.0 };
                p.y += dir * 16.0 * (lunge * PI).sin();
            }
            let border = if Some(i) == active && self.fx.is_none() {
                Some(ACCENT)
            } else if targets.contains(&i) {
                Some(if f.team == Team::Player { GREEN } else { RED })
            } else if self.selected == Some(f.slot) && f.team == Team::Player {
                Some(WHITE)
            } else {
                None
            };
            self.draw_card(assets, i, p, border);
        }

        if let Some(fx) = &self.fx {
            self.draw_fx(fx);
        }
        self.draw_panel(player_turn, &targets);
    }

    fn draw_card(&self, assets: &Assets, id: usize, p: Vec2, border: Option<Color>) {
        let f = &self.battle.fighters[id];
        let s = f.kind.stats();
        draw_rectangle(p.x, p.y, CARD_W, CARD_H, Color::new(0.14, 0.13, 0.12, 1.0));
        draw_rectangle(p.x, p.y, CARD_W, 4.0, team_color(f.team));
        assets.draw_unit(f.kind, f.team, p.x + CARD_W / 2.0, p.y + 46.0, 72.0);
        text_centered(f.kind.name(), p.x + CARD_W / 2.0, p.y + 97.0, 17.0, INK);
        hp_bar(p.x + 8.0, p.y + 104.0, CARD_W - 16.0, f.hp, s.max_hp);
        text(&format!("{}/{}", f.hp.max(0), s.max_hp), p.x + 8.0, p.y + 125.0, 17.0, DIM);
        let init = format!("i{}", s.initiative);
        let w = measure_text(&init, None, 17, 1.0).width;
        text(&init, p.x + CARD_W - 8.0 - w, p.y + 125.0, 17.0, DIM);
        if f.is_hero {
            text("*", p.x + 6.0, p.y + 22.0, 26.0, ACCENT);
        }
        if !self.battle.is_deploying() && f.alive() && self.battle.blocked(id) {
            text_centered("blocked", p.x + CARD_W / 2.0, p.y + 18.0, 16.0, DIM);
        }
        if let Some(c) = border {
            draw_rectangle_lines(p.x - 2.0, p.y - 2.0, CARD_W + 4.0, CARD_H + 4.0, 4.0, c);
        }
    }

    fn draw_fx(&self, fx: &Fx) {
        let k = fx.t / FX_TIME;
        let mut per_target: Vec<(usize, Vec<String>, bool)> = Vec::new();
        for h in &fx.hits {
            let label = match (h.heal, h.amount) {
                (true, n) => format!("+{n}"),
                (false, 0) => "blocked".to_string(),
                (false, n) => format!("-{n}"),
            };
            match per_target.iter_mut().find(|(t, _, _)| *t == h.target) {
                Some(entry) => entry.1.push(label),
                None => per_target.push((h.target, vec![label], h.heal)),
            }
        }
        for (target, labels, heal) in per_target {
            let f = &self.battle.fighters[target];
            let p = cell_pos(f.team, f.slot);
            let flash = if heal {
                Color::new(0.2, 1.0, 0.3, 0.45 * (1.0 - k))
            } else {
                Color::new(1.0, 0.1, 0.1, 0.5 * (1.0 - k))
            };
            draw_rectangle(p.x, p.y, CARD_W, CARD_H, flash);
            let label = labels.join(" ");
            let y = p.y + 50.0 - 30.0 * k;
            text_centered(&label, p.x + CARD_W / 2.0 + 1.0, y + 1.0, 30.0, BLACK);
            text_centered(&label, p.x + CARD_W / 2.0, y, 30.0, if heal { GREEN } else { WHITE });
        }
    }

    fn draw_panel(&self, player_turn: bool, targets: &[usize]) {
        let x = PANEL_X;
        let w = screen_width() - x - 16.0;
        draw_rectangle(x, OY, w, PANEL_H, PANEL);

        let (title, color) = match self.battle.active() {
            _ if self.battle.is_deploying() => ("Deploy your squad", ACCENT),
            Some(a) if self.battle.fighters[a].team == Team::Player => ("Your move", team_color(Team::Player)),
            Some(_) => ("Enemy move", team_color(Team::Enemy)),
            None => ("Battle over", INK),
        };
        text(title, x + 12.0, OY + 30.0, 26.0, color);
        if !self.battle.is_deploying() {
            text(&format!("Round {}/{}", self.battle.round, MAX_ROUNDS), x + w - 130.0, OY + 30.0, 20.0, DIM);
        }

        // Hovered unit (or the active one) details.
        if let Some(id) = self.fighter_under_mouse().or(self.battle.active()) {
            let f = &self.battle.fighters[id];
            let s = f.kind.stats();
            text(f.kind.name(), x + 12.0, OY + 64.0, 24.0, INK);
            text(
                &format!("HP {}/{}  Armor {}  Init {}", f.hp.max(0), s.max_hp, s.armor, s.initiative),
                x + 12.0,
                OY + 86.0,
                18.0,
                DIM,
            );
            text(&f.kind.describe_attack(), x + 12.0, OY + 106.0, 18.0, DIM);
        }

        let mut y = OY + 140.0;
        if self.battle.is_deploying() {
            for line in [
                "Click a card, then a cell to move",
                "or swap it. Warriors hit the enemy",
                "front row first; shooters and mages",
                "hit anyone. A back-row warrior waits",
                "until your own front row falls.",
            ] {
                text(line, x + 12.0, y, 18.0, DIM);
                y += 20.0;
            }
            return;
        }

        text("Turn order", x + 12.0, y, 20.0, INK);
        for (n, id) in self.battle.queue().take(6).enumerate() {
            let q = &self.battle.fighters[id];
            let marker = if n == 0 { "> " } else { "  " };
            text(&format!("{marker}{}", q.kind.name()), x + 12.0, y + 22.0 + n as f32 * 20.0, 18.0, team_color(q.team));
        }
        y += 160.0;
        text("Log (newest first)", x + 12.0, y, 20.0, INK);
        for (n, line) in self.battle.log.iter().rev().take(10).enumerate() {
            text(line, x + 12.0, y + 22.0 + n as f32 * 20.0, 17.0, DIM);
        }

        if player_turn {
            let hint = if targets.is_empty() {
                "Nothing in reach: Space to wait"
            } else {
                "Click a framed card, Space to wait"
            };
            text(hint, x + 12.0, OY + PANEL_H - 14.0, 18.0, ACCENT);
        }
    }

    fn result_overlay(&self, game: &mut Game, message: &mut Option<String>, outcome: Outcome) -> Option<Screen> {
        let (w, h) = (440.0, 190.0);
        let (x, y) = ((screen_width() - w) / 2.0, (screen_height() - h) / 2.0);
        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.5));
        draw_rectangle(x, y, w, h, PANEL);
        draw_rectangle_lines(x, y, w, h, 2.0, ACCENT);
        let (title, sub, color) = match outcome {
            Outcome::Victory => ("Victory!", "", ACCENT),
            Outcome::Stalemate => ("Stalemate", "Nobody breaks. You withdraw.", INK),
            _ => ("Defeat", "", RED),
        };
        text_centered(title, x + w / 2.0, y + 60.0, 50.0, color);
        text_centered(sub, x + w / 2.0, y + 92.0, 20.0, DIM);
        if !button(x + w / 2.0 - 100.0, y + 118.0, 200.0, 48.0, "Continue", true) {
            return None;
        }
        let losses = |lost: usize| if lost > 0 { format!(", {lost} fell") } else { String::new() };
        match game.resolve_battle(&self.battle) {
            BattleResult::Defeat => Some(Screen::GameOver),
            BattleResult::Victory { .. } if game.won() => Some(Screen::Victory),
            BattleResult::Victory { reward, lost } => {
                *message = Some(format!("Victory! +{reward} gold{}.", losses(lost)));
                Some(Screen::WorldMap)
            }
            BattleResult::Withdrew { lost } => {
                *message = Some(format!("You withdraw from the camp{}.", losses(lost)));
                Some(Screen::WorldMap)
            }
        }
    }
}
