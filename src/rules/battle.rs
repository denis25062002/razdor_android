use super::formation::{Row, Slot};
use super::rng::Rng;
use super::units::{AttackKind, UnitKind};

/// After this many rounds the battle ends undecided.
pub const MAX_ROUNDS: u32 = 20;

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
    Stalemate,
}

#[derive(Clone, Debug)]
pub struct Fighter {
    pub kind: UnitKind,
    pub team: Team,
    pub hp: i32,
    pub slot: Slot,
    pub is_hero: bool,
    /// Index into the player's squad, for writing HP/slot back after the battle.
    pub squad_index: Option<usize>,
}

impl Fighter {
    pub fn alive(&self) -> bool {
        self.hp > 0
    }
}

/// One strike or heal, for the UI to animate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub target: usize,
    /// Damage dealt (0 = blocked by armor) or HP restored.
    pub amount: i32,
    pub heal: bool,
    pub killed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ActionError {
    NotDeploying,
    InvalidTarget,
    NotYourTurn,
}

pub struct Battle {
    pub fighters: Vec<Fighter>,
    pub round: u32,
    pub log: Vec<String>,
    order: Vec<usize>,
    turn: usize,
    deploying: bool,
    stalemate: bool,
    rng: Rng,
}

impl Battle {
    /// `player` entries are (kind, current hp, slot, squad index); squad index 0 is the hero.
    /// Starts in the deploy phase; call [`Battle::begin`] to fight.
    pub fn new(player: &[(UnitKind, i32, Slot, usize)], enemies: &[(UnitKind, Slot)], seed: u64) -> Self {
        let mut fighters: Vec<Fighter> = player
            .iter()
            .map(|&(kind, hp, slot, idx)| Fighter {
                kind,
                team: Team::Player,
                hp,
                slot,
                is_hero: idx == 0,
                squad_index: Some(idx),
            })
            .collect();
        fighters.extend(enemies.iter().map(|&(kind, slot)| Fighter {
            kind,
            team: Team::Enemy,
            hp: kind.stats().max_hp,
            slot,
            is_hero: false,
            squad_index: None,
        }));
        Battle {
            fighters,
            round: 0,
            log: Vec::new(),
            order: Vec::new(),
            turn: 0,
            deploying: true,
            stalemate: false,
            rng: Rng::new(seed),
        }
    }

    pub fn is_deploying(&self) -> bool {
        self.deploying
    }

    /// Living fighter of `team` standing on `slot`.
    pub fn at(&self, team: Team, slot: Slot) -> Option<usize> {
        self.fighters.iter().position(|f| f.alive() && f.team == team && f.slot == slot)
    }

    /// Deploy phase: move a player card to `to`, swapping with whoever is there.
    pub fn move_card(&mut self, from: Slot, to: Slot) -> Result<(), ActionError> {
        if !self.deploying {
            return Err(ActionError::NotDeploying);
        }
        let a = self.at(Team::Player, from).ok_or(ActionError::InvalidTarget)?;
        if let Some(b) = self.at(Team::Player, to) {
            self.fighters[b].slot = from;
        }
        self.fighters[a].slot = to;
        Ok(())
    }

    /// End deployment and start round 1.
    pub fn begin(&mut self) {
        if self.deploying {
            self.deploying = false;
            self.start_round();
        }
    }

    fn start_round(&mut self) {
        if self.round >= MAX_ROUNDS {
            self.stalemate = true;
            self.log.push(format!("{MAX_ROUNDS} rounds pass, nobody breaks"));
            return;
        }
        self.round += 1;
        let mut order: Vec<usize> = (0..self.fighters.len()).filter(|&i| self.fighters[i].alive()).collect();
        order.sort_by_key(|&i| {
            let f = &self.fighters[i];
            (-f.kind.stats().initiative, f.team == Team::Enemy, i)
        });
        self.order = order;
        self.turn = 0;
        self.log.push(format!("-- Round {} --", self.round));
    }

    /// Fighter whose turn it is; `None` while deploying or once the battle is over.
    pub fn active(&self) -> Option<usize> {
        if self.deploying || self.outcome() != Outcome::Ongoing {
            return None;
        }
        self.order.get(self.turn).copied()
    }

