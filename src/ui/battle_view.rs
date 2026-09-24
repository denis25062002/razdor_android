//! The battle screen: both formations (enemy on top, front rows facing in the middle), unit
//! cards with the original's stat strip, a side panel with the hovered unit's full stats,
//! the turn order and the log. Hovering a target previews the action ("-N hits", curse
//! effects); left click does the default action, right click the alternative (e.g. a mage's
//! strike instead of its curse).

use std::collections::VecDeque;
use std::f32::consts::PI;

use macroquad::prelude::*;

use razdor::rules::battle::{ActionKind, Battle, Hit, Outcome, Preview, Step, Team, XpAward};
use razdor::rules::content::{MagicDirection, Stat};
use razdor::rules::formation::{Row, Slot};
use razdor::rules::game::{BattleResult, Game};
use razdor::rules::units::Stats;

use super::assets::{team_color, Assets};
use super::dialog::Dialog;
use super::widgets::*;
use super::Screen;

const OX: f32 = 24.0;
const OY: f32 = 64.0;
/// Empty strip between the two formations.
const MID_GAP: f32 = 36.0;
const PANEL_W: f32 = 380.0;
const AI_DELAY: f32 = 0.45;
const STRIKE_TIME: f32 = 0.6;
const MOVE_TIME: f32 = 0.25;
/// Stat higher than at the battle's start (blessing).
const RAISED: Color = Color::new(0.45, 0.72, 1.0, 1.0);
const FRIENDLY: Color = Color::new(0.35, 0.65, 1.0, 1.0);
const XP_COLOR: Color = Color::new(0.35, 0.95, 0.95, 1.0);

/// Card and grid geometry for the current window and formation.
#[derive(Clone, Copy)]
struct Layout {
    rows: usize,
    card_w: f32,
    card_h: f32,
    pitch_x: f32,
    pitch_y: f32,
    panel_x: f32,
}

impl Layout {
    fn new(battle: &Battle) -> Layout {
        let rows = battle.formation.rows().len();
        let cols = battle.formation.cols as f32;
        let avail_h = screen_height() - OY - 16.0 - MID_GAP;
        let avail_w = screen_width() - PANEL_W - OX - 24.0;
        let mut pitch_y = (avail_h / (2 * rows) as f32).min(146.0);
        let mut pitch_x = (pitch_y - 10.0) * 0.86 + 10.0;
        if pitch_x * cols > avail_w {
            let k = avail_w / (pitch_x * cols);
            pitch_x *= k;
            pitch_y *= k;
        }
        Layout {
            rows,
            card_w: pitch_x - 10.0,
            card_h: pitch_y - 10.0,
            pitch_x,
            pitch_y,
            panel_x: OX + cols * pitch_x + 16.0,
        }
    }

    /// Screen rows top to bottom: enemy reserve, back, front | player front, back, reserve.
    fn cell_pos(&self, team: Team, slot: Slot) -> Vec2 {
        let r = match slot.row {
            Row::Front => 0,
            Row::Back => 1,
            Row::Reserve => 2,
        };
        let (line, gap) = match team {
            Team::Enemy => (self.rows - 1 - r, 0.0),
            Team::Player => (self.rows + r, MID_GAP),
        };
        vec2(OX + slot.col as f32 * self.pitch_x, OY + line as f32 * self.pitch_y + gap)
    }

    fn panel_height(&self) -> f32 {
        (2.0 * self.rows as f32 * self.pitch_y + MID_GAP - 10.0).max(560.0).min(screen_height() - OY - 8.0)
    }
}

/// An action being animated.
enum FxKind {
    /// The actor lunges, the target flashes and shows the number.
    Act { hit: Hit },
    /// The actor slides between two cells.
    Move { from: Vec2, to: Vec2 },
}

struct Fx {
    actor: usize,
    kind: FxKind,
    t: f32,
}

impl Fx {
    fn duration(&self) -> f32 {
        match self.kind {
            FxKind::Act { .. } => STRIKE_TIME,
            FxKind::Move { .. } => MOVE_TIME,
        }
    }
}

pub struct BattleView {
    battle: Battle,
    fx: Option<Fx>,
    ai_timer: f32,
    /// Deploy phase: card picked up to move.
    selected: Option<Slot>,
    /// XP shares, computed once the battle is over.
    xp: Option<Vec<XpAward>>,
}

