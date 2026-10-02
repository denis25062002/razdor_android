//! Experience, as the original computes it (docs/reference/original-mechanics/experience.md):
//! a unit's strength ("tactical cost") from its stats, a side's strength in battle, the
//! battle's XP pool and each survivor's share, and the modifiers applied to the share.
//!
//! Levels are numbered from 1 as hired (the original shows "Уровень 1" for its internal
//! level 0); `level − 1` is the original's level wherever a formula uses it.

use super::content::{Bonus, Content, MagicDirection, MagicSchool, Stat, UnitId};
use super::formation::Row;
use super::units::Stats;

/// Community cap on the XP one unit gains from one battle; the rest is lost for that
/// battle, but XP beyond a level is kept towards the next one.
pub const MAX_BATTLE_XP: i32 = 5256;
/// The original's difficulty factor F without "impossible difficulty" (`OptValue10`); with
/// it F is 100.
pub const NORMAL_DIFFICULTY: i32 = 120;
/// A ceiling of the percent stats that grow by levels (protections, regeneration,
/// vampirism).
const PERCENT_STAT_CAP: i32 = 99;

/// Delphi's `Round`: halves go to the even neighbour.
pub fn round_half_even(x: f64) -> i64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 {
        let t = x.trunc();
        if t as i64 % 2 == 0 {
            t as i64
        } else {
            r as i64
        }
    } else {
        r as i64
    }
}

/// XP needed from `level` (1 = as hired) to the next: `round(StartExpirience ×
/// (LevelMultipler/100)^(level−1))`, taken as the difference of two partial sums of that
/// geometric series as the original does.
pub fn xp_to_next(start: i32, multiplier: i32, level: i32) -> i32 {
    let r = multiplier as f64 / 100.0;
    let a = start as f64;
    let partial = |n: i32| -> f64 {
        let (mut sum, mut term) = (0.0, a);
        for _ in 0..n.max(0) {
            sum += term;
            term *= r;
        }
        sum
    };
    let l = (level - 1).max(0);
    round_half_even(partial(l + 1) - partial(l)).clamp(1, i32::MAX as i64) as i32
}

/// A percent stat after `levels` level-ups: each level cuts what is left to 100 by `d`
/// percent, `100 − round((100 − base)·(1 − d/100)^levels)`, at most 99.
pub fn percent_stat(base: i32, d: i32, levels: i32) -> i32 {
    let mut rest = (100 - base) as f32;
    for _ in 0..levels.max(0) {
        rest -= d as f32 * rest / 100.0;
    }
    (100 - round_half_even(rest as f64) as i32).min(PERCENT_STAT_CAP)
}

/// Whether a stat grows by the percent rule instead of `+d` per level.
pub fn is_percent_stat(s: Stat) -> bool {
    matches!(s, Stat::ProtectLife | Stat::ProtectDeath | Stat::ProtectElemental | Stat::Regen | Stat::Vampirizm)
}

/// A unit's battle role for its side's strength: set by whichever attack is largest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Melee,
    Shooter,
    Mage,
}

/// The role in the side-strength sum: a score is the best of melee, ranged and magic plus a
/// third of the other two (plus 10 for `GodAnger`, 15 `ArmorIgnore`, 20 `GodStrike`, the
/// melee attack again for `Counterblow`, 10 `FlankStrike`); a unit whose ranged attack
/// reaches two thirds of that score is a shooter, one whose magic does a mage, the rest
/// fight in melee.
pub fn role(s: &Stats) -> Role {
    let (ab, sh, mp) = (s[Stat::AttackBlow], s[Stat::AttackShot], s[Stat::MagicPower]);
    let mut score = 0;
    if ab >= sh && ab >= mp {
        score = (sh + mp) / 3 + ab;
    }
    if sh >= ab && sh >= mp {
        score = (ab + mp) / 3 + sh;
    }
    if mp >= ab && mp >= sh {
        score = (ab + sh) / 3 + mp;
    }
    for (b, add) in [(Bonus::GodAnger, 10), (Bonus::ArmorIgnore, 15), (Bonus::GodStrike, 20), (Bonus::FlankStrike, 10)] {
        if s.has(&b) {
            score += add;
        }
    }
    if s.has(&Bonus::Counterblow) {
        score += ab;
    }
    let reach = score as f64 / 1.5;
    if mp as f64 >= reach {
        Role::Mage
    } else if sh as f64 >= reach {
        Role::Shooter
    } else {
        Role::Melee
    }
}

