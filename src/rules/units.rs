//! Units: effective stats and persistent unit instances (level, XP, HP, items).
//!
//! A unit's role follows from its stats, as in the original (mechanics.md 1.2): melee
//! attack > 0 makes a warrior, ranged attack > 0 a shooter, magic power > 0 with a school a
//! mage. A unit can be several at once.

use std::ops::{Index, IndexMut};

use crate::i18n::tr;
use super::content::{Bonus, Content, ItemId, MagicDirection, MagicSchool, Nature, Stat, StatMods, UnitId, WageKind};
use super::experience;
use super::formation::{Row, Slot};
use super::items::{self, SLOTS};

/// Effective stats of a unit: numbers by [`Stat`] plus magic, nature and bonuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stats {
    values: [i32; Stat::ALL.len()],
    pub magic: Option<MagicSchool>,
    pub direction: Option<MagicDirection>,
    pub nature: Nature,
    /// The unit's own bonus plus those granted by worn items.
    pub bonuses: Vec<Bonus>,
    /// Community: percent of physical damage ignored.
    pub evasion: i32,
    /// Community per-unit overrides of the magic power floor and drain.
    pub min_magic_power: Option<i32>,
    pub mana_drain: Option<i32>,
}

fn stat_index(s: Stat) -> usize {
    Stat::ALL.iter().position(|x| *x == s).expect("in ALL")
}

impl Index<Stat> for Stats {
    type Output = i32;
    fn index(&self, s: Stat) -> &i32 {
        &self.values[stat_index(s)]
    }
}

impl IndexMut<Stat> for Stats {
    fn index_mut(&mut self, s: Stat) -> &mut i32 {
        &mut self.values[stat_index(s)]
    }
}

impl Stats {
    /// Base stats of a type at `level` (1 = as hired): the definition plus `d-*` for each
    /// level above the first. Magic power needs a school (without one it is 0); protections,
    /// regeneration and vampirism grow by [`experience::percent_stat`] (experience.md §2).
    pub fn of_level(content: &Content, id: UnitId, level: i32) -> Stats {
        let def = content.unit(id);
        let mut s = Stats {
            values: Stat::ALL.map(|st| def.stat(st)),
            magic: def.magic,
            direction: def.magic_direction,
            nature: def.nature,
            bonuses: def.bonus.iter().cloned().collect(),
            evasion: def.evasion.unwrap_or(0),
            min_magic_power: def.min_magic_power,
            mana_drain: def.mana_drain,
        };
        let levels = (level - 1).max(0);
        if def.magic.is_none() {
            // Without a school a unit has no magic power at all.
            s[Stat::MagicPower] = 0;
        }
        for (&st, &d) in &def.level_up {
            if st == Stat::MagicPower && def.magic.is_none() {
                continue;
            }
            if experience::is_percent_stat(st) {
                s[st] = experience::percent_stat(s[st], d, levels);
            } else {
                s[st] += d * levels;
            }
        }
        for st in Stat::ALL.into_iter().filter(|&st| experience::is_percent_stat(st) && !def.level_up.contains_key(&st)) {
            s[st] = experience::percent_stat(s[st], 0, 0);
        }
        s
    }

    /// Adds `times` × each modifier.
    pub fn add(&mut self, mods: &StatMods, times: i32) {
        for (&st, &v) in mods {
            self[st] += v * times;
        }
    }

    pub fn has(&self, b: &Bonus) -> bool {
        self.bonuses.contains(b)
    }

    pub fn has_any(&self, bs: &[Bonus]) -> bool {
        bs.iter().any(|b| self.has(b))
    }

    pub fn is_warrior(&self) -> bool {
        self[Stat::AttackBlow] > 0
    }

    pub fn is_shooter(&self) -> bool {
        self[Stat::AttackShot] > 0
    }

    pub fn is_mage(&self) -> bool {
        self[Stat::MagicPower] > 0 && self.magic.is_some()
    }

    /// Whom the mage's magic reaches; a school without a direction counts as `ToAll` (guess).
    pub fn magic_direction(&self) -> MagicDirection {
        self.direction.unwrap_or(MagicDirection::ToAll)
    }