fn all_cells(battle: &Battle) -> Vec<(Team, Slot)> {
    [Team::Enemy, Team::Player].into_iter().flat_map(|t| battle.formation.slots().map(move |s| (t, s))).collect()
}

/// "+" for a stat above its start-of-battle value, "-" below.
fn stat_color(cur: i32, base: i32) -> Color {
    match cur.cmp(&base) {
        std::cmp::Ordering::Greater => RAISED,
        std::cmp::Ordering::Less => RED,
        std::cmp::Ordering::Equal => INK,
    }
}

/// Draws coloured text pieces one after another.
fn pieces(parts: &[(String, Color)], x: f32, y: f32, size: f32) {
    let mut x = x;
    for (s, c) in parts {
        text(s, x, y, size, *c);
        x += measure(s, size).width;
    }
}

/// The card's attack piece: `A:` melee, `S:` ranged, `Pwr:` magic.
fn attack_piece(s: &Stats, base: &Stats) -> (String, Color) {
    let (label, st) = if s.is_warrior() || base.is_warrior() {
        ("A", Stat::AttackBlow)
    } else if s.is_shooter() || base.is_shooter() {
        ("S", Stat::AttackShot)
    } else {
        ("Pwr", Stat::MagicPower)
    };
    (format!("{label}: {}", s[st]), stat_color(s[st], base[st]))
}

fn preview_text(p: Preview, kind: ActionKind, hp: i32) -> String {
    match p {
        Preview::Damage(d) if d >= hp => format!("{}: -{d} hits, kills", kind.label()),
        Preview::Damage(d) => format!("{}: -{d} hits", kind.label()),
        Preview::Heal(h) => format!("{}: +{h} hits", kind.label()),
        Preview::Buff(b) => format!("{}: {}", kind.label(), b.describe()),
    }
}

impl BattleView {
    pub fn new(battle: Battle) -> Self {
        BattleView { battle, fx: None, ai_timer: 0.0, selected: None, xp: None }
    }

    fn cell_under_mouse(&self, l: &Layout) -> Option<(Team, Slot)> {
        all_cells(&self.battle).into_iter().find(|&(t, s)| {
            let p = l.cell_pos(t, s);
            mouse_in(p.x, p.y, l.card_w, l.card_h)
        })
    }

    fn fighter_under_mouse(&self, l: &Layout) -> Option<usize> {
        let (team, slot) = self.cell_under_mouse(l)?;
        self.battle.at(team, slot)
    }

    pub fn frame(&mut self, game: &mut Game, assets: &Assets, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
        let l = Layout::new(&self.battle);
        let dt = get_frame_time();
        if let Some(fx) = &mut self.fx {
            fx.t += dt;
            if fx.t >= fx.duration() {
                self.fx = None;
            }
        }

        if self.battle.is_deploying() {
            self.deploy_input(&l);
        } else if self.fx.is_none() {
            if let Some(active) = self.battle.active() {
                if self.battle.fighters[active].team == Team::Player {
                    self.player_input(&l, active);
                } else {
                    self.ai_timer += dt;
                    if self.ai_timer >= AI_DELAY {
                        self.ai_timer = 0.0;
                        self.fx = match self.battle.ai_step() {
                            Some(Step::Act { actor, hit }) => Some(Fx { actor, kind: FxKind::Act { hit }, t: 0.0 }),
                            Some(Step::Move { actor, from, to }) => {
                                let team = self.battle.fighters[actor].team;
                                let kind = FxKind::Move { from: l.cell_pos(team, from), to: l.cell_pos(team, to) };
                                Some(Fx { actor, kind, t: 0.0 })
                            }
                            Some(Step::Wait { .. }) | None => None,
                        };
                    }
                }
            }
        }

        let outcome = self.battle.outcome();
        let over = !self.battle.is_deploying() && outcome != Outcome::Ongoing && self.fx.is_none();
        if over && self.xp.is_none() {
            self.xp = Some(self.battle.xp_awards(Team::Player));
        }

        self.draw(&l, assets);

        if self.battle.is_deploying() && button(l.panel_x + 12.0, OY + l.panel_height() - 64.0, 200.0, 48.0, "Fight!", true) {
            self.selected = None;
            self.battle.begin();
        }

        if over {
            return self.result_overlay(&l, game, message, dialogs, outcome);
        }
        None
    }

