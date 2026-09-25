//! What the editor offers to place, and the names it shows.
//!
//! The object and building palettes come from the player's `Objects.ugs` at runtime (its
//! record keys are the `.DTm` object `(class, sprite)` and building `(picture type,
//! variant)` keys, and a building sprite carries its footprint). Without an install a small
//! fallback palette of our own is offered. Unit, artefact and spell names come from the
//! [`Content`] the editor runs with. The labels in this file are Razdor's own English.

use crate::dt::dtm::BuildingType;
use crate::dt::gfx::{ObjectSprite, ObjectSprites};
use crate::rules::content::Content;

use super::geometry::is_massif;

/// A map object the palette offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectKey {
    pub class: u8,
    pub sprite: u8,
}

/// A building picture the palette offers, with its footprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingPicture {
    pub picture_type: u8,
    pub variant: u8,
    pub size: (u8, u8),
}

/// The objects and building pictures that can be placed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Palette {
    /// Sorted by class, then sprite.
    pub objects: Vec<ObjectKey>,
    /// Sorted by picture type, then variant.
    pub buildings: Vec<BuildingPicture>,
    /// Built from the install's art (keys are checked on save); otherwise the fallback.
    pub from_install: bool,
}

/// Object classes, with Razdor's labels. 1–8 are massifs, 9–12 plants.
pub const OBJECT_CLASSES: [(u8, &str); 11] = [
    (1, "Grass hills"),
    (2, "Green hill"),
    (3, "Rocky hills"),
    (4, "Yellow hill"),
    (5, "Mountains"),
    (6, "Dark mountains"),
    (8, "Stones"),
    (9, "Trees and bushes"),
    (10, "Dead trees"),
    (11, "Dense thicket"),
    (12, "Plants (12)"),
];

pub fn object_class_label(class: u8) -> String {
    OBJECT_CLASSES.iter().find(|c| c.0 == class).map_or(format!("Class {class}"), |c| c.1.to_string())
}

/// Building type labels (byte 6).
pub fn building_type_label(kind: u8) -> &'static str {
    match BuildingType::from_code(kind) {
        Some(BuildingType::Palace) => "Palace",
        Some(BuildingType::Town) => "Town",
        Some(BuildingType::Village) => "Village",
        Some(BuildingType::Castle) => "Castle",
        Some(BuildingType::Fort) => "Fort",
        Some(BuildingType::Tavern) => "Tavern",
        Some(BuildingType::Market) => "Market",
        Some(BuildingType::Church) => "Church",
        Some(BuildingType::Smithy) => "Smithy / house",
        Some(BuildingType::Shipyard) => "Shipyard",
        Some(BuildingType::Altar) => "Altar",
        Some(BuildingType::DungeonEntrance) => "Dungeon entrance",
        Some(BuildingType::Ruins) => "Ruins",
        Some(BuildingType::StoneBridge) => "Stone bridge",
        Some(BuildingType::WoodenBridge) => "Wooden bridge",
        Some(BuildingType::Obelisk) => "Obelisk",
        None => "Unknown building",
    }
}

/// Terrain labels (codes 0–15, the editor's surface palette order).
pub const SURFACE_LABELS: [&str; 16] = [
    "Shallows, fords",
    "Coastal water",
    "Deep sea",
    "Lava fields",
    "Road",
    "Grass lowland",
    "Grass plain",
    "Dry plain",
    "Marsh",
    "Impassable swamp",
    "Sand and dunes",
    "Clay soil",
    "Stony soil",
    "Scorched land",
    "Snowy ground",
    "Impassable snow",
];

/// Army map models (byte 5), 1-based.
pub const ARMY_MODELS: [(u8, &str); 12] = [
    (1, "Hero: knight"),
    (2, "Hero: archmage"),
    (3, "Hero: ranger"),
    (4, "Feudal lord"),
    (5, "Bandits"),
    (6, "Peasants"),
    (7, "Inactive"),
    (8, "Lantern"),
    (9, "Event point"),
    (10, "Necromancer"),
    (11, "Ghosts"),
    (12, "Zombies"),
];

