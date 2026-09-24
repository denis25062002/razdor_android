use std::collections::{HashMap, VecDeque};

use super::rng::Rng;
use super::units::{AttackKind, UnitKind};

pub type Pos = (i32, i32);

pub const GRID_W: i32 = 10;
pub const GRID_H: i32 = 8;

const PLAYER_DEPLOY: [Pos; 5] = [(1, 3), (1, 5), (1, 1), (0, 4), (0, 2)];
const ENEMY_DEPLOY: [Pos; 6] = [(8, 4), (8, 2), (8, 6), (9, 3), (9, 5), (9, 1)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Team {
    Player,
    Enemy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ongoing,
    Victory,
    Defeat,
}

#[derive(Clone, Debug)]
pub struct Fighter {
    pub kind: UnitKind,
    pub team: Team,
    pub hp: i32,
    pub pos: Pos,
    pub is_hero: bool,
    /// Index into the player's squad, for writing HP back after the battle.
    pub squad_index: Option<usize>,
}

impl Fighter {
    pub fn alive(&self) -> bool {
        self.hp > 0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ActionError {
    NotReachable,
    InvalidTarget,
    BattleOver,
}

pub struct Battle {
    pub fighters: Vec<Fighter>,
    pub round: u32,
    pub log: Vec<String>,
    order: Vec<usize>,
    turn: usize,
    moved: bool,
    rng: Rng,
}

pub fn chebyshev(a: Pos, b: Pos) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

fn in_bounds(p: Pos) -> bool {
    p.0 >= 0 && p.1 >= 0 && p.0 < GRID_W && p.1 < GRID_H
}

impl Battle {
    /// `player` entries are (kind, current hp, squad index); squad index 0 is the hero.
    pub fn new(player: &[(UnitKind, i32, usize)], enemies: &[UnitKind], seed: u64) -> Self {
        let mut fighters = Vec::new();
        for (&(kind, hp, idx), &pos) in player.iter().zip(PLAYER_DEPLOY.iter()) {
            fighters.push(Fighter {
                kind,
                team: Team::Player,
                hp,
                pos,
                is_hero: idx == 0,
                squad_index: Some(idx),
            });
        }
        for (&kind, &pos) in enemies.iter().zip(ENEMY_DEPLOY.iter()) {
            fighters.push(Fighter {
                kind,
                team: Team::Enemy,
                hp: kind.stats().max_hp,
                pos,
                is_hero: false,
                squad_index: None,
            });
        }
        let mut b = Battle {
            fighters,
            round: 0,
            log: Vec::new(),
            order: Vec::new(),
            turn: 0,
            moved: false,
            rng: Rng::new(seed),
        };
        b.start_round();
        b
    }

    fn start_round(&mut self) {
        self.round += 1;
        let mut order: Vec<usize> = (0..self.fighters.len()).filter(|&i| self.fighters[i].alive()).collect();
        order.sort_by_key(|&i| {
            let f = &self.fighters[i];
            (-f.kind.stats().initiative, f.team == Team::Enemy, i)
        });
        self.order = order;
        self.turn = 0;
        self.moved = false;
        self.log.push(format!("-- Round {} --", self.round));
    }

    /// Id of the fighter whose turn it is.
    pub fn active(&self) -> usize {
        self.order[self.turn]
    }

    pub fn has_moved(&self) -> bool {
        self.moved
    }

    /// Upcoming fighters this round, starting with the active one.
    pub fn queue(&self) -> impl Iterator<Item = usize> + '_ {
        self.order[self.turn..].iter().copied().filter(|&i| self.fighters[i].alive())
    }

    pub fn occupant(&self, p: Pos) -> Option<usize> {
        self.fighters.iter().position(|f| f.alive() && f.pos == p)
    }

    pub fn outcome(&self) -> Outcome {
        let hero_alive = self.fighters.iter().any(|f| f.is_hero && f.alive());
        let enemies_alive = self.fighters.iter().any(|f| f.team == Team::Enemy && f.alive());
        if !hero_alive {
            Outcome::Defeat
        } else if !enemies_alive {
            Outcome::Victory
        } else {
            Outcome::Ongoing
        }
    }

    /// Cells the fighter can end its move on, mapped to their BFS parent.
    /// Includes the fighter's own cell. Empty movement once it has moved this turn.
    pub fn reachable(&self, id: usize) -> HashMap<Pos, Pos> {
        let f = &self.fighters[id];
        let limit = if id == self.active() && self.moved { 0 } else { f.kind.stats().moves };
        let mut parents = HashMap::new();
        parents.insert(f.pos, f.pos);
        let mut queue = VecDeque::from([(f.pos, 0)]);
        while let Some((p, d)) = queue.pop_front() {
            if d == limit {
                continue;
            }
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let n = (p.0 + dx, p.1 + dy);
                    if (dx, dy) == (0, 0) || !in_bounds(n) || parents.contains_key(&n) || self.occupant(n).is_some() {
                        continue;
                    }
                    parents.insert(n, p);
                    queue.push_back((n, d + 1));
                }
            }
        }
        parents
    }

    fn path(parents: &HashMap<Pos, Pos>, dest: Pos) -> Vec<Pos> {
        let mut path = vec![dest];
        let mut cur = dest;
        while parents[&cur] != cur {
            cur = parents[&cur];
            path.push(cur);
        }
        path.reverse();
        path
    }

    fn has_adjacent_enemy(&self, id: usize, at: Pos) -> bool {
        let team = self.fighters[id].team;
        self.fighters.iter().any(|o| o.alive() && o.team != team && chebyshev(o.pos, at) == 1)
    }

    /// Whether `id` standing on `from` may act on `target`.
    pub fn can_target_from(&self, id: usize, from: Pos, target: usize) -> bool {
        let f = &self.fighters[id];
        let t = &self.fighters[target];
        if !t.alive() {
            return false;
        }
        let dist = chebyshev(from, t.pos);
        match f.kind.stats().attack {
            AttackKind::Melee => t.team != f.team && dist == 1,
            AttackKind::Ranged { range, .. } => t.team != f.team && dist <= range,
            AttackKind::Heal { range, .. } => {
                t.team == f.team && dist <= range && t.hp < t.kind.stats().max_hp
            }
        }
    }

    pub fn targets_from(&self, id: usize, from: Pos) -> Vec<usize> {
        (0..self.fighters.len()).filter(|&t| self.can_target_from(id, from, t)).collect()
    }

    pub fn targets(&self, id: usize) -> Vec<usize> {
        self.targets_from(id, self.fighters[id].pos)
    }

    /// Move the active fighter. Returns the path walked (for animation).
    /// If nothing can be targeted from the new cell, the turn ends automatically.
    pub fn move_active(&mut self, dest: Pos) -> Result<Vec<Pos>, ActionError> {
        if self.outcome() != Outcome::Ongoing {
            return Err(ActionError::BattleOver);
        }
        let id = self.active();
        let parents = self.reachable(id);
        if !parents.contains_key(&dest) || dest == self.fighters[id].pos {
            return Err(ActionError::NotReachable);
        }
        let path = Self::path(&parents, dest);
        self.fighters[id].pos = dest;
        self.moved = true;
        if self.targets(id).is_empty() {
            self.end_turn();
        }
        Ok(path)
    }

    /// Attack or heal `target` with the active fighter; ends the turn.
    pub fn act(&mut self, target: usize) -> Result<(), ActionError> {
        if self.outcome() != Outcome::Ongoing {
            return Err(ActionError::BattleOver);
        }
        let id = self.active();
        if !self.can_target(id, target) {
            return Err(ActionError::InvalidTarget);
        }
        let stats = self.fighters[id].kind.stats();
        let name = self.fighters[id].kind.name();
        let tname = self.fighters[target].kind.name();
        match stats.attack {
            AttackKind::Heal { amount, .. } => {
                let t = &mut self.fighters[target];
                let before = t.hp;
                t.hp = (t.hp + amount).min(t.kind.stats().max_hp);
                self.log.push(format!("{name} heals {tname} for {}", t.hp - before));
            }
            attack => {
                let mut dmg = self.rng.range(stats.dmg_min, stats.dmg_max);
                let magic = matches!(attack, AttackKind::Ranged { magic: true, .. });
                if !magic {
                    dmg -= self.fighters[target].kind.stats().armor;
                }
                if matches!(attack, AttackKind::Ranged { .. }) && self.has_adjacent_enemy(id, self.fighters[id].pos) {
                    dmg /= 2;
                }
                let dmg = dmg.max(1);
                let t = &mut self.fighters[target];
                t.hp -= dmg;
                let killed = if t.hp <= 0 { ", killed!" } else { "" };
                self.log.push(format!("{name} hits {tname} for {dmg}{killed}"));
            }
        }
        self.end_turn();
        Ok(())
    }

    pub fn can_target(&self, id: usize, target: usize) -> bool {
        self.can_target_from(id, self.fighters[id].pos, target)
    }

    pub fn skip(&mut self) {
        if self.outcome() == Outcome::Ongoing {
            self.end_turn();
        }
    }

    fn end_turn(&mut self) {
        if self.outcome() != Outcome::Ongoing {
            return;
        }
        self.moved = false;
        loop {
            self.turn += 1;
            if self.turn >= self.order.len() {
                self.start_round();
                return;
            }
            if self.fighters[self.active()].alive() {
                return;
            }
        }
    }

    /// Plays the active fighter's whole turn automatically.
    /// Returns the path moved (possibly just the start cell).
    pub fn ai_turn(&mut self) -> Vec<Pos> {
        let id = self.active();
        let start = self.fighters[id].pos;
        let parents = self.reachable(id);

        // 1. Best action reachable this turn: weakest target, then shortest walk.
        let mut best: Option<(i32, usize, Pos, usize)> = None;
        for (&cell, _) in &parents {
            let steps = chebyshev(start, cell) as usize;
            for t in self.targets_from(id, cell) {
                let key = (self.fighters[t].hp, steps, cell, t);
                if best.map_or(true, |b| (key.0, key.1) < (b.0, b.1)) {
                    best = Some(key);
                }
            }
        }
        if let Some((_, _, cell, target)) = best {
            let path = Self::path(&parents, cell);
            self.fighters[id].pos = cell;
            self.moved = true;
            let _ = self.act(target);
            return path;
        }

        // 2. Otherwise close in on the nearest opponent.
        let team = self.fighters[id].team;
        let foes: Vec<Pos> = self.fighters.iter().filter(|f| f.alive() && f.team != team).map(|f| f.pos).collect();
        let dist_to_foe = |p: Pos| foes.iter().map(|&e| chebyshev(p, e)).min().unwrap_or(0);
        let cell = parents
            .keys()
            .copied()
            .min_by_key(|&c| (dist_to_foe(c), chebyshev(start, c), c))
            .unwrap_or(start);
        let path = Self::path(&parents, cell);
        self.fighters[id].pos = cell;
        self.log.push(format!("{} advances", self.fighters[id].kind.name()));
        self.end_turn();
        path
    }

    /// Final (squad index, hp) of the player's fighters, dead ones with hp <= 0.
    pub fn player_results(&self) -> Vec<(usize, i32)> {
        self.fighters.iter().filter_map(|f| f.squad_index.map(|i| (i, f.hp))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use UnitKind::*;

    fn battle(player: &[UnitKind], enemies: &[UnitKind]) -> Battle {
        let p: Vec<_> = player.iter().enumerate().map(|(i, &k)| (k, k.stats().max_hp, i)).collect();
        Battle::new(&p, enemies, 7)
    }

    #[test]
    fn turn_order_by_initiative_player_first_on_ties() {
        // Ranger 7, Bandit chief 6, Archmage 6 — tie goes to the player.
        let b = battle(&[Ranger, Archmage], &[BanditChief]);
        let order: Vec<_> = b.queue().map(|i| b.fighters[i].kind).collect();
        assert_eq!(order, vec![Ranger, Archmage, BanditChief]);
    }

    #[test]
    fn movement_limited_by_moves_and_blocked_by_units() {
        let mut b = battle(&[Knight, Spearman], &[Bandit]);
        let knight = b.active();
        let reach = b.reachable(knight);
        let start = b.fighters[knight].pos;
        assert!(reach.keys().all(|&p| chebyshev(p, start) <= 3));
        let spear_pos = b.fighters[1].pos;
        assert!(!reach.contains_key(&spear_pos));
        assert_eq!(b.move_active((9, 0)), Err(ActionError::NotReachable));
    }

    #[test]
    fn melee_requires_adjacency_and_armor_reduces_damage() {
        let mut b = battle(&[Knight], &[Bandit]);
        b.fighters[1].pos = (5, 3);
        let knight = b.active();
        assert!(!b.can_target(knight, 1));
        b.fighters[0].pos = (4, 3);
        assert!(b.can_target(knight, 1));
        b.act(1).unwrap();
        let dealt = 26 - b.fighters[1].hp;
        assert!((10 - 1..=14 - 1).contains(&dealt), "dealt {dealt}");
    }

    #[test]
    fn magic_ignores_armor() {
        let mut b = battle(&[Archmage], &[BanditChief]);
        b.act(1).unwrap_err(); // out of range (distance 7)
        b.fighters[0].pos = (3, 3);
        b.act(1).unwrap();
        let dealt = 55 - b.fighters[1].hp;
        assert!((9..=13).contains(&dealt), "dealt {dealt}");
    }

    #[test]
    fn ranged_halved_when_enemy_adjacent() {
        let mut b = battle(&[Ranger], &[BanditChief]);
        b.fighters[1].pos = (2, 3); // adjacent to ranger at (1,3)
        b.act(1).unwrap();
        let dealt = 55 - b.fighters[1].hp;
        assert!((1..=(11 - 3) / 2).contains(&dealt), "dealt {dealt}");
    }

    #[test]
    fn heal_targets_wounded_allies_and_caps() {
        let mut b = battle(&[Knight, Healer], &[Bandit]);
        b.skip(); // knight's turn
        // bandit (init 4) and healer (init 3): bandit goes before healer
        assert_eq!(b.fighters[b.active()].kind, Bandit);
        b.skip();
        let healer = b.active();
        assert_eq!(b.fighters[healer].kind, Healer);
        assert!(!b.can_target(healer, 0), "full hp ally is not a target");
        b.fighters[0].hp = 55;
        b.act(0).unwrap();
        assert_eq!(b.fighters[0].hp, 60);
    }

    #[test]
    fn victory_and_defeat() {
        let mut b = battle(&[Knight], &[Bandit]);
        assert_eq!(b.outcome(), Outcome::Ongoing);
        b.fighters[1].hp = 0;
        assert_eq!(b.outcome(), Outcome::Victory);
        let mut b = battle(&[Knight, Spearman], &[Bandit]);
        b.fighters[0].hp = 0;
        assert_eq!(b.outcome(), Outcome::Defeat);
    }

    #[test]
    fn dead_units_are_skipped_in_turn_order() {
        let mut b = battle(&[Knight], &[Bandit, Bandit]);
        b.fighters[1].hp = 0;
        b.skip(); // knight
        assert_eq!(b.active(), 2);
    }

    #[test]
    fn ai_approaches_then_attacks() {
        let mut b = battle(&[Spearman], &[Bandit]);
        b.fighters[0].is_hero = true;
        b.skip(); // spearman (init 4 tie, player first)
        let before = chebyshev(b.fighters[0].pos, b.fighters[1].pos);
        b.ai_turn();
        let after = chebyshev(b.fighters[0].pos, b.fighters[1].pos);
        assert!(after < before);
        // Put it next door: next AI turn it attacks.
        b.fighters[1].pos = (2, 3);
        b.skip();
        b.ai_turn();
        assert!(b.fighters[0].hp < 30);
    }

    #[test]
    fn ai_prefers_weakest_target() {
        let mut b = battle(&[Knight, Spearman], &[BanditArcher]);
        b.fighters[1].hp = 5;
        b.skip(); // knight; archer (init 5) acts before spearman (init 4)
        assert_eq!(b.fighters[b.active()].kind, BanditArcher);
        b.fighters[2].pos = (4, 4);
        b.ai_turn();
        assert!(b.fighters[1].hp < 5 || !b.fighters[1].alive());
        assert_eq!(b.fighters[0].hp, 60);
    }

    #[test]
    fn moving_with_no_targets_ends_turn() {
        let mut b = battle(&[Knight], &[Bandit]);
        let knight = b.active();
        b.move_active((2, 3)).unwrap();
        assert_ne!(b.active(), knight);
    }
}
