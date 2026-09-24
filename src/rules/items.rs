//! Items: weapons, armor, artifacts and potions, read from `data/items.txt`.

use std::sync::OnceLock;

const ITEMS: &str = include_str!("../../data/items.txt");

/// Item slots every unit has.
pub const SLOTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemType {
    Weapon,
    Armor,
    Helmet,
    Shield,
    Ring,
    Amulet,
    Boots,
    Cloak,
    Potion,
}

impl ItemType {
    const ALL: [(ItemType, &'static str); 9] = [
        (ItemType::Weapon, "weapon"),
        (ItemType::Armor, "armor"),
        (ItemType::Helmet, "helmet"),
        (ItemType::Shield, "shield"),
        (ItemType::Ring, "ring"),
        (ItemType::Amulet, "amulet"),
        (ItemType::Boots, "boots"),
        (ItemType::Cloak, "cloak"),
        (ItemType::Potion, "potion"),
    ];

    pub fn name(self) -> &'static str {
        ItemType::ALL.iter().find(|(t, _)| *t == self).map_or("?", |(_, n)| n)
    }
}

/// Flat stat changes while the item is worn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bonus {
    pub hp: i32,
    pub dmg: i32,
    pub armor: i32,
    pub init: i32,
    pub actions: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Heals this much at the start of every round.
    Regen(i32),
    /// One more action per turn.
    ExtraAction,
    /// Flank strikes against the wearer are not doubled.
    NoFlank,
    /// The wearer's attacks ignore armor.
    MagicStrike,
    /// Potion: restores HP, capped at max.
    Heal(i32),
    /// Potion: more damage for the rest of the battle.
    Might(i32),
}

impl Effect {
    fn is_potion(self) -> bool {
        matches!(self, Effect::Heal(_) | Effect::Might(_))
    }