pub const BEHAVIOURS: [&str; 3] = ["Feudal", "Rogue", "Peasant"];
pub const TARGET_MODELS: [&str; 5] = ["Standard", "Aggressive", "Passive", "Hoarding", "Trading"];
pub const SHIPS: [&str; 4] = ["No ship", "Hero ship", "Pirates", "Merchant"];
/// Factions 1–4 (the editor's colours: green, blue, yellow, red).
pub const FACTIONS: [&str; 4] = ["Player (green)", "Ally (blue)", "Neighbour (yellow)", "Enemy (red)"];
pub const HERO_CLASSES: [&str; 3] = ["Knight", "Archmage", "Ranger"];
pub const SCENARIO_KINDS: [&str; 3] = ["Standalone", "First map of a campaign", "Later campaign map"];
pub const CARRY_OVER: [&str; 7] = ["Gold", "Gods' favour", "Fame", "Experience and level", "Personal artefacts", "Whole inventory", "Whole army"];

/// Footprint Razdor gives a building type when no install tells it (our own choice).
pub fn fallback_size(kind: u8) -> (u8, u8) {
    match BuildingType::from_code(kind) {
        Some(BuildingType::Town) => (7, 7),
        Some(BuildingType::Village) => (4, 4),
        Some(BuildingType::Castle) => (4, 4),
        Some(BuildingType::Shipyard) => (3, 3),
        Some(BuildingType::StoneBridge | BuildingType::WoodenBridge) => (1, 1),
        _ => (2, 2),
    }
}

impl Palette {
    /// The palette of the install's `Objects.ugs`.
    pub fn from_sprites(sprites: &ObjectSprites) -> Palette {
        let mut objects: Vec<ObjectKey> = sprites
            .sprites
            .iter()
            .filter(|s| s.section == ObjectSprite::DECORATIONS)
            .filter_map(|s| Some(ObjectKey { class: u8::try_from(s.cat).ok()?, sprite: u8::try_from(s.idx).ok()? }))
            .collect();
        objects.sort();
        objects.dedup();
        let mut buildings: Vec<BuildingPicture> = sprites
            .sprites
            .iter()
            .filter(|s| s.section == ObjectSprite::BUILDINGS)
            .filter_map(|s| {
                Some(BuildingPicture { picture_type: u8::try_from(s.cat).ok()?, variant: u8::try_from(s.idx).ok()?, size: s.footprint()? })
            })
            .collect();
        buildings.sort_by_key(|b| (b.picture_type, b.variant));
        buildings.dedup_by_key(|b| (b.picture_type, b.variant));
        Palette { objects, buildings, from_install: true }
    }

    /// Without an install: every object class with sprite ids 10–13 for massifs and 0–8 for
    /// plants, and variant 0 of every building type with [`fallback_size`]. The game may not
    /// have art for all of these; save checks them only against an install's palette.
    pub fn fallback() -> Palette {
        let mut objects = Vec::new();
        for (class, _) in OBJECT_CLASSES {
            let ids: Vec<u8> = if is_massif(class) { (10..=13).collect() } else { (0..=8).collect() };
            objects.extend(ids.into_iter().map(|sprite| ObjectKey { class, sprite }));
        }
        let buildings = (1..=15u8).map(|t| BuildingPicture { picture_type: t, variant: 0, size: fallback_size(t) }).collect();
        Palette { objects, buildings, from_install: false }
    }

    /// Object classes present, in order.
    pub fn classes(&self) -> Vec<u8> {
        let mut c: Vec<u8> = self.objects.iter().map(|o| o.class).collect();
        c.dedup();
        c
    }

    pub fn sprites_of(&self, class: u8) -> impl Iterator<Item = ObjectKey> + '_ {
        self.objects.iter().copied().filter(move |o| o.class == class)
    }

    pub fn has_object(&self, class: u8, sprite: u8) -> bool {
        self.objects.binary_search(&ObjectKey { class, sprite }).is_ok()
    }

    /// Picture variants of a building picture type.
    pub fn pictures_of(&self, picture_type: u8) -> impl Iterator<Item = BuildingPicture> + '_ {
        self.buildings.iter().copied().filter(move |b| b.picture_type == picture_type)
    }

    pub fn picture(&self, picture_type: u8, variant: u8) -> Option<BuildingPicture> {
        self.buildings.iter().copied().find(|b| b.picture_type == picture_type && b.variant == variant)
    }

    /// The footprint of a picture, else the fallback size of its type.
    pub fn footprint(&self, picture_type: u8, variant: u8) -> (u8, u8) {
        self.picture(picture_type, variant).map_or(fallback_size(picture_type), |p| p.size)
    }
}