/// A unit's strength from its stats (before `CostMultipler`): an attack value `A` and a
/// toughness `H`, `round(3.2·H·(A+1)/200)` plus bonus terms. See experience.md §1 for the
/// terms. `building_defence` is the defence of the building the unit stands in.
pub fn strength(s: &Stats, building_defence: i32, shot_weapon_range: i32) -> i32 {
    let hp = s[Stat::Hits];
    if hp == 0 {
        return 0;
    }
    let h = hp as f64;
    let (ab, db, sh, ds, mp) = (s[Stat::AttackBlow], s[Stat::DefenceBlow], s[Stat::AttackShot], s[Stat::DefenceShot], s[Stat::MagicPower]);
    let (ab_f, sh_f, mp_f) = (ab as f64, sh as f64, mp as f64);
    let has = |b: Bonus| s.has(&b);
    let bd = building_defence;
    // Melee units count their melee defence in full; the others lose 5 points of it and
    // gain 5 of ranged defence.
    let shooter = sh > ab && sh > mp;
    let caster = ab_f / 2.0 < mp_f && sh_f / 2.0 < mp_f;
    let melee = !(shooter || caster);
    let regen = s[Stat::Regen] as f64 * h / 100.0;
    let (blow, shot) = if melee {
        ((db + bd) as f64, regen + (ds + bd) as f64)
    } else {
        let x = db + bd;
        ((if x > 5 { x - 5 } else { 0 }) as f64, regen + (ds + bd + 5) as f64)
    };
    let mut d = round_half_even(((blow / 21.5).exp() / 1.17 + (shot / 30.3).exp() / 1.07) * h) as f64;
    if has(Bonus::SpearDefense) {
        d *= 1.15;
    }
    if has(Bonus::Unvulnerabe) || has(Bonus::Ghost) {
        d = h * 10.0;
    }
    if has(Bonus::VampirsGist) || has(Bonus::OldVampirsGist) || has(Bonus::Evasive) {
        d *= 1.5;
    }
    if has(Bonus::Garrison) {
        d *= 2.0;
    }
    let mut tough = h + d;
    if has(Bonus::Dead) || has(Bonus::FastDead) {
        tough *= 1.7;
    }
    let protect = (s[Stat::ProtectLife] + s[Stat::ProtectDeath]) as f64 + s[Stat::ProtectElemental] as f64 * 1.5;
    tough += tough * protect / 560.0;

    let mut a = 0.0;
    if ab >= sh && ab >= mp {
        a = ab_f;
    }
    if sh >= ab && sh >= mp {
        a = sh_f * 1.4;
    }
    if has(Bonus::ArmorIgnore) || has(Bonus::VampirsGist) || has(Bonus::OldVampirsGist) || has(Bonus::Artillery) {
        a += 18.0;
    }
    let man = s[Stat::Manevres];
    if man > 0 && mp > ab && mp > sh {
        a = mp_f;
        match s.magic_direction() {
            MagicDirection::ToEnemy => {
                a = 0.8 * a * (man - 1) as f64 / man as f64 + a * 0.2;
                if s.magic == Some(MagicSchool::Elemental) {
                    a += 25.0;
                }
            }
            MagicDirection::ToAll => a *= 1.2,
            MagicDirection::ToAlly => {}
        }
    }
    if has(Bonus::Garrison) {
        a *= 2.0;
    }
    if has(Bonus::GodAnger) {
        a += 10.0;
    }
    if has(Bonus::GodStrike) {
        a += 20.0;
    }
    if has(Bonus::Counterblow) {
        a += ab_f;
    }
    if has(Bonus::FlankStrike) {
        a += (ab / 3) as f64;
    }
    let fast = if has(Bonus::HorseAtack) || has(Bonus::OldVampirsGist) || has(Bonus::FastDead) { a * 0.15 } else { 0.0 };
    a = man as f64 * a + fast;
    let init = s[Stat::Initiative];
    a = if init > 0 { a + a * init as f64 / 100.0 } else { 0.0 };
    tough += tough * s[Stat::Vampirizm] as f64 / 100.0 * (a / h);

    let mut t = 3.2 * tough * (a + 1.0) / 200.0;
    if has(Bonus::DeathCurse) || has(Bonus::Ghost) {
        t += 150.0;
    }
    if has(Bonus::Poison) {
        t *= 1.1;
    }
    let mut t = round_half_even(t).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    if sh >= shot_weapon_range && !has(Bonus::Artillery) {
        t /= 3;
    }
    if t == 0 {
        1
    } else {
        t
    }
}

