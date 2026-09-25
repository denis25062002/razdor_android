//! Units: effective stats and persistent unit instances (level, XP, HP, items).
//!
//! A unit's role follows from its stats, as in the original (mechanics.md 1.2): melee
//! attack > 0 makes a warrior, ranged attack > 0 a shooter, magic power > 0 with a school a
//! mage. A unit can be several at once.

use std::ops::{Index, IndexMut};

use super::content::{Bonus, Content, ItemId, MagicDirection, MagicSchool, Nature, Stat, StatMods, UnitId, WageKind, MAX_XP_GAIN};
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
    /// Base stats of a type at `level` (1 = as hired): the definition plus `d-*` per level.
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
        s.add(&def.level_up, level - 1);
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
            r.push("warrior");
        }
        if self.is_shooter() {
            r.push("shooter");
        }
        if self.is_mage() {
            r.push("mage");
        }
        if r.is_empty() {
            r.push("civilian");
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
    /// Paydays missed in a row.
    pub unpaid_days: i32,
    /// Hiring kind for the wage formula.
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
            unpaid_days: 0,
            wage_kind,
            died_at: None,
            items: [None; SLOTS],
            potions: Vec::new(),
            named: 0,
            from_event: false,
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

    /// Adds XP (at most [`MAX_XP_GAIN`] at once) and levels up while enough is banked; each
    /// level adds the type's `d-*` gains, the HP gain also to current HP. Returns levels gained.
    pub fn gain_xp(&mut self, content: &Content, amount: i32) -> i32 {
        self.xp += amount.clamp(0, MAX_XP_GAIN);
        let mut gained = 0;
        loop {
            let need = self.xp_to_next(content);
            if self.xp < need {
                break;
            }
            self.xp -= need;
            let before = self.max_hp(content);
            self.level += 1;
            gained += 1;
            self.hp = (self.hp + self.max_hp(content) - before).max(1);
        }
        gained
    }

    /// Classes this unit may be promoted to now (`NextUnitN` with `NextUnitNLevel` reached).
    pub fn promotions(&self, content: &Content) -> Vec<UnitId> {
        content
            .unit(self.def)
            .upgrades
            .iter()
            .filter(|u| self.level >= u.level.max(1))
            .filter_map(|u| u.target.map(UnitId))
            .filter(|id| content.try_unit(*id).is_some())
            .collect()
    }

    /// Switch to class `to` from the upgrade tree. The unit starts the new class at level 1
    /// with no XP and the same fraction of its HP (our guess; the original's handling is not
    /// decoded). Items the new class may not wear are taken off and returned.
    pub fn promote(&mut self, content: &Content, to: UnitId) -> Result<Vec<ItemId>, PromoteError> {
        if !self.promotions(content).contains(&to) {
            return Err(PromoteError::NotAvailable);
        }
        let (hp, max) = (self.hp, self.max_hp(content));
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
        self.hp = (hp * self.max_hp(content) / max.max(1)).max(1);
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
        u.upgrades = vec![Upgrade { target_name: "guard".into(), target: Some(2), level: 2 }];
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
        assert_eq!(u.hp, 40, "HP gains also heal");
    }

    #[test]
    fn xp_gain_is_capped() {
        let mut m = militia();
        m.start_experience = 100_000;
        let c = content(vec![m], vec![]);
        let mut u = Unit::new(&c, UnitId(1), slot());
        u.gain_xp(&c, 1_000_000);
        assert_eq!(u.xp, MAX_XP_GAIN);
    }

    #[test]
    fn promotion_through_the_upgrade_tree() {
        let c = content(vec![militia(), warrior(2, 30, 8)], vec![]);
        let mut u = Unit::new(&c, UnitId(1), slot());
        assert!(u.promotions(&c).is_empty(), "needs level 2");
        assert_eq!(u.promote(&c, UnitId(2)), Err(PromoteError::NotAvailable));
        u.gain_xp(&c, 60);
        assert_eq!(u.promotions(&c), vec![UnitId(2)]);
        u.hp = 30; // of 55
        assert_eq!(u.promote(&c, UnitId(2)), Ok(vec![]));
        assert_eq!((u.def, u.level, u.xp, u.hp), (UnitId(2), 1, 0, 27));
    }
}
