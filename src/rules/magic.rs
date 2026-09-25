//! World-map spells (`docs/reference/mechanics.md` §3.1–3.2): the hero casts a spell of his
//! book on his own army (`Target=Hero`) or on a nearby enemy army (`Target=Enemy`,
//! `OneEnemy`). Casting costs mana **and game time**: the world moves on meanwhile (armies
//! walk, the noon report comes, the scenario's events run), so the target may run off or reach
//! the hero first.
//!
//! - Cost: `CostMana`, time: `TimeCast` hours. The Archmage casts twice as fast for half the
//!   mana; a `Caster` in the army takes another 20% off both ([`cast_cost`]).
//! - Duration: `TimeWork` hours; empty or 0 is instant, 9999 or more lasts for good
//!   ([`Duration`]). Lasting `d-`/`p-` modifiers change the stats of the army's units in its
//!   next battles while they last ([`apply`], hooked into [`Game::start_battle`]).
//! - `DeltaFixedHits` / `DeltaPercentHits` heal or wound every living unit at once when the
//!   spell is ready.
//! - The scenario's events cast spells on the player's army through the same path
//!   ([`Game::apply_spell_to_army`]), for free and at once.
//!
//! Choices where the sources are silent are marked *(guess)* and listed in mechanics.md §8.3.

use serde::{Deserialize, Serialize};

use crate::dt::data::SpellTarget;

use super::clock::MINUTES_PER_HOUR;
use super::content::{Bonus, HeroClass, SpellDef, Stat};
use super::game::{troop_unit, Event, Game};
use super::units::Stats;

/// `TimeWork` at or above this lasts for good (the data use 9999).
pub const PERMANENT_HOURS: i32 = 9999;
/// An enemy army can be targeted within this many cells of the hero, if he can see it
/// *(guess: the original's range is not known; 3 cells is about the distance at which the
/// footage shows armies closing in)*.
pub const CAST_RANGE: i32 = 3;
/// The Community bonus token of units that cast world spells 20% faster and cheaper.
pub const CASTER_BONUS: &str = "Caster";
/// Slice of game time simulated at once while casting, as for waits.
const STEP_MINUTES: f32 = 5.0;

/// A lasting spell on an army: which spell, and the game minute it ends (`None`: never).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveSpell {
    pub spell: u32,
    pub until: Option<u64>,
}

impl ActiveSpell {
    pub fn lasts_at(&self, now: u64) -> bool {
        self.until.is_none_or(|t| now < t)
    }
}

/// How long a spell's modifiers last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duration {
    Instant,
    Minutes(u64),
    Permanent,
}

impl Duration {
    /// From `TimeWork` (hours at caster level 0; the level scaling is unknown and not
    /// applied *(guess)*).
    pub fn of(spell: &SpellDef) -> Duration {
        match spell.time_work {
            None | Some(..=0) => Duration::Instant,
            Some(h) if h >= PERMANENT_HOURS => Duration::Permanent,
            Some(h) => Duration::Minutes(h as u64 * MINUTES_PER_HOUR),
        }
    }
}

/// Mana and game minutes a cast takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastCost {
    pub mana: i32,
    pub minutes: u64,
}

/// `CostMana` and `TimeCast` hours; the Archmage halves both, a `Caster` takes 20% off
/// both. The two stack (×0.5 × 0.8) *(guess)*; results are rounded to whole mana and minutes.
pub fn cast_cost(spell: &SpellDef, archmage: bool, caster: bool) -> CastCost {
    let mut mana = spell.cost_mana.max(0) as f64;
    let mut minutes = spell.time_cast.unwrap_or(0).max(0) as f64 * MINUTES_PER_HOUR as f64;
    if archmage {
        mana /= 2.0;
        minutes /= 2.0;
    }
    if caster {
        mana *= 0.8;
        minutes *= 0.8;
    }
    CastCost { mana: mana.round() as i32, minutes: minutes.round() as u64 }
}

/// Casts on an enemy army (`Enemy`, and `OneEnemy`, which Razdor treats alike *(guess)*);
/// everything else on the hero's own army.
pub fn targets_enemy(spell: &SpellDef) -> bool {
    matches!(spell.target, Some(SpellTarget::Enemy | SpellTarget::OneEnemy))
}

/// The spell changes stats for a while (it has modifiers and a duration).
pub fn is_lasting(spell: &SpellDef) -> bool {
    let mods = !spell.add.is_empty() || !spell.percent.is_empty() || spell.life_lose_percent.is_some_and(|p| p < 0);
    mods && Duration::of(spell) != Duration::Instant
}

