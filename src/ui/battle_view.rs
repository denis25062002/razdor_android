//! The battle screen, laid out as the original's (video notes §3, refs 09 and 10): a red
//! marble window over the map titled with both armies, the acting (or hovered) unit's panel
//! on the left, and on the right the enemy's formation on top, a hint strip, and the
//! player's formation below, front rows facing each other in the middle. Each card is the
//! unit's portrait with the original's stat strip ("A: 45 D: 35/40 / Mnvr: 1 Ini: 12 /
//! Hits: 70"); empty cells show where each kind of unit belongs. The acting card has a green
//! frame, cells it can step to blue ones and its targets red (hostile) or blue (friendly).
//! Hovering a target previews the action ("Click to curse X" / "Initiative -5 Actions -1");
//! a click does it. As in the original each cell has one action: a hostile mage curses a
//! target without a negative modifier and strikes the others, a friendly one heals the
//! wounded and blesses the rest.

use std::collections::VecDeque;
use std::f32::consts::PI;

use macroquad::prelude::*;

use razdor::rules::battle::{ActionKind, Battle, EndReason, Fighter, Hit, Outcome, Preview, Step, Team, XpAward};
use razdor::rules::content::{HeroClass, ItemId, MagicSchool, Stat};
use razdor::rules::formation::{Row, Slot};
use razdor::rules::game::{BattleResult, Foe, Game};

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::chrome::{self, shadow_centered, shadow_right, CellIcon, Skin, BLUE_TEXT, CREAM, GOLD, RED_TEXT};
use super::dialog::Dialog;
use super::unit_sheet::{self, Sheet};
use super::widgets::*;
use super::world_view;
use super::Screen;

const AI_DELAY: f32 = 0.45;
const STRIKE_TIME: f32 = 0.7;
const MOVE_TIME: f32 = 0.25;
const ACTIVE: Color = Color::new(0.35, 1.0, 0.35, 1.0);
const FRIENDLY: Color = Color::new(0.35, 0.55, 1.0, 1.0);
const HOSTILE: Color = Color::new(1.0, 0.35, 0.35, 1.0);

/// Window, panel and grid geometry, in screen pixels (from the 960×720 reference × `k`).
#[derive(Clone, Copy)]
struct Layout {
    k: f32,
    win: Rect,
    panel: Rect,
    strip: Rect,
    card: Vec2,
    pitch: Vec2,
    grid_x: f32,
    formation: razdor::rules::formation::Formation,
}

impl Layout {
    fn new(battle: &Battle) -> Layout {
        let k = chrome::k();
        let (sw, sh) = (screen_width(), screen_height());
        let (ww, wh) = ((836.0 * k).round(), (600.0 * k).round());
        let x = ((sw - ww) / 2.0).round();
        let y = ((sh - chrome::bar_height() - wh) / 2.0).max(2.0).round();
        let win = Rect::new(x, y, ww, wh);
        let panel = Rect::new(x + 2.0 * k, y + 27.0 * k, 244.0 * k, 570.0 * k);
        let (rx, rw) = (x + 248.0 * k, 586.0 * k);
        let strip = Rect::new(rx, y + 302.0 * k, rw, 20.0 * k);
        let f = battle.formation;
        let lines = f.display_lines() as f32;
        let c = 1.0f32.min(2.0 / lines).min(6.0 / f.cols as f32);
        let card = vec2(88.0 * c * k, 128.0 * c * k).round();
        let pitch = vec2(96.0 * c * k, 133.0 * c * k);
        let grid_w = f.cols as f32 * pitch.x - 8.0 * c * k;
        Layout { k, win, panel, strip, card, pitch, grid_x: (rx + (rw - grid_w) / 2.0).round(), formation: f }
    }

    /// Top-left of a cell. Display line 0 is the front row: the enemy's is just above the
    /// strip, the player's just below it.
    fn cell_pos(&self, team: Team, slot: Slot) -> Vec2 {
        let (line, col) = self.formation.display(slot);
        let x = self.grid_x + col as f32 * self.pitch.x;
        let y = match team {
            Team::Enemy => self.strip.y - 9.0 * self.k - self.card.y - line as f32 * self.pitch.y,
            Team::Player => self.strip.y + self.strip.h + 10.0 * self.k + line as f32 * self.pitch.y,
        };
        vec2(x, y).round()
    }

