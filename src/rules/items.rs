//! Items (artefacts): slots, class limits, stat modifiers and potions (mechanics.md 4).
//!
//! A unit wears up to 4 items, only one weapon (melee weapon, bow or staff), never two of the
//! same type. Melee weapons need a warrior, ranged weapons a shooter, staffs a mage. Potions
//! and trade goods are not worn: potions are drunk from the army screen, their healing is
//! instant and their other modifiers last until the end of the next battle.

use crate::i18n::tr;
use super::content::{ArtefactDef, ArtefactType, Bonus, Content, Stat};
pub use super::content::{ItemId, Source};
use super::units::{Stats, Unit};

/// Item slots every unit has.
pub const SLOTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquipError {
    NoFreeSlot,
    /// Already wears an item of this type.
    SameType,
    /// Already holds a weapon or staff.
    SecondWeapon,
    /// A melee weapon or shield on a non-warrior, a bow on a non-shooter (or on artillery),
    /// or a staff on a non-mage.
    WrongClass,
    /// A holy item on an undead unit.
    Unholy,
    /// The crown on a unit that may not wear it.
    NotAllowed,
    /// Potions and trade goods cannot be worn.
    NotWearable,
    /// The dead cannot hold items.
    Dead,
    NotAPotion,
    PackFull,
    NoSuchItem,
}

/// One `p-` modifier of an item, potion or spell on stat value `x`: `x + x·p/100`, truncated
/// (economy.md §5). On a percent stat (the three protections, regeneration, vampirism) a
/// positive `p` closes that share of the gap to 100 instead, as levels do
/// (experience.md): the player observes «Святое писание» (`p-ProtectDeath=30`) giving a
/// unit without Death protection 30%, where the plain rule would give 0% of 0 *(guess for
/// units that already have some: 20% becomes 44%)*.
pub fn percent_mod(st: Stat, x: i32, p: i32) -> i32 {
    if p > 0 && crate::rules::experience::is_percent_stat(st) {
        x + (100 - x).max(0) * p / 100
    } else {
        x + x * p / 100
    }
}

/// Applies worn items and active potions to `stats` in the original's order
/// (original-mechanics/economy.md §5): each worn item's `f-` in slot order replaces its stat
/// when above 0 (a later slot wins); the potions' `d-`, then the items' `d-`; the potions'
/// `p-`, then each item's `p-` in turn, compounding ([`percent_mod`], truncated each time).
/// A potion's `f-Hits` is its healing and does not count here. Item bonuses are added to the
/// unit's, and an item's magic school replaces the unit's. Spells come after
/// (`magic::apply`).
pub fn apply(content: &Content, stats: &mut Stats, worn: &[ItemId], potions: &[ItemId]) {
    let worn: Vec<&ArtefactDef> = worn.iter().map(|&i| content.item(i)).collect();
    let potions: Vec<&ArtefactDef> = potions.iter().map(|&i| content.item(i)).collect();
    for d in &worn {
        for (&st, &v) in &d.fixed {
            if v > 0 {
                stats[st] = v;
            }
        }
    }
    for d in potions.iter().chain(&worn) {
        stats.add(&d.add, 1);
    }
    for d in potions.iter().chain(&worn) {
        for (&st, &v) in &d.percent {
            stats[st] = percent_mod(st, stats[st], v);
        }
    }
    for d in &worn {
        if let Some(b) = &d.bonus {
            stats.bonuses.push(b.clone());
        }
        if d.magic.is_some() {
            stats.magic = d.magic;
        }
    }
    stats.clamp();
}

/// Items the undead cannot wear (0x49765c, economy.md §5): the church's holy things.
const HOLY: [u32; 13] = [12, 46, 59, 72, 73, 74, 75, 76, 77, 85, 94, 120, 131];
/// «Королевская корона», worn only by the hero (or an army's leader) and these unit types.
const CROWN: u32 = 154;
const CROWN_WEARERS: [u32; 23] = [1, 2, 3, 11, 13, 15, 36, 42, 45, 46, 48, 49, 53, 56, 58, 69, 70, 72, 73, 77, 89, 97, 99];

