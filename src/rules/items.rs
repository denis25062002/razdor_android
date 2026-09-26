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
    /// A melee weapon on a non-warrior, a bow on a non-shooter or a staff on a non-mage.
    WrongClass,
    /// Potions and trade goods cannot be worn.
    NotWearable,
    /// The dead cannot hold items.
    Dead,
    NotAPotion,
    PackFull,
    NoSuchItem,
}

/// Applies worn items and active potions to `stats` in the original's order
/// (original-mechanics/economy.md §5): each worn item's `f-` in slot order replaces its stat
/// when above 0 (a later slot wins); the potions' `d-`, then the items' `d-`; the potions'
/// `p-`, then each item's `p-` in turn, compounding (`x += x·p/100`, truncated each time).
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
            stats[st] += stats[st] * v / 100;
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

/// Slot `item` would go into on `unit`, or why it can't be worn.
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
        ArtefactType::BlowWeapon => base.is_warrior(),
        ArtefactType::ShotWeapon => base.is_shooter(),
        ArtefactType::Staff => base.is_mage(),
        _ => true,
    };
    if !class_ok {
        return Err(EquipError::WrongClass);
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
    use crate::rules::content::{Bonus, MagicDirection, MagicSchool, StatMods, UnitId};
    use crate::rules::formation::{Row, Slot};

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
