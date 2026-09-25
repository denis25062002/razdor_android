//! Game content: unit, item and spell definitions plus the battle and economy options.
//!
//! The rules only ever see a [`Content`]. It is built either from the player's Discord Times
//! install ([`Content::from_dt`]) or from the built-in demo data in `data/` ([`Content::builtin`]),
//! which is our own content written in the same ini schema. Definitions reuse the reader types
//! of [`crate::dt::data`]; field meanings are in `docs/reference/mechanics.md`.

use std::collections::HashMap;

pub use crate::dt::data::{
    ArtefactDef, ArtefactType, Bonus, GlobalOptions, MagicDirection, MagicSchool, Nature, SpellDef, Stat, StatMods,
    UnitDef, Upgrade,
};
use crate::dt::data::{parse_artefacts, parse_spells, parse_units};
use crate::dt::ini::Ini;
use crate::dt::install::DtInstall;

use super::formation::Formation;

const BUILTIN_UNITS: &str = include_str!("../../data/units.ini");
const BUILTIN_ITEMS: &str = include_str!("../../data/items.ini");
const BUILTIN_SPELLS: &str = include_str!("../../data/spells.ini");

/// Community cap on XP gained at once (mechanics.md 1.4).
pub const MAX_XP_GAIN: i32 = 5256;

/// A unit type: its `GlobalIndex`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct UnitId(pub u32);

/// An item type: its `GlobalIndex`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct ItemId(pub u32);

/// The three hero classes. Their unit ids are 1–3 in the original and in the demo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum HeroClass {
    /// Army takes 10% less physical damage.
    Knight,
    /// Casts world spells faster and cheaper.
    Archmage,
    /// Faster on the map, heals the army daily.
    Ranger,
}

impl HeroClass {
    pub const ALL: [HeroClass; 3] = [HeroClass::Knight, HeroClass::Archmage, HeroClass::Ranger];

    pub fn unit(self) -> UnitId {
        UnitId(match self {
            HeroClass::Knight => 1,
            HeroClass::Archmage => 2,
            HeroClass::Ranger => 3,
        })
    }

    pub fn of_unit(id: UnitId) -> Option<HeroClass> {
        HeroClass::ALL.into_iter().find(|h| h.unit() == id)
    }
}

/// The two hiring kinds of the wage code (mechanics.md 1.5). Which units the original puts
/// in which kind is unknown; Razdor's guess: `Nature=Rogue` units are mercenaries, everyone
/// else a recruit ([`WageKind::of`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WageKind {
    /// Kind 1: `round(Cost / CostRecrutDiv × f)` with the cost brackets.
    Recruit,
    /// Kind 2: `Cost / CostMercenaryDiv`.
    Mercenary,
}

impl WageKind {
    /// Razdor's guess for a unit type's hiring kind: rogues are mercenaries.
    pub fn of(def: &UnitDef) -> WageKind {
        if def.nature == Nature::Rogue {
            WageKind::Mercenary
        } else {
            WageKind::Recruit
        }
    }
}

/// Where an item can turn up in the demo world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Market,
    Loot,
    Tribute,
}

impl Source {
    fn parse(s: &str) -> Option<Source> {
        match s {
            "market" => Some(Source::Market),
            "loot" => Some(Source::Loot),
            "tribute" => Some(Source::Tribute),
            _ => None,
        }
    }
}

/// Everything the rules need to know about units, items, spells and options.
#[derive(Clone, Debug)]
pub struct Content {
    pub units: Vec<UnitDef>,
    pub items: Vec<ArtefactDef>,
    pub spells: Vec<SpellDef>,
    /// `_Global.ini` values: magic divisors, drain and floors, `Row2Def`, `BattleEndTurn`,
    /// XP constants, wage divisors, loot, sale price …
    pub options: GlobalOptions,
    pub formation: Formation,
    unit_index: HashMap<u32, usize>,
    item_index: HashMap<u32, usize>,
}

