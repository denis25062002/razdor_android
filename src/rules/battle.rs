use super::formation::{Row, Slot, COLS};
use super::rng::Rng;
use super::items::{Effect, ItemId, Passives, SLOTS};
use super::units::{AttackKind, Stats, Unit, UnitKind};

/// After this many rounds the battle ends undecided.
pub const MAX_ROUNDS: u32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Team {
    Player,
    Enemy,
}

impl Team {
    pub fn other(self) -> Team {
        match self {
            Team::Player => Team::Enemy,
            Team::Enemy => Team::Player,
        }
    }
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
    /// Stats with gear and drunk potions applied.
    pub stats: Stats,
    pub passives: Passives,
    /// Gear and potions; potions vanish once drunk.
    pub items: [Option<ItemId>; SLOTS],
}

impl Fighter {
    fn new(unit: &Unit, team: Team, squad_index: Option<usize>) -> Self {
        Fighter {
            kind: unit.kind,
            team,
            hp: unit.hp,
            slot: unit.slot,
            is_hero: squad_index == Some(0),
            squad_index,
            stats: unit.stats(),
            passives: unit.passives(),
            items: unit.items,
        }
    }

    pub fn alive(&self) -> bool {
        self.hp > 0
    }

    /// Item slots holding a potion this fighter could drink right now.
    pub fn drinkable(&self) -> Vec<usize> {
        (0..SLOTS)
            .filter(|&i| match self.items[i].and_then(|it| it.def().effect) {
                Some(Effect::Heal(_)) => self.hp < self.stats.max_hp,
                Some(Effect::Might(_)) => true,
                _ => false,
            })
            .collect()
    }
}

/// One strike or heal, for the UI to animate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub target: usize,
    /// Damage dealt or HP restored.
    pub amount: i32,
    pub heal: bool,
    pub flank: bool,
    pub killed: bool,
    /// A potion of might: `amount` is the damage gained.
    pub boost: bool,
}

/// One AI action, for the UI to animate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Strike { actor: usize, hit: Hit },
    Move { actor: usize, from: Slot, to: Slot },
    Wait { actor: usize },
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
    actions_left: i32,
    deploying: bool,
    stalemate: bool,
    rng: Rng,
}

fn neighbours(s: Slot) -> impl Iterator<Item = Slot> {
    let other_row = Slot::new(if s.row == Row::Front { Row::Back } else { Row::Front }, s.col);
    let left = s.col.checked_sub(1).map(|c| Slot::new(s.row, c));
    let right = (s.col + 1 < COLS).then(|| Slot::new(s.row, s.col + 1));
    [Some(other_row), left, right].into_iter().flatten()
}