    fn portrait(&self, p: Vec2) -> Rect {
        Rect::new(p.x, p.y, self.card.x, self.card.x)
    }
}

/// An action being animated.
enum FxKind {
    /// The actor lunges, the target shows the hit's animation and number.
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
    /// The result box has shown and its music started (once).
    result_cued: bool,
    /// The last line of the log shown in the strip, and for how long more.
    news: Option<(String, f32)>,
}

fn all_cells(battle: &Battle) -> Vec<(Team, Slot)> {
    [Team::Enemy, Team::Player].into_iter().flat_map(|t| battle.formation.slots().map(move |s| (t, s))).collect()
}


/// "Click to curse X" and the effect below it, as the original's hover box.
fn preview_lines(p: Preview, kind: ActionKind, name: &str, hp: i32) -> (String, String) {
    let verb = match kind {
        ActionKind::Melee | ActionKind::LongStrike | ActionKind::Shot | ActionKind::Strike => "attack",
        ActionKind::Curse => "curse",
        ActionKind::Heal => "heal",
        ActionKind::Bless => "bless",
    };
    let head = format!("Click to {verb} \"{name}\"");
    let effect = match p {
        Preview::Damage(d) if d >= hp => format!("Damage -{d} hits (kills)"),
        Preview::Damage(d) if kind == ActionKind::LongStrike => format!("Long strike: damage -{d} hits"),
        Preview::Damage(d) => format!("Damage -{d} hits"),
        Preview::Heal(h) => format!("Heals +{h} hits"),
        Preview::Buff(b) => {
            let parts: Vec<String> = [("Attack", b.attack), ("Defence", b.defence), ("Initiative", b.initiative), ("Actions", b.actions)]
                .iter()
                .filter(|(_, v)| *v != 0)
                .map(|(n, v)| format!("{n}: {v:+}"))
                .collect();
            if parts.is_empty() {
                "No effect".to_string()
            } else {
                parts.join("  ")
            }
        }
    };
    (head, effect)
}

/// The sound of `actor`'s action `kind`: shooters with a ranged attack of at least
/// `ShotWeaponRange` fire cannon.
fn action_cue(battle: &Battle, actor: usize, kind: ActionKind) -> Cue {
    match kind {
        ActionKind::Melee | ActionKind::LongStrike => Cue::Fight,
        ActionKind::Shot if battle.fighters[actor].stats[Stat::AttackShot] >= battle.content().options.shot_weapon_range => Cue::Cannon,
        ActionKind::Shot => Cue::Shoot,
        ActionKind::Heal => Cue::Cure,
        ActionKind::Bless => Cue::Bless,
        ActionKind::Strike | ActionKind::Curse => Cue::Sorcery,
    }
}

/// The original's animation for an action (`Graphics/Battle`, `Graphics/Spells`), its size
/// relative to the card and its tint.
fn effect_art(kind: ActionKind, school: Option<MagicSchool>) -> (&'static str, f32) {
    match kind {
        ActionKind::Melee | ActionKind::LongStrike => ("Battle/--KUSKI.ugs", 1.9),
        ActionKind::Shot => ("Battle/--KUSKIBIG.ugs", 1.7),
        ActionKind::Strike => match school {
            Some(MagicSchool::Life) => ("Spells/S-Light-Front.ugs", 1.5),
            Some(MagicSchool::Death) => ("Spells/S-Fog.ugs", 1.5),
            _ => ("Spells/S-Fire.ugs", 1.5),
        },
        ActionKind::Curse => ("Spells/S-Fontain.ugs", 1.5),
        ActionKind::Heal => ("Battle/--CURE.ugs", 1.6),
        ActionKind::Bless => ("Spells/S-Swirl.ugs", 1.4),
    }
}