/// Tactical cost ("Сила" in the editor): [`strength`] × `CostMultipler` / 100, at least 1.
pub fn tactical(content: &Content, id: UnitId, s: &Stats, building_defence: i32) -> i32 {
    let mult = content.unit(id).cost_multiplier;
    let mult = if mult > 0 { mult } else { 100 };
    let v = strength(s, building_defence, content.options.shot_weapon_range) as i64 * mult as i64 / 100;
    (v.clamp(0, i32::MAX as i64) as i32).max(1)
}

/// A unit's level value (mode 0 of the original's tactical cost, 0x4a02a0): [`strength`]
/// of its level stats (no items, no building) × `CostMultipler` / 100, without the Community
/// "at least 1", so a type with a multiplier of 0 is worth 0. An event that adds a unit to a
/// full army dismisses the unit with the lowest one.
pub fn level_value(content: &Content, id: UnitId, level_stats: &Stats) -> i64 {
    strength(level_stats, 0, content.options.shot_weapon_range) as i64 * content.unit(id).cost_multiplier as i64 / 100
}

/// One unit in a side's strength sum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideUnit {
    pub tactical: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub row: Row,
    pub role: Role,
}

/// A side's strength: each living unit counts `round(tactical·HP/maxHP)`; back-row units
/// that are not melee count twice, and the back row is scaled by `min(1, 0.8·front/back +
/// 0.2)`; the reserve counts in full. A side of one unit that is not melee counts a fifth.
pub fn side_strength(units: &[SideUnit]) -> i64 {
    let mut rows = [0i64; 3];
    let living: Vec<&SideUnit> = units.iter().filter(|u| u.hp > 0).collect();
    for u in &living {
        let v = round_half_even(u.tactical as f64 * u.hp as f64 / u.max_hp.max(1) as f64);
        let k = (u.row.number() - 1) as usize;
        rows[k] += v;
        if u.row == Row::Back && u.role != Role::Melee {
            rows[k] += v;
        }
    }
    let f = if rows[1] != 0 { (rows[0] as f64 * 0.8 / rows[1] as f64 + 0.2).min(1.0) } else { 1.0 };
    let mut total = round_half_even(rows[1] as f64 * f + rows[2] as f64 + rows[0] as f64);
    if living.len() == 1 && living[0].role != Role::Melee {
        total /= 5;
    }
    total
}