impl Battle {
    /// `player` entries are (squad index, unit); squad index 0 is the hero.
    /// Starts in the deploy phase; call [`Battle::begin`] to fight.
    pub fn new(player: &[(usize, &Unit)], enemies: &[(UnitKind, Slot)], seed: u64) -> Self {
        let mut fighters: Vec<Fighter> =
            player.iter().map(|&(idx, unit)| Fighter::new(unit, Team::Player, Some(idx))).collect();
        fighters.extend(enemies.iter().map(|&(kind, slot)| Fighter::new(&Unit::new(kind, slot), Team::Enemy, None)));
        Battle {
            fighters,
            round: 0,
            log: Vec::new(),
            order: Vec::new(),
            turn: 0,
            actions_left: 0,
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
            self.collapse(Team::Player);
            self.collapse(Team::Enemy);
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
            (-f.stats.initiative, f.team == Team::Enemy, i)
        });
        self.order = order;
        self.turn = 0;
        self.log.push(format!("-- Round {} --", self.round));
        for f in self.fighters.iter_mut().filter(|f| f.alive() && f.passives.regen > 0) {
            let healed = (f.hp + f.passives.regen).min(f.stats.max_hp) - f.hp;
            if healed > 0 {
                f.hp += healed;
                self.log.push(format!("{} regenerates +{healed}", f.kind.name()));
            }
        }
        self.begin_turn();
    }

    fn begin_turn(&mut self) {
        if let Some(&id) = self.order.get(self.turn) {
            self.actions_left = self.fighters[id].stats.actions;
        }
    }

    /// Fighter whose turn it is; `None` while deploying or once the battle is over.
    pub fn active(&self) -> Option<usize> {
        if self.deploying || self.outcome() != Outcome::Ongoing {
            return None;
        }
        self.order.get(self.turn).copied()
    }

    pub fn actions_left(&self) -> i32 {
        self.actions_left
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

    /// With its front row gone, a side's back row steps forward.
    fn collapse(&mut self, team: Team) {
        if self.row_occupied(team, Row::Front) || !self.row_occupied(team, Row::Back) {
            return;
        }
        for f in self.fighters.iter_mut().filter(|f| f.alive() && f.team == team) {
            f.slot.row = Row::Front;
        }
        let side = if team == Team::Player { "Your" } else { "The enemy" };
        self.log.push(format!("{side} rear steps forward"));
    }

    /// A warrior in the back row cannot strike at all.
    pub fn helpless(&self, id: usize) -> bool {
        let f = &self.fighters[id];
        f.stats.attack == AttackKind::Melee && f.slot.row == Row::Back
    }

    /// Whether `id`, standing on `from`, may act on `target`; `Some(true)` for a flank strike.
    fn reach_from(&self, id: usize, from: Slot, target: usize) -> Option<bool> {
        let f = &self.fighters[id];
        let t = &self.fighters[target];
        if !f.alive() || !t.alive() {
            return None;
        }
        match f.stats.attack {
            AttackKind::Heal { .. } => (t.team == f.team && t.hp < t.stats.max_hp).then_some(false),
            AttackKind::Ranged | AttackKind::Magic => (t.team != f.team).then_some(false),
            AttackKind::Melee => {
                if t.team == f.team || from.row != Row::Front || t.slot.row != Row::Front {
                    return None;
                }
                // Front, front-left and front-right are all in reach; a diagonal hit is a flank
                // strike only while the cell straight ahead is empty (and the target has no
                // guard against flanking).
                let opposite_empty = self.at(t.team, Slot::new(Row::Front, from.col)).is_none();
                match t.slot.col.abs_diff(from.col) {
                    0 => Some(false),
                    1 => Some(opposite_empty && !t.passives.no_flank),
                    _ => None,
                }
            }
        }
    }

    pub fn can_target(&self, id: usize, target: usize) -> bool {
        self.reach_from(id, self.fighters[id].slot, target).is_some()
    }

    pub fn is_flank(&self, id: usize, target: usize) -> bool {
        self.reach_from(id, self.fighters[id].slot, target) == Some(true)
    }

    pub fn targets(&self, id: usize) -> Vec<usize> {
        (0..self.fighters.len()).filter(|&t| self.can_target(id, t)).collect()
    }

    fn has_target_from(&self, id: usize, from: Slot) -> bool {
        (0..self.fighters.len()).any(|t| self.reach_from(id, from, t).is_some())
    }

    /// Empty own cells the fighter could step to.
    pub fn moves(&self, id: usize) -> Vec<Slot> {
        let f = &self.fighters[id];
        neighbours(f.slot).filter(|&s| self.at(f.team, s).is_none()).collect()
    }

    fn spend_action(&mut self) {
        self.actions_left -= 1;
        if self.actions_left <= 0 {
            self.end_turn();
        }
    }

    /// Step the active fighter to a neighbouring empty cell; costs one action.
    pub fn move_active(&mut self, to: Slot) -> Result<(), ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        if !self.moves(id).contains(&to) {
            return Err(ActionError::InvalidTarget);
        }
        self.fighters[id].slot = to;
        let team = self.fighters[id].team;
        self.log.push(format!("{} moves", self.fighters[id].kind.name()));
        self.collapse(team);
        self.spend_action();
        Ok(())
    }

    /// Attack or heal `target` with the active fighter; costs one action.
    pub fn act(&mut self, target: usize) -> Result<Hit, ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        let flank = self.reach_from(id, self.fighters[id].slot, target).ok_or(ActionError::InvalidTarget)?;
        let stats = self.fighters[id].stats;
        let name = self.fighters[id].kind.name();
        let tname = self.fighters[target].kind.name();

        let hit = if let AttackKind::Heal { amount } = stats.attack {
            let t = &mut self.fighters[target];
            let before = t.hp;
            t.hp = (t.hp + amount).min(t.stats.max_hp);
            let healed = t.hp - before;
            self.log.push(format!("{name} heals {tname} +{healed}"));
            Hit { target, amount: healed, heal: true, flank: false, killed: false, boost: false }
        } else {
            let ignores_armor = stats.attack == AttackKind::Magic || self.fighters[id].passives.magic_strike;
            let armor = if ignores_armor { 0 } else { self.fighters[target].stats.armor };
            let roll = self.rng.range(stats.dmg_min, stats.dmg_max) * if flank { 2 } else { 1 };
            let dmg = (roll - armor).max(1);
            let t = &mut self.fighters[target];
            t.hp -= dmg;
            let killed = !t.alive();
            let how = if flank { " from the flank" } else { "" };
            self.log.push(if killed {
                format!("{name} kills {tname}{how} ({dmg})")
            } else {
                format!("{name} hits {tname}{how} for {dmg}")
            });
            let team = self.fighters[target].team;
            self.collapse(team);
            Hit { target, amount: dmg, heal: false, flank, killed, boost: false }
        };
        self.spend_action();
        Ok(hit)
    }

    /// Drink the potion in item slot `slot` of the active fighter; costs one action.
    pub fn drink(&mut self, slot: usize) -> Result<Hit, ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        if !self.fighters[id].drinkable().contains(&slot) {
            return Err(ActionError::InvalidTarget);
        }
        let f = &mut self.fighters[id];
        let item = f.items[slot].take().expect("drinkable slot holds a potion").def();
        let (name, potion) = (f.kind.name(), &item.name);
        let hit = match item.effect {
            Some(Effect::Heal(n)) => {
                let healed = (f.hp + n).min(f.stats.max_hp) - f.hp;
                f.hp += healed;
                self.log.push(format!("{name} drinks {potion} +{healed}"));
                Hit { target: id, amount: healed, heal: true, flank: false, killed: false, boost: false }
            }
            Some(Effect::Might(n)) => {
                f.stats.add_damage(n);
                self.log.push(format!("{name} drinks {potion}: +{n} dmg"));
                Hit { target: id, amount: n, heal: false, flank: false, killed: false, boost: true }
            }
            _ => unreachable!("drinkable() only allows potions"),
        };
        self.spend_action();
        Ok(hit)
    }

    /// End the active fighter's turn, forfeiting any remaining actions.
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
                self.begin_turn();
                return;
            }
        }
    }

    /// Target the AI would pick for the active fighter.
    pub fn ai_choice(&self) -> Option<usize> {
        let id = self.active()?;
        let heals = matches!(self.fighters[id].stats.attack, AttackKind::Heal { .. });
        let key = |t: usize| {
            let f = &self.fighters[t];
            // Healers pick the lowest HP fraction (permille); attackers the lowest HP, flanks first.
            let hp = if heals { f.hp * 1000 / f.stats.max_hp } else { f.hp };
            (hp, !self.is_flank(id, t), t)
        };
        self.targets(id).into_iter().min_by_key(|&t| key(t))
    }

    /// How good a cell is for a warrior that currently has nothing to hit.
    fn approach_score(&self, id: usize, s: Slot) -> (bool, bool, bool, i32) {
        let team = self.fighters[id].team;
        let dist = self
            .fighters
            .iter()
            .filter(|e| e.alive() && e.team != team && e.slot.row == Row::Front)
            .map(|e| e.slot.col.abs_diff(s.col) as i32)
            .min()
            .unwrap_or(0);
        let front_free = self.at(team, Slot::new(Row::Front, s.col)).is_none();
        (self.has_target_from(id, s), s.row == Row::Front, front_free, -dist)
    }

    /// Cell the AI would step to, if stepping strictly improves its position.
    fn ai_move(&self) -> Option<Slot> {
        let id = self.active()?;
        if self.fighters[id].stats.attack != AttackKind::Melee {
            return None;
        }
        let here = self.approach_score(id, self.fighters[id].slot);
        self.moves(id)
            .into_iter()
            .map(|s| (self.approach_score(id, s), s))
            .filter(|(score, _)| *score > here)
            .max_by_key(|&(score, s)| (score, std::cmp::Reverse(s.col)))
            .map(|(_, s)| s)
    }

    /// Plays one action of the active fighter automatically.
    pub fn ai_step(&mut self) -> Option<Step> {
        let actor = self.active()?;
        if let Some(t) = self.ai_choice() {
            let hit = self.act(t).ok()?;
            return Some(Step::Strike { actor, hit });
        }
        if let Some(to) = self.ai_move() {
            let from = self.fighters[actor].slot;
            self.move_active(to).ok()?;
            return Some(Step::Move { actor, from, to });
        }
        self.skip();
        Some(Step::Wait { actor })
    }

    /// Final (squad index, hp, slot, items left) of the player's fighters; dead ones have hp <= 0.
    pub fn player_results(&self) -> Vec<(usize, i32, Slot, [Option<ItemId>; SLOTS])> {
        self.fighters.iter().filter_map(|f| f.squad_index.map(|i| (i, f.hp, f.slot, f.items))).collect()
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

    fn units(player: &[(UnitKind, Slot)]) -> Vec<Unit> {
        player.iter().map(|&(k, s)| Unit::new(k, s)).collect()
    }

    fn battle_of(units: &[Unit], enemies: &[(UnitKind, Slot)], seed: u64) -> Battle {
        let p: Vec<_> = units.iter().enumerate().collect();
        let mut bt = Battle::new(&p, enemies, seed);
        bt.begin();
        bt
    }

    fn battle(player: &[(UnitKind, Slot)], enemies: &[(UnitKind, Slot)]) -> Battle {
        battle_of(&units(player), enemies, 7)
    }

    fn kinds(bt: &Battle) -> Vec<UnitKind> {
        bt.queue().map(|i| bt.fighters[i].kind).collect()
    }

    #[test]
    fn turn_order_by_initiative_player_first_on_ties() {
        // Ranger 7, Archmage 6, Bandit chief 6: the tie goes to the player.
        let bt = battle(&[(Ranger, b(2)), (Archmage, b(3)), (Knight, f(2))], &[(BanditChief, f(2))]);
        assert_eq!(kinds(&bt), vec![Ranger, Archmage, BanditChief, Knight]);
    }

    #[test]
    fn warrior_hits_front_and_both_diagonals() {
        let mut bt = battle(&[(Knight, f(2))], &[(Bandit, f(1)), (Bandit, f(2)), (Bandit, f(3))]);
        let knight = bt.active().unwrap();
        assert_eq!(bt.targets(knight), vec![1, 2, 3]);
        assert!(!bt.is_flank(knight, 1), "opposite occupied: diagonal is a plain hit");
        bt.fighters[2].hp = 0;
        assert_eq!(bt.targets(knight), vec![1, 3]);
        assert!(bt.is_flank(knight, 1), "opposite empty: diagonal is a flank strike");
    }

    #[test]
    fn warrior_cannot_reach_two_columns_away() {
        let bt = battle(&[(Knight, f(0))], &[(Bandit, f(2)), (BanditArcher, b(1))]);
        assert!(bt.targets(0).is_empty());
    }

    #[test]
    fn flank_strike_doubles_attack_before_armor() {
        // Knight 10-14 x2 = 20-28, minus bandit armor 1.
        let mut bt = battle(&[(Knight, f(2))], &[(Bandit, f(3))]);
        let hit = bt.act(1).unwrap();
        assert!(hit.flank);
        assert!((19..=27).contains(&hit.amount), "{hit:?}");
    }

    #[test]
    fn back_row_warrior_is_helpless() {
        let bt = battle(&[(Knight, b(2)), (Spearman, f(2))], &[(Bandit, f(2))]);
        assert!(bt.helpless(0));
        assert!(bt.targets(0).is_empty());
    }

    #[test]
    fn rear_collapses_forward_when_front_row_dies() {
        let mut bt = battle(&[(Ranger, b(2))], &[(Bandit, f(2)), (BanditArcher, b(4))]);
        // Ranger alone in the back row: it already stepped forward at the start.
        assert_eq!(bt.fighters[0].slot, f(2));
        bt.fighters[1].hp = 1;
        bt.act(1).unwrap();
        assert_eq!(bt.fighters[2].slot, f(4), "archer steps into the front row");
    }

    #[test]
    fn armor_always_lets_one_through_and_magic_ignores_it() {
        for seed in 0..30 {
            let mut bt = battle_of(&units(&[(Knight, f(2))]), &[(BanditArcher, b(2))], seed);
            bt.skip(); // knight and archer both init 5: knight first
            let hit = bt.act(0).unwrap();
            assert!((1..=3).contains(&hit.amount), "{hit:?}");
        }
        let mut bt = battle(&[(Archmage, b(2))], &[(BanditChief, f(2))]);
        let hit = bt.act(1).unwrap();
        assert!((9..=13).contains(&hit.amount));
    }

    #[test]
    fn actions_cover_moving_and_attacking() {
        // Ranger has 2 actions: move, then shoot, and only then the turn passes.
        let mut bt = battle(&[(Ranger, b(2)), (Knight, f(2))], &[(BanditChief, f(2))]);
        let ranger = bt.active().unwrap();
        assert_eq!(bt.actions_left(), 2);
        bt.move_active(b(3)).unwrap();
        assert_eq!(bt.active(), Some(ranger));
        bt.act(2).unwrap();
        assert_ne!(bt.active(), Some(ranger));
    }

    #[test]
    fn moves_are_to_adjacent_empty_own_cells() {
        let bt = battle(&[(Knight, f(0)), (Spearman, f(1))], &[(Bandit, f(0))]);
        assert_eq!(bt.moves(0), vec![b(0)]);
        let mut bt = battle(&[(Knight, f(0)), (Spearman, f(1))], &[(Bandit, f(0))]);
        assert_eq!(bt.move_active(f(2)), Err(ActionError::InvalidTarget));
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
        let squad = units(&[(Knight, f(2)), (Archer, b(2))]);
        let p: Vec<_> = squad.iter().enumerate().collect();
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
    fn ai_prefers_weakest_reachable_target() {
        // Bandit at f(3) faces the swordsman; the wounded spearman two columns away is out of reach.
        let mut bt = battle(&[(Knight, f(2)), (Swordsman, f(3)), (Spearman, f(5))], &[(Bandit, f(3))]);
        bt.fighters[1].hp = 20;
        bt.fighters[2].hp = 3;
        while bt.fighters[bt.active().unwrap()].kind != Bandit {
            bt.skip();
        }
        assert_eq!(bt.ai_choice(), Some(1));
    }

    #[test]
    fn ai_warrior_steps_toward_a_target() {
        // Chief at f(4), the only player card at f(2): nothing in reach, so it walks left.
        let mut bt = battle(&[(Knight, f(2))], &[(BanditChief, f(4))]);
        assert_eq!(bt.fighters[bt.active().unwrap()].kind, BanditChief);
        let step = bt.ai_step().unwrap();
        assert_eq!(step, Step::Move { actor: 1, from: f(4), to: f(3) });
    }

    #[test]
    fn ai_back_row_warrior_steps_forward() {
        let mut bt = battle(&[(Knight, f(2))], &[(Bandit, f(2)), (BanditChief, b(3))]);
        assert_eq!(bt.fighters[bt.active().unwrap()].kind, BanditChief);
        let step = bt.ai_step().unwrap();
        assert_eq!(step, Step::Move { actor: 2, from: b(3), to: f(3) });
    }

    fn geared(kind: UnitKind, slot: Slot, items: &[&str]) -> Unit {
        let mut u = Unit::new(kind, slot);
        for (i, id) in items.iter().enumerate() {
            u.items[i] = Some(ItemId::named(id));
        }
        u
    }

    #[test]
    fn gear_counts_in_battle() {
        let squad = [geared(Spearman, f(2), &["boots_haste", "chainmail"])];
        let bt = battle_of(&squad, &[(Bandit, f(2))], 1);
        assert_eq!(bt.fighters[0].stats.armor, 4);
        assert_eq!(bt.actions_left(), 2, "boots: init 6 goes first, +1 action");
    }

    #[test]
    fn guardian_cloak_stops_flank_doubling() {
        let squad = [geared(Knight, f(3), &["cloak_guard"])];
        let mut bt = battle_of(&squad, &[(BanditChief, f(2))], 1);
        assert_eq!(bt.fighters[bt.active().unwrap()].kind, BanditChief);
        assert!(bt.can_target(1, 0));
        assert!(!bt.is_flank(1, 0));
        let hit = bt.act(0).unwrap();
        // Chief 11-15 minus knight armor 5 + cloak 1.
        assert!(!hit.flank && (5..=9).contains(&hit.amount), "{hit:?}");
    }

    #[test]
    fn rune_blade_ignores_armor() {
        for seed in 0..20 {
            let squad = [geared(Spearman, f(2), &["rune_blade"])];
            let mut bt = battle_of(&squad, &[(BanditChief, f(2))], seed);
            bt.skip(); // chief (6) acts first
            let hit = bt.act(1).unwrap();
            assert!((6..=9).contains(&hit.amount), "spearman 5-8 +1, armor ignored: {hit:?}");
        }
    }

    #[test]
    fn regen_heals_each_round_up_to_max() {
        let mut squad = [geared(Knight, f(2), &["amulet_life"])];
        squad[0].hp = 70;
        let mut bt = battle_of(&squad, &[(Bandit, f(2))], 1);
        assert_eq!(bt.fighters[0].hp, 73, "round 1 start");
        bt.fighters[0].hp = 74;
        bt.skip();
        bt.skip();
        assert_eq!(bt.round, 2);
        assert_eq!(bt.fighters[0].hp, 75, "capped at 60 + 15");
    }

    #[test]
    fn potions_cost_an_action_and_are_used_up() {
        let mut squad = [geared(Knight, f(2), &["heal_potion", "might_potion"])];
        let mut bt = battle_of(&squad, &[(Bandit, f(2))], 1);
        assert_eq!(bt.fighters[0].drinkable(), vec![1], "full HP: no point healing");
        assert_eq!(bt.drink(0), Err(ActionError::InvalidTarget));
        let hit = bt.drink(1).unwrap();
        assert!(hit.boost && hit.amount == 3);
        assert_eq!((bt.fighters[0].stats.dmg_min, bt.fighters[0].stats.dmg_max), (13, 17));
        assert_eq!(bt.fighters[0].items[1], None);
        assert_ne!(bt.active(), Some(0), "the drink used the knight's only action");

        squad[0].hp = 30;
        let mut bt = battle_of(&squad, &[(Bandit, f(2))], 1);
        let hit = bt.drink(0).unwrap();
        assert!(hit.heal && hit.amount == 15 && bt.fighters[0].hp == 45);
        let results = bt.player_results();
        assert_eq!(results[0].3[0], None, "drunk potion is gone after the battle");
        assert_eq!(results[0].3[1], Some(ItemId::named("might_potion")));
    }
}