    /// Upcoming fighters this round, starting with the active one.
    pub fn queue(&self) -> impl Iterator<Item = usize> + '_ {
        self.order.iter().skip(self.turn).copied().filter(|&i| self.fighters[i].alive())
    }

    pub fn outcome(&self) -> Outcome {
        let hero_alive = self.fighters.iter().any(|f| f.is_hero && f.alive());
        let enemies_alive = self.fighters.iter().any(|f| f.team == Team::Enemy && f.alive());
        if !hero_alive {
            Outcome::Defeat
        } else if !enemies_alive {
            Outcome::Victory
        } else if self.stalemate {
            Outcome::Stalemate
        } else {
            Outcome::Ongoing
        }
    }

    fn row_occupied(&self, team: Team, row: Row) -> bool {
        self.fighters.iter().any(|f| f.alive() && f.team == team && f.slot.row == row)
    }

    /// A warrior in the back row is stuck behind its own living front line.
    pub fn blocked(&self, id: usize) -> bool {
        let f = &self.fighters[id];
        f.kind.stats().attack == AttackKind::Melee && f.slot.row == Row::Back && self.row_occupied(f.team, Row::Front)
    }

    pub fn can_target(&self, id: usize, target: usize) -> bool {
        let f = &self.fighters[id];
        let t = &self.fighters[target];
        if !f.alive() || !t.alive() {
            return false;
        }
        match f.kind.stats().attack {
            AttackKind::Heal { .. } => t.team == f.team && t.hp < t.kind.stats().max_hp,
            AttackKind::Ranged | AttackKind::Magic => t.team != f.team,
            AttackKind::Melee => {
                t.team != f.team
                    && !self.blocked(id)
                    && (t.slot.row == Row::Front || !self.row_occupied(t.team, Row::Front))
            }
        }
    }

    pub fn targets(&self, id: usize) -> Vec<usize> {
        (0..self.fighters.len()).filter(|&t| self.can_target(id, t)).collect()
    }

    /// Attack or heal `target` with the active fighter; ends its turn.
    pub fn act(&mut self, target: usize) -> Result<Vec<Hit>, ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        if !self.can_target(id, target) {
            return Err(ActionError::InvalidTarget);
        }
        let stats = self.fighters[id].kind.stats();
        let name = self.fighters[id].kind.name();
        let tname = self.fighters[target].kind.name();
        let mut hits = Vec::new();

        if let AttackKind::Heal { amount } = stats.attack {
            let t = &mut self.fighters[target];
            let before = t.hp;
            t.hp = (t.hp + amount).min(t.kind.stats().max_hp);
            let healed = t.hp - before;
            self.log.push(format!("{name} heals {tname} +{healed}"));
            hits.push(Hit { target, amount: healed, heal: true, killed: false });
        } else {
            let armor = if stats.attack == AttackKind::Magic { 0 } else { self.fighters[target].kind.stats().armor };
            for _ in 0..stats.attacks {
                if !self.fighters[target].alive() {
                    break;
                }
                let dmg = (self.rng.range(stats.dmg_min, stats.dmg_max) - armor).max(0);
                let t = &mut self.fighters[target];
                t.hp -= dmg;
                let killed = !t.alive();
                self.log.push(match (dmg, killed) {
                    (0, _) => format!("{name} hits {tname}: blocked"),
                    (_, true) => format!("{name} kills {tname} ({dmg})"),
                    _ => format!("{name} hits {tname} for {dmg}"),
                });
                hits.push(Hit { target, amount: dmg, heal: false, killed });
            }
        }
        self.end_turn();
        Ok(hits)
    }

    pub fn skip(&mut self) {
        if let Some(id) = self.active() {
            self.log.push(format!("{} waits", self.fighters[id].kind.name()));
            self.end_turn();
        }
    }

    fn end_turn(&mut self) {
        if self.outcome() != Outcome::Ongoing {
            return;
        }
        loop {
            self.turn += 1;
            if self.turn >= self.order.len() {
                self.start_round();
                return;
            }
            if self.fighters[self.order[self.turn]].alive() {
                return;
            }
        }
    }

    /// Target the AI would pick for the active fighter.
    pub fn ai_choice(&self) -> Option<usize> {
        let id = self.active()?;
        let heals = matches!(self.fighters[id].kind.stats().attack, AttackKind::Heal { .. });
        let key = |t: usize| {
            let f = &self.fighters[t];
            // Healers pick the lowest HP fraction (permille); attackers the lowest HP.
            if heals { f.hp * 1000 / f.kind.stats().max_hp } else { f.hp }
        };
        self.targets(id).into_iter().min_by_key(|&t| (key(t), t))
    }

    /// Plays the active fighter's turn automatically.
    /// Returns the actor and its hits (empty if it had to wait).
    pub fn ai_turn(&mut self) -> Option<(usize, Vec<Hit>)> {
        let id = self.active()?;
        match self.ai_choice() {
            Some(t) => Some((id, self.act(t).unwrap_or_default())),
            None => {
                self.skip();
                Some((id, Vec::new()))
            }
        }
    }

    /// Final (squad index, hp, slot) of the player's fighters; dead ones have hp <= 0.
    pub fn player_results(&self) -> Vec<(usize, i32, Slot)> {
        self.fighters.iter().filter_map(|f| f.squad_index.map(|i| (i, f.hp, f.slot))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Row::*;
    use UnitKind::*;

    const fn f(col: u8) -> Slot {
        Slot::new(Front, col)
    }
    const fn b(col: u8) -> Slot {
        Slot::new(Back, col)
    }

    fn battle(player: &[(UnitKind, Slot)], enemies: &[(UnitKind, Slot)]) -> Battle {
        let p: Vec<_> = player.iter().enumerate().map(|(i, &(k, s))| (k, k.stats().max_hp, s, i)).collect();
        let mut bt = Battle::new(&p, enemies, 7);
        bt.begin();
        bt
    }

    #[test]
    fn turn_order_by_initiative_player_first_on_ties() {
        // Ranger 7, Archmage 6, Bandit chief 6: the tie goes to the player.
        let bt = battle(&[(Ranger, b(2)), (Archmage, b(3))], &[(BanditChief, f(2))]);
        let order: Vec<_> = bt.queue().map(|i| bt.fighters[i].kind).collect();
        assert_eq!(order, vec![Ranger, Archmage, BanditChief]);
    }

    #[test]
    fn warrior_reaches_back_row_only_when_front_is_empty() {
        let mut bt = battle(&[(Knight, f(2))], &[(Bandit, f(0)), (BanditArcher, b(5))]);
        let knight = bt.active().unwrap();
        assert_eq!(bt.targets(knight), vec![1]);
        bt.fighters[1].hp = 0;
        assert_eq!(bt.targets(knight), vec![2]);
    }

    #[test]
    fn back_row_warrior_is_blocked_by_own_front_line() {
        let mut bt = battle(&[(Knight, b(2)), (Spearman, f(0))], &[(Bandit, f(0))]);
        assert!(bt.blocked(0));
        assert!(bt.targets(0).is_empty());
        bt.fighters[1].hp = 0;
        assert!(!bt.blocked(0));
        assert_eq!(bt.targets(0), vec![2]);
    }

    #[test]
    fn shooters_and_mages_hit_any_row() {
        let bt = battle(&[(Ranger, b(0)), (Archmage, b(1))], &[(Bandit, f(0)), (BanditArcher, b(5))]);
        assert_eq!(bt.targets(0), vec![2, 3]);
        assert_eq!(bt.targets(1), vec![2, 3]);
    }

    #[test]
    fn armor_can_block_completely_and_magic_ignores_it() {
        // Bandit archer 5-8 vs knight armor 5 gives 0..=3; over many seeds some are blocked.
        let mut blocked = false;
        for seed in 0..30 {
            let p = [(Knight, 60, f(2), 0)];
            let mut bt = Battle::new(&p, &[(BanditArcher, b(2))], seed);
            bt.begin();
            bt.skip(); // knight and archer both init 5: knight first
            let hits = bt.act(0).unwrap();
            assert!(hits[0].amount <= 3);
            blocked |= hits[0].amount == 0;
        }
        assert!(blocked, "armor never blocked");

        let mut bt = battle(&[(Archmage, b(2))], &[(BanditChief, f(2))]);
        let hits = bt.act(1).unwrap();
        assert!((9..=13).contains(&hits[0].amount));
    }

    #[test]
    fn ranger_strikes_twice() {
        let mut bt = battle(&[(Ranger, b(2))], &[(BanditChief, f(2))]);
        let hits = bt.act(1).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(bt.fighters[1].hp, 65 - hits.iter().map(|h| h.amount).sum::<i32>());
    }

    #[test]
    fn extra_attacks_stop_when_target_dies() {
        let mut bt = battle(&[(Ranger, b(2))], &[(Bandit, f(2)), (Bandit, f(3))]);
        bt.fighters[1].hp = 1;
        let hits = bt.act(1).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].killed);
    }

    #[test]
    fn heal_targets_wounded_allies_and_caps() {
        let mut bt = battle(&[(Knight, f(2)), (Healer, b(2))], &[(Bandit, f(2))]);
        bt.skip(); // knight (5)
        bt.skip(); // bandit (4)
        let healer = bt.active().unwrap();
        assert_eq!(bt.fighters[healer].kind, Healer);
        assert!(bt.targets(healer).is_empty(), "nobody is wounded");
        bt.fighters[0].hp = 55;
        bt.act(0).unwrap();
        assert_eq!(bt.fighters[0].hp, 60);
    }

    #[test]
    fn deploy_moves_and_swaps_only_before_the_fight() {
        let p = [(Knight, 60, f(2), 0), (Archer, 22, b(2), 1)];
        let mut bt = Battle::new(&p, &[(Bandit, f(2))], 1);
        assert_eq!(bt.active(), None);
        bt.move_card(f(2), b(2)).unwrap(); // swap
        assert_eq!((bt.fighters[0].slot, bt.fighters[1].slot), (b(2), f(2)));
        bt.move_card(f(2), f(5)).unwrap(); // move to an empty cell
        assert_eq!(bt.fighters[1].slot, f(5));
        assert_eq!(bt.move_card(f(0), f(1)), Err(ActionError::InvalidTarget));
        bt.begin();
        assert_eq!(bt.move_card(f(5), f(4)), Err(ActionError::NotDeploying));
    }

    #[test]
    fn victory_defeat_and_stalemate() {
        let mut bt = battle(&[(Knight, f(2))], &[(Bandit, f(2))]);
        assert_eq!(bt.outcome(), Outcome::Ongoing);
        bt.fighters[1].hp = 0;
        assert_eq!(bt.outcome(), Outcome::Victory);

        let mut bt = battle(&[(Knight, f(2)), (Spearman, f(3))], &[(Bandit, f(2))]);
        bt.fighters[0].hp = 0;
        assert_eq!(bt.outcome(), Outcome::Defeat);

        let mut bt = battle(&[(Knight, f(2))], &[(Bandit, f(2))]);
        while bt.outcome() == Outcome::Ongoing {
            bt.skip();
        }
        assert_eq!(bt.outcome(), Outcome::Stalemate);
        assert_eq!(bt.round, MAX_ROUNDS);
    }

    #[test]
    fn dead_units_are_skipped_in_turn_order() {
        let mut bt = battle(&[(Knight, f(2))], &[(Bandit, f(1)), (Bandit, f(2))]);
        bt.fighters[1].hp = 0;
        bt.skip(); // knight
        assert_eq!(bt.active(), Some(2));
    }

    #[test]
    fn ai_prefers_weakest_legal_target() {
        // The spearman is weakest but in the back row; a bandit must hit the front row.
        let mut bt = battle(&[(Knight, f(2)), (Swordsman, f(3)), (Spearman, b(2))], &[(Bandit, f(2))]);
        bt.fighters[1].hp = 20;
        bt.fighters[2].hp = 3;
        bt.skip(); // knight (5)
        bt.skip(); // swordsman (5)
        bt.skip(); // spearman (4, ties go to the player)
        assert_eq!(bt.fighters[bt.active().unwrap()].kind, Bandit);
        assert_eq!(bt.ai_choice(), Some(1));
    }

    #[test]
    fn ai_waits_when_it_has_no_action() {
        let mut bt = battle(&[(Knight, f(2))], &[(Bandit, f(2)), (BanditChief, b(2))]);
        assert_eq!(bt.fighters[bt.active().unwrap()].kind, BanditChief);
        let (actor, hits) = bt.ai_turn().unwrap();
        assert_eq!(actor, 2);
        assert!(hits.is_empty(), "chief is blocked behind the bandit");
    }
}