/// A side's XP pool (48bb10), from `base` = a twentieth of the enemy's strength at the
/// start (integer division), the HP lost, the pre-simulation's predicted loss `pred` and the
/// largest loss in one turn:
/// - lost, no prediction: `base × max(0, (HP₀ − lost)/HP₀)`;
/// - lost and a prediction: `pred × q + base + maxTurn` with `q = pred/lost` in [0.8, 3];
/// - nothing lost: `3 × pred + base + maxTurn`;
///
/// rounded half to even. The exe also derives a strength-ratio term from
/// `MainExpCorrection` and `ExpCorrection`, but never uses it.
pub fn battle_pool(enemy_start_strength: i64, start_hp: i64, hp_lost: i64, predicted: i64, max_turn_loss: i64) -> i64 {
    let base = enemy_start_strength / 20;
    if hp_lost <= 0 {
        return 3 * predicted + base + max_turn_loss;
    }
    if predicted > 0 {
        let q = (predicted as f64 / hp_lost as f64).clamp(0.8, 3.0);
        return round_half_even(predicted as f64 * q + base as f64 + max_turn_loss as f64);
    }
    if start_hp <= 0 {
        return base;
    }
    let kept = ((start_hp - hp_lost) as f64 / start_hp as f64).max(0.0);
    round_half_even(base as f64 * kept)
}

/// A survivor's share of the pool: `t = pool / 4 / N₀` (N₀ = units at the start, the dead
/// included), then `t·((4 − row) + row·useful/(actions taken + actions left))`, or just `t`
/// when the unit took and had no actions; below 0.5 it is 1.
pub fn share(pool: i64, start_count: usize, row: Row, useful: i32, taken: i32, left: i32) -> i32 {
    let t = pool as f64 * 0.25 / start_count.max(1) as f64;
    let r = row.number() as f64;
    let spent = taken + left;
    let s = if spent > 0 { (4.0 - r) * t + r * t * useful as f64 / spent as f64 } else { t };
    let s = if s < 0.5 { 1.0 } else { s };
    round_half_even(s).clamp(0, i32::MAX as i64) as i32
}

/// What one of the player's units gains from its share after a won battle:
/// `round(share × HeroExpirienceModificator × F × correction / 1 000 000)`, at most
/// [`MAX_BATTLE_XP`]. `correction` is the beaten army's experience correction (100 for a
/// garrison), F the difficulty factor.
pub fn player_gain(share: i32, hero_modificator: i32, difficulty: i32, correction: i32) -> i32 {
    let x = share as f64 * hero_modificator as f64 * difficulty as f64 * correction as f64 / 1_000_000.0;
    (round_half_even(x).abs().min(MAX_BATTLE_XP as i64)) as i32
}