/// Slot `item` would go into on `unit`, or why it can't be worn. The original's wear rules
/// (0x49765c, economy.md §5): a melee weapon or a shield needs melee attack, a ranged
/// weapon ranged attack and no artillery (a type whose ranged attack is above
/// `ShotWeaponRange`), a staff magic; one weapon, no two items of a type; no holy items on
/// the undead; the crown only on the hero and some unit types.
pub fn slot_for(content: &Content, unit: &Unit, item: ItemId) -> Result<usize, EquipError> {
    let def = content.try_item(item).ok_or(EquipError::NoSuchItem)?;
    if !unit.alive() {
        return Err(EquipError::Dead);
    }
    if matches!(def.kind, ArtefactType::Potion | ArtefactType::Item) {
        return Err(EquipError::NotWearable);
    }
    let base = unit.base_stats(content);
    let class_ok = match def.kind {
        ArtefactType::BlowWeapon | ArtefactType::Shield => base.is_warrior(),
        ArtefactType::ShotWeapon => base.is_shooter() && content.unit(unit.def).attack_shot <= content.options.shot_weapon_range,
        ArtefactType::Staff => base.is_mage(),
        _ => true,
    };
    if !class_ok {
        return Err(EquipError::WrongClass);
    }
    if HOLY.contains(&item.0) && base.has_any(&[Bonus::Dead, Bonus::FastDead]) {
        return Err(EquipError::Unholy);
    }
    if item.0 == CROWN && unit.wage_kind != crate::rules::content::WageKind::Leader && !CROWN_WEARERS.contains(&unit.def.0) {
        return Err(EquipError::NotAllowed);
    }
    let worn: Vec<&ArtefactDef> = unit.items.iter().flatten().map(|&i| content.item(i)).collect();
    if def.kind.is_weapon() && worn.iter().any(|w| w.kind.is_weapon()) {
        return Err(EquipError::SecondWeapon);
    }
    if worn.iter().any(|w| w.kind == def.kind) {
        return Err(EquipError::SameType);
    }
    unit.items.iter().position(Option::is_none).ok_or(EquipError::NoFreeSlot)
}

/// Healing of a potion (`f-Hits`).
pub fn heal_amount(def: &ArtefactDef) -> i32 {
    def.fixed.get(&Stat::Hits).copied().unwrap_or(0)
}

/// A potion changes stats besides healing.
fn has_lasting_effect(def: &ArtefactDef) -> bool {
    !def.add.is_empty() || !def.percent.is_empty() || def.fixed.keys().any(|s| *s != Stat::Hits)
}

/// Drinks potion `item` on `unit`: heals at once (capped at max HP), and its other modifiers
/// last until the end of the next battle. Returns HP restored.
pub fn drink(content: &Content, unit: &mut Unit, item: ItemId) -> Result<i32, EquipError> {
    let def = content.try_item(item).ok_or(EquipError::NoSuchItem)?;
    if def.kind != ArtefactType::Potion {
        return Err(EquipError::NotAPotion);
    }
    if !unit.alive() {
        return Err(EquipError::Dead);
    }
    if has_lasting_effect(def) {
        unit.potions.push(item);
    }
    let before = unit.hp;
    unit.hp = (unit.hp + heal_amount(def)).min(unit.max_hp(content));
    Ok(unit.hp - before)
}

/// Price the market pays before the difficulty factor and a Merchant: `ItemSaleCost`% of the
/// price (`Game::sell_price` has the whole rule).
pub fn sell_price(content: &Content, item: ItemId) -> i32 {
    (content.item(item).cost * content.options.item_sale_cost / 100).max(0)
}

pub fn kind_name(kind: ArtefactType) -> &'static str {
    match kind {
        ArtefactType::BlowWeapon => tr("melee weapon"),
        ArtefactType::ShotWeapon => tr("ranged weapon"),
        ArtefactType::Staff => tr("staff"),
        ArtefactType::Armor => tr("armour"),
        ArtefactType::Helm => tr("helm"),
        ArtefactType::Shield => tr("shield"),
        ArtefactType::Ring => tr("ring"),
        ArtefactType::Amulet => tr("amulet"),
        ArtefactType::Potion => tr("potion"),
        ArtefactType::Item => tr("trade goods"),
    }
}

/// Short label of a stat for descriptions and cards.
pub fn stat_label(s: Stat) -> &'static str {
    match s {
        Stat::Hits => tr("hits"),
        Stat::AttackBlow => tr("attack"),
        Stat::DefenceBlow => tr("defence"),
        Stat::AttackShot => tr("shot"),
        Stat::DefenceShot => tr("shot defence"),
        Stat::MagicPower => tr("magic"),
        Stat::Initiative => tr("initiative"),
        Stat::Manevres => tr("actions"),
        Stat::ProtectLife => tr("life prot."),
        Stat::ProtectDeath => tr("death prot."),
        Stat::ProtectElemental => tr("elem. prot."),
        Stat::Regen => tr("regen %"),
        Stat::Vampirizm => tr("vampirism %"),
    }
}

