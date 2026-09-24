use super::formation::{Row, Slot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackKind {
    /// Warrior: reaches only the enemy front row while it stands.
    Melee,
    /// Shooter: any enemy.
    Ranged,
    /// Mage: any enemy, ignores armor.
    Magic,
    /// Mage: restores HP to any wounded ally.
    Heal { amount: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnitKind {
    Knight,
    Archmage,
    Ranger,
    Spearman,
    Archer,
    Swordsman,
    Healer,
    Bandit,
    BanditArcher,
    BanditChief,
}

#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub max_hp: i32,
    pub dmg_min: i32,
    pub dmg_max: i32,
    pub armor: i32,
    pub initiative: i32,
    /// Action points per turn: each attack, heal or move costs one.
    pub actions: i32,
    pub attack: AttackKind,
}

impl AttackKind {
    pub fn role(self) -> &'static str {
        match self {
            AttackKind::Melee => "warrior",
            AttackKind::Ranged => "shooter",
            AttackKind::Magic | AttackKind::Heal { .. } => "mage",
        }
    }

    /// Row a newly hired unit of this kind is placed in.
    pub fn preferred_row(self) -> Row {
        match self {
            AttackKind::Melee => Row::Front,
            _ => Row::Back,
        }
    }
}

impl UnitKind {
    pub const HEROES: [UnitKind; 3] = [UnitKind::Knight, UnitKind::Archmage, UnitKind::Ranger];
    pub const ALL: [UnitKind; 10] = [
        UnitKind::Knight,
        UnitKind::Archmage,
        UnitKind::Ranger,
        UnitKind::Spearman,
        UnitKind::Archer,
        UnitKind::Swordsman,
        UnitKind::Healer,
        UnitKind::Bandit,
        UnitKind::BanditArcher,
        UnitKind::BanditChief,
    ];

    pub fn name(self) -> &'static str {
        match self {
            UnitKind::Knight => "Knight",
            UnitKind::Archmage => "Archmage",
            UnitKind::Ranger => "Ranger",
            UnitKind::Spearman => "Spearman",
            UnitKind::Archer => "Archer",
            UnitKind::Swordsman => "Swordsman",
            UnitKind::Healer => "Healer",
            UnitKind::Bandit => "Bandit",
            UnitKind::BanditArcher => "Bandit archer",
            UnitKind::BanditChief => "Bandit chief",
        }
    }

    /// File stem used by the optional sprite override directory.
    pub fn asset_key(self) -> &'static str {
        match self {
            UnitKind::Knight => "knight",
            UnitKind::Archmage => "archmage",
            UnitKind::Ranger => "ranger",
            UnitKind::Spearman => "spearman",
            UnitKind::Archer => "archer",
            UnitKind::Swordsman => "swordsman",
            UnitKind::Healer => "healer",
            UnitKind::Bandit => "bandit",
            UnitKind::BanditArcher => "bandit_archer",
            UnitKind::BanditChief => "bandit_chief",
        }
    }

    pub fn stats(self) -> Stats {
        use AttackKind::*;
        let s = |max_hp, dmg_min, dmg_max, armor, initiative, actions, attack| Stats {
            max_hp,
            dmg_min,
            dmg_max,
            armor,
            initiative,
            actions,
            attack,
        };
        match self {
            UnitKind::Knight => s(60, 10, 14, 5, 5, 1, Melee),
            UnitKind::Archmage => s(32, 9, 13, 0, 6, 1, Magic),
            UnitKind::Ranger => s(40, 5, 7, 1, 7, 2, Ranged),
            UnitKind::Spearman => s(30, 5, 8, 2, 4, 1, Melee),
            UnitKind::Archer => s(22, 5, 8, 0, 5, 1, Ranged),
            UnitKind::Swordsman => s(38, 7, 10, 3, 5, 1, Melee),
            UnitKind::Healer => s(20, 0, 0, 0, 3, 1, Heal { amount: 10 }),
            UnitKind::Bandit => s(28, 6, 9, 1, 4, 1, Melee),
            UnitKind::BanditArcher => s(20, 5, 8, 0, 5, 1, Ranged),
            UnitKind::BanditChief => s(65, 11, 15, 3, 6, 1, Melee),
        }
    }

    /// One-line summary of how the unit fights, for cards and tooltips.
    pub fn describe_attack(self) -> String {
        let s = self.stats();
        let actions = if s.actions > 1 { format!(", {} actions", s.actions) } else { String::new() };
        match s.attack {
            AttackKind::Heal { amount } => format!("mage, heals {amount}{actions}"),
            a => format!("{}, dmg {}-{}{actions}", a.role(), s.dmg_min, s.dmg_max),
        }
    }

    pub fn cost(self) -> i32 {
        match self {
            UnitKind::Spearman => 30,
            UnitKind::Archer => 40,
            UnitKind::Swordsman => 50,
            UnitKind::Healer => 45,
            _ => 0,
        }
    }

    /// Daily pay; heroes are free.
    pub fn wage(self) -> i32 {
        match self {
            UnitKind::Spearman => 3,
            UnitKind::Archer => 4,
            UnitKind::Swordsman => 5,
            UnitKind::Healer => 4,
            _ => 0,
        }
    }

    pub fn starting_gold(self) -> i32 {
        match self {
            UnitKind::Knight => 100,
            UnitKind::Archmage => 120,
            UnitKind::Ranger => 110,
            _ => 0,
        }
    }
}

/// A persistent squad member on the world map.
#[derive(Clone, Debug)]
pub struct Unit {
    pub kind: UnitKind,
    pub hp: i32,
    /// Cell in the squad's battle formation.
    pub slot: Slot,
    /// Missed the last payday: refuses to fight until paid.
    pub unpaid: bool,
}

impl Unit {
    pub fn new(kind: UnitKind, slot: Slot) -> Self {
        Unit { kind, hp: kind.stats().max_hp, slot, unpaid: false }
    }

    pub fn heal_full(&mut self) {
        self.hp = self.kind.stats().max_hp;
    }
}