/// No units, items or spells: the placeholder a loaded save holds until its content is set.
impl Default for Content {
    fn default() -> Content {
        Content::new(Vec::new(), Vec::new(), Vec::new(), GlobalOptions::default(), Formation::WIDE)
    }
}

impl Content {
    pub fn new(
        units: Vec<UnitDef>,
        items: Vec<ArtefactDef>,
        spells: Vec<SpellDef>,
        options: GlobalOptions,
        formation: Formation,
    ) -> Content {
        let unit_index = units.iter().enumerate().map(|(i, u)| (u.id, i)).collect();
        let item_index = items.iter().enumerate().map(|(i, a)| (a.id, i)).collect();
        Content { units, items, spells, options, formation, unit_index, item_index }
    }

    /// Content read from a Discord Times install. The formation is the Community wide row
    /// (2 × 6), as in the player's install.
    pub fn from_dt(dt: &DtInstall) -> Content {
        Content::new(dt.units.clone(), dt.artefacts.clone(), dt.spells.clone(), dt.options.clone(), Formation::WIDE)
    }

    /// The built-in demo: `data/units.ini`, `data/items.ini` and `data/spells.ini` (our own
    /// content). The
    /// options are the vanilla defaults except for faster XP, so the short demo shows levels.
    pub fn builtin() -> Content {
        let units = parse_units(&Ini::parse(BUILTIN_UNITS)).unwrap_or_else(|e| panic!("data/units.ini: {e}"));
        let items = parse_artefacts(&Ini::parse(BUILTIN_ITEMS)).unwrap_or_else(|e| panic!("data/items.ini: {e}"));
        let spells = parse_spells(&Ini::parse(BUILTIN_SPELLS)).unwrap_or_else(|e| panic!("data/spells.ini: {e}"));
        let options = GlobalOptions { hero_experience_modificator: 100, main_exp_correction: 60, ..GlobalOptions::default() };
        Content::new(units, items, spells, options, Formation::WIDE)
    }

    pub fn try_unit(&self, id: UnitId) -> Option<&UnitDef> {
        self.unit_index.get(&id.0).map(|&i| &self.units[i])
    }

    /// Unit definition. Panics on an unknown id (ids come from this content).
    pub fn unit(&self, id: UnitId) -> &UnitDef {
        self.try_unit(id).unwrap_or_else(|| panic!("no unit {}", id.0))
    }

    pub fn try_item(&self, id: ItemId) -> Option<&ArtefactDef> {
        self.item_index.get(&id.0).map(|&i| &self.items[i])
    }

    /// Item definition. Panics on an unknown id.
    pub fn item(&self, id: ItemId) -> &ArtefactDef {
        self.try_item(id).unwrap_or_else(|| panic!("no item {}", id.0))
    }