/// Applies lasting spells to `stats`: all `d-` values are added, then the `p-` percentages
/// of all spells are summed per stat and applied, as for items. A negative `p-LifeLose` (the
/// scripted curses) counts as a percent loss of maximum HP *(guess)*.
pub fn apply(stats: &mut Stats, spells: &[&SpellDef]) {
    for s in spells {
        stats.add(&s.add, 1);
    }
    let mut pct = std::collections::BTreeMap::<Stat, i32>::new();
    for s in spells {
        for (&st, &v) in &s.percent {
            *pct.entry(st).or_default() += v;
        }
        if let Some(p) = s.life_lose_percent.filter(|&p| p < 0) {
            *pct.entry(Stat::Hits).or_default() += p;
        }
    }
    for (st, p) in pct {
        stats[st] += stats[st] * p / 100;
    }
    stats.clamp();
}

/// Instant HP change of a spell for a unit of maximum `max`: `DeltaFixedHits` plus
/// `DeltaPercentHits`% of the maximum.
fn instant_hits(spell: &SpellDef, max: i32) -> i32 {
    spell.delta_fixed_hits.unwrap_or(0) + max * spell.delta_percent_hits.unwrap_or(0) / 100
}

/// Whom to cast on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastTarget {
    /// The hero's own army.
    Own,
    /// An army on the map, by its [`crate::rules::world::Army::uid`].
    Army(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastError {
    NotInBook,
    NoSuchSpell,
    NotEnoughMana,
    /// An own-army spell cast on an enemy, or the other way round.
    WrongTarget,
    /// The enemy is too far away or out of sight.
    OutOfRange,
    /// A battle is pending.
    Busy,
}

/// How a cast ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastOutcome {
    /// The spell took effect. `hits`: HP healed (+) or dealt (−) in all; `killed`: enemy
    /// troops that fell; `destroyed`: the whole enemy army fell.
    Done { hits: i32, killed: usize, destroyed: bool },
    /// An enemy caught the hero while he was casting: the mana is spent, the spell is lost
    /// *(guess)*.
    Interrupted,
    /// The target left the range (or the map) before the spell was ready; the mana is spent
    /// *(guess)*.
    TargetLost,
}

/// A finished cast: how it ended, and what happened while time passed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cast {
    pub outcome: CastOutcome,
    pub events: Vec<Event>,
}

impl Game {
    /// Spell definition by its 1-based id.
    pub fn spell(&self, id: u32) -> Option<&SpellDef> {
        self.content.spells.iter().find(|s| s.id == id)
    }

    /// The spells of the hero's book that the content knows, in book order.
    pub fn book(&self) -> Vec<&SpellDef> {
        self.spells.iter().filter_map(|&id| self.spell(id as u32)).collect()
    }

    /// What casting `spell` costs this hero: see [`cast_cost`]. The `Caster` bonus counts
    /// when any living unit of the army has it *(guess)*.
    pub fn cast_cost(&self, spell: &SpellDef) -> CastCost {
        let archmage = self.hero_class() == Some(HeroClass::Archmage);
        cast_cost(spell, archmage, self.squad_has(&Bonus::parse(CASTER_BONUS)))
    }

    /// Hostile armies the hero can cast on now: within [`CAST_RANGE`] cells and on explored
    /// ground. Indices into `world.armies`, nearest first.
    pub fn spell_targets(&self) -> Vec<usize> {
        let map = &self.world.map;
        let here = self.tile();
        let mut v: Vec<(i32, usize)> = self
            .world
            .armies
            .iter()
            .enumerate()
            .filter(|(_, a)| a.hostile() && self.fog.explored(a.tile(map)))
            .map(|(i, a)| (map.distance(a.tile(map), here), i))
            .filter(|&(d, _)| d <= CAST_RANGE)
            .collect();
        v.sort();
        v.into_iter().map(|(_, i)| i).collect()
    }

    /// Lasting spells on the hero's army, with the minute each ends.
    pub fn active_spells(&self) -> &[ActiveSpell] {
        &self.effects
    }

    /// Definitions of the lasting spells on the hero's army now.
    pub fn army_spells(&self) -> Vec<&SpellDef> {
        let now = self.clock.total_minutes() as u64;
        self.effects.iter().filter(|e| e.lasts_at(now)).filter_map(|e| self.spell(e.spell)).collect()
    }

