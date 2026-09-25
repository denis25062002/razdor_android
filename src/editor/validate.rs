//! Checks before saving: everything the file format, the original game or Razdor would
//! trip over, as readable messages.
//!
//! Errors block saving; warnings are shown and saved anyway. Checks against unit, artefact
//! and spell ids need the install's [`Names`]; checks of object and building pictures need
//! a palette read from the install ([`Palette::from_install`]).

use std::fmt;

use crate::dt::dtm::{Scenario, BUILDING_SIZE, EVENT_SIZE};
use crate::dt::text;

use super::geometry::Footprint;
use super::palette::{building_type_label, Names, Palette};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

/// What an issue is about (ids are 1-based, as the map stores them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Map,
    Settings,
    Hero(usize),
    Object(usize),
    Building(u16),
    Army(u8),
    Point(u8),
    Event(u16),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    pub place: Place,
    pub message: String,
}

impl fmt::Display for Place {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Place::Map => write!(f, "Map"),
            Place::Settings => write!(f, "Scenario settings"),
            Place::Hero(k) => write!(f, "{} start", super::palette::HERO_CLASSES.get(*k).unwrap_or(&"Hero")),
            Place::Object(i) => write!(f, "Object {}", i + 1),
            Place::Building(id) => write!(f, "Building {id}"),
            Place::Army(id) => write!(f, "Army {id}"),
            Place::Point(id) => write!(f, "Point {id}"),
            Place::Event(id) => write!(f, "Event {id}"),
        }
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{} ({s}): {}", self.place, self.message)
    }
}

/// Largest map side the editor accepts (the original's generator offers up to 800).
pub const MAX_SIDE: u32 = 800;
/// References to buildings, armies and points are single bytes.
pub const MAX_RECORDS: usize = 255;

struct Checker<'a> {
    s: &'a Scenario,
    names: Option<&'a Names>,
    palette: Option<&'a Palette>,
    out: Vec<Issue>,
}

