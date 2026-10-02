//! Custom battles (a Razdor extra, issue #1): two armies picked from the content's unit
//! types, each unit at a level and with the items it may wear, fought with the normal battle
//! rules outside any campaign. Either side can be played by the player or by the battle AI
//! (both by the AI to watch). Nothing here touches a game or a save, and a round leaves the
//! Community patch's battle globals as it found them.

use std::sync::Arc;

use super::battle::{patch_globals, set_patch_globals, Battle, Outcome, PatchGlobals, Team};
use super::content::{Content, ItemId, UnitId};
use super::formation::{Formation, Row, Slot};
use super::items::{self, EquipError};
use super::units::Unit;

/// The highest level the setup offers.
pub const MAX_LEVEL: i32 = 30;

/// Who plays a side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Player,
    Ai,
}

impl Control {
    pub fn other(self) -> Control {
        match self {
            Control::Player => Control::Ai,
            Control::Ai => Control::Player,
        }
    }
}

/// One unit of an army: its type, level and worn items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub unit: UnitId,
    pub level: i32,
    pub items: Vec<ItemId>,
}

impl Pick {
    pub fn new(unit: UnitId) -> Pick {
        Pick { unit, level: 1, items: Vec::new() }
    }
}

/// The two armies and how they fight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    /// By [`Team::index`]: the player's army (below on the battle screen), then the enemy's.
    pub armies: [Vec<Pick>; 2],
    pub control: [Control; 2],
    /// The Community wide row (6 columns), else the vanilla 3 × 4.
    pub wide: bool,
}

/// The unit types an army can take: every type of the content that has hit points, in the
/// content's order.
pub fn choices(content: &Content) -> Vec<UnitId> {
    content.units.iter().filter(|u| u.hits > 0).map(|u| UnitId(u.id)).collect()
}

/// `pick` as a unit standing on `slot`: its level, its items (those it may wear, in order),
/// full health.
pub fn unit(content: &Content, pick: &Pick, slot: Slot) -> Unit {
    let mut u = Unit::new(content, pick.unit, slot);
    u.level = pick.level.clamp(1, MAX_LEVEL);
    for &item in &pick.items {
        if let Ok(s) = items::slot_for(content, &u, item) {
            items::put_on(content, &mut u, s, item);
        }
    }
    u.heal_full(content);
    u
}

/// The items `pick` could put on next, in the content's order.
pub fn wearable(content: &Content, pick: &Pick) -> Vec<ItemId> {
    let u = unit(content, pick, Slot::new(Row::Front, 0));
    content.items.iter().map(|d| ItemId(d.id)).filter(|&i| items::slot_for(content, &u, i).is_ok()).collect()
}

impl Setup {
    /// The first setup: the first three unit types on both sides, the player against the AI,
    /// in the content's formation.
    pub fn new(content: &Content) -> Setup {
        let army: Vec<Pick> = choices(content).into_iter().take(3).map(Pick::new).collect();
        Setup { armies: [army.clone(), army], control: [Control::Player, Control::Ai], wide: content.formation == Formation::WIDE }
    }

    pub fn formation(&self) -> Formation {
        if self.wide {
            Formation::WIDE
        } else {
            Formation::VANILLA
        }
    }

    /// Units an army can hold: the formation's cells.
    pub fn capacity(&self) -> usize {
        self.formation().capacity()
    }

    pub fn army(&self, team: Team) -> &[Pick] {
        &self.armies[team.index()]
    }

    /// Adds a unit of type `unit` at level 1; false when the army is full.
    pub fn add(&mut self, team: Team, unit: UnitId) -> bool {
        let cap = self.capacity();
        let army = &mut self.armies[team.index()];
        if army.len() >= cap {
            return false;
        }
        army.push(Pick::new(unit));
        true
    }

    pub fn remove(&mut self, team: Team, i: usize) {
        let army = &mut self.armies[team.index()];
        if i < army.len() {
            army.remove(i);
        }
    }

    /// Sets unit `i`'s level, kept within 1..=[`MAX_LEVEL`].
    pub fn set_level(&mut self, team: Team, i: usize, level: i32) {
        if let Some(p) = self.armies[team.index()].get_mut(i) {
            p.level = level.clamp(1, MAX_LEVEL);
        }
    }

    /// Puts `item` on unit `i`, by the game's wear rules (one weapon, one item of a kind...).
    pub fn wear(&mut self, content: &Content, team: Team, i: usize, item: ItemId) -> Result<(), EquipError> {
        let p = self.armies[team.index()].get_mut(i).ok_or(EquipError::NotAllowed)?;
        items::slot_for(content, &unit(content, p, Slot::new(Row::Front, 0)), item)?;
        p.items.push(item);
        Ok(())
    }

    /// Takes off unit `i`'s `k`-th item.
    pub fn take_off(&mut self, team: Team, i: usize, k: usize) {
        if let Some(p) = self.armies[team.index()].get_mut(i) {
            if k < p.items.len() {
                p.items.remove(k);
            }
        }
    }