    fn deploy_input(&mut self, l: &Layout) {
        if is_key_pressed(KeyCode::Enter) {
            self.battle.begin();
            return;
        }
        if !clicked() {
            return;
        }
        let Some((Team::Player, slot)) = self.cell_under_mouse(l) else {
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

    fn player_input(&mut self, l: &Layout, active: usize) {
        if is_key_pressed(KeyCode::Space) {
            self.battle.skip();
            return;
        }
        let right = is_mouse_button_pressed(MouseButton::Right);
        if !clicked() && !right {
            return;
        }
        if let Some(t) = self.fighter_under_mouse(l) {
            let opts = self.battle.options(active, t);
            let kind = if right { opts.get(1) } else { opts.first() };
            if let Some(&kind) = kind {
                if let Ok(hit) = self.battle.act_with(t, kind) {
                    self.fx = Some(Fx { actor: active, kind: FxKind::Act { hit }, t: 0.0 });
                }
            }
        } else if let Some((Team::Player, to)) = self.cell_under_mouse(l) {
            let from = self.battle.fighters[active].slot;
            if !right && self.battle.move_active(to).is_ok() {
                let kind = FxKind::Move { from: l.cell_pos(Team::Player, from), to: l.cell_pos(Team::Player, to) };
                self.fx = Some(Fx { actor: active, kind, t: 0.0 });
            }
        }
    }

    fn draw(&self, l: &Layout, assets: &Assets) {
        clear_background(Color::from_rgba(30, 32, 28, 255));
        let b = &self.battle;
        let active = b.active();
        let player_turn = active.is_some_and(|a| b.fighters[a].team == Team::Player) && self.fx.is_none();
        let (targets, moves) = match (player_turn, active) {
            (true, Some(a)) => (b.targets(a), b.moves(a)),
            _ => (Vec::new(), Vec::new()),
        };
        let hovered_cell = self.cell_under_mouse(l);

        // Empty cells: the reserve is darker and labelled.
        for (team, slot) in all_cells(b) {
            let p = l.cell_pos(team, slot);
            let fill = if slot.row == Row::Reserve { Color::new(0.15, 0.16, 0.14, 1.0) } else { Color::new(0.2, 0.22, 0.19, 1.0) };
            draw_rectangle(p.x, p.y, l.card_w, l.card_h, fill);
            if slot.row == Row::Reserve {
                text_centered("reserve", p.x + l.card_w / 2.0, p.y + l.card_h / 2.0, 16.0, Color::new(0.4, 0.4, 0.36, 1.0));
            }
            let is_move = team == Team::Player && moves.contains(&slot);
            if is_move {
                draw_rectangle(p.x, p.y, l.card_w, l.card_h, Color::new(0.3, 0.55, 1.0, 0.12));
            }
            let lit = (b.is_deploying() || is_move) && team == Team::Player && hovered_cell == Some((team, slot));
            let edge = if is_move { FRIENDLY } else { Color::new(0.35, 0.37, 0.32, 1.0) };
            draw_rectangle_lines(p.x, p.y, l.card_w, l.card_h, if lit { 3.0 } else { 1.0 }, if lit { INK } else { edge });
        }
        let mid = l.cell_pos(Team::Player, Slot::new(Row::Front, 0)).y - MID_GAP / 2.0;
        let grid_w = b.formation.cols as f32 * l.pitch_x - 10.0;
        draw_rectangle(OX, mid - 11.0, grid_w, 22.0, Color::new(0.45, 0.1, 0.08, 0.9));
        text_centered("battle", OX + grid_w / 2.0, mid + 6.0, 20.0, INK);

        for (i, f) in b.fighters.iter().enumerate() {
            let in_fx = self.fx.as_ref().is_some_and(|fx| matches!(&fx.kind, FxKind::Act { hit } if hit.target == i) || fx.actor == i);
            if !f.alive() && !in_fx {
                continue;
            }
            let mut p = l.cell_pos(f.team, f.slot);
            if let Some(fx) = self.fx.as_ref().filter(|fx| fx.actor == i) {
                match fx.kind {
                    FxKind::Act { .. } => {
                        let lunge = (fx.t / (STRIKE_TIME * 0.5)).min(1.0);
                        let dir = if f.team == Team::Player { -1.0 } else { 1.0 };
                        p.y += dir * 16.0 * (lunge * PI).sin();
                    }
                    FxKind::Move { from, to } => p = from.lerp(to, (fx.t / MOVE_TIME).min(1.0)),
                }
            }
            let border = if Some(i) == active && self.fx.is_none() {
                Some(GREEN)
            } else if targets.contains(&i) {
                Some(if f.team == Team::Player { FRIENDLY } else { RED })
            } else if self.selected == Some(f.slot) && f.team == Team::Player {
                Some(WHITE)
            } else {
                None
            };
            self.draw_card(l, assets, i, p, border);
        }

        if let Some(fx) = &self.fx {
            self.draw_fx(l, fx);
        }
        if let Some(xp) = &self.xp {
            for a in xp {
                let f = &b.fighters[a.fighter];
                let p = l.cell_pos(f.team, f.slot);
                draw_rectangle(p.x + 6.0, p.y + l.card_h * 0.36, l.card_w - 12.0, 24.0, Color::new(0.0, 0.2, 0.25, 0.85));
                text_centered(&format!("XP +{}", a.xp), p.x + l.card_w / 2.0, p.y + l.card_h * 0.36 + 18.0, 20.0, XP_COLOR);
            }
        }
        self.draw_panel(l, player_turn, &targets, &moves);
        if player_turn {
            self.draw_preview(l, active.expect("player turn"));
        }
    }

    fn draw_card(&self, l: &Layout, assets: &Assets, id: usize, p: Vec2, border: Option<Color>) {
        let f = &self.battle.fighters[id];
        let (s, base) = (&f.stats, &f.base);
        let (w, h) = (l.card_w, l.card_h);
        // Whole pixels keep the small pixel font crisp.
        let p = p.round();
        let fs = (h * 0.115).clamp(11.0, 16.0).round();
        draw_rectangle(p.x, p.y, w, h, Color::new(0.14, 0.13, 0.12, 1.0));
        draw_rectangle(p.x, p.y, w, 4.0, team_color(f.team));
        assets.draw_unit(f.unit, f.team, p.x + w / 2.0, p.y + h * 0.26, h * 0.4);
        hp_bar(p.x + 6.0, p.y + h * 0.48, w - 12.0, f.hp, s.max_hp());
        // Long names shrink to fit the card.
        let mut name_fs = fs + 1.0;
        while name_fs > 8.0 && measure(&f.name, name_fs).width > w - 6.0 {
            name_fs -= 1.0;
        }
        text_centered(&f.name, p.x + w / 2.0, p.y + h * 0.6, name_fs, INK);
        let (dx, x) = (fs * 0.3, p.x + 5.0);
        let def = |st: Stat| (format!("{}", s[st]), stat_color(s[st], base[st]));
        let (db, ds) = (def(Stat::DefenceBlow), def(Stat::DefenceShot));
        pieces(&[attack_piece(s, base), ("  D: ".into(), INK), db, ("/".into(), INK), ds], x, p.y + h * 0.72, fs);
        let mn = s[Stat::Manevres];
        let ini = s[Stat::Initiative];
        pieces(
            &[
                ("Mnvr: ".into(), INK),
                (mn.to_string(), stat_color(mn, base[Stat::Manevres])),
                ("  Ini: ".into(), INK),
                (ini.to_string(), stat_color(ini, base[Stat::Initiative])),
            ],
            x,
            p.y + h * 0.84,
            fs,
        );
        let hits = if f.hp < s.max_hp() { format!("Hits: {}/{}", f.hp.max(0), s.max_hp()) } else { format!("Hits: {}", s.max_hp()) };
        text(&hits, x, p.y + h * 0.96, fs, INK);
        text(&format!("L{}", f.level), p.x + dx + 2.0, p.y + fs + 4.0, fs, ACCENT);
        if f.is_hero {
            text("*", p.x + w - 16.0, p.y + 24.0, 26.0, ACCENT);
        }
        if f.blessing().is_some() {
            draw_circle(p.x + w - 10.0, p.y + h * 0.4, 5.0, RAISED);
        }
        if f.curse().is_some() {
            draw_circle(p.x + w - 10.0, p.y + h * 0.4 + 12.0, 5.0, PURPLE);
        }
        let fighting = !self.battle.is_deploying() && self.battle.outcome() == Outcome::Ongoing;
        if fighting && f.alive() && f.slot.row != Row::Reserve && self.battle.helpless(id) {
            text_centered("can't reach", p.x + w / 2.0, p.y + h * 0.44, fs, DIM);
        }
        if let Some(c) = border {
            draw_rectangle_lines(p.x - 2.0, p.y - 2.0, w + 4.0, h + 4.0, 4.0, c);
        }
    }

    fn draw_fx(&self, l: &Layout, fx: &Fx) {
        let FxKind::Act { hit } = &fx.kind else { return };
        let k = fx.t / STRIKE_TIME;
        let f = &self.battle.fighters[hit.target];
        let p = l.cell_pos(f.team, f.slot);
        let (flash, label, color) = match hit.kind {
            ActionKind::Heal => (Color::new(0.2, 1.0, 0.3, 0.45 * (1.0 - k)), format!("+{}", hit.amount), GREEN),
            ActionKind::Bless => (Color::new(0.4, 0.7, 1.0, 0.45 * (1.0 - k)), "blessed".into(), RAISED),
            ActionKind::Curse => (Color::new(0.7, 0.2, 0.9, 0.45 * (1.0 - k)), "cursed".into(), PURPLE),
            ActionKind::LongStrike => (Color::new(1.0, 0.1, 0.1, 0.5 * (1.0 - k)), format!("-{} long!", hit.amount), WHITE),
            ActionKind::Strike => (Color::new(1.0, 0.5, 0.1, 0.5 * (1.0 - k)), format!("-{}", hit.amount), ORANGE),
            _ => (Color::new(1.0, 0.1, 0.1, 0.5 * (1.0 - k)), format!("-{}", hit.amount), WHITE),
        };
        draw_rectangle(p.x, p.y, l.card_w, l.card_h, flash);
        let y = p.y + l.card_h * 0.4 - 30.0 * k;
        text_centered(&label, p.x + l.card_w / 2.0 + 1.0, y + 1.0, 26.0, BLACK);
        text_centered(&label, p.x + l.card_w / 2.0, y, 26.0, color);
        if let Some(c) = hit.counter {
            let a = &self.battle.fighters[fx.actor];
            let q = l.cell_pos(a.team, a.slot);
            text_centered(&format!("-{c} counter"), q.x + l.card_w / 2.0, q.y + l.card_h * 0.4 - 30.0 * k, 22.0, RED);
        }
    }

    /// Tooltip over a hovered target: what the left (and right) click would do.
    fn draw_preview(&self, l: &Layout, active: usize) {
        let Some(t) = self.fighter_under_mouse(l) else { return };
        let opts = self.battle.options(active, t);
        if opts.is_empty() {
            return;
        }
        let hp = self.battle.fighters[t].hp;
        let mut lines: Vec<String> = Vec::new();
        for (n, &k) in opts.iter().take(2).enumerate() {
            let s = preview_text(self.battle.preview(active, t, k), k, hp);
            lines.push(if n == 0 { s } else { format!("right click: {s}") });
        }
        let (mx, my) = mouse_position();
        let w = lines.iter().map(|s| measure(s, 18.0).width).fold(0.0, f32::max) + 16.0;
        let h = lines.len() as f32 * 20.0 + 10.0;
        let x = (mx + 14.0).min(screen_width() - w - 4.0);
        draw_rectangle(x, my + 14.0, w, h, Color::new(0.08, 0.07, 0.06, 0.95));
        draw_rectangle_lines(x, my + 14.0, w, h, 1.0, ACCENT);
        for (n, s) in lines.iter().enumerate() {
            text(s, x + 8.0, my + 32.0 + n as f32 * 20.0, 18.0, if n == 0 { INK } else { DIM });
        }
    }

    /// The hovered (or active) unit's full stats, as in the original's left panel.
    fn unit_details(&self, id: usize, x: f32, y: f32) -> f32 {
        let f = &self.battle.fighters[id];
        let (s, base) = (&f.stats, &f.base);
        text(&format!("{}  (level {})", f.name, f.level), x, y, 24.0, INK);
        let mut y = y + 24.0;
        let row = |label: &str, st: Stat, y: &mut f32| {
            let (cur, b) = (s[st], base[st]);
            if cur != 0 || b != 0 {
                pieces(&[(format!("{label} "), DIM), (cur.to_string(), stat_color(cur, b))], x, *y, 18.0);
                *y += 19.0;
            }
        };
        pieces(&[("Hits ".into(), DIM), (format!("{}/{}", f.hp.max(0), s.max_hp()), INK)], x, y, 18.0);
        y += 19.0;
        row("Melee attack", Stat::AttackBlow, &mut y);
        row("Ranged attack", Stat::AttackShot, &mut y);
        if s.is_mage() {
            let school = s.magic.map_or(String::new(), |m| format!("{m:?}"));
            let dir = match s.magic_direction() {
                MagicDirection::ToAlly => "allies",
                MagicDirection::ToEnemy => "enemies",
                MagicDirection::ToAll => "all",
            };
            row(&format!("Magic power ({school}, {dir})"), Stat::MagicPower, &mut y);
        }
        pieces(
            &[
                ("Defence ".into(), DIM),
                (s[Stat::DefenceBlow].to_string(), stat_color(s[Stat::DefenceBlow], base[Stat::DefenceBlow])),
                (" melee / ".into(), DIM),
                (s[Stat::DefenceShot].to_string(), stat_color(s[Stat::DefenceShot], base[Stat::DefenceShot])),
                (" ranged".into(), DIM),
            ],
            x,
            y,
            18.0,
        );
        y += 19.0;
        let (pl, pe, pd) = (s[Stat::ProtectLife], s[Stat::ProtectElemental], s[Stat::ProtectDeath]);
        if pl + pe + pd > 0 {
            text(&format!("Magic protection {pl}% / {pe}% / {pd}%"), x, y, 18.0, DIM);
            y += 19.0;
        }
        row("Initiative", Stat::Initiative, &mut y);
        row("Actions", Stat::Manevres, &mut y);
        row("Regeneration %", Stat::Regen, &mut y);
        let mut notes: Vec<String> = s.bonuses.iter().map(|b| b.token().to_string()).collect();
        if let Some(b) = f.blessing() {
            notes.push(format!("blessed ({})", b.describe()));
        }
        if let Some(c) = f.curse() {
            notes.push(format!("cursed ({})", c.describe()));
        }
        if f.poisoned {
            notes.push("poisoned".into());
        }
        if !notes.is_empty() {
            text(&notes.join(", "), x, y, 16.0, ACCENT);
            y += 18.0;
        }
        y
    }

    fn draw_panel(&self, l: &Layout, player_turn: bool, targets: &[usize], moves: &[Slot]) {
        let b = &self.battle;
        let x = l.panel_x;
        let w = screen_width() - x - 16.0;
        let h = l.panel_height();
        draw_rectangle(x, OY, w, h, PANEL);

        let (title, color) = match b.active() {
            _ if b.is_deploying() => ("Deploy your army", ACCENT),
            Some(a) if b.fighters[a].team == Team::Player => ("Your move", team_color(Team::Player)),
            Some(_) => ("Enemy move", team_color(Team::Enemy)),
            None => ("Battle over", INK),
        };
        text(title, x + 12.0, OY + 30.0, 26.0, color);
        if !b.is_deploying() {
            let limit = b.content().options.battle_end_turn;
            text(&format!("Turn {}/{limit}", b.round), x + w - 130.0, OY + 30.0, 20.0, DIM);
        }
        if let Some(a) = b.active() {
            let total = b.fighters[a].stats[Stat::Manevres];
            text(&format!("Actions {}/{}", b.actions_left(), total.max(b.actions_left())), x + w - 130.0, OY + 52.0, 18.0, DIM);
        }

        let mut y = OY + 72.0;
        if let Some(id) = self.fighter_under_mouse(l).or(b.active()) {
            y = self.unit_details(id, x + 12.0, y) + 10.0;
        }

        if b.is_deploying() {
            let row2 = format!("has +{} defence against shots. When a front", b.content().options.row2_def);
            let lines = [
                "Click a card, then a cell to move or swap it.",
                "Warriors fight from the front row and hit the",
                "three cells opposite; with those empty, a long",
                "strike reaches the nearest enemy, halving its",
                "defence. Shooters and mages in the back row",
                "reach anyone not in the reserve. The back row",
                &row2,
                "row falls, the rear steps forward. The reserve",
                "cannot act or be attacked; units step out of it.",
            ];
            y = y.max(OY + 190.0);
            for line in lines {
                text(line, x + 12.0, y, 17.0, DIM);
                y += 18.0;
            }
            return;
        }

        text("Turn order", x + 12.0, y + 6.0, 20.0, INK);
        let queue: Vec<usize> = if b.outcome() == Outcome::Ongoing { b.queue().take(6).collect() } else { Vec::new() };
        for (n, &id) in queue.iter().enumerate() {
            let q = &b.fighters[id];
            let marker = if n == 0 { "> " } else { "  " };
            let line = format!("{marker}{} ({})", q.name, q.stats[Stat::Initiative]);
            text(&line, x + 12.0, y + 26.0 + n as f32 * 19.0, 17.0, team_color(q.team));
        }
        y += 26.0 + 6.0 * 19.0 + 6.0;
        text("Log (newest first)", x + 12.0, y, 20.0, INK);
        let room = ((OY + h - 30.0 - y) / 19.0).max(0.0) as usize;
        for (n, line) in b.log.iter().rev().take(room.min(10)).enumerate() {
            text(line, x + 12.0, y + 20.0 + n as f32 * 19.0, 16.0, DIM);
        }

        if player_turn {
            let hint = match (targets.is_empty(), moves.is_empty()) {
                (false, _) => "Click a framed card (right: other action). Space: end",
                (true, false) => "Nothing in reach: step to a lit cell, or Space",
                (true, true) => "Nothing to do: Space to end the turn",
            };
            text(hint, x + 12.0, OY + h - 12.0, 17.0, ACCENT);
        }
    }

    /// Result box over the side panel, so the XP badges on the cards stay visible.
    fn result_overlay(&self, l: &Layout, game: &mut Game, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>, outcome: Outcome) -> Option<Screen> {
        let (w, h) = (360.0, 200.0);
        let panel_w = screen_width() - l.panel_x - 16.0;
        let (x, y) = (l.panel_x + (panel_w - w).max(0.0) / 2.0, OY + 120.0);
        draw_rectangle(x, y, w, h, Color::new(0.1, 0.09, 0.08, 1.0));
        draw_rectangle_lines(x, y, w, h, 2.0, ACCENT);
        let (title, sub, color) = match outcome {
            Outcome::Victory => ("Victory!", "", ACCENT),
            Outcome::Stalemate => ("Stalemate", "The turns run out. Both sides pull back.", INK),
            _ => ("Defeat", "Your whole army has fallen.", RED),
        };
        text_centered(title, x + w / 2.0, y + 56.0, 50.0, color);
        text_centered(sub, x + w / 2.0, y + 86.0, 20.0, DIM);
        let total: i32 = self.xp.iter().flatten().map(|a| a.xp).sum();
        if total > 0 {
            text_centered(&format!("Experience gained: {total}"), x + w / 2.0, y + 112.0, 20.0, XP_COLOR);
        }
        if !button(x + w / 2.0 - 100.0, y + 132.0, 200.0, 48.0, "Continue", true) {
            return None;
        }
        let losses = |lost: usize| if lost > 0 { format!(", {lost} fell") } else { String::new() };
        let result = game.resolve_battle(&self.battle);
        let levels = |ups: &[(usize, i32)]| -> String {
            ups.iter().map(|&(i, lvl)| format!(", {} reaches level {lvl}", game.squad[i].name(&game.content))).collect()
        };
        match result {
            BattleResult::Defeat => Some(Screen::GameOver),
            BattleResult::Victory { .. } if game.won() => Some(Screen::Victory),
            victory @ BattleResult::Victory { .. } => {
                dialogs.extend(Dialog::victory(game, &victory));
                *message = None;
                Some(Screen::WorldMap)
            }
            BattleResult::Withdrew { lost, level_ups } => {
                *message = Some(format!("Nobody breaks. You withdraw{}{}.", losses(lost), levels(&level_ups)));
                Some(Screen::WorldMap)
            }
        }
    }
}