/// What an AI unit gains from its share: `share × AIExpiriencePercent div 100`.
pub fn ai_gain(share: i32, ai_percent: i32) -> i32 {
    (share as i64 * ai_percent as i64 / 100).max(0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::testkit::*;
    use crate::rules::content::UnitDef;

    fn stats_of(u: UnitDef) -> Stats {
        let c = content(vec![u], vec![]);
        Stats::of_level(&c, UnitId(c.units[0].id), 1)
    }

    #[test]
    fn delphi_round_is_half_even() {
        assert_eq!([0.5, 1.5, 2.5, 3.5, -2.5, 2.4, 2.6].map(round_half_even), [0, 2, 2, 4, -2, 2, 3]);
    }

    #[test]
    fn xp_table_is_geometric_from_the_first_level() {
        // A sorceress (580, 140%): "Уровень 1, 0 / 580" as hired; the cuirassier (400) needs
        // 560 at level 2; the archmage hero (90, 160%) 590 at level 5.
        assert_eq!(xp_to_next(580, 140, 1), 580);
        assert_eq!(xp_to_next(400, 140, 2), 560);
        assert_eq!(xp_to_next(90, 160, 5), 590);
        assert_eq!((1..=4).map(|l| xp_to_next(60, 140, l)).collect::<Vec<_>>(), vec![60, 84, 118, 165]);
    }

    #[test]
    fn percent_stats_close_the_gap_to_100() {
        assert_eq!(percent_stat(20, 5, 0), 20);
        assert_eq!(percent_stat(20, 5, 1), 24, "80 × 0.95 = 76 left");
        assert_eq!(percent_stat(0, 10, 2), 19, "100 × 0.9² = 81 left");
        assert_eq!(percent_stat(90, 50, 5), 99, "capped");
        assert_eq!(percent_stat(100, 0, 0), 99, "even without levels");
    }

    #[test]
    fn roles_follow_the_largest_attack() {
        assert_eq!(role(&stats_of(warrior(1, 20, 5))), Role::Melee);
        assert_eq!(role(&stats_of(shooter(1, 20))), Role::Shooter);
        assert_eq!(role(&stats_of(mage(1, 20, MagicSchool::Life, MagicDirection::ToAlly))), Role::Mage);
        // A warrior with a weak bow: score 30 + 12/3 = 34, the bow would need 22.7; with a
        // bow of 26 the score is 38 and 26 reaches its two thirds.
        let mut u = warrior(1, 30, 5);
        u.attack_shot = 12;
        assert_eq!(role(&stats_of(u.clone())), Role::Melee);
        u.attack_shot = 26;
        assert_eq!(role(&stats_of(u)), Role::Shooter);
    }

    #[test]
    fn strength_of_a_plain_warrior() {
        // H = 50; D = round((e^(5/21.5)/1.17 + e^(5/30.3)/1.07)·50) = round((1.0785 + 1.1023)·50) = 109;
        // H' = 159; A = 20·1 + 20·10/100 = 22; T = round(3.2·159·23/200) = 59.
        let s = stats_of(warrior(1, 20, 5));
        assert_eq!(strength(&s, 0, 60), 59);
        // A building's defence makes it tougher.
        assert!(strength(&s, 10, 60) > 59);
        // No initiative: no attack value, T = round(3.2·159·1/200) = 3.
        let mut slow = warrior(1, 20, 5);
        slow.initiative = 0;
        assert_eq!(strength(&stats_of(slow), 0, 60), 3);
    }

    #[test]
    fn strength_bonus_terms() {
        let base = strength(&stats_of(warrior(1, 20, 5)), 0, 60);
        let with = |b: Bonus| {
            let mut u = warrior(1, 20, 5);
            u.bonus = Some(b);
            strength(&stats_of(u), 0, 60)
        };
        assert!(with(Bonus::Unvulnerabe) > 3 * base, "D = 10·H");
        assert_eq!(with(Bonus::DeathCurse), base + 150);
        assert!(with(Bonus::GodStrike) > with(Bonus::GodAnger));
        assert!(with(Bonus::Poison) > base);
        // Cannons: a ranged attack of ShotWeaponRange or more divides by 3, except Artillery.
        let gun = strength(&stats_of(shooter(1, 60)), 0, 60);
        assert_eq!(gun, strength(&stats_of(shooter(1, 60)), 0, 61) / 3);
        let mut art = shooter(1, 60);
        art.bonus = Some(Bonus::Artillery);
        assert!(strength(&stats_of(art), 0, 60) > gun * 2);
    }

    #[test]
    fn mage_strength_by_direction_and_school() {
        let m = |dir, school| {
            let mut u = mage(1, 20, school, dir);
            u.manevres = 2;
            strength(&stats_of(u), 0, 60)
        };
        let ally = m(MagicDirection::ToAlly, MagicSchool::Life);
        let all = m(MagicDirection::ToAll, MagicSchool::Life);
        let enemy = m(MagicDirection::ToEnemy, MagicSchool::Life);
        let elemental = m(MagicDirection::ToEnemy, MagicSchool::Elemental);
        // ToAll ×1.2; ToEnemy with 2 actions: 0.8·20·1/2 + 0.2·20 = 12 per action.
        assert!(all > ally && ally > enemy, "{all} {ally} {enemy}");
        assert!(elemental > all, "+25 for Elemental");
    }

    #[test]
    fn tactical_applies_cost_multipler() {
        let mut u = warrior(1, 20, 5);
        u.cost_multiplier = 50;
        let c = content(vec![u], vec![]);
        let s = Stats::of_level(&c, UnitId(1), 1);
        assert_eq!(tactical(&c, UnitId(1), &s, 0), 29);
    }

    fn su(tactical: i32, hp: i32, row: Row, role: Role) -> SideUnit {
        SideUnit { tactical, hp, max_hp: 100, row, role }
    }

    #[test]
    fn side_strength_rows_and_roles() {
        // Front only.
        assert_eq!(side_strength(&[su(100, 100, Row::Front, Role::Melee), su(50, 50, Row::Front, Role::Melee)]), 125);
        // A back-row shooter counts twice, scaled by min(1, 0.8·100/200 + 0.2) = 0.6.
        assert_eq!(side_strength(&[su(100, 100, Row::Front, Role::Melee), su(100, 100, Row::Back, Role::Shooter)]), 220);
        // A back-row warrior counts once: 0.8·100/100 + 0.2 = 1.
        assert_eq!(side_strength(&[su(100, 100, Row::Front, Role::Melee), su(100, 100, Row::Back, Role::Melee)]), 200);
        // The reserve in full; the dead not at all.
        assert_eq!(side_strength(&[su(100, 100, Row::Reserve, Role::Melee), su(80, 0, Row::Front, Role::Melee)]), 100);
        // A lone survivor that is not melee counts a fifth: 200·0.2 div 5.
        assert_eq!(side_strength(&[su(100, 100, Row::Back, Role::Shooter)]), 8);
        assert_eq!(side_strength(&[su(100, 100, Row::Reserve, Role::Mage), su(80, 0, Row::Front, Role::Melee)]), 20);
        assert_eq!(side_strength(&[su(100, 100, Row::Front, Role::Melee)]), 100);
    }

    #[test]
    fn pool_from_enemy_strength_and_hp_kept() {
        assert_eq!(battle_pool(2019, 500, 0, 0, 0), 100, "2019 div 20");
        assert_eq!(battle_pool(2000, 500, 125, 0, 0), 75);
        assert_eq!(battle_pool(2000, 500, 900, 0, 0), 0);
        // The worked example: base 50, pred 120, lost 60 → q = 2, maxTurn 40: 120·2 + 50 + 40.
        assert_eq!(battle_pool(1000, 500, 60, 120, 40), 330);
        assert_eq!(battle_pool(1000, 500, 600, 120, 40), 120 * 4 / 5 + 50 + 40, "q at least 0.8");
        assert_eq!(battle_pool(1000, 500, 10, 120, 40), 120 * 3 + 50 + 40, "q at most 3");
        assert_eq!(battle_pool(1000, 500, 0, 120, 0), 3 * 120 + 50, "nothing lost");
    }

    #[test]
    fn shares_by_row_and_activity() {
        // pool 160, 4 units: t = 10.
        assert_eq!(share(160, 4, Row::Front, 0, 2, 0), 30, "front, idle: 3t");
        assert_eq!(share(160, 4, Row::Front, 2, 2, 0), 40, "front, busy: 4t");
        assert_eq!(share(160, 4, Row::Back, 1, 2, 0), 30, "back, half busy: 2t + 2t/2");
        assert_eq!(share(160, 4, Row::Reserve, 0, 1, 1), 10, "reserve, idle: t");
        assert_eq!(share(160, 4, Row::Front, 0, 0, 0), 10, "no actions at all: t");
        assert_eq!(share(0, 4, Row::Front, 0, 1, 0), 1, "at least 1");
    }

    #[test]
    fn player_and_ai_modifiers() {
        // HeroExpirienceModificator 50, normal difficulty 120, correction 100: ×0.6.
        assert_eq!(player_gain(40, 50, 120, 100), 24);
        assert_eq!(player_gain(40, 50, 100, 100), 20, "impossible difficulty");
        assert_eq!(player_gain(40, 50, 120, 250), 60, "the beaten army's correction");
        assert_eq!(player_gain(1_000_000, 100, 100, 100), MAX_BATTLE_XP, "Community cap");
        assert_eq!(ai_gain(40, 100), 40);
        assert_eq!(ai_gain(45, 50), 22);
    }
}