    /// Fits the setup to `content` (another install or mod, an old setup): unknown unit types
    /// and items go, and each army keeps at most the formation's cells.
    pub fn fit(&mut self, content: &Content) {
        let cap = self.capacity();
        for army in &mut self.armies {
            army.retain(|p| content.try_unit(p.unit).is_some());
            for p in army.iter_mut() {
                p.items.retain(|&i| content.try_item(i).is_some());
                p.level = p.level.clamp(1, MAX_LEVEL);
            }
            army.truncate(cap);
        }
    }

    /// Both armies have someone to fight.
    pub fn ready(&self) -> bool {
        self.armies.iter().all(|a| !a.is_empty())
    }

    /// The battle of this setup on `content` in the chosen formation, the battle AI at the
    /// improved level or not: each side placed by the original's auto-arrange (as the enemy
    /// of any battle is), ready to begin. Nobody is a hero: no unit's fall ends anything.
    /// `None` while an army is empty.
    pub fn battle(&self, content: &Content, improved_ai: bool) -> Option<Battle> {
        if !self.ready() {
            return None;
        }
        let content = Arc::new(content.with_formation(self.formation()));
        let start = Slot::new(Row::Front, 0);
        let player: Vec<Unit> = self.army(Team::Player).iter().map(|p| unit(&content, p, start)).collect();
        let enemies: Vec<Unit> = self.army(Team::Enemy).iter().map(|p| unit(&content, p, start)).collect();
        // Squad indices from 1: index 0 would make the first unit a hero.
        let squad: Vec<(usize, &Unit)> = player.iter().enumerate().map(|(i, u)| (i + 1, u)).collect();
        let mut b = Battle::new(content, &squad, &enemies, Team::Player);
        b.set_improved_ai(improved_ai);
        for team in Team::BOTH {
            b.auto_arrange(team);
        }
        Some(b)
    }
}

/// The custom battles of one run of the program: the setup, kept from round to round (and
/// when the player leaves for the main menu and comes back), and the rounds' score.
#[derive(Clone, Debug)]
pub struct Session {
    pub setup: Setup,
    /// Rounds the player's army (below) won and lost.
    pub won: u32,
    pub lost: u32,
    /// The patch's globals as a round found them, put back when it ends.
    saved: Option<PatchGlobals>,
}

impl Session {
    pub fn new(content: &Content) -> Session {
        Session { setup: Setup::new(content), won: 0, lost: 0, saved: None }
    }

    /// The next round's battle (the setup as it stands), begun: the first unit is to act.
    /// The patch's globals are kept aside until [`Session::end_round`].
    pub fn start_round(&mut self, content: &Content, improved_ai: bool) -> Option<Battle> {
        self.setup.fit(content);
        let mut b = self.setup.battle(content, improved_ai)?;
        if self.saved.is_none() {
            self.saved = Some(patch_globals());
        }
        b.begin();
        Some(b)
    }