    pub fn describe(self) -> String {
        match self {
            Effect::Regen(n) => format!("regenerates {n}/round"),
            Effect::ExtraAction => "+1 action".into(),
            Effect::NoFlank => "no flank damage taken".into(),
            Effect::MagicStrike => "attacks ignore armor".into(),
            Effect::Heal(n) => format!("drink: heal {n}"),
            Effect::Might(n) => format!("drink: +{n} dmg this battle"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Market,
    Loot,
    Tribute,
}

#[derive(Clone, Debug)]
pub struct ItemDef {
    pub id: String,
    pub name: String,
    pub ty: ItemType,
    pub price: i32,
    pub bonus: Bonus,
    pub effect: Option<Effect>,
    pub sources: Vec<Source>,
}

impl ItemDef {
    pub fn sell_price(&self) -> i32 {
        self.price / 2
    }

    /// Short summary, e.g. "weapon, dmg+4 init-1".
    pub fn describe(&self) -> String {
        let b = self.bonus;
        let mut parts = vec![self.ty.name().to_string()];
        for (v, name) in [(b.hp, "hp"), (b.dmg, "dmg"), (b.armor, "armor"), (b.init, "init"), (b.actions, "actions")] {
            if v != 0 {
                parts.push(format!("{name}{v:+}"));
            }
        }
        if let Some(e) = self.effect {
            parts.push(e.describe());
        }
        parts.join(", ")
    }
}

/// Index into the item table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId(pub u16);

impl ItemId {
    pub fn def(self) -> &'static ItemDef {
        &catalog().items[self.0 as usize]
    }

    /// Looks an item up by its `id` column. Panics if there is none (for tests and fixed data).
    pub fn named(id: &str) -> ItemId {
        catalog().find(id).unwrap_or_else(|| panic!("no item '{id}'"))
    }
}

#[derive(Clone, Debug)]
pub struct Catalog {
    pub items: Vec<ItemDef>,
}

/// The shipped item table. Panics with the parse error if `data/items.txt` is malformed.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| Catalog::parse(ITEMS).unwrap_or_else(|e| panic!("data/items.txt: {e}")))
}

fn parse_int(s: &str, what: &str) -> Result<i32, String> {
    s.parse().map_err(|_| format!("bad {what} '{s}'"))
}

fn parse_bonus(s: &str) -> Result<Bonus, String> {
    let mut b = Bonus::default();
    for tok in s.split_whitespace() {
        let at = tok.find(['+', '-']).ok_or_else(|| format!("bonus '{tok}' needs a sign, e.g. dmg+2"))?;
        let v = parse_int(&tok[at..], "bonus value")?;
        match &tok[..at] {
            "hp" => b.hp += v,
            "dmg" => b.dmg += v,
            "armor" => b.armor += v,
            "init" => b.init += v,
            "actions" => b.actions += v,
            other => return Err(format!("unknown bonus '{other}'")),
        }
    }
    Ok(b)
}

fn parse_effect(s: &str) -> Result<Option<Effect>, String> {
    let words: Vec<&str> = s.split_whitespace().collect();
    let n = |w: &[&str]| match w {
        [_, v] => parse_int(v, "effect value"),
        _ => Err(format!("effect '{s}' needs one number")),
    };
    Ok(Some(match words.first().copied() {
        None => return Ok(None),
        Some("regen") => Effect::Regen(n(&words)?),
        Some("heal") => Effect::Heal(n(&words)?),
        Some("might") => Effect::Might(n(&words)?),
        Some("extra_action") if words.len() == 1 => Effect::ExtraAction,
        Some("no_flank") if words.len() == 1 => Effect::NoFlank,
        Some("magic_strike") if words.len() == 1 => Effect::MagicStrike,
        Some(_) => return Err(format!("unknown effect '{s}'")),
    }))
}

fn parse_sources(s: &str) -> Result<Vec<Source>, String> {
    let sources = s
        .split_whitespace()
        .map(|w| match w {
            "market" => Ok(Source::Market),
            "loot" => Ok(Source::Loot),
            "tribute" => Ok(Source::Tribute),
            other => Err(format!("unknown source '{other}'")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if sources.is_empty() {
        return Err("item needs at least one source".into());
    }
    Ok(sources)
}

fn parse_line(line: &str) -> Result<ItemDef, String> {
    let cols: Vec<&str> = line.split('|').map(str::trim).collect();
    let [id, name, ty, price, bonus, effect, sources] = cols[..] else {
        return Err(format!("expected 7 columns, found {}", cols.len()));
    };
    if id.is_empty() || name.is_empty() {
        return Err("id and name must not be empty".into());
    }
    let ty = ItemType::ALL.iter().find(|(_, n)| *n == ty).map(|(t, _)| *t).ok_or(format!("unknown type '{ty}'"))?;
    let price = parse_int(price, "price")?;
    if price < 0 {
        return Err("price must not be negative".into());
    }
    let bonus = parse_bonus(bonus)?;
    let effect = parse_effect(effect)?;
    let potion = ty == ItemType::Potion;
    if potion && (effect.is_none_or(|e| !e.is_potion()) || bonus != Bonus::default()) {
        return Err("a potion needs a heal or might effect and no bonuses".into());
    }
    if !potion && effect.is_some_and(Effect::is_potion) {
        return Err("heal and might are potion effects".into());
    }
    Ok(ItemDef { id: id.into(), name: name.into(), ty, price, bonus, effect, sources: parse_sources(sources)? })
}

impl Catalog {
    /// Parses the item table; errors carry the 1-based line number.
    pub fn parse(text: &str) -> Result<Catalog, String> {
        let mut items: Vec<ItemDef> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let item = parse_line(line).map_err(|e| format!("line {}: {e}", n + 1))?;
            if items.iter().any(|i| i.id == item.id) {
                return Err(format!("line {}: duplicate id '{}'", n + 1, item.id));
            }
            items.push(item);
        }
        if items.is_empty() {
            return Err("no items".into());
        }
        Ok(Catalog { items })
    }

    pub fn find(&self, id: &str) -> Option<ItemId> {
        self.items.iter().position(|i| i.id == id).map(|i| ItemId(i as u16))
    }

    pub fn ids(&self) -> impl Iterator<Item = ItemId> {
        (0..self.items.len() as u16).map(ItemId)
    }

    pub fn from_source(&self, source: Source) -> Vec<ItemId> {
        self.ids().filter(|i| self.items[i.0 as usize].sources.contains(&source)).collect()
    }
}

/// Passive effects of worn gear that the battle checks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Passives {
    pub regen: i32,
    pub no_flank: bool,
    pub magic_strike: bool,
}

/// Worn (non-potion) items.
pub fn gear(items: &[Option<ItemId>; SLOTS]) -> impl Iterator<Item = &'static ItemDef> + '_ {
    items.iter().flatten().map(|i| i.def()).filter(|d| d.ty != ItemType::Potion)
}

pub fn passives(items: &[Option<ItemId>; SLOTS]) -> Passives {
    let mut p = Passives::default();
    for d in gear(items) {
        match d.effect {
            Some(Effect::Regen(n)) => p.regen += n,
            Some(Effect::NoFlank) => p.no_flank = true,
            Some(Effect::MagicStrike) => p.magic_strike = true,
            _ => {}
        }
    }
    p
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquipError {
    NoFreeSlot,
    /// Already wears an item of this type (only potions may be doubled up).
    SameType,
    PackFull,
    NoSuchItem,
}

/// Slot `item` would go into, or why it can't be worn.
pub fn free_slot_for(items: &[Option<ItemId>; SLOTS], item: ItemId) -> Result<usize, EquipError> {
    let ty = item.def().ty;
    if ty != ItemType::Potion && items.iter().flatten().any(|i| i.def().ty == ty) {
        return Err(EquipError::SameType);
    }
    items.iter().position(Option::is_none).ok_or(EquipError::NoFreeSlot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_table_parses() {
        let c = catalog();
        assert_eq!(c.items.len(), 12);
        let axe = ItemId::named("war_axe").def();
        assert_eq!((axe.ty, axe.price, axe.bonus.dmg, axe.bonus.init), (ItemType::Weapon, 120, 4, -1));
        assert_eq!(ItemId::named("amulet_life").def().effect, Some(Effect::Regen(3)));
        assert_eq!(ItemId::named("heal_potion").def().sources, vec![Source::Market, Source::Loot, Source::Tribute]);
        for s in [Source::Market, Source::Loot, Source::Tribute] {
            assert!(!c.from_source(s).is_empty(), "{s:?}");
        }
        assert_eq!(axe.describe(), "weapon, dmg+4, init-1");
    }

    #[test]
    fn parse_errors_name_the_line() {
        let bad = [
            ("x | X | weapon | 5 | dmg+1 | market", "7 columns"),
            ("x | X | sword | 5 | | | market", "unknown type"),
            ("x | X | weapon | cheap | | | market", "bad price"),
            ("x | X | weapon | 5 | dmg2 | | market", "needs a sign"),
            ("x | X | weapon | 5 | luck+1 | | market", "unknown bonus"),
            ("x | X | weapon | 5 | | fly | market", "unknown effect"),
            ("x | X | weapon | 5 | | regen | market", "one number"),
            ("x | X | weapon | 5 | | heal 5 | market", "potion effects"),
            ("x | X | potion | 5 | hp+1 | heal 5 | market", "a potion needs"),
            ("x | X | potion | 5 | | | market", "a potion needs"),
            ("x | X | weapon | 5 | | | shop", "unknown source"),
            ("x | X | weapon | 5 | | |", "at least one source"),
        ];
        for (line, want) in bad {
            let err = Catalog::parse(&format!("# header\n{line}")).unwrap_err();
            assert!(err.starts_with("line 2: ") && err.contains(want), "{line}: {err}");
        }
        let dup = "a | A | ring | 1 | | | loot\na | B | ring | 1 | | | loot";
        assert!(Catalog::parse(dup).unwrap_err().contains("duplicate"));
    }

    #[test]
    fn one_item_per_type_but_potions_stack() {
        let (sword, axe, potion) = (ItemId::named("short_sword"), ItemId::named("war_axe"), ItemId::named("heal_potion"));
        let mut items = [Some(sword), None, None, None];
        assert_eq!(free_slot_for(&items, axe), Err(EquipError::SameType));
        items[1] = Some(potion);
        assert_eq!(free_slot_for(&items, potion), Ok(2));
        items[2] = Some(potion);
        items[3] = Some(potion);
        assert_eq!(free_slot_for(&items, potion), Err(EquipError::NoFreeSlot));
    }

    #[test]
    fn passives_come_from_worn_gear() {
        let items = [Some(ItemId::named("amulet_life")), Some(ItemId::named("cloak_guard")), None, None];
        assert_eq!(passives(&items), Passives { regen: 3, no_flank: true, magic_strike: false });
    }
}