/// Short name of a unit bonus for descriptions ("Long weapon"); the ini token for tokens
/// the game does not know.
pub fn bonus_name(b: &Bonus) -> String {
    let name = match b {
        Bonus::SpearDefense => tr("Long weapon"),
        Bonus::HorseAtack => tr("Fast attack"),
        Bonus::ArmorIgnore => tr("Piercing blow"),
        Bonus::ArmyMedic => tr("Healer"),
        Bonus::Merchant => tr("Expert trader"),
        Bonus::DeathCurse => tr("Death's curse"),
        Bonus::GodAnger => tr("Wrath of God"),
        Bonus::GodStrike => tr("Anger of God"),
        Bonus::Unvulnerabe => tr("Invulnerable"),
        Bonus::VampirsGist => tr("Dark gift"),
        Bonus::OldVampirsGist => tr("Dark art"),
        Bonus::Evasive => tr("Evasive"),
        Bonus::Ghost => tr("Ghost"),
        Bonus::Artillery => tr("Barrage"),
        Bonus::Garrison => tr("Garrison"),
        Bonus::AddPayment => tr("Quartermaster"),
        Bonus::Poison => tr("Poisoned weapon"),
        Bonus::Dead => tr("Undead"),
        Bonus::FastDead => tr("Fast undead"),
        Bonus::Counterblow => tr("Counterblow"),
        Bonus::FlankStrike => tr("Flank strike"),
        Bonus::Hunger => tr("Hunger"),
        Bonus::Berserk => tr("Berserk"),
        Bonus::Exhaustion => tr("Exhausting magic"),
        Bonus::Drying => tr("Withering magic"),
        Bonus::CtrPoison => tr("Poisonous body"),
        Bonus::Suicide => tr("Last strike"),
        Bonus::Caster => tr("Spellcaster"),
        Bonus::Splash => tr("Sweeping blow"),
        Bonus::Fortify => tr("Entrenchment"),
        Bonus::Dominate => tr("Dominance"),
        Bonus::PoisonS => tr("Strong poison"),
        Bonus::Concentration => tr("Concentration"),
        Bonus::Potent => tr("Potent magic"),
        Bonus::Stun => tr("Stunning blow"),
        Bonus::FirstShot => tr("First shot"),
        Bonus::Bastion => tr("Bastion"),
        Bonus::Flying => tr("Flying"),
        Bonus::Bleed => tr("Bleeding wounds"),
        Bonus::PreventiveStrike => tr("Preventive strike"),
        Bonus::Flock => tr("Strength in numbers"),
        Bonus::ArmorBreaker => tr("Armour breaker"),
        Bonus::NoHeal => tr("Festering wounds"),
        Bonus::FasterAttack => tr("Swift assault"),
        Bonus::PoisonArmorIgnore => tr("Poisoned piercing"),
        Bonus::HoldLine => tr("Hold the line"),
        Bonus::Neutralize => tr("Neutralising blow"),
        Bonus::KillingStrike => tr("Killing strike"),
        Bonus::BloodThrist => tr("Bloodthirst"),
        Bonus::Assault => tr("Assault"),
        Bonus::EternalGift => tr("Lasting gift"),
        Bonus::FateGift => tr("Gift of fate"),
        other => other.token(),
    };
    name.to_string()
}