/// "Battle: the army of hero Stings against Castle Morgen!"
fn battle_title(game: &Game) -> String {
    let enemy = match game.foe {
        Some(Foe::Army(i)) => game.world.armies.get(i).map(|a| if a.name.trim().is_empty() { a.leader_name.clone() } else { a.name.clone() }),
        Some(Foe::Garrison(l)) => game.world.locations.get(l).map(|l| l.name.clone()),
        None => None,
    }
    .filter(|n| !n.trim().is_empty())
    .unwrap_or_else(|| "the enemy".to_string());
    format!("Battle: the army of hero {} against {}!", game.hero_name(), enemy.trim())
}

impl BattleView {

    pub fn new(battle: Battle) -> Self {
        BattleView { battle, fx: None, ai_timer: 0.0, selected: None, xp: None, result_cued: false, news: None }
    }

    fn cell_under_mouse(&self, l: &Layout) -> Option<(Team, Slot)> {
        all_cells(&self.battle).into_iter().find(|&(t, s)| {
            let p = l.cell_pos(t, s);
            mouse_in(p.x, p.y, l.card.x, l.card.y)
        })
    }

    fn fighter_under_mouse(&self, l: &Layout) -> Option<usize> {
        let (team, slot) = self.cell_under_mouse(l)?;
        self.battle.at(team, slot)
    }