/// One choice of a picker: the id the map stores and the name shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: u32,
    pub name: String,
}

/// Names of the units, artefacts and spells of the editor's content (read from the player's
/// install at runtime, or the built-in demo), for the pickers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Names {
    /// By `GlobalIndex`.
    pub units: Vec<Choice>,
    /// By `GlobalIndex`.
    pub artefacts: Vec<Choice>,
    /// By 1-based index.
    pub spells: Vec<Choice>,
}

impl Names {
    pub fn from_content(c: &Content) -> Names {
        Names {
            units: c.units.iter().map(|u| Choice { id: u.id, name: u.name.clone() }).collect(),
            artefacts: c.items.iter().map(|a| Choice { id: a.id, name: a.name.clone() }).collect(),
            spells: c.spells.iter().enumerate().map(|(i, s)| Choice { id: i as u32 + 1, name: s.name.clone() }).collect(),
        }
    }

    fn name(list: &[Choice], id: u32, what: &str) -> String {
        match id {
            0 => "(none)".to_string(),
            _ => list.iter().find(|c| c.id == id).map_or(format!("{what} #{id}"), |c| c.name.clone()),
        }
    }

    pub fn unit(&self, id: u32) -> String {
        Names::name(&self.units, id, "unit")
    }

    pub fn artefact(&self, id: u32) -> String {
        Names::name(&self.artefacts, id, "artefact")
    }

    pub fn spell(&self, id: u32) -> String {
        Names::name(&self.spells, id, "spell")
    }

    pub fn has_unit(&self, id: u32) -> bool {
        self.units.iter().any(|c| c.id == id)
    }

    pub fn has_artefact(&self, id: u32) -> bool {
        self.artefacts.iter().any(|c| c.id == id)
    }

    pub fn has_spell(&self, id: u32) -> bool {
        self.spells.iter().any(|c| c.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::gfx::Image;

    fn sprite(section: u32, cat: u32, idx: u32, extra: Vec<u8>) -> ObjectSprite {
        ObjectSprite { section, cat, idx, cell: 1, extra, image: Image { width: 1, height: 1, rgba: vec![0; 4] } }
    }

    fn size(a: u32, b: u32) -> Vec<u8> {
        a.to_le_bytes().into_iter().chain(b.to_le_bytes()).collect()
    }

    #[test]
    fn palette_from_sprites() {
        let sprites = ObjectSprites {
            sprites: vec![
                sprite(0, 9, 5, vec![0; 4]),
                sprite(0, 1, 20, vec![0; 4]),
                sprite(0, 9, 1, vec![0; 4]),
                sprite(1, 3, 1, size(4, 4)),
                sprite(1, 3, 0, size(4, 4)),
                sprite(1, 9, 0, size(4, 3)),
            ],
        };
        let p = Palette::from_sprites(&sprites);
        assert!(p.from_install);
        assert_eq!(p.classes(), vec![1, 9]);
        assert_eq!(p.sprites_of(9).map(|o| o.sprite).collect::<Vec<_>>(), vec![1, 5]);
        assert!(p.has_object(1, 20) && !p.has_object(1, 21));
        assert_eq!(p.pictures_of(3).map(|b| b.variant).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(p.footprint(9, 0), (4, 3));
        // Unknown pictures fall back to our own size.
        assert_eq!(p.footprint(1, 7), fallback_size(1));
    }

    #[test]
    fn fallback_palette_covers_every_type() {
        let p = Palette::fallback();
        assert!(!p.from_install);
        assert!((1..=15).all(|t| p.picture(t, 0).is_some()));
        assert_eq!(p.classes().len(), OBJECT_CLASSES.len());
    }

    #[test]
    fn names_from_content() {
        let c = Content::builtin();
        let n = Names::from_content(&c);
        assert_eq!(n.unit(1), c.units[0].name);
        assert_eq!(n.unit(0), "(none)");
        assert_eq!(n.unit(9999), "unit #9999");
        assert!(n.has_unit(1) && !n.has_unit(9999));
        assert_eq!(n.spells.first().map(|s| s.id), Some(1));
    }
}