impl Checker<'_> {
    fn error(&mut self, place: Place, message: String) {
        self.out.push(Issue { severity: Severity::Error, place, message });
    }

    fn warn(&mut self, place: Place, message: String) {
        self.out.push(Issue { severity: Severity::Warning, place, message });
    }

    fn inside(&self, x: u16, y: u16) -> bool {
        (x as u32) < self.s.width() && (y as u32) < self.s.height()
    }

    fn string(&mut self, place: Place, what: &str, s: &str) {
        if s.contains('\0') {
            self.error(place, format!("the {what} contains a NUL character"));
        } else if text::decode(&text::encode(s)) != s {
            let bad: String = s.chars().filter(|c| text::decode(&text::encode(&c.to_string())) != c.to_string()).take(5).collect();
            self.warn(place, format!("the {what} has characters Windows-1251 cannot store ({bad}); they are saved as '?'"));
        }
    }

    fn building_ref(&mut self, place: Place, what: &str, id: u32) {
        if id as usize > self.s.buildings.len() {
            self.error(place, format!("{what} refers to building {id}, which does not exist"));
        }
    }

    fn army_ref(&mut self, place: Place, what: &str, id: u32) {
        if id as usize > self.s.armies.len() {
            self.error(place, format!("{what} refers to army {id}, which does not exist"));
        }
    }

    fn event_ref(&mut self, place: Place, what: &str, id: u32) {
        if id as usize > self.s.events.len() {
            self.error(place, format!("{what} refers to event {id}, which does not exist"));
        }
    }

    fn unit(&mut self, place: Place, what: &str, id: u8) {
        if let Some(n) = self.names {
            if id != 0 && !n.has_unit(id as u32) {
                self.error(place, format!("{what}: unit {id} is not in the game's unit list"));
            }
        }
    }

    fn artefact(&mut self, place: Place, what: &str, id: u32) {
        if let Some(n) = self.names {
            if id != 0 && !n.has_artefact(id) {
                self.error(place, format!("{what}: artefact {id} is not in the game's artefact list"));
            }
        }
    }

    fn spell(&mut self, place: Place, what: &str, id: u8) {
        if let Some(n) = self.names {
            if id != 0 && !n.has_spell(id as u32) {
                self.error(place, format!("{what}: spell {id} is not in the game's spell list"));
            }
        }
    }

    fn relations(&mut self, place: Place, what: &str, r: &[i8]) {
        if r.iter().any(|v| !(-3..=3).contains(v)) {
            self.error(place, format!("{what} must lie between -3 and 3"));
        }
    }

    fn faction(&mut self, place: Place, f: u8) {
        if !(1..=4).contains(&f) {
            self.error(place, format!("faction {f} is not one of 1-4 (player, ally, neighbour, enemy)"));
        }
    }

    fn map(&mut self) {
        let s = self.s;
        let (w, h) = (s.width(), s.height());
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
            self.error(Place::Map, format!("map size {w}x{h}: each side must be 1-{MAX_SIDE}"));
        }
        if s.terrain.len() as u64 != w as u64 * h as u64 {
            self.error(Place::Map, format!("the terrain has {} cells, the size needs {}", s.terrain.len(), w as u64 * h as u64));
        }
        if w != h {
            self.warn(Place::Map, "the map is not square; every shipped map is".into());
        }
        if let Some(c) = s.terrain.iter().find(|c| **c > 15) {
            self.error(Place::Map, format!("terrain code {c} is not one of the 16 surfaces"));
        }
        let from_install = self.palette.filter(|p| p.from_install);
        for (i, o) in s.objects.iter().enumerate() {
            if !self.inside(o.x, o.y) {
                self.error(Place::Object(i), format!("object at ({}, {}) is outside the map", o.x, o.y));
            }
            if let Some(p) = from_install {
                if !p.has_object(o.class, o.sprite) {
                    self.error(Place::Object(i), format!("the game has no picture for object class {} sprite {}", o.class, o.sprite));
                }
            }
        }
    }

    fn settings(&mut self) {
        let s = self.s;
        let h = &s.header;
        for (what, v) in [("title", &s.title), ("description", &s.description), ("campaign name", &s.campaign_name), ("next map", &s.next_map)] {
            self.string(Place::Settings, what, v);
        }
        if !s.next_map.is_empty() && !s.next_map.to_ascii_lowercase().ends_with(".dtm") {
            self.warn(Place::Settings, format!("the next map \"{}\" should be a .DTm file name", s.next_map));
        }
        self.event_ref(Place::Settings, "the victory event", h.victory_event as u32);
        self.event_ref(Place::Settings, "the defeat event", h.defeat_event as u32);
        for row in &h.relations {
            self.relations(Place::Settings, "the faction relations", row);
        }
        if h.scenario_kind > 2 {
            self.error(Place::Settings, format!("scenario kind {} is not one of 0-2", h.scenario_kind));
        }
        if s.named_characters.len() > 32 {
            self.error(Place::Settings, format!("{} named characters; at most 32 fit", s.named_characters.len()));
        }
        for (k, n) in s.named_characters.iter().enumerate() {
            self.unit(Place::Settings, &format!("named character {}", k + 1), n.unit);
            self.string(Place::Settings, &format!("name of named character {}", k + 1), &n.name);
        }
        for (k, p) in h.heroes.iter().enumerate() {
            let place = Place::Hero(k);
            if !self.inside(p.x, p.y) {
                self.error(place, format!("the start ({}, {}) is outside the map", p.x, p.y));
            }
            self.building_ref(place, "the start building", p.start_building as u32);
            for t in p.troops.iter().filter(|t| t.unit != 0) {
                self.unit(place, "starting troops", t.unit);
            }
            for a in p.artifacts {
                self.artefact(place, "starting artefacts", a as u32);
            }
            for sp in p.spells {
                self.spell(place, "starting spells", sp);
            }
            if p.gold > i16::MAX as u32 || p.mana > i16::MAX as u32 {
                self.error(place, format!("gold and mana must be at most {} (the game reads 16 bits)", i16::MAX));
            }
        }
    }

    fn buildings(&mut self) {
        let s = self.s;
        if s.buildings.len() > MAX_RECORDS {
            self.error(Place::Map, format!("{} buildings; at most {MAX_RECORDS} can be referred to", s.buildings.len()));
        }
        let footprints: Vec<Footprint> = s.buildings.iter().map(|b| Footprint::of(b.x as i32, b.y as i32, b.size_x, b.size_y)).collect();
        let from_install = self.palette.filter(|p| p.from_install);
        for (i, b) in s.buildings.iter().enumerate() {
            let id = i as u16 + 1;
            let place = Place::Building(id);
            if b.kind > 15 {
                self.error(place, format!("building type {} is not one of 0-15", b.kind));
            }
            if b.size_x == 0 || b.size_y == 0 {
                self.error(place, "the footprint size must be at least 1x1".into());
            }
            if !footprints[i].inside(s.width(), s.height()) {
                self.error(place, format!("the footprint of the {} at ({}, {}) reaches outside the map", building_type_label(b.kind), b.x, b.y));
            }
            let is_bridge = matches!(b.kind, 13 | 14);
            if !is_bridge {
                let bounds = footprints[i].bounds();
                if let Some(j) = (0..i).find(|&j| !matches!(s.buildings[j].kind, 13 | 14) && footprints[j].bounds().overlaps(&bounds)) {
                    self.warn(place, format!("overlaps building {}", j + 1));
                }
            }
            if let Some(p) = from_install {
                match p.picture(b.picture_type, b.picture_variant) {
                    None => self.error(place, format!("the game has no picture {} of type {}", b.picture_variant, b.picture_type)),
                    Some(pic) if pic.size != (b.size_x, b.size_y) => self.warn(
                        place,
                        format!("footprint {}x{} differs from its picture's {}x{}", b.size_x, b.size_y, pic.size.0, pic.size.1),
                    ),
                    Some(_) => {}
                }
            }
            if b.event_count as usize > b.event_slots.len() {
                self.error(place, format!("{} local events; at most {} fit", b.event_count, b.event_slots.len()));
            }
            for e in b.events() {
                self.event_ref(place, "a local event", e as u32);
            }
            if b.owner_army != 0 && b.owner_army != 0xFF {
                self.army_ref(place, "the owner", b.owner_army as u32);
            }
            self.building_ref(place, "the linked building", b.linked_building as u32);
            if b.linked_building as u16 == id {
                self.warn(place, "is linked to itself".into());
            }
            self.faction(place, b.faction);
            self.relations(place, "attitudes", &b.relations);
            for t in b.garrison.iter().filter(|t| t.unit != 0) {
                self.unit(place, "garrison", t.unit);
            }
            for r in b.barracks.iter().filter(|r| r.unit != 0) {
                self.unit(place, "barracks", r.unit);
                if r.start_count > r.max_count {
                    self.warn(place, format!("barracks start with {} units but hold at most {}", r.start_count, r.max_count));
                }
            }
            for a in b.artifacts() {
                self.artefact(place, "goods", a as u32);
            }
            for sp in b.spells_for_sale {
                self.spell(place, "spells for sale", sp);
            }
            if b.kind != 12 && b.random_artifacts_for_sale > 0 && b.price_min > b.price_max {
                self.warn(place, format!("the lowest price {} is above the highest {}", b.price_min, b.price_max));
            }
            for (what, v) in [("name", &b.name), ("owner name", &b.owner_name), ("description", &b.description)] {
                self.string(place, what, v);
            }
        }
    }

    fn armies(&mut self) {
        let s = self.s;
        if s.armies.len() > MAX_RECORDS {
            self.error(Place::Map, format!("{} armies; at most {MAX_RECORDS} fit", s.armies.len()));
        }
        for (i, a) in s.armies.iter().enumerate() {
            let place = Place::Army(i.min(254) as u8 + 1);
            if a.id as usize != i + 1 {
                self.error(place, format!("stores id {}; armies must be numbered in order", a.id));
            }
            if !self.inside(a.x, a.y) {
                self.error(place, format!("({}, {}) is outside the map", a.x, a.y));
            }
            if !(1..=12).contains(&a.model) {
                self.error(place, format!("map model {} is not one of 1-12", a.model));
            }
            self.building_ref(place, "the home building", a.home_building as u32);
            if a.named_character as usize > s.named_characters.len() {
                self.error(place, format!("named character {} does not exist", a.named_character));
            }
            self.unit(place, "the leader", a.leader_unit);
            for t in a.troops.iter().filter(|t| t.unit != 0) {
                self.unit(place, "troops", t.unit);
            }
            if a.leader_unit == 0 && a.troops().next().is_none() {
                self.warn(place, "has neither a leader nor troops".into());
            }
            for x in a.artifacts {
                self.artefact(place, "carried artefacts", x as u32);
            }
            self.spell(place, "the spell on the army", a.spell);
            self.faction(place, a.faction);
            self.relations(place, "attitudes", &a.relations);
            if a.target_model > 4 {
                self.error(place, format!("target model {} is not one of 0-4", a.target_model));
            }
            for (what, v) in [("name", &a.name), ("leader name", &a.leader_name), ("description", &a.description)] {
                self.string(place, what, v);
            }
        }
    }

    fn points(&mut self) {
        let s = self.s;
        if s.points.len() > MAX_RECORDS {
            self.error(Place::Map, format!("{} points; at most {MAX_RECORDS} fit", s.points.len()));
        }
        for (i, p) in s.points.iter().enumerate() {
            let place = Place::Point(i.min(254) as u8 + 1);
            if p.id as usize != i + 1 {
                self.error(place, format!("stores id {}; points must be numbered in order", p.id));
            }
            if !self.inside(p.x, p.y) {
                self.error(place, format!("({}, {}) is outside the map", p.x, p.y));
            }
            if !matches!(p.model, 8 | 9) {
                self.error(place, format!("model {} is neither 8 (lantern) nor 9 (event point)", p.model));
            }
            if p.event_count as usize > p.event_slots.len() {
                self.error(place, format!("{} events; at most {} fit", p.event_count, p.event_slots.len()));
            }
            for e in p.events() {
                self.event_ref(place, "an attached event", e as u32);
            }
            if p.radius > 24 {
                self.warn(place, format!("radius {} is above the original's 24", p.radius));
            }
        }
    }

    fn events(&mut self) {
        let s = self.s;
        if s.events.len() > u16::MAX as usize {
            self.error(Place::Map, format!("{} events; at most {} fit", s.events.len(), u16::MAX));
        }
        for (i, e) in s.events.iter().enumerate() {
            let place = Place::Event(i.min(u16::MAX as usize - 1) as u16 + 1);
            let (c, r) = (&e.conditions, &e.results);
            for b in c.buildings {
                self.building_ref(place, "a building condition", b as u32);
            }
            for a in c.defeated_armies.into_iter().chain(c.beaten_armies).chain([c.meet_army, c.army_active, c.army_inactive, c.army_at_home]) {
                self.army_ref(place, "a condition", a as u32);
            }
            for a in r.activate_armies.into_iter().chain([r.deactivate_army, r.show_army, r.start_battle_with, r.removed_units_to_army, r.units_from_army]) {
                self.army_ref(place, "a result", a as u32);
            }
            for x in c.happened_yes.into_iter().chain(c.happened_no).chain(c.not_happened).chain([r.relative_event, r.completes_quest, r.chained_event]) {
                self.event_ref(place, "a condition or result", x as u32);
            }
            for l in r.light_lanterns {
                if l as usize > s.points.len() {
                    self.error(place, format!("lights point {l}, which does not exist"));
                }
            }
            if e.custom_picture.as_ref().is_some_and(|p| p.len() > u16::MAX as usize) {
                self.error(place, "the event picture is larger than 65535 bytes".into());
            }
            for (what, v) in [("title", &e.title), ("question", &e.question), ("message", &e.message)] {
                self.string(place, what, v);
            }
        }
    }
}

