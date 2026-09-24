use super::battle::{Battle, Outcome};
use super::formation::{free_slot, Slot};
use super::units::{Unit, UnitKind};
use super::world::{LocationKind, World, HOME};

/// Whole squad including the hero: one 2×6 formation.
pub const MAX_SQUAD: usize = 12;

#[derive(Debug, PartialEq, Eq)]
pub enum TravelError {
    NoRoad,
}

#[derive(Debug, PartialEq, Eq)]
pub enum HireError {
    NotOffered,
    NotEnoughGold,
    SquadFull,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Arrival {
    Town,
    Battle,
    Cleared,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BattleResult {
    Victory { reward: i32, lost: usize },
    Withdrew { lost: usize },
    Defeat,
}

pub struct Game {
    /// Squad member 0 is always the hero.
    pub squad: Vec<Unit>,
    pub gold: i32,
    pub day: u32,
    pub location: usize,
    pub world: World,
    battles: u64,
    seed: u64,
}

impl Game {
    pub fn new(hero: UnitKind, seed: u64) -> Self {
        Game {
            squad: vec![Unit::new(hero, free_slot(&[], hero.stats().attack.preferred_row()).unwrap())],
            gold: hero.starting_gold(),
            day: 1,
            location: HOME,
            world: World::standard(),
            battles: 0,
            seed,
        }
    }

    pub fn hero(&self) -> &Unit {
        &self.squad[0]
    }

    pub fn recruits_here(&self) -> &[UnitKind] {
        match &self.world.locations[self.location].kind {
            LocationKind::Town { recruits } => recruits,
            LocationKind::Camp { .. } => &[],
        }
    }

    pub fn travel(&mut self, dest: usize) -> Result<Arrival, TravelError> {
        if !self.world.connected(self.location, dest) {
            return Err(TravelError::NoRoad);
        }
        self.location = dest;
        self.day += 1;
        let loc = &self.world.locations[dest];
        Ok(match loc.kind {
            LocationKind::Town { .. } => {
                self.squad.iter_mut().for_each(Unit::heal_full);
                Arrival::Town
            }
            LocationKind::Camp { .. } if loc.cleared => Arrival::Cleared,
            LocationKind::Camp { .. } => Arrival::Battle,
        })
    }

    pub fn hire(&mut self, kind: UnitKind) -> Result<(), HireError> {
        if !self.recruits_here().contains(&kind) {
            return Err(HireError::NotOffered);
        }
        let taken: Vec<Slot> = self.squad.iter().map(|u| u.slot).collect();
        let slot = match free_slot(&taken, kind.stats().attack.preferred_row()) {
            Some(slot) if self.squad.len() < MAX_SQUAD => slot,
            _ => return Err(HireError::SquadFull),
        };
        if self.gold < kind.cost() {
            return Err(HireError::NotEnoughGold);
        }
        self.gold -= kind.cost();
        self.squad.push(Unit::new(kind, slot));
        Ok(())
    }

    /// Battle against the camp at the current location.
    pub fn start_battle(&mut self) -> Battle {
        let enemies = match &self.world.locations[self.location].kind {
            LocationKind::Camp { enemies, .. } => enemies.clone(),
            LocationKind::Town { .. } => Vec::new(),
        };
        let player: Vec<_> = self.squad.iter().enumerate().map(|(i, u)| (u.kind, u.hp, u.slot, i)).collect();
        self.battles += 1;
        Battle::new(&player, &enemies, self.seed.wrapping_add(self.battles * 7919))
    }

    pub fn resolve_battle(&mut self, battle: &Battle) -> BattleResult {
        for (i, hp, slot) in battle.player_results() {
            self.squad[i].hp = hp;
            self.squad[i].slot = slot;
        }
        let before = self.squad.len();
        let hero = self.squad.remove(0);
        self.squad.retain(|u| u.hp > 0);
        self.squad.insert(0, hero);
        let lost = before - self.squad.len();

        match battle.outcome() {
            Outcome::Victory => {}
            Outcome::Stalemate => return BattleResult::Withdrew { lost },
            _ => return BattleResult::Defeat,
        }
        let loc = &mut self.world.locations[self.location];
        loc.cleared = true;
        let reward = match loc.kind {
            LocationKind::Camp { reward, .. } => reward,
            LocationKind::Town { .. } => 0,
        };
        self.gold += reward;
        BattleResult::Victory { reward, lost }
    }

    pub fn won(&self) -> bool {
        self.world.all_camps_cleared()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use UnitKind::*;

    #[test]
    fn hire_checks_offer_gold_and_cap() {
        let mut g = Game::new(Knight, 1);
        assert_eq!(g.hire(Swordsman), Err(HireError::NotOffered));
        g.hire(Spearman).unwrap();
        assert_eq!(g.gold, 70);
        g.hire(Spearman).unwrap();
        g.hire(Spearman).unwrap();
        assert_eq!(g.hire(Archer), Err(HireError::NotEnoughGold));
        g.gold = 10_000;
        for _ in 0..8 {
            g.hire(Archer).unwrap();
        }
        assert_eq!(g.squad.len(), MAX_SQUAD);
        assert_eq!(g.hire(Archer), Err(HireError::SquadFull));
    }

    #[test]
    fn hired_units_take_free_cells_in_their_row() {
        use crate::rules::formation::Row;
        let mut g = Game::new(Knight, 1);
        g.hire(Spearman).unwrap();
        g.hire(Archer).unwrap();
        let slots: Vec<_> = g.squad.iter().map(|u| u.slot).collect();
        assert_eq!(slots, vec![Slot::new(Row::Front, 2), Slot::new(Row::Front, 3), Slot::new(Row::Back, 2)]);
    }

    #[test]
    fn travel_needs_road_and_costs_a_day() {
        let mut g = Game::new(Ranger, 1);
        assert_eq!(g.travel(3), Err(TravelError::NoRoad));
        assert_eq!(g.travel(1), Ok(Arrival::Battle));
        assert_eq!(g.day, 2);
    }

    #[test]
    fn town_heals_squad() {
        let mut g = Game::new(Knight, 1);
        g.squad[0].hp = 3;
        g.travel(2).unwrap();
        assert_eq!(g.squad[0].hp, 60);
    }

    #[test]
    fn victory_pays_clears_and_removes_dead() {
        let mut g = Game::new(Knight, 1);
        g.hire(Spearman).unwrap();
        g.travel(1).unwrap();
        let mut b = g.start_battle();
        for f in b.fighters.iter_mut() {
            if f.kind != Knight {
                f.hp = 0;
            }
        }
        b.fighters[0].hp = 17;
        let gold = g.gold;
        assert_eq!(g.resolve_battle(&b), BattleResult::Victory { reward: 100, lost: 1 });
        assert_eq!(g.gold, gold + 100);
        assert_eq!(g.squad.len(), 1);
        assert_eq!(g.hero().hp, 17);
        assert_eq!(g.travel(0).unwrap(), Arrival::Town);
        assert_eq!(g.travel(1).unwrap(), Arrival::Cleared);
    }

    #[test]
    fn auto_played_battles_always_finish() {
        for seed in 0..50 {
            let mut g = Game::new(Knight, seed);
            g.hire(Spearman).unwrap();
            g.hire(Spearman).unwrap();
            g.travel(1).unwrap();
            let mut b = g.start_battle();
            b.begin();
            let mut steps = 0;
            while b.outcome() == Outcome::Ongoing {
                b.ai_turn();
                steps += 1;
                assert!(steps < 1000, "seed {seed}: battle never ended");
            }
            g.resolve_battle(&b);
        }
    }

    #[test]
    fn stalemate_withdraws_without_clearing() {
        let mut g = Game::new(Knight, 1);
        g.travel(1).unwrap();
        let mut b = g.start_battle();
        b.begin();
        while b.outcome() == Outcome::Ongoing {
            b.skip();
        }
        assert_eq!(g.resolve_battle(&b), BattleResult::Withdrew { lost: 0 });
        assert!(!g.world.locations[1].cleared);
    }

    #[test]
    fn clearing_both_camps_wins() {
        let mut g = Game::new(Knight, 1);
        assert!(!g.won());
        for l in g.world.locations.iter_mut() {
            l.cleared = true;
        }
        assert!(g.won());
    }
}