    /// Percent protection against hostile magic of `school`.
    pub fn protection(&self, school: MagicSchool) -> i32 {
        self[match school {
            MagicSchool::Life => Stat::ProtectLife,
            MagicSchool::Death => Stat::ProtectDeath,
            MagicSchool::Elemental => Stat::ProtectElemental,
        }]
    }

    pub fn max_hp(&self) -> i32 {
        self[Stat::Hits]
    }

    /// Stats never go negative; a unit keeps at least 1 HP maximum.
    pub fn clamp(&mut self) {
        for v in &mut self.values {
            *v = (*v).max(0);
        }
        self[Stat::Hits] = self[Stat::Hits].max(1);
    }

    /// Row a newly hired unit of this kind goes to: warriors in front, the rest behind.
    pub fn preferred_row(&self) -> Row {
        if self.is_warrior() {
            Row::Front
        } else {
            Row::Back
        }
    }

    /// "warrior", "shooter", "mage" or a combination such as "warrior, mage".
    pub fn role(&self) -> String {
        let mut r = Vec::new();
        if self.is_warrior() {
            r.push(tr("warrior"));
        }
        if self.is_shooter() {
            r.push(tr("shooter"));
        }
        if self.is_mage() {
            r.push(tr("mage"));
        }
        if r.is_empty() {
            r.push(tr("civilian"));
        }
        r.join(", ")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PromoteError {
    /// Not in this unit's upgrade tree, or its level is too low.
    NotAvailable,
}

/// A persistent army member.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Unit {
    pub def: UnitId,
    /// 1 as hired.
    pub level: i32,
    /// Progress towards the next level.
    pub xp: i32,
    pub hp: i32,
    /// Cell in the battle formation.
    pub slot: Slot,
    /// Missed the last payday: refuses to fight until paid.
    pub unpaid: bool,
    /// Game minute of the last payday it was paid (or it joined). A unit whose last pay is
    /// more than `MaxTimeNotUpkeep` ago leaves at a noon when money is short.
    #[serde(default)]
    pub last_paid: u64,
    /// Hiring kind for the wage formula (the hero is [`WageKind::Leader`]).
    pub wage_kind: WageKind,
    /// Game minute of death; a corpse (HP 0) stays in the army until it is resurrected or
    /// buried (mechanics.md 2.5).
    pub died_at: Option<u64>,
    /// Worn items.
    pub items: [Option<ItemId>; SLOTS],
    /// Drunk potions whose effect lasts until the end of the next battle.
    pub potions: Vec<ItemId>,
    /// A named character of the scenario (1-based); 0 for an ordinary unit.
    pub named: u8,
    /// Joined through a scenario event (events can take such units away again).
    pub from_event: bool,
    /// The last time the garrison tab opened while it was in the hero's army (unit+0x1bb):
    /// a garrison unit counts as paid a day after it (economy.md §2). 0: never, as a hire.
    #[serde(default)]
    pub seen: u64,
}

impl Unit {
    pub fn new(content: &Content, def: UnitId, slot: Slot) -> Unit {
        let hp = content.unit(def).hits.max(1);
        let wage_kind = WageKind::of(content.unit(def));
        Unit {
            def,
            level: 1,
            xp: 0,
            hp,
            slot,
            unpaid: false,
            last_paid: 0,
            wage_kind,
            died_at: None,
            items: [None; SLOTS],
            potions: Vec::new(),
            named: 0,
            from_event: false,
            seen: 0,
        }
    }

