#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackKind {
    Melee,
    Ranged { range: i32, magic: bool },
    Heal { amount: i32, range: i32 },
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
    pub moves: i32,
    pub initiative: i32,
    pub attack: AttackKind,
}

impl UnitKind {
    pub const HEROES: [UnitKind; 3] = [UnitKind::Knight, UnitKind::Archmage, UnitKind::Ranger];

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
        let s = |max_hp, dmg_min, dmg_max, armor, moves, initiative, attack| Stats {
            max_hp,
            dmg_min,
            dmg_max,
            armor,
            moves,
            initiative,
            attack,
        };
        match self {
            UnitKind::Knight => s(60, 10, 14, 4, 3, 5, Melee),
            UnitKind::Archmage => s(32, 9, 13, 0, 3, 6, Ranged { range: 6, magic: true }),
            UnitKind::Ranger => s(40, 8, 11, 1, 4, 7, Ranged { range: 7, magic: false }),
            UnitKind::Spearman => s(30, 5, 8, 2, 3, 4, Melee),
            UnitKind::Archer => s(22, 5, 8, 0, 3, 5, Ranged { range: 6, magic: false }),
            UnitKind::Swordsman => s(38, 7, 10, 3, 3, 5, Melee),
            UnitKind::Healer => s(20, 0, 0, 0, 3, 3, Heal { amount: 10, range: 4 }),
            UnitKind::Bandit => s(26, 5, 8, 1, 3, 4, Melee),
            UnitKind::BanditArcher => s(18, 4, 7, 0, 3, 5, Ranged { range: 6, magic: false }),
            UnitKind::BanditChief => s(55, 9, 13, 3, 3, 6, Melee),
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
}

impl Unit {
    pub fn new(kind: UnitKind) -> Self {
        Unit { kind, hp: kind.stats().max_hp }
    }

    pub fn heal_full(&mut self) {
        self.hp = self.kind.stats().max_hp;
    }
}