/// All issues of a scenario, errors first.
pub fn validate(s: &Scenario, names: Option<&Names>, palette: Option<&Palette>) -> Vec<Issue> {
    let mut c = Checker { s, names, palette, out: Vec::new() };
    c.map();
    c.settings();
    c.buildings();
    c.armies();
    c.points();
    c.events();
    let mut out = c.out;
    // The writer must produce a payload that reads back to the same bytes. Only checked when
    // nothing structural is wrong (the writer assumes a consistent terrain size).
    if !out.iter().any(|i| i.severity == Severity::Error) {
        if let Err(message) = self_check(s) {
            out.push(Issue { severity: Severity::Error, place: Place::Map, message });
        }
    }
    out.sort_by_key(|i| std::cmp::Reverse(i.severity));
    out
}

/// The payload of `s` reads back and serialises to the same bytes: section sizes, record
/// counts and the strings (one per field, in record order) all line up.
pub fn self_check(s: &Scenario) -> Result<Vec<u8>, String> {
    let bytes = s.to_payload();
    let back = Scenario::parse_payload(&bytes).map_err(|e| format!("the written map does not read back: {e}"))?;
    let sizes = [back.buildings.len() * BUILDING_SIZE, back.events.len() * EVENT_SIZE];
    if sizes != [s.buildings.len() * BUILDING_SIZE, s.events.len() * EVENT_SIZE] || back.armies.len() != s.armies.len() || back.points.len() != s.points.len() {
        return Err("the written map has a different number of records".into());
    }
    if back.to_payload() != bytes {
        return Err("the written map does not serialise back to the same bytes".into());
    }
    Ok(bytes)
}

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Army, Building, Event, NamedCharacter, Point};
    use crate::editor::palette::{BuildingPicture, ObjectKey};

    fn map(w: u32, h: u32) -> Scenario {
        let mut s = Scenario::default();
        s.header.width = w;
        s.header.height = h;
        s.terrain = vec![6; (w * h) as usize];
        s
    }

    fn errors(s: &Scenario) -> Vec<String> {
        validate(s, None, None).into_iter().filter(|i| i.severity == Severity::Error).map(|i| i.to_string()).collect()
    }

    fn building(x: u16, y: u16, size: (u8, u8)) -> Building {
        Building { x, y, kind: 3, picture_type: 3, size_x: size.0, size_y: size.1, faction: 3, ..Building::default() }
    }

    #[test]
    fn an_empty_map_is_valid() {
        assert!(errors(&map(10, 10)).is_empty());
    }

    #[test]
    fn size_and_terrain() {
        let mut s = map(4, 4);
        s.terrain.pop();
        assert!(errors(&s)[0].contains("terrain has 15 cells"));
        let mut s = map(4, 4);
        s.terrain[3] = 16;
        assert!(errors(&s)[0].contains("terrain code 16"));
        let s = map(0, 0);
        assert!(errors(&s)[0].contains("each side"));
        let w = validate(&map(4, 3), None, None);
        assert!(w.iter().any(|i| i.severity == Severity::Warning && i.message.contains("not square")));
    }

    #[test]
    fn footprints_must_be_inside() {
        let mut s = map(10, 10);
        s.buildings = vec![building(9, 9, (4, 4)), building(2, 2, (4, 4)), building(5, 2, (4, 3))];
        let e = errors(&s);
        assert_eq!(e.len(), 2, "{e:?}");
        assert!(e[0].starts_with("Building 2") && e[0].contains("outside"));
        // 4x3 needs an extra row above: rows -1..=2 for y = 2.
        assert!(e[1].starts_with("Building 3"));
        s.buildings[2].y = 3;
        s.buildings[1].x = 3;
        s.buildings[1].y = 3;
        assert!(errors(&s).is_empty());
    }

    #[test]
    fn overlapping_buildings_warn() {
        let mut s = map(10, 10);
        s.buildings = vec![building(5, 5, (2, 2)), building(6, 6, (2, 2))];
        let all = validate(&s, None, None);
        assert!(all.iter().any(|i| i.place == Place::Building(2) && i.message.contains("overlaps building 1")));
    }

    #[test]
    fn dangling_references_are_errors() {
        let mut s = map(10, 10);
        let mut b = building(5, 5, (2, 2));
        b.owner_army = 3;
        b.linked_building = 7;
        b.event_count = 1;
        b.event_slots[0] = 2;
        s.buildings = vec![b];
        s.header.victory_event = 1;
        s.header.heroes[1].start_building = 4;
        s.armies = vec![Army { id: 1, x: 1, y: 1, model: 4, faction: 4, home_building: 9, leader_unit: 1, ..Army::default() }];
        s.points = vec![Point { id: 1, model: 9, event_count: 1, event_slots: [5, 0, 0, 0, 0, 0, 0, 0, 0, 0], ..Point::default() }];
        let e = errors(&s).join("\n");
        for needle in ["victory event refers to event 1", "Archmage start (error): the start building refers to building 4", "local event refers to event 2", "owner refers to army 3", "linked building refers to building 7", "home building refers to building 9", "Point 1 (error): an attached event refers to event 5"] {
            assert!(e.contains(needle), "{needle} missing in\n{e}");
        }
    }

    #[test]
    fn ids_must_follow_record_order() {
        let mut s = map(10, 10);
        s.armies = vec![Army { id: 2, model: 4, faction: 4, leader_unit: 1, ..Army::default() }];
        s.points = vec![Point { id: 3, model: 8, ..Point::default() }];
        let e = errors(&s);
        assert!(e.iter().any(|m| m.contains("stores id 2")));
        assert!(e.iter().any(|m| m.contains("stores id 3")));
    }

    #[test]
    fn content_ids_need_names() {
        let mut s = map(10, 10);
        s.header.heroes[0].troops[0] = crate::dt::dtm::Troop { unit: 250, level: 0, count: 1 };
        s.header.heroes[0].spells[0] = 99;
        assert!(errors(&s).is_empty(), "without names nothing is checked");
        let names = Names::from_content(&crate::rules::content::Content::builtin());
        let e: Vec<String> = validate(&s, Some(&names), None).iter().map(|i| i.to_string()).collect();
        assert!(e.iter().any(|m| m.contains("unit 250 is not in the game's unit list")), "{e:?}");
        assert!(e.iter().any(|m| m.contains("spell 99")));
    }

    #[test]
    fn install_palette_checks_pictures() {
        let mut s = map(10, 10);
        s.objects = vec![crate::dt::dtm::MapObject { x: 1, y: 1, class: 9, sprite: 7 }];
        s.buildings = vec![building(5, 5, (2, 2))];
        let p = Palette { objects: vec![ObjectKey { class: 9, sprite: 1 }], buildings: vec![BuildingPicture { picture_type: 3, variant: 0, size: (4, 4) }], from_install: true };
        let all = validate(&s, None, Some(&p));
        assert!(all.iter().any(|i| i.severity == Severity::Error && i.message.contains("object class 9 sprite 7")));
        assert!(all.iter().any(|i| i.severity == Severity::Warning && i.message.contains("differs from its picture's 4x4")));
        let fallback = Palette { from_install: false, ..p };
        assert!(validate(&s, None, Some(&fallback)).iter().all(|i| i.severity != Severity::Error));
    }

    #[test]
    fn strings_nul_and_encoding() {
        let mut s = map(10, 10);
        s.title = "Замок\0".into();
        s.description = "snow ☃".into();
        let all = validate(&s, None, None);
        assert!(all.iter().any(|i| i.severity == Severity::Error && i.message.contains("title contains a NUL")));
        assert!(all.iter().any(|i| i.severity == Severity::Warning && i.message.contains("(☃)")));
        s.title = "Замок".into();
        assert!(!has_errors(&validate(&s, None, None)));
    }

    #[test]
    fn too_many_named_characters() {
        let mut s = map(10, 10);
        s.named_characters = (0..33).map(|_| NamedCharacter { unit: 1, name: "x".into() }).collect();
        assert!(errors(&s).iter().any(|m| m.contains("33 named characters")));
    }

    #[test]
    fn event_references() {
        let mut s = map(10, 10);
        let mut e = Event::default();
        e.conditions.meet_army = 2;
        e.results.chained_event = 5;
        e.results.light_lanterns[0] = 1;
        s.events = vec![e];
        let e = errors(&s).join("\n");
        assert!(e.contains("refers to army 2") && e.contains("refers to event 5") && e.contains("lights point 1"), "{e}");
    }

    #[test]
    fn self_check_accepts_a_valid_map() {
        let mut s = map(6, 6);
        s.buildings = vec![building(3, 3, (2, 2))];
        s.title = "Test".into();
        let bytes = self_check(&s).unwrap();
        assert_eq!(Scenario::parse_payload(&bytes).unwrap().title, "Test");
    }
}