    pub fn unit_ids(&self) -> impl Iterator<Item = UnitId> + '_ {
        self.units.iter().map(|u| UnitId(u.id))
    }

    pub fn item_ids(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.items.iter().map(|a| ItemId(a.id))
    }

    /// Stable short name: the demo's `Key=`, else `unit<id>`.
    pub fn unit_key(&self, id: UnitId) -> String {
        self.unit(id).extra.get("Key").cloned().unwrap_or_else(|| format!("unit{}", id.0))
    }

    /// Stable short name: the demo's `Key=`, else `item<id>`.
    pub fn item_key(&self, id: ItemId) -> String {
        self.item(id).extra.get("Key").cloned().unwrap_or_else(|| format!("item{}", id.0))
    }

    pub fn unit_by_key(&self, key: &str) -> Option<UnitId> {
        self.unit_ids().find(|&id| self.unit_key(id) == key)
    }

    pub fn item_by_key(&self, key: &str) -> Option<ItemId> {
        self.item_ids().find(|&id| self.item_key(id) == key)
    }

    /// Where an item turns up. The demo lists it (`Sources=`); for an install: potions
    /// everywhere, trade goods as loot, other priced items in markets and as loot, personal
    /// (negative price) items nowhere.
    pub fn sources(&self, id: ItemId) -> Vec<Source> {
        let def = self.item(id);
        if let Some(list) = def.extra.get("Sources") {
            return list.split_whitespace().filter_map(Source::parse).collect();
        }
        match def.kind {
            _ if def.cost <= 0 => Vec::new(),
            ArtefactType::Potion => vec![Source::Market, Source::Loot, Source::Tribute],
            ArtefactType::Item => vec![Source::Loot],
            _ => vec![Source::Market, Source::Loot],
        }
    }

    pub fn items_from(&self, source: Source) -> Vec<ItemId> {
        self.item_ids().filter(|&i| self.sources(i).contains(&source)).collect()
    }

    /// Gold a hero class starts with (demo `StartGold=`, else 100).
    pub fn start_gold(&self, hero: HeroClass) -> i32 {
        self.try_unit(hero.unit()).and_then(|u| u.extra.get("StartGold")).and_then(|g| g.parse().ok()).unwrap_or(100)
    }

    /// Spells a demo hero class has in its book at the start (demo `StartSpells=`).
    pub fn start_spells(&self, hero: HeroClass) -> Vec<u8> {
        let list = self.try_unit(hero.unit()).and_then(|u| u.extra.get("StartSpells"));
        list.map_or_else(Vec::new, |l| l.split_whitespace().filter_map(|x| x.parse().ok()).collect())
    }

    /// XP needed to go from `level` (1 = as hired) to the next:
    /// `StartExpirience × (LevelMultipler/100)^(level−1)`, rounded (mechanics.md 1.4).
    pub fn xp_to_next(&self, id: UnitId, level: i32) -> i32 {
        let u = self.unit(id);
        let k = u.level_multiplier.max(100) as f64 / 100.0;
        (u.start_experience.max(1) as f64 * k.powi((level - 1).max(0))).round() as i32
    }

    /// Strength estimate used for XP: `Cost × CostMultipler/100`, +10% per level above the
    /// first. The level term is our guess (the exe's exact formula is not decoded).
    pub fn tactical_cost(&self, id: UnitId, level: i32) -> i32 {
        let u = self.unit(id);
        let mult = if u.cost_multiplier > 0 { u.cost_multiplier } else { 100 };
        let base = u.cost.max(1) * mult / 100;
        base * (9 + level.max(1)) / 10
    }

    /// Daily wage of a unit type hired as `kind` (mechanics.md 1.5). The hero is free; the
    /// caller handles that and `AddPayment`.
    pub fn wage_for(&self, id: UnitId, kind: WageKind) -> i32 {
        match kind {
            WageKind::Recruit => self.wage(id),
            WageKind::Mercenary => self.unit(id).cost.max(0) / self.options.cost_mercenary_div.max(1),
        }
    }

    /// Community Update: `Nature=Elemental` units are hired, healed, resurrected and paid in
    /// mana instead of gold.
    pub fn paid_in_mana(&self, id: UnitId) -> bool {
        self.unit(id).nature == Nature::Elemental
    }

    /// Daily wage of a unit type (mechanics.md 1.5, hiring kind 1):
    /// `round(Cost / CostRecrutDiv × f)`, f = 0.25 / 0.5 / 0.75 / 1 for cost ≤50 / ≤100 / ≤150 / more.
    /// The hero is free; the caller handles that and `AddPayment`.
    pub fn wage(&self, id: UnitId) -> i32 {
        let c = self.unit(id).cost.max(0);
        let quarters = match c {
            0..=50 => 1,
            51..=100 => 2,
            101..=150 => 3,
            _ => 4,
        };
        let div = self.options.cost_recrut_div.max(1);
        // round(c / div × quarters / 4) in integers.
        (2 * c * quarters + 4 * div) / (8 * div)
    }
}

/// Helpers for tests: blank definitions and a synthetic content.
#[cfg(test)]
pub(crate) mod testkit {
    use super::*;
    use std::collections::BTreeMap;