    /// Remembers the newest log line for the strip.
    fn note_log(&mut self) {
        if let Some(line) = self.battle.log.last() {
            if !line.starts_with("--") && self.news.as_ref().is_none_or(|(n, _)| n != line) {
                self.news = Some((line.clone(), 2.5));
            }
        }
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
        if let Some((_, t)) = &mut self.news {
            *t -= dt;
            if *t <= 0.0 {
                self.news = None;
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
                            Some(Step::Act { actor, hit }) => {
                                cue(action_cue(&self.battle, actor, hit.kind));
                                Some(Fx { actor, kind: FxKind::Act { hit }, t: 0.0 })
                            }
                            Some(Step::Move { actor, from, to }) => {
                                cue(Cue::CardMove);
                                let team = self.battle.fighters[actor].team;
                                let kind = FxKind::Move { from: l.cell_pos(team, from), to: l.cell_pos(team, to) };
                                Some(Fx { actor, kind, t: 0.0 })
                            }
                            Some(Step::Wait { .. }) | None => None,
                        };
                        self.note_log();
                    }
                }
            }
        }

        let outcome = self.battle.outcome();
        let over = !self.battle.is_deploying() && outcome != Outcome::Ongoing && self.fx.is_none();
        if over && self.xp.is_none() {
            // What the player's units gain: only a victory pays (experience.md §3).
            self.xp = Some(self.battle.player_xp());
        }

        world_view::backdrop(game, assets);
        self.draw(&l, game, assets);

        if self.battle.is_deploying() {
            let r = Rect::new(l.strip.x + l.strip.w - 118.0 * l.k, l.strip.y - 3.0 * l.k, 112.0 * l.k, l.strip.h + 6.0 * l.k);
            if button(r.x, r.y, r.w, r.h, "Fight!", true) {
                self.selected = None;
                self.battle.begin();
            }
        }

        if over {
            if !self.result_cued {
                // The triumph plays as soon as the victory box appears, and carries on over the
                // map afterwards (a sting is not cut by the move to the map).
                self.result_cued = true;
                if outcome == Outcome::Victory {
                    cue(Cue::Triumph);
                }
            }
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
                if self.battle.move_card(from, slot).is_ok() {
                    cue(Cue::CardMove);
                }
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
        if !clicked() {
            return;
        }
        if let Some(t) = self.fighter_under_mouse(l) {
            let opts = self.battle.options(active, t);
            if let Some(&kind) = opts.first() {
                if let Ok(hit) = self.battle.act_with(t, kind) {
                    cue(action_cue(&self.battle, active, kind));
                    self.fx = Some(Fx { actor: active, kind: FxKind::Act { hit }, t: 0.0 });
                    self.note_log();
                }
            } else if t == active {
                // A click on its own card passes one action, as in the original.
                self.battle.pass();
            }
        } else if let Some((Team::Player, to)) = self.cell_under_mouse(l) {
            let from = self.battle.fighters[active].slot;
            if self.battle.move_active(to).is_ok() {
                cue(Cue::CardMove);
                let kind = FxKind::Move { from: l.cell_pos(Team::Player, from), to: l.cell_pos(Team::Player, to) };
                self.fx = Some(Fx { actor: active, kind, t: 0.0 });
            }
        }
    }

    fn draw(&self, l: &Layout, game: &Game, assets: &Assets) {
        let b = &self.battle;
        let k = l.k;
        let active = b.active();
        let player_turn = active.is_some_and(|a| b.fighters[a].team == Team::Player) && self.fx.is_none();
        let (targets, moves) = match (player_turn, active) {
            (true, Some(a)) => (b.targets(a), b.moves(a)),
            _ => (Vec::new(), Vec::new()),
        };
        let hovered_cell = self.cell_under_mouse(l);

        // The window: red marble, the title with both armies, the turn in the corner.
        let (_, _) = chrome::window(l.win, &battle_title(game), Skin::Red, false);
        // The frame between the panel and the formations.
        draw_line(l.panel.x + l.panel.w + 1.0, l.panel.y, l.panel.x + l.panel.w + 1.0, l.panel.y + l.panel.h, 1.5 * k, chrome::SILVER);

        // Empty cells, with the lit cells a step can go to.
        for (team, slot) in all_cells(b) {
            let p = l.cell_pos(team, slot);
            if b.at(team, slot).is_some_and(|i| b.fighters[i].alive()) {
                continue;
            }
            chrome::empty_cell(Rect::new(p.x, p.y, l.card.x, l.card.y), CellIcon::of(b.formation, slot), true);
            let is_move = team == Team::Player && moves.contains(&slot);
            let sq = l.portrait(p);
            if is_move {
                chrome::glow_frame(sq, FRIENDLY, false);
            }
            let lit = (b.is_deploying() || is_move) && team == Team::Player && hovered_cell == Some((team, slot));
            if lit {
                draw_rectangle(sq.x, sq.y, sq.w, sq.h, Color::new(0.4, 0.6, 1.0, 0.18));
            }
        }

        // The strip between the formations: what to do, or what just happened.
        let (hint, color) = self.strip_text(player_turn, &targets, &moves);
        chrome::divider(l.strip);
        // While deploying the Fight! button takes the strip's right end.
        let text_r = if b.is_deploying() { Rect { w: l.strip.w - 124.0 * k, ..l.strip } } else { Rect { w: l.strip.w - 60.0 * k, x: l.strip.x + 30.0 * k, ..l.strip } };
        chrome::hint_text(text_r, &hint, color);
        if !b.is_deploying() {
            let limit = b.content().options.battle_end_turn;
            let size = (11.0 * k).round();
            shadow_right(&format!("Turn {}/{limit}", b.round), l.strip.x + l.strip.w - 6.0 * k, l.strip.y + l.strip.h * 0.5 + size * 0.36, size, GOLD);
        }

        // Cards: the order of the next units after the active one, as small numbers.
        let queue: Vec<usize> = if b.outcome() == Outcome::Ongoing && !b.is_deploying() { b.queue().skip(1).take(3).collect() } else { Vec::new() };
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
                        p.y += dir * 14.0 * k * (lunge * PI).sin();
                    }
                    FxKind::Move { from, to } => p = from.lerp(to, (fx.t / MOVE_TIME).min(1.0)),
                }
            }
            let hovered = hovered_cell == Some((f.team, f.slot));
            let frame = if Some(i) == active && self.fx.is_none() && !b.is_deploying() {
                Some((ACTIVE, true))
            } else if targets.contains(&i) {
                let c = if f.team == Team::Player { FRIENDLY } else { HOSTILE };
                Some((if hovered { c } else { Color { a: 0.6, ..c } }, hovered))
            } else if self.selected == Some(f.slot) && f.team == Team::Player {
                Some((WHITE, true))
            } else {
                None
            };
            let order = queue.iter().position(|&q| q == i);
            self.draw_card(l, assets, i, p, frame, hovered && targets.contains(&i), order);
        }

        if let Some(fx) = &self.fx {
            self.draw_fx(l, fx);
        }
        if let Some(xp) = &self.xp {
            for a in xp {
                let f = &b.fighters[a.fighter];
                let sq = l.portrait(l.cell_pos(f.team, f.slot));
                let y = sq.y + sq.h * 0.38;
                draw_rectangle(sq.x + 4.0 * k, y, sq.w - 8.0 * k, 20.0 * k, Color::new(0.0, 0.12, 0.16, 0.8));
                shadow_centered(&format!("XP +{}", a.xp), sq.x + sq.w / 2.0, y + 15.0 * k, (15.0 * k).round(), XP_COLOR);
                if self.levels_gained(a) > 0 {
                    let y = y + 22.0 * k;
                    draw_rectangle(sq.x + 4.0 * k, y, sq.w - 8.0 * k, 18.0 * k, Color::new(0.3, 0.22, 0.02, 0.85));
                    shadow_centered("Level up!", sq.x + sq.w / 2.0, y + 14.0 * k, (13.0 * k).round(), GOLD);
                }
            }
        }
        // The left panel: the hovered unit, else the one acting.
        if let Some(id) = self.fighter_under_mouse(l).filter(|&i| b.fighters[i].alive()).or(active).or_else(|| b.fighters.iter().position(|f| f.is_hero)) {
            self.draw_panel(l, game, assets, id);
        } else {
            chrome::parchment(l.panel, true);
        }
        if player_turn {
            self.draw_preview(l, active.expect("player turn"));
        }
    }

    /// The strip's text: the deploy help, the last action, or what the player can do.
    fn strip_text(&self, player_turn: bool, targets: &[usize], moves: &[Slot]) -> (String, Color) {
        let b = &self.battle;
        if b.is_deploying() {
            return ("Arrange your army: click a card, then a cell. Enter or Fight! starts the battle".into(), GOLD);
        }
        if b.outcome() != Outcome::Ongoing {
            return ("The battle is over".into(), GOLD);
        }
        if let Some((line, _)) = &self.news {
            if !player_turn || self.fx.is_some() {
                return (line.clone(), CREAM);
            }
        }
        if player_turn {
            let hint = match (targets.is_empty(), moves.is_empty()) {
                (false, _) => "Click a framed card to act, your own to pass one action; SPACE ends the turn",
                (true, false) => "Nothing in reach: step to a lit cell, or press SPACE",
                (true, true) => "Nothing to do: press SPACE to end the turn",
            };
            return (hint.into(), Color::new(1.0, 0.55, 0.25, 1.0));
        }
        ("The enemy moves...".into(), Color::new(1.0, 0.55, 0.25, 1.0))
    }

    /// XP needed for fighter `f`'s next level, as the battle began.
    fn need(&self, f: &Fighter) -> i32 {
        self.battle.content().xp_to_next(f.unit, f.level)
    }

    /// Levels the award `a` will add to its fighter.
    fn levels_gained(&self, a: &XpAward) -> i32 {
        let f = &self.battle.fighters[a.fighter];
        let c = self.battle.content();
        let (mut level, mut xp, mut n) = (f.level, f.xp + a.xp, 0);
        while n < 100 && xp >= c.xp_to_next(f.unit, level) {
            xp -= c.xp_to_next(f.unit, level);
            level += 1;
            n += 1;
        }
        n
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_card(&self, l: &Layout, assets: &Assets, id: usize, p: Vec2, frame: Option<(Color, bool)>, aimed: bool, order: Option<usize>) {
        let f = &self.battle.fighters[id];
        let (s, base) = (&f.stats, &f.base);
        let k = l.k;
        let p = p.round();
        let (w, h) = (l.card.x, l.card.y);
        // The card's shadow, the portrait, the stat strip.
        draw_rectangle(p.x + 4.0 * k, p.y + 4.0 * k, w, h, Color::new(0.0, 0.0, 0.0, 0.45));
        let sq = l.portrait(p);
        assets.draw_portrait(f.unit, f.team, sq);
        if aimed {
            let tint = if f.team == Team::Player { Color::new(0.3, 0.5, 1.0, 0.22) } else { Color::new(1.0, 0.1, 0.05, 0.25) };
            draw_rectangle(sq.x, sq.y, sq.w, sq.h, tint);
        }
        draw_rectangle_lines(sq.x, sq.y, sq.w, sq.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let strip = Rect::new(p.x, p.y + w, w, h - w);
        unit_sheet::stat_strip(strip, s, base, f.power, f.hp, frame.is_some_and(|(c, _)| c == ACTIVE));

        // Badges: blessed / cursed / poisoned in the top-right corner, the hero's mark and
        // the turn order at the top left.
        let bs = 20.0 * k;
        let mut bx = sq.x + sq.w - bs * 0.6;
        let by = sq.y + bs * 0.6;
        for (on, art, c) in [
            (f.blessed, "army-2", BLUE_TEXT),
            (f.cursed, "army-3", PURPLE),
            (f.poisoned(), "sign-poison", GREEN),
            (f.bleed > 0, "Bonus39", RED),
        ] {
            if on {
                chrome::badge(art, bx, by, bs, c);
                bx -= bs * 0.9;
            }
        }
        if f.is_hero {
            chrome::badge("SI_Helm", sq.x + bs * 0.6, by, bs, GOLD);
        }
        if let Some(n) = order {
            let (ox, oy) = (sq.x + 3.0 * k, sq.y + sq.h - 16.0 * k);
            draw_rectangle(ox, oy, 13.0 * k, 13.0 * k, Color::new(0.0, 0.0, 0.0, 0.55));
            shadow_centered(&(n + 1).to_string(), ox + 6.5 * k, oy + 11.0 * k, (11.0 * k).round(), CREAM);
        }
        let fighting = !self.battle.is_deploying() && self.battle.outcome() == Outcome::Ongoing;
        if fighting && f.alive() && f.slot.row != Row::Reserve && self.battle.helpless(id) {
            draw_rectangle(sq.x, sq.y + sq.h - 16.0 * k, sq.w, 15.0 * k, Color::new(0.0, 0.0, 0.0, 0.5));
            shadow_centered("can't reach", sq.x + sq.w / 2.0, sq.y + sq.h - 4.0 * k, (11.0 * k).round(), Color::new(0.8, 0.8, 0.75, 1.0));
        }
        if let Some((c, strong)) = frame {
            chrome::glow_frame(sq, c, strong);
        }
    }

    fn draw_fx(&self, l: &Layout, fx: &Fx) {
        let FxKind::Act { hit } = &fx.kind else { return };
        let k = fx.t / STRIKE_TIME;
        let f = &self.battle.fighters[hit.target];
        let sq = l.portrait(l.cell_pos(f.team, f.slot));
        let school = self.battle.fighters[fx.actor].stats.magic;
        let (art, size) = effect_art(hit.kind, school);
        let centre = vec2(sq.x + sq.w / 2.0, sq.y + sq.h / 2.0);
        let drawn = chrome::effect(art, centre, sq.w * size, k, WHITE);
        let (flash, label, color) = match hit.kind {
            ActionKind::Heal => (Color::new(0.2, 1.0, 0.3, 0.4 * (1.0 - k)), format!("+{}", hit.amount), GREEN),
            ActionKind::Bless => (Color::new(0.4, 0.7, 1.0, 0.4 * (1.0 - k)), "blessed".into(), BLUE_TEXT),
            ActionKind::Curse => (Color::new(0.7, 0.2, 0.9, 0.4 * (1.0 - k)), "cursed".into(), PURPLE),
            ActionKind::LongStrike => (Color::new(1.0, 0.1, 0.1, 0.45 * (1.0 - k)), format!("-{} long!", hit.amount), WHITE),
            ActionKind::Strike => (Color::new(1.0, 0.5, 0.1, 0.45 * (1.0 - k)), format!("-{}", hit.amount), ORANGE),
            _ => (Color::new(1.0, 0.1, 0.1, 0.45 * (1.0 - k)), format!("-{}", hit.amount), WHITE),
        };
        if !drawn || hit.kind.is_hostile() {
            draw_rectangle(sq.x, sq.y, sq.w, sq.h, Color { a: flash.a * if drawn { 0.5 } else { 1.0 }, ..flash });
        }
        let y = sq.y + sq.h * 0.45 - 26.0 * l.k * k;
        shadow_centered(&label, sq.x + sq.w / 2.0, y, (22.0 * l.k).round(), color);
        if let Some(c) = hit.counter {
            let a = &self.battle.fighters[fx.actor];
            let q = l.portrait(l.cell_pos(a.team, a.slot));
            shadow_centered(&format!("-{c} counter"), q.x + q.w / 2.0, q.y + q.h * 0.45 - 26.0 * l.k * k, (18.0 * l.k).round(), RED);
        }
    }

    /// The hover box over a target (or the acting unit's own card, or a cell to step to):
    /// what a click would do.
    fn draw_preview(&self, l: &Layout, active: usize) {
        let b = &self.battle;
        let (head, effect, effect_color) = if let Some(t) = self.fighter_under_mouse(l) {
            let opts = b.options(active, t);
            match opts.first() {
                Some(&kind) => {
                    let (h, e) = preview_lines(b.preview(active, t, kind), kind, &b.fighters[t].name, b.fighters[t].hp);
                    let c = if kind.is_hostile() { Color::new(0.75, 0.05, 0.02, 1.0) } else { Color::new(0.05, 0.25, 0.75, 1.0) };
                    (h, Some(e), c)
                }
                None if t == active => (format!("Click (or press SPACE) to pass one action of \"{}\"", b.fighters[t].name), None, BLACK),
                None => return,
            }
        } else if let Some((Team::Player, slot)) = self.cell_under_mouse(l) {
            if !b.moves(active).contains(&slot) {
                return;
            }
            (format!("Click to move \"{}\" here", b.fighters[active].name), None, BLACK)
        } else {
            return;
        };
        let k = l.k;
        let size = (13.0 * k).round();
        let lh = 16.0 * k;
        let w = measure(&head, size).width.max(effect.as_ref().map_or(0.0, |e| measure(e, size).width)) + 14.0 * k;
        let h = lh * if effect.is_some() { 2.0 } else { 1.0 } + 8.0 * k;
        let (mx, my) = mouse_position();
        let x = (mx - w * 0.4).clamp(2.0, screen_width() - w - 2.0);
        let y = (my - h - 6.0 * k).max(2.0);
        draw_rectangle(x, y, w, h, Color::new(0.93, 0.89, 0.72, 0.95));
        draw_rectangle_lines(x, y, w, h, 1.0, Color::new(0.2, 0.15, 0.1, 1.0));
        text(&head, x + 7.0 * k, y + lh, size, Color::new(0.08, 0.05, 0.02, 1.0));
        if let Some(e) = effect {
            text(&e, x + 7.0 * k, y + 2.0 * lh, size, effect_color);
        }
    }

    /// The unit panel on the left, as the original's.
    fn draw_panel(&self, l: &Layout, game: &Game, assets: &Assets, id: usize) {
        let b = &self.battle;
        let f = &b.fighters[id];
        let items: [Option<ItemId>; 4] = match f.squad_index.and_then(|i| game.squad.get(i)) {
            Some(u) if f.team == Team::Player => u.items,
            _ => [None; 4],
        };
        let mut status = Vec::new();
        if b.active() == Some(id) && !b.is_deploying() {
            status.push((format!("Acting: {} of {} actions left", b.actions_left(), f.stats[Stat::Manevres].max(b.actions_left())), Color::new(0.5, 1.0, 0.5, 1.0)));
        }
        if !f.mods.is_empty() {
            status.push((format!("This turn: {}", f.mods.describe()), BLUE_TEXT));
        }
        if f.poisoned() {
            status.push(("Poisoned".into(), Color::new(0.5, 1.0, 0.4, 1.0)));
        }
        if f.bleed > 0 {
            status.push(("Bleeding".into(), RED_TEXT));
        }
        let hero = f.is_hero.then(|| HeroClass::ALL.into_iter().find(|h| h.unit() == f.unit)).flatten();
        let sheet = Sheet {
            kind: f.unit,
            name: &f.name,
            level: f.level,
            xp: f.xp,
            need: self.need(f),
            hp: f.hp,
            now: &f.stats,
            start: &f.base,
            power: f.power,
            wage: if f.is_hero || f.team == Team::Enemy { 0 } else { b.content().wage(f.unit) },
            items,
            back_row: f.slot.row == Row::Back,
            in_building: b.building_defence(f.team) > 0,
            hero,
            status,
        };
        let mut hover = None;
        unit_sheet::draw(assets, b.content(), l.panel, &sheet, false, &mut hover);
    }

    /// The result box over the unit panel, so the XP badges on the cards stay visible.
    fn result_overlay(&self, l: &Layout, game: &mut Game, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>, outcome: Outcome) -> Option<Screen> {
        let k = l.k;
        let r = Rect::new(l.panel.x + 8.0 * k, l.panel.y + 150.0 * k, l.panel.w - 16.0 * k, 200.0 * k);
        let (title, sub, color) = match (outcome, self.battle.end_reason()) {
            (Outcome::Victory, Some(EndReason::Surrender(_))) => ("Victory!", "The enemy surrenders.", GOLD),
            (Outcome::Victory, Some(EndReason::TurnLimit)) => ("Victory!", "The turns run out; the field is yours.", GOLD),
            (Outcome::Victory, _) => ("Victory!", "", GOLD),
            (_, Some(EndReason::Surrender(_))) => ("Defeat", "Your army surrenders.", RED_TEXT),
            _ => ("Defeat", "Your whole army has fallen.", RED_TEXT),
        };
        let (inner, _) = chrome::window(r, if outcome == Outcome::Victory { "Victory over the enemy!" } else { "Defeat in battle!" }, Skin::Marble, false);
        shadow_centered(title, inner.x + inner.w / 2.0, inner.y + 40.0 * k, (32.0 * k).round(), color);
        for (i, line) in wrap(sub, inner.w - 16.0 * k, (13.0 * k).round()).iter().enumerate() {
            shadow_centered(line, inner.x + inner.w / 2.0, inner.y + 64.0 * k + i as f32 * 15.0 * k, (13.0 * k).round(), CREAM);
        }
        let total: i32 = self.xp.iter().flatten().map(|a| a.xp).sum();
        if total > 0 {
            shadow_centered(&format!("Experience gained: {total}"), inner.x + inner.w / 2.0, inner.y + 100.0 * k, (14.0 * k).round(), XP_COLOR);
        }
        let (bw, bh) = (120.0 * k, 30.0 * k);
        let pressed = button(inner.x + (inner.w - bw) / 2.0, inner.y + inner.h - bh - 10.0 * k, bw, bh, "OK", true) || key(KeyCode::Enter);
        if !pressed {
            return None;
        }
        let losses = |lost: usize| if lost > 0 { format!(", {lost} fell") } else { String::new() };
        let result = game.resolve_battle(&self.battle);
        if let BattleResult::Victory { level_ups, .. } = &result {
            if !level_ups.is_empty() {
                cue(Cue::Upgrade);
            }
        }
        match result {
            BattleResult::Defeat => Some(Screen::GameOver),
            BattleResult::Victory { .. } if game.won() => Some(Screen::Victory),
            victory @ BattleResult::Victory { .. } => {
                dialogs.extend(Dialog::victory(game, &victory));
                *message = None;
                Some(Screen::WorldMap)
            }
            BattleResult::Withdrew { lost } => {
                *message = Some(format!("Nobody breaks. You withdraw{}; no experience without a victory.", losses(lost)));
                Some(Screen::WorldMap)
            }
        }
    }
}