    /// Counts a finished round.
    pub fn record(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Victory => self.won += 1,
            Outcome::Defeat => self.lost += 1,
            Outcome::Ongoing => {}
        }
    }

    /// The round is over or left: the patch's globals are as before it.
    pub fn end_round(&mut self) {
        if let Some(g) = self.saved.take() {
            set_patch_globals(g);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::testkit::*;
    use crate::rules::content::ArtefactType;
    use crate::rules::units::Stats;

    fn demo() -> Content {
        Content::builtin()
    }

    #[test]
    fn the_first_setup_is_the_player_against_the_ai_with_three_units_each() {
        let c = demo();
        let s = Setup::new(&c);
        assert_eq!(s.control, [Control::Player, Control::Ai]);
        assert!(s.armies.iter().all(|a| a.len() == 3));
        assert_eq!(s.wide, c.formation == Formation::WIDE);
        assert!(s.ready());
        assert!(choices(&c).iter().all(|&u| c.unit(u).hits > 0));
    }

    #[test]
    fn an_army_holds_up_to_the_formations_cells() {
        let c = demo();
        let mut s = Setup::new(&c);
        let u = choices(&c)[0];
        while s.add(Team::Player, u) {}
        assert_eq!(s.army(Team::Player).len(), s.capacity());
        assert_eq!(s.capacity(), 12);
        s.remove(Team::Player, 0);
        assert_eq!(s.army(Team::Player).len(), 11);
        s.remove(Team::Player, 99);
        assert_eq!(s.army(Team::Player).len(), 11, "no such unit: nothing goes");
        for _ in 0..12 {
            s.remove(Team::Enemy, 0);
        }
        assert!(!s.ready() && s.battle(&c, false).is_none(), "an empty army cannot fight");
    }

    #[test]
    fn the_battle_has_the_picked_units_at_their_levels() {
        let c = demo();
        let mut s = Setup::new(&c);
        s.wide = false;
        s.set_level(Team::Player, 1, 5);
        s.set_level(Team::Enemy, 0, 999);
        let b = s.battle(&c, true).unwrap();
        assert_eq!(b.formation, Formation::VANILLA, "the setup's formation, not the content's");
        assert_eq!(b.ai_level, 2, "the improved AI");
        assert_eq!(b.fighters.len(), 6);
        let side = |t: Team| b.fighters.iter().filter(|f| f.team == t).collect::<Vec<_>>();
        for t in Team::BOTH {
            let army = side(t);
            assert_eq!(army.iter().map(|f| f.unit).collect::<Vec<_>>(), s.army(t).iter().map(|p| p.unit).collect::<Vec<_>>());
            assert!(army.iter().all(|f| !f.is_hero && f.hp == f.max_hp()), "nobody is a hero, everyone is whole");
            let mut cells: Vec<Slot> = army.iter().map(|f| f.slot).collect();
            assert!(cells.iter().all(|&c| b.formation.contains(c)));
            cells.sort_by_key(|c| (c.row, c.col));
            cells.dedup();
            assert_eq!(cells.len(), army.len(), "one unit a cell");
        }
        assert_eq!(side(Team::Player)[1].level, 5);
        assert_eq!(side(Team::Enemy)[0].level, MAX_LEVEL, "levels stay within the setup's range");
        let lvl5 = Stats::of_level(&c, s.army(Team::Player)[1].unit, 5);
        assert_eq!(side(Team::Player)[1].max_hp(), lvl5.max_hp());
    }

    #[test]
    fn items_are_worn_by_the_games_rules() {
        let mut c = content(vec![warrior(10, 30, 5), shooter(11, 20)], vec![item(200, ArtefactType::BlowWeapon), item(201, ArtefactType::Amulet), item(202, ArtefactType::Potion)]);
        c.items[0].add.insert(crate::rules::content::Stat::AttackBlow, 7);
        let mut s = Setup { armies: [vec![Pick::new(UnitId(10))], vec![Pick::new(UnitId(11))]], control: [Control::Player, Control::Ai], wide: true };
        let pick = &s.army(Team::Player)[0];
        assert_eq!(wearable(&c, pick), vec![ItemId(200), ItemId(201)], "no potion");
        assert!(s.wear(&c, Team::Player, 0, ItemId(200)).is_ok());
        assert_eq!(s.wear(&c, Team::Player, 0, ItemId(200)), Err(EquipError::SecondWeapon));
        assert_eq!(s.wear(&c, Team::Enemy, 0, ItemId(200)), Err(EquipError::WrongClass), "a shooter has no melee attack");
        assert_eq!(wearable(&c, &s.army(Team::Player)[0]), vec![ItemId(201)]);
        let b = s.battle(&c, false).unwrap();
        assert_eq!(b.fighters[0].items[0], Some(ItemId(200)));
        assert_eq!(b.fighters[0].base[crate::rules::content::Stat::AttackBlow], 37, "the item's +7");
        s.take_off(Team::Player, 0, 0);
        assert!(s.army(Team::Player)[0].items.is_empty());
    }

    #[test]
    fn a_setup_fits_other_content() {
        let c = demo();
        let mut s = Setup::new(&c);
        s.armies[0].push(Pick { unit: UnitId(9999), level: 1, items: vec![ItemId(9999)] });
        s.armies[0][0].items.push(ItemId(9998));
        s.wide = false;
        for _ in 0..20 {
            s.armies[1].push(Pick::new(choices(&c)[0]));
        }
        s.fit(&c);
        assert_eq!(s.army(Team::Player).len(), 3, "the unknown type goes");
        assert!(s.army(Team::Player)[0].items.is_empty(), "so does an unknown item");
        assert_eq!(s.army(Team::Enemy).len(), 12);
    }

    #[test]
    fn the_session_keeps_the_setup_and_the_score_across_rounds() {
        let c = demo();
        let mut session = Session::new(&c);
        session.setup.set_level(Team::Player, 0, 4);
        session.setup.control = [Control::Ai, Control::Ai];
        let before = patch_globals();
        let mut first = session.start_round(&c, false).unwrap();
        assert!(!first.is_deploying() && first.active().is_some(), "the round begins at once");
        let outcome = first.auto_play_to_end();
        assert_ne!(outcome, Outcome::Ongoing);
        session.record(outcome);
        session.end_round();
        assert_eq!(patch_globals(), before, "a round changes nothing a campaign battle would see");
        assert_eq!(session.won + session.lost, 1);
        // Again: the same armies, the same battle, the same result.
        assert_eq!(session.setup.army(Team::Player)[0].level, 4);
        let mut again = session.start_round(&c, false).unwrap();
        assert_eq!(again.auto_play_to_end(), outcome);
        assert_eq!(again.log, first.log);
        session.record(again.outcome());
        session.end_round();
        assert_eq!(session.won + session.lost, 2);
        assert_eq!(session.setup.control, [Control::Ai, Control::Ai]);
        session.record(Outcome::Ongoing);
        assert_eq!(session.won + session.lost, 2, "a battle left unfinished does not count");
    }
}