    /// Definitions of the lasting spells on army `i` of the map now.
    pub fn spells_on_army(&self, i: usize) -> Vec<&SpellDef> {
        let now = self.clock.total_minutes() as u64;
        self.world.armies[i].effects.iter().filter(|e| e.lasts_at(now)).filter_map(|e| self.spell(e.spell)).collect()
    }

    /// Stats of squad member `u` with the lasting spells on the army (what it fights with).
    pub fn stats_with_spells(&self, u: usize) -> Stats {
        let mut s = self.squad[u].stats(&self.content);
        apply(&mut s, &self.army_spells());
        s
    }

    /// Drops the spells whose time is up, on the hero's army and on every army.
    pub(crate) fn expire_spells(&mut self) {
        let now = self.clock.total_minutes() as u64;
        self.effects.retain(|e| e.lasts_at(now));
        let w = &mut self.world;
        for a in w.armies.iter_mut().chain(w.inactive.iter_mut()) {
            a.effects.retain(|e| e.lasts_at(now));
        }
    }

    fn effect_of(&self, spell: &SpellDef) -> Option<ActiveSpell> {
        let now = self.clock.total_minutes() as u64;
        let until = match Duration::of(spell) {
            Duration::Instant => return None,
            Duration::Minutes(m) => Some(now + m),
            Duration::Permanent => None,
        };
        is_lasting(spell).then_some(ActiveSpell { spell: spell.id, until })
    }

    /// Casts `spell` of the book on `target`: pays the mana, lets [`CastCost::minutes`] of
    /// game time pass (armies move, events run; an enemy reaching the hero interrupts it),
    /// then the spell takes effect if the target is still there.
    pub fn cast(&mut self, spell: u32, target: CastTarget) -> Result<Cast, CastError> {
        if self.foe.is_some() {
            return Err(CastError::Busy);
        }
        if !self.spells.iter().any(|&s| s as u32 == spell) {
            return Err(CastError::NotInBook);
        }
        let def = self.spell(spell).ok_or(CastError::NoSuchSpell)?.clone();
        match target {
            CastTarget::Own if targets_enemy(&def) => return Err(CastError::WrongTarget),
            CastTarget::Army(_) if !targets_enemy(&def) => return Err(CastError::WrongTarget),
            CastTarget::Army(uid) if !self.spell_targets().iter().any(|&i| self.world.armies[i].uid == uid) => {
                return Err(CastError::OutOfRange)
            }
            _ => {}
        }
        let cost = self.cast_cost(&def);
        if self.mana < cost.mana {
            return Err(CastError::NotEnoughMana);
        }
        self.mana -= cost.mana;
        self.stop();
        let mut events = Vec::new();
        let mut left = cost.minutes as f32;
        while left > 0.0 {
            let slice = left.min(STEP_MINUTES);
            left -= slice;
            self.pass_time(slice, &mut events);
            if let Some(e) = self.contact() {
                self.meet(e, &mut events);
            }
            if self.foe.is_some() {
                return Ok(Cast { outcome: CastOutcome::Interrupted, events });
            }
        }
        let outcome = match target {
            CastTarget::Own => {
                let hits = self.apply_spell_to_army(&def);
                CastOutcome::Done { hits, killed: 0, destroyed: false }
            }
            CastTarget::Army(uid) => match self.spell_targets().into_iter().find(|&i| self.world.armies[i].uid == uid) {
                Some(i) => self.apply_spell_to_enemy(&def, i),
                None => CastOutcome::TargetLost,
            },
        };
        Ok(Cast { outcome, events })
    }

    /// The spell takes effect on the hero's army: instant healing or wounds on every living
    /// unit (wounds leave at least 1 HP *(guess)*), and its lasting modifiers (a spell cast
    /// again starts its time anew). A positive `p-LifeLose` lifts the life-draining curses
    /// *(guess)*. Returns the HP change in all. Events cast spells through this too.
    pub fn apply_spell_to_army(&mut self, spell: &SpellDef) -> i32 {
        let c = self.content.clone();
        let mut total = 0;
        for u in self.squad.iter_mut().filter(|u| u.alive()) {
            let max = u.max_hp(&c);
            let d = instant_hits(spell, max);
            let hp = (u.hp + d).clamp(1, max.max(1));
            total += hp - u.hp;
            u.hp = hp;
        }
        if spell.life_lose_percent.is_some_and(|p| p > 0) {
            self.effects.retain(|e| c.spells.iter().find(|s| s.id == e.spell).is_none_or(|s| s.life_lose_percent.is_none_or(|p| p >= 0)));
        }
        if let Some(e) = self.effect_of(spell) {
            self.effects.retain(|x| x.spell != e.spell);
            self.effects.push(e);
        }
        total
    }