    pub fn unit(id: u32, name: &str) -> UnitDef {
        UnitDef {
            id,
            name: name.to_string(),
            description: String::new(),
            icon_index: 0,
            cost: 50,
            cost_multiplier: 100,
            cost_gold_div: 1,
            start_experience: 60,
            level_multiplier: 140,
            hits: 50,
            attack_blow: 0,
            attack_shot: 0,
            magic_power: 0,
            defence_blow: 0,
            defence_shot: 0,
            protect_life: 0,
            protect_death: 0,
            protect_elemental: 0,
            initiative: 10,
            manevres: 1,
            regen: 0,
            vampirism: 0,
            magic: None,
            magic_direction: None,
            nature: Nature::Normal,
            bonus: None,
            surrender: 0,
            upgrades: Vec::new(),
            level_up: StatMods::new(),
            evasion: None,
            min_magic_power: None,
            mana_drain: None,
            extra: BTreeMap::new(),
        }
    }

    pub fn warrior(id: u32, atk: i32, def: i32) -> UnitDef {
        UnitDef { attack_blow: atk, defence_blow: def, defence_shot: def, ..unit(id, &format!("warrior{id}")) }
    }

    pub fn shooter(id: u32, atk: i32) -> UnitDef {
        UnitDef { attack_shot: atk, ..unit(id, &format!("shooter{id}")) }
    }

    pub fn mage(id: u32, power: i32, school: MagicSchool, dir: MagicDirection) -> UnitDef {
        UnitDef { magic_power: power, magic: Some(school), magic_direction: Some(dir), ..unit(id, &format!("mage{id}")) }
    }

    pub fn item(id: u32, kind: ArtefactType) -> ArtefactDef {
        ArtefactDef {
            id,
            name: format!("item{id}"),
            description: String::new(),
            icon: String::new(),
            cost: 100,
            kind,
            bonus: None,
            magic: None,
            add: StatMods::new(),
            percent: StatMods::new(),
            fixed: StatMods::new(),
            extra: BTreeMap::new(),
        }
    }

    /// A spell with id `id` that costs `gold` to learn.
    pub fn spell(id: u32, gold: i32) -> SpellDef {
        SpellDef {
            id,
            name: format!("spell{id}"),
            cost_gold: gold,
            cost_mana: 50,
            school: Some(MagicSchool::Life),
            time_work: None,
            time_cast: Some(1),
            target: None,
            icons: Default::default(),
            effects: Default::default(),
            delta_fixed_hits: Some(10),
            delta_percent_hits: None,
            add: StatMods::new(),
            percent: StatMods::new(),
            life_lose_percent: None,
            extra: BTreeMap::new(),
        }
    }