/// Short summary, e.g. "melee weapon, attack +6, initiative -1".
pub fn describe(content: &Content, item: ItemId) -> String {
    let d = content.item(item);
    let mut parts = vec![kind_name(d.kind).to_string()];
    for (&st, &v) in &d.fixed {
        parts.push(if d.kind == ArtefactType::Potion && st == Stat::Hits {
            crate::trf!("heals {v}", v)
        } else {
            format!("{} = {v}", stat_label(st))
        });
    }
    for (&st, &v) in &d.add {
        parts.push(format!("{} {v:+}", stat_label(st)));
    }
    for (&st, &v) in &d.percent {
        parts.push(format!("{} {v:+}%", stat_label(st)));
    }
    if let Some(b) = &d.bonus {
        parts.push(bonus_name(b));
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::testkit::*;
    use crate::rules::content::{Bonus, MagicDirection, MagicSchool, StatMods, UnitDef, UnitId};
    use crate::rules::formation::{Row, Slot};

    #[test]
    fn a_percent_bonus_on_a_protection_closes_the_gap_to_100() {
        // «Святое писание»: p-ProtectDeath=30 on a unit with none.
        assert_eq!(percent_mod(Stat::ProtectDeath, 0, 30), 30);
        assert_eq!(percent_mod(Stat::ProtectDeath, 20, 30), 44, "20 + 80 × 30%");
        assert_eq!(percent_mod(Stat::Vampirizm, 0, 25), 25, "«Кровопийца» works from nothing");
        assert_eq!(percent_mod(Stat::ProtectLife, 60, -50), 30, "a curse scales it down");
        assert_eq!(percent_mod(Stat::AttackBlow, 40, 25), 50, "other stats as before");
        assert_eq!(percent_mod(Stat::ProtectElemental, 100, 30), 100);
    }

    fn gear() -> Vec<ArtefactDef> {
        let mut sword = item(1, ArtefactType::BlowWeapon);
        sword.add = StatMods::from([(Stat::AttackBlow, 5)]);
        let mut bow = item(2, ArtefactType::ShotWeapon);
        bow.add = StatMods::from([(Stat::AttackShot, 4)]);
        let staff = item(3, ArtefactType::Staff);
        let mut armour = item(4, ArtefactType::Armor);
        armour.fixed = StatMods::from([(Stat::DefenceBlow, 26)]);
        armour.add = StatMods::from([(Stat::DefenceBlow, 2)]);
        let mut ring = item(5, ArtefactType::Ring);
        ring.percent = StatMods::from([(Stat::AttackBlow, 50), (Stat::DefenceBlow, 10)]);
        ring.bonus = Some(Bonus::ArmorIgnore);
        let mut potion = item(6, ArtefactType::Potion);
        potion.fixed = StatMods::from([(Stat::Hits, 30)]);
        let mut might = item(7, ArtefactType::Potion);
        might.add = StatMods::from([(Stat::AttackBlow, 10)]);
        let armour2 = item(8, ArtefactType::Armor);
        let goods = item(9, ArtefactType::Item);
        vec![sword, bow, staff, armour, ring, potion, might, armour2, goods]
    }

    fn setup() -> (Content, Unit, Unit) {
        let c = content(
            vec![warrior(1, 20, 5), shooter(2, 10), mage(3, 10, MagicSchool::Death, MagicDirection::ToEnemy)],
            gear(),
        );
        let w = Unit::new(&c, UnitId(1), Slot::new(Row::Front, 0));
        let s = Unit::new(&c, UnitId(2), Slot::new(Row::Back, 0));
        (c, w, s)
    }

    #[test]
    fn modifiers_apply_fixed_then_flat_then_percent() {
        let (c, mut w, _) = setup();
        w.items = [Some(ItemId(1)), Some(ItemId(4)), Some(ItemId(5)), None];
        let s = w.stats(&c);
        // attack: (20 + 5) × 1.5; defence: fixed 26, +2, then +10%.
        assert_eq!((s[Stat::AttackBlow], s[Stat::DefenceBlow]), (37, 30));
        assert!(s.has(&Bonus::ArmorIgnore));
    }

    #[test]
    fn item_percentages_compound_one_item_at_a_time() {
        let (c, mut w, _) = setup();
        let mut ring2 = item(10, ArtefactType::Amulet);
        ring2.percent = StatMods::from([(Stat::AttackBlow, 50)]);
        let c = content(c.units.clone(), c.items.iter().cloned().chain([ring2]).collect());
        w.items = [Some(ItemId(5)), Some(ItemId(10)), None, None];
        // 20 → +50% = 30 → +50% = 45 (summed it would be 40).
        assert_eq!(w.stats(&c)[Stat::AttackBlow], 45);
    }

    #[test]
    fn one_weapon_one_per_type_and_class_limits() {
        let (c, mut w, s) = setup();
        assert_eq!(slot_for(&c, &w, ItemId(1)), Ok(0));
        assert_eq!(slot_for(&c, &w, ItemId(2)), Err(EquipError::WrongClass), "bow on a warrior");
        assert_eq!(slot_for(&c, &w, ItemId(3)), Err(EquipError::WrongClass), "staff on a warrior");
        assert_eq!(slot_for(&c, &s, ItemId(1)), Err(EquipError::WrongClass), "sword on a shooter");
        assert_eq!(slot_for(&c, &s, ItemId(2)), Ok(0));
        let m = Unit::new(&c, UnitId(3), Slot::new(Row::Back, 1));
        assert_eq!(slot_for(&c, &m, ItemId(3)), Ok(0));
        w.items[0] = Some(ItemId(1));
        assert_eq!(slot_for(&c, &w, ItemId(1)), Err(EquipError::SecondWeapon));
        w.items[1] = Some(ItemId(4));
        assert_eq!(slot_for(&c, &w, ItemId(8)), Err(EquipError::SameType));
        assert_eq!(slot_for(&c, &w, ItemId(6)), Err(EquipError::NotWearable));
        assert_eq!(slot_for(&c, &w, ItemId(9)), Err(EquipError::NotWearable));
        w.hp = 0;
        assert_eq!(slot_for(&c, &w, ItemId(5)), Err(EquipError::Dead));
    }

    #[test]
    fn the_originals_wear_rules_for_shields_artillery_holy_things_and_the_crown() {
        let undead = UnitDef { bonus: Some(Bonus::Dead), ..warrior(7, 20, 5) };
        let cannon = shooter(6, 70);
        let (shield, bow, holy, crown) = (item(30, ArtefactType::Shield), item(31, ArtefactType::ShotWeapon), item(73, ArtefactType::Amulet), item(154, ArtefactType::Helm));
        let c = content(vec![warrior(5, 20, 5), shooter(8, 10), cannon, undead, warrior(1, 30, 5)], vec![shield, bow, holy, crown]);
        let at = Slot::new(Row::Front, 0);
        let (knight, archer, gun, ghoul, hero) =
            (Unit::new(&c, UnitId(5), at), Unit::new(&c, UnitId(8), at), Unit::new(&c, UnitId(6), at), Unit::new(&c, UnitId(7), at), Unit::new(&c, UnitId(1), at));
        assert_eq!(slot_for(&c, &knight, ItemId(30)), Ok(0), "a shield for a warrior");
        assert_eq!(slot_for(&c, &archer, ItemId(30)), Err(EquipError::WrongClass), "not for a shooter");
        assert_eq!(slot_for(&c, &archer, ItemId(31)), Ok(0));
        assert_eq!(slot_for(&c, &gun, ItemId(31)), Err(EquipError::WrongClass), "artillery: ranged 70 > ShotWeaponRange 60");
        assert_eq!(slot_for(&c, &knight, ItemId(73)), Ok(0));
        assert_eq!(slot_for(&c, &ghoul, ItemId(73)), Err(EquipError::Unholy), "«Святое писание» is holy");
        assert_eq!(slot_for(&c, &knight, ItemId(154)), Err(EquipError::NotAllowed), "type 5 may not wear the crown");
        assert_eq!(slot_for(&c, &hero, ItemId(154)), Ok(0), "type 1 may");
        let mut leader = knight.clone();
        leader.wage_kind = crate::rules::content::WageKind::Leader;
        assert_eq!(slot_for(&c, &leader, ItemId(154)), Ok(0), "the hero or a leader may");
    }

    #[test]
    fn full_slots_are_reported() {
        let (c, mut w, _) = setup();
        let c = content(c.units.clone(), c.items.iter().cloned().chain([item(10, ArtefactType::Amulet), item(11, ArtefactType::Helm)]).collect());
        w.items = [Some(ItemId(1)), Some(ItemId(4)), Some(ItemId(5)), Some(ItemId(10))];
        assert_eq!(slot_for(&c, &w, ItemId(11)), Err(EquipError::NoFreeSlot));
    }

    #[test]
    fn potions_heal_now_and_buff_until_the_next_battle_ends() {
        let (c, mut w, _) = setup();
        w.hp = 10;
        assert_eq!(drink(&c, &mut w, ItemId(6)), Ok(30));
        assert!(w.potions.is_empty(), "healing only: nothing lasts");
        assert_eq!(drink(&c, &mut w, ItemId(6)), Ok(10), "capped at max HP");
        assert_eq!(drink(&c, &mut w, ItemId(7)), Ok(0));
        assert_eq!(w.stats(&c)[Stat::AttackBlow], 30);
        assert_eq!(drink(&c, &mut w, ItemId(1)), Err(EquipError::NotAPotion));
    }

    #[test]
    fn sale_price_and_descriptions() {
        let (c, _, _) = setup();
        assert_eq!(sell_price(&c, ItemId(1)), 25, "ItemSaleCost 25%");
        assert_eq!(describe(&c, ItemId(4)), "armour, defence = 26, defence +2");
        assert_eq!(describe(&c, ItemId(6)), "potion, heals 30");
        assert_eq!(describe(&c, ItemId(5)), "ring, attack +50%, defence +10%, Piercing blow");
    }
}