    /// The spell takes effect on army `i` of the map: instant wounds (a troop they kill
    /// falls; an army with nobody left is beaten, with no loot to take *(guess)*) and its
    /// lasting modifiers.
    fn apply_spell_to_enemy(&mut self, spell: &SpellDef, i: usize) -> CastOutcome {
        let c = self.content.clone();
        let effect = self.effect_of(spell);
        let a = &mut self.world.armies[i];
        let (mut hits, mut killed) = (0, 0);
        a.troops.retain_mut(|t| {
            let max = troop_unit(&c, t).max_hp(&c);
            let hp = max - t.hurt;
            let new = (hp + instant_hits(spell, max)).min(max);
            hits += new.max(0) - hp;
            t.hurt = max - new;
            let alive = new > 0;
            killed += usize::from(!alive);
            alive
        });
        if let Some(e) = effect {
            a.effects.retain(|x| x.spell != e.spell);
            a.effects.push(e);
        }
        let destroyed = a.troops.is_empty();
        if destroyed {
            let a = self.world.armies.remove(i);
            if a.id != 0 {
                self.beaten_armies.insert(a.id);
            }
            let events = self.run_script();
            self.pending.extend(events);
        }
        CastOutcome::Done { hits, killed, destroyed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::battle::{Outcome, Team};
    use crate::rules::content::testkit as ck;
    use crate::rules::content::{Content, StatMods, UnitDef};
    use crate::rules::formation::Formation;
    use crate::rules::game::Foe;
    use crate::rules::world::testkit::{self as tk, army, hero, scenario, troop};
    use std::sync::Arc;

    fn spell(id: u32, mana: i32, cast: i32, work: Option<i32>) -> SpellDef {
        SpellDef { cost_mana: mana, time_cast: Some(cast), time_work: work, delta_fixed_hits: None, ..ck::spell(id, 100) }
    }

    /// 1 heal +30 (instant, 4 h), 2 armour d-DefenceBlow +5 and p-Hits +20 for 10 h,
    /// 3 lightning −15 on an enemy (6 h), 4 weakness p-AttackBlow −50 on an enemy for 8 h,
    /// 5 a permanent curse p-LifeLose −20, 6 its lifting p-LifeLose +20.
    fn spells() -> Vec<SpellDef> {
        let heal = SpellDef { delta_fixed_hits: Some(30), ..spell(1, 200, 4, None) };
        let armour = SpellDef {
            add: StatMods::from([(Stat::DefenceBlow, 5)]),
            percent: StatMods::from([(Stat::Hits, 20)]),
            ..spell(2, 300, 6, Some(10))
        };
        let lightning = SpellDef { delta_fixed_hits: Some(-15), target: Some(SpellTarget::Enemy), ..spell(3, 500, 6, None) };
        let weakness =
            SpellDef { percent: StatMods::from([(Stat::AttackBlow, -50)]), target: Some(SpellTarget::Enemy), ..spell(4, 160, 2, Some(8)) };
        let curse = SpellDef { life_lose_percent: Some(-20), ..spell(5, 1, 1, Some(9999)) };
        let lift = SpellDef { life_lose_percent: Some(20), time_cast: None, ..spell(6, 1, 1, None) };
        vec![heal, armour, lightning, weakness, curse, lift]
    }

    fn content() -> Arc<Content> {
        let c = tk::content();
        let mut units = c.units.clone();
        // Unit 12: a caster (bonus Caster), unit 13: a frail peasant (10 HP).
        units.push(UnitDef { bonus: Some(Bonus::parse(CASTER_BONUS)), ..ck::warrior(12, 5, 1) });
        units.push(UnitDef { hits: 10, ..ck::warrior(13, 3, 0) });
        Arc::new(Content::new(units, c.items.clone(), spells(), c.options.clone(), Formation::WIDE))
    }

    /// A 24×6 strip; the hero starts at (2, 2) with two warriors, 1000 mana and all spells.
    fn game(class: HeroClass) -> Game {
        let mut s = scenario(24, 6);
        s.header.heroes[0] = hero(2, 2, 200, &[troop(4, 0, 2)]);
        s.header.heroes[1] = hero(2, 2, 200, &[troop(4, 0, 2)]);
        let mut g = Game::from_scenario(content(), &s, class, 5);
        g.mana = 1000;
        g.spells = (1..=6).collect();
        g
    }

    fn with_enemy(g: &mut Game, at: (u16, u16), troops: &[crate::dt::dtm::Troop]) -> u32 {
        let mut s = scenario(24, 6);
        s.armies = vec![army(9, at.0, at.1, -2, troops)];
        let w = crate::rules::world::World::from_scenario(&s, &g.content);
        let a = w.armies[0].clone();
        let uid = a.uid;
        g.world.armies.push(a);
        uid
    }

    #[test]
    fn cost_and_time_with_archmage_and_caster() {
        let s = spell(1, 200, 4, None);
        assert_eq!(cast_cost(&s, false, false), CastCost { mana: 200, minutes: 240 });
        assert_eq!(cast_cost(&s, true, false), CastCost { mana: 100, minutes: 120 }, "twice as fast for half the mana");
        assert_eq!(cast_cost(&s, false, true), CastCost { mana: 160, minutes: 192 }, "Caster: -20%");
        assert_eq!(cast_cost(&s, true, true), CastCost { mana: 80, minutes: 96 });

        let mut g = game(HeroClass::Archmage);
        assert_eq!(g.hero_class(), Some(HeroClass::Archmage));
        assert_eq!(g.cast_cost(g.spell(1).unwrap()), CastCost { mana: 100, minutes: 120 });
        g.squad[1].def = crate::rules::content::UnitId(12);
        assert_eq!(g.cast_cost(g.spell(1).unwrap()), CastCost { mana: 80, minutes: 96 });
    }

    #[test]
    fn casting_spends_mana_and_game_time() {
        let mut g = game(HeroClass::Knight);
        g.squad[1].hp = 5;
        let start = g.clock.total_minutes();
        let cast = g.cast(1, CastTarget::Own).unwrap();
        assert_eq!(cast.outcome, CastOutcome::Done { hits: 30, killed: 0, destroyed: false });
        assert_eq!((g.mana, g.clock.total_minutes() - start), (800, 240.0));
        assert_eq!(g.squad[1].hp, 35);
        assert!(g.active_spells().is_empty(), "instant");
        g.mana = 10;
        assert_eq!(g.cast(1, CastTarget::Own), Err(CastError::NotEnoughMana));
        assert_eq!(g.cast(3, CastTarget::Own), Err(CastError::WrongTarget));
        g.spells.clear();
        assert_eq!(g.cast(1, CastTarget::Own), Err(CastError::NotInBook));
    }

    #[test]
    fn instant_heal_is_capped_and_wounds_leave_one_hp() {
        let mut g = game(HeroClass::Knight);
        let max = g.squad[1].max_hp(&g.content);
        g.squad[1].hp = max - 10;
        g.squad[2].hp = 0; // a corpse is not raised
        let heal = g.spell(1).unwrap().clone();
        assert_eq!(g.apply_spell_to_army(&heal), 10 + (g.hero().max_hp(&g.content) - g.hero().hp));
        assert_eq!((g.squad[1].hp, g.squad[2].hp), (max, 0));
        let wound = SpellDef { delta_fixed_hits: Some(-1000), ..heal };
        g.apply_spell_to_army(&wound);
        assert_eq!(g.squad[1].hp, 1);
    }

    #[test]
    fn lasting_spells_expire_after_their_duration() {
        let mut g = game(HeroClass::Knight);
        g.cast(2, CastTarget::Own).unwrap();
        let now = g.clock.total_minutes() as u64;
        assert_eq!(g.active_spells(), &[ActiveSpell { spell: 2, until: Some(now + 600) }]);
        g.wait(9);
        assert_eq!(g.army_spells().len(), 1);
        g.wait(1);
        assert!(g.active_spells().is_empty(), "10 h later");
        // Permanent curses stay until lifted.
        let curse = g.spell(5).unwrap().clone();
        g.apply_spell_to_army(&curse);
        assert_eq!(g.active_spells(), &[ActiveSpell { spell: 5, until: None }]);
        g.wait(24 * 30);
        assert_eq!(g.active_spells().len(), 1);
        let max = g.squad[0].max_hp(&g.content);
        assert_eq!(g.stats_with_spells(0).max_hp(), max - max * 20 / 100);
        let lift = g.spell(6).unwrap().clone();
        g.apply_spell_to_army(&lift);
        assert!(g.active_spells().is_empty());
    }

    #[test]
    fn lasting_effects_apply_to_stats_in_battle() {
        let mut g = game(HeroClass::Knight);
        let uid = with_enemy(&mut g, (12, 2), &[troop(4, 0, 2)]);
        let before = g.squad[1].stats(&g.content);
        g.cast(2, CastTarget::Own).unwrap();
        let s = g.stats_with_spells(1);
        assert_eq!(s[Stat::DefenceBlow], before[Stat::DefenceBlow] + 5);
        assert_eq!(s.max_hp(), before.max_hp() + before.max_hp() * 20 / 100);
        // Weakness on the enemy: walk up to it, cast.
        g.world.armies[0].pos = g.world.map.center((4, 2));
        g.world.armies[0].ignore_until = f64::MAX; // leaves the hero alone while he casts
        let cast = g.cast(4, CastTarget::Army(uid)).unwrap();
        assert!(matches!(cast.outcome, CastOutcome::Done { .. }), "{cast:?}");
        let enemy_atk = crate::rules::units::Stats::of_level(&g.content, crate::rules::content::UnitId(4), 1)[Stat::AttackBlow];
        g.foe = Some(Foe::Army(0));
        let b = g.start_battle();
        let mine = b.fighters.iter().find(|f| f.squad_index == Some(1)).unwrap();
        assert_eq!(mine.base[Stat::DefenceBlow], before[Stat::DefenceBlow] + 5);
        assert_eq!((mine.max_hp(), mine.hp), (s.max_hp(), g.squad[1].hp + s.max_hp() - before.max_hp()));
        let theirs = b.fighters.iter().find(|f| f.team == Team::Enemy).unwrap();
        assert_eq!(theirs.stats[Stat::AttackBlow], enemy_atk - enemy_atk * 50 / 100);
        assert_eq!(b.outcome(), Outcome::Ongoing);
    }

    #[test]
    fn enemy_spells_need_a_target_in_range_and_can_kill() {
        let mut g = game(HeroClass::Knight);
        let far = with_enemy(&mut g, (20, 2), &[troop(13, 0, 1)]);
        assert_eq!(g.cast(3, CastTarget::Army(far)), Err(CastError::OutOfRange));
        assert_eq!(g.cast(1, CastTarget::Army(far)), Err(CastError::WrongTarget));
        g.world.armies.clear();
        // Two peasants (10 HP) and a warrior: lightning kills the peasants, wounds the other.
        let uid = with_enemy(&mut g, (5, 2), &[troop(13, 0, 2), troop(4, 0, 1)]);
        g.world.armies[0].ignore_until = f64::MAX;
        let cast = g.cast(3, CastTarget::Army(uid)).unwrap();
        assert_eq!(cast.outcome, CastOutcome::Done { hits: -35, killed: 2, destroyed: false });
        let a = &g.world.armies[0];
        assert_eq!((a.troops.len(), a.troops[0].hurt), (1, 15));
        g.foe = Some(Foe::Army(0));
        let b = g.start_battle();
        let e = b.fighters.iter().find(|f| f.team == Team::Enemy).unwrap();
        assert_eq!(e.hp, e.max_hp() - 15, "fights wounded");
        g.foe = None;
        // The warrior has 50 HP: three more bolts end the army.
        for _ in 0..3 {
            g.mana = 1000;
            g.cast(3, CastTarget::Army(uid)).unwrap();
        }
        assert!(g.world.armies.is_empty());
        assert!(g.beaten_armies.contains(&9));
    }

    #[test]
    fn events_cast_through_the_same_path() {
        use crate::rules::events::EventWorld;
        let mut g = game(HeroClass::Knight);
        g.squad[1].hp = 1;
        let mana = g.mana;
        let t = g.clock;
        g.apply_spell(1);
        assert_eq!((g.squad[1].hp, g.mana, g.clock), (31, mana, t), "free and at once");
        g.apply_spell(2);
        assert_eq!(g.active_spells().len(), 1);
        g.apply_spell(200); // unknown: nothing
        assert_eq!(g.active_spells().len(), 1);
    }

    #[test]
    fn an_enemy_reaching_the_hero_interrupts_the_cast() {
        let mut g = game(HeroClass::Knight);
        with_enemy(&mut g, (6, 2), &[troop(4, 0, 1)]);
        let mana = g.mana;
        let cast = g.cast(2, CastTarget::Own).unwrap();
        assert_eq!(cast.outcome, CastOutcome::Interrupted);
        assert!(cast.events.iter().any(|e| matches!(e, Event::Encounter(_))));
        assert_eq!(g.mana, mana - 300);
        assert!(g.active_spells().is_empty());
    }
}