    pub fn content(units: Vec<UnitDef>, items: Vec<ArtefactDef>) -> Content {
        Content::new(units, items, Vec::new(), GlobalOptions::default(), Formation::WIDE)
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    #[test]
    fn builtin_parses_our_demo_data() {
        let c = Content::builtin();
        assert_eq!(c.units.len(), 10);
        assert_eq!(c.items.len(), 15);
        assert_eq!(c.spells.len(), 5);
        assert!(c.spells.iter().all(|s| s.extra.is_empty()), "unknown spell keys");
        for h in HeroClass::ALL {
            assert!(c.try_unit(h.unit()).is_some(), "{h:?}");
        }
        assert!(c.units.iter().all(|u| u.extra.keys().all(|k| matches!(k.as_str(), "Key" | "StartGold" | "StartSpells"))), "unknown unit keys");
        assert!(c.items.iter().all(|i| i.extra.keys().all(|k| k == "Key" || k == "Sources")), "unknown item keys");
        let spear = c.unit_by_key("spearman").unwrap();
        assert_eq!(c.unit(spear).bonus, Some(Bonus::SpearDefense));
        assert_eq!(c.unit(spear).upgrades[0].target, c.unit_by_key("swordsman").map(|u| u.0));
        assert_eq!(c.start_gold(HeroClass::Archmage), 120);
        assert_eq!(c.start_spells(HeroClass::Archmage), vec![1, 4]);
        assert!(c.start_spells(HeroClass::Knight).is_empty());
        for s in [Source::Market, Source::Loot, Source::Tribute] {
            assert!(!c.items_from(s).is_empty(), "{s:?}");
        }
        let candle = c.item_by_key("candlestick").unwrap();
        assert_eq!(c.item(candle).kind, ArtefactType::Item);
    }

    #[test]
    fn xp_to_next_level_is_geometric() {
        // 60, then ×1.4 each level: 60, 84, 118 (mechanics.md 1.4).
        let c = content(vec![unit(1, "militia")], vec![]);
        let need: Vec<i32> = (1..=4).map(|l| c.xp_to_next(UnitId(1), l)).collect();
        assert_eq!(need, vec![60, 84, 118, 165]);
    }

    #[test]
    fn wages_follow_the_cost_brackets() {
        let mut units = vec![];
        for (id, cost) in [(1, 50), (2, 100), (3, 150), (4, 280), (5, 0)] {
            units.push(UnitDef { cost, ..unit(id, "u") });
        }
        let c = content(units, vec![]);
        // 50/2×0.25 = 6.25, 100/2×0.5 = 25, 150/2×0.75 = 56.25, 280/2 = 140.
        let wages: Vec<i32> = (1..=5).map(|i| c.wage(UnitId(i))).collect();
        assert_eq!(wages, vec![6, 25, 56, 140, 0]);
        // Kind 2 (mercenary): Cost / CostMercenaryDiv, no brackets.
        let merc: Vec<i32> = (1..=5).map(|i| c.wage_for(UnitId(i), WageKind::Mercenary)).collect();
        assert_eq!(merc, vec![25, 50, 75, 140, 0]);
        assert_eq!(c.wage_for(UnitId(1), WageKind::Recruit), 6);
    }

    #[test]
    fn rogues_are_mercenaries_and_elementals_are_paid_in_mana() {
        let rogue = UnitDef { nature: Nature::Rogue, ..unit(1, "rogue") };
        let elemental = UnitDef { nature: Nature::Elemental, ..unit(2, "golem") };
        let c = content(vec![rogue, elemental, unit(3, "militia")], vec![]);
        assert_eq!(WageKind::of(c.unit(UnitId(1))), WageKind::Mercenary);
        assert_eq!(WageKind::of(c.unit(UnitId(3))), WageKind::Recruit);
        assert!(c.paid_in_mana(UnitId(2)) && !c.paid_in_mana(UnitId(1)));
    }

    #[test]
    fn default_sources_for_install_items() {
        let mut personal = item(3, ArtefactType::Ring);
        personal.cost = -500;
        let c = content(vec![], vec![item(1, ArtefactType::Potion), item(2, ArtefactType::Armor), personal, item(4, ArtefactType::Item)]);
        assert_eq!(c.sources(ItemId(1)), vec![Source::Market, Source::Loot, Source::Tribute]);
        assert_eq!(c.sources(ItemId(2)), vec![Source::Market, Source::Loot]);
        assert!(c.sources(ItemId(3)).is_empty());
        assert_eq!(c.sources(ItemId(4)), vec![Source::Loot]);
    }

    #[test]
    fn real_install_content_loads() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Content::from_dt(&dt);
        assert_eq!((c.units.len(), c.items.len(), c.spells.len()), (dt.units.len(), dt.artefacts.len(), dt.spells.len()));
        assert!(c.units.len() >= 100 && c.items.len() >= 150 && c.spells.len() >= 30);
        assert!(c.unit_ids().all(|id| c.try_unit(id).is_some()));
        assert!(c.item_ids().all(|id| c.try_item(id).is_some()));
        for h in HeroClass::ALL {
            assert!(c.try_unit(h.unit()).is_some());
        }
        assert_eq!(c.formation, Formation::WIDE);
        assert_eq!(c.options.battle_end_turn, 25);
        // Every unit's XP table and wage are computable.
        assert!(c.unit_ids().all(|id| c.xp_to_next(id, 1) > 0 && c.wage(id) >= 0));
        assert!(!c.items_from(Source::Market).is_empty());
    }
}