    pub fn name<'a>(&self, content: &'a Content) -> &'a str {
        &content.unit(self.def).name
    }

    /// Stats without items or potions.
    pub fn base_stats(&self, content: &Content) -> Stats {
        Stats::of_level(content, self.def, self.level)
    }

    /// Level stats with worn items and active potions applied (see [`items::apply`]).
    pub fn stats(&self, content: &Content) -> Stats {
        let mut s = self.base_stats(content);
        let worn: Vec<ItemId> = self.items.iter().flatten().copied().collect();
        items::apply(content, &mut s, &worn, &self.potions);
        s
    }

    pub fn max_hp(&self, content: &Content) -> i32 {
        self.stats(content).max_hp()
    }

    pub fn heal_full(&mut self, content: &Content) {
        self.hp = self.max_hp(content);
    }

    pub fn alive(&self) -> bool {
        self.hp > 0
    }

    pub fn xp_to_next(&self, content: &Content) -> i32 {
        content.xp_to_next(self.def, self.level)
    }

    /// Tactical cost with worn items, standing in a building of `building_defence`.
    pub fn tactical(&self, content: &Content, building_defence: i32) -> i32 {
        experience::tactical(content, self.def, &self.stats(content), building_defence)
    }

    /// Adds XP and levels up while enough is banked, keeping what is left over; a negative
    /// amount adds nothing. Each level adds the type's `d-*` gains. An unhurt unit stays at
    /// full health; a wounded one keeps its hit points (experience.md §2). Returns levels
    /// gained. The per-battle Community cap is applied by the caller.
    pub fn gain_xp(&mut self, content: &Content, amount: i32) -> i32 {
        let unhurt = self.alive() && self.hp >= self.max_hp(content);
        self.xp = self.xp.saturating_add(amount.max(0));
        let mut gained = 0;
        loop {
            let need = self.xp_to_next(content);
            if self.xp < need {
                break;
            }
            self.xp -= need;
            self.level += 1;
            gained += 1;
        }
        if gained > 0 && unhurt {
            self.hp = self.max_hp(content);
        }
        gained
    }

    /// The upgrade tree: every `NextUnitN` option of this class, with its required level
    /// and whether it can be taken now.
    pub fn upgrade_tree(&self, content: &Content) -> Vec<(UnitId, i32, bool)> {
        let ready = self.can_promote();
        content
            .unit(self.def)
            .upgrades
            .iter()
            .filter_map(|u| u.target.map(|t| (UnitId(t), u.level.max(1) + 1)))
            .filter(|(id, _)| content.try_unit(*id).is_some())
            .map(|(id, level)| (id, level, ready))
            .collect()
    }

    /// The original lets the player promote any unit that has gained a level (level 2 here,
    /// its level 1), whatever `NextUnitNLevel` says; the hero never.
    fn can_promote(&self) -> bool {
        self.level >= 2
    }

    /// Classes this unit may be promoted to now.
    pub fn promotions(&self, content: &Content) -> Vec<UnitId> {
        self.upgrade_tree(content).into_iter().filter(|&(_, _, ok)| ok).map(|(id, _, _)| id).collect()
    }

    /// Switch to class `to` from the upgrade tree, free of charge. The unit starts the new
    /// class at level 1 with no XP (experience.md §4); an unhurt unit is at the new class's
    /// full health, a wounded one keeps its hit points. Items the new class may not wear
    /// are taken off and returned *(guess)*.
    pub fn promote(&mut self, content: &Content, to: UnitId) -> Result<Vec<ItemId>, PromoteError> {
        if !self.promotions(content).contains(&to) {
            return Err(PromoteError::NotAvailable);
        }
        let unhurt = self.alive() && self.hp >= self.max_hp(content);
        self.def = to;
        self.level = 1;
        self.xp = 0;
        let mut removed = Vec::new();
        let worn: Vec<ItemId> = self.items.iter().flatten().copied().collect();
        self.items = [None; SLOTS];
        for item in worn {
            match items::slot_for(content, self, item) {
                Ok(slot) => self.items[slot] = Some(item),
                Err(_) => removed.push(item),
            }
        }
        let max = self.max_hp(content);
        self.hp = if unhurt { max } else { self.hp.min(max) };
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::testkit::*;
    use crate::rules::content::{UnitDef, Upgrade};

    fn militia() -> UnitDef {
        let mut u = warrior(1, 20, 5);
        u.level_up = StatMods::from([(Stat::Hits, 5), (Stat::AttackBlow, 2), (Stat::Initiative, 1)]);
        u.upgrades = vec![Upgrade { target_name: "guard".into(), target: Some(2), level: 2, slot: 2 }];
        u
    }

    fn slot() -> Slot {
        Slot::new(Row::Front, 0)
    }

    #[test]
    fn roles_follow_the_stats() {
        let c = content(vec![warrior(1, 10, 0), shooter(2, 10), mage(3, 10, MagicSchool::Life, MagicDirection::ToAlly)], vec![]);
        let s = |id| Stats::of_level(&c, UnitId(id), 1);
        assert!(s(1).is_warrior() && !s(1).is_shooter() && !s(1).is_mage());
        assert!(s(2).is_shooter() && s(2).preferred_row() == Row::Back);
        assert!(s(3).is_mage() && s(3).role() == "mage");
    }

    #[test]
    fn level_ups_apply_d_deltas() {
        let c = content(vec![militia(), warrior(2, 30, 8)], vec![]);
        let mut u = Unit::new(&c, UnitId(1), slot());
        u.hp = 30;
        assert_eq!(u.gain_xp(&c, 59), 0);
        assert_eq!(u.gain_xp(&c, 1 + 84 + 10), 2, "60 then 84");
        assert_eq!((u.level, u.xp), (3, 10));
        let s = u.stats(&c);
        assert_eq!((s.max_hp(), s[Stat::AttackBlow], s[Stat::Initiative]), (60, 24, 12));
        assert_eq!(u.hp, 30, "a wounded unit keeps its hit points");
        let mut fresh = Unit::new(&c, UnitId(1), slot());
        fresh.gain_xp(&c, 60);
        assert_eq!(fresh.hp, 55, "an unhurt one is at the new maximum");
    }

    #[test]
    fn percent_stats_grow_towards_100_and_magic_needs_a_school() {
        let mut u = militia();
        u.protect_life = 20;
        u.magic_power = 5;
        u.level_up = StatMods::from([(Stat::ProtectLife, 5), (Stat::MagicPower, 3), (Stat::Regen, 10)]);
        let c = content(vec![u], vec![]);
        let s = Stats::of_level(&c, UnitId(1), 3);
        assert_eq!((s[Stat::ProtectLife], s[Stat::Regen], s[Stat::MagicPower]), (28, 19, 0), "no school, no magic");
    }

    #[test]
    fn xp_overflow_is_kept_and_negative_xp_ignored() {
        let c = content(vec![militia()], vec![]);
        let mut u = Unit::new(&c, UnitId(1), slot());
        assert_eq!(u.gain_xp(&c, -50), 0);
        assert_eq!(u.xp, 0);
        // 60 + 84 + 118 = 262; 300 leaves 38 towards level 4.
        assert_eq!(u.gain_xp(&c, 300), 3);
        assert_eq!((u.level, u.xp), (4, 38));
        // No cap outside battles.
        let mut big = Unit::new(&c, UnitId(1), slot());
        big.gain_xp(&c, 100_000);
        assert!(big.level > 10);
    }

    #[test]
    fn promotion_through_the_upgrade_tree() {
        let c = content(vec![militia(), warrior(2, 30, 8)], vec![]);
        let mut u = Unit::new(&c, UnitId(1), slot());
        assert_eq!(u.upgrade_tree(&c), vec![(UnitId(2), 3, false)]);
        assert!(u.promotions(&c).is_empty(), "needs a level");
        assert_eq!(u.promote(&c, UnitId(2)), Err(PromoteError::NotAvailable));
        u.gain_xp(&c, 60);
        assert_eq!(u.level, 2);
        assert_eq!(u.promotions(&c), vec![UnitId(2)], "NextUnitNLevel is not checked for the player");
        u.gain_xp(&c, 20);
        u.hp = 30; // of 55
        assert_eq!(u.promote(&c, UnitId(2)), Ok(vec![]));
        assert_eq!((u.def, u.level, u.xp, u.hp), (UnitId(2), 1, 0, 30), "level 1, XP reset, HP kept");
    }
}
