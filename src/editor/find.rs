//! The editor's Ctrl+F: finds the records of a map by name or id — buildings (also by their
//! type, owner, garrison, barracks, goods and spells), armies (by leader, troops and items),
//! points, events (by title, question and message), the hero starts and the map objects (by
//! class) — for the window to list and jump to. Matching is `crate::search::matches`: every
//! word of the query somewhere in the record, any case, Ё as Е.

use crate::dt::dtm::{Scenario, Troop};
use crate::i18n::tr;
use crate::search::{self, Match};
use crate::trf;

use super::events::split_title;
use super::palette::{building_type_label, object_class_label, Names, HERO_CLASSES};
use super::validate::Place;

/// Most hits listed (a word like "trees" would find every tree of a big map).
pub const MAX_HITS: usize = 500;

/// A record found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub place: Place,
    /// What the list shows: the record and its name.
    pub label: String,
    /// The part of `label` to light up.
    pub marked: Option<std::ops::Range<usize>>,
    /// Where the query was found when not in the name ("garrison: Лучник").
    pub detail: Option<String>,
}

/// One record as the search reads it: its place, the label's head ("Army 3"), its name, and
/// its other fields with their labels.
struct Record {
    place: Place,
    head: String,
    id: u32,
    name: String,
    fields: Vec<(String, String)>,
}

fn troop_names(names: &Names, troops: &[Troop]) -> String {
    troops.iter().filter(|t| t.unit != 0).map(|t| names.unit(t.unit as u32)).collect::<Vec<_>>().join(", ")
}

fn records(s: &Scenario, names: &Names) -> Vec<Record> {
    let mut out = Vec::new();
    for (k, class) in HERO_CLASSES.iter().enumerate().take(s.header.heroes.len()) {
        let class = tr(class).to_string();
        out.push(Record { place: Place::Hero(k), head: Place::Hero(k).to_string(), id: 0, name: class, fields: Vec::new() });
    }
    for (i, b) in s.buildings.iter().enumerate() {
        let id = i as u32 + 1;
        let place = Place::Building(id as u16);
        let goods: Vec<String> = b.artifact_slots.iter().take(super::records::GOODS).filter(|&&a| a != 0).map(|&a| names.artefact(a as u32)).collect();
        let spells: Vec<String> = b.spells_for_sale.iter().filter(|&&sp| sp != 0).map(|&sp| names.spell(sp as u32)).collect();
        let recruits: Vec<String> = b.barracks.iter().filter(|r| r.unit != 0).map(|r| names.unit(r.unit as u32)).collect();
        let fields = vec![
            (tr("type").to_string(), building_type_label(b.kind).to_string()),
            (tr("owner").to_string(), b.owner_name.clone()),
            (tr("garrison").to_string(), troop_names(names, &b.garrison)),
            (tr("barracks").to_string(), recruits.join(", ")),
            (tr("goods").to_string(), goods.join(", ")),
            (tr("spells").to_string(), spells.join(", ")),
            (tr("description").to_string(), b.description.clone()),
        ];
        out.push(Record { place, head: place.to_string(), id, name: b.name.clone(), fields });
    }
    for (i, a) in s.armies.iter().enumerate() {
        let id = i as u32 + 1;
        let place = Place::Army(id as u8);
        let items: Vec<String> = a.artifacts.iter().filter(|&&x| x != 0).map(|&x| names.artefact(x as u32)).collect();
        let leader = if a.leader_unit != 0 { names.unit(a.leader_unit as u32) } else { String::new() };
        let fields = vec![
            (tr("leader").to_string(), format!("{} {leader}", a.leader_name)),
            (tr("troops").to_string(), troop_names(names, &a.troops)),
            (tr("items").to_string(), items.join(", ")),
            (tr("description").to_string(), a.description.clone()),
        ];
        out.push(Record { place, head: place.to_string(), id, name: a.name.clone(), fields });
    }
    for (i, _) in s.points.iter().enumerate() {
        let id = i as u32 + 1;
        let place = Place::Point(id as u8);
        out.push(Record { place, head: place.to_string(), id, name: String::new(), fields: Vec::new() });
    }
    for (i, e) in s.events.iter().enumerate() {
        let id = i as u32 + 1;
        let place = Place::Event(id as u16);
        let fields = vec![(tr("question").to_string(), e.question.clone()), (tr("message").to_string(), e.message.clone())];
        out.push(Record { place, head: place.to_string(), id, name: split_title(&e.title).name, fields });
    }
    for (i, o) in s.objects.iter().enumerate() {
        let place = Place::Object(i);
        let name = trf!("{class}, picture {sprite}", class = object_class_label(o.class), sprite = o.sprite);
        out.push(Record { place, head: place.to_string(), id: i as u32 + 1, name, fields: Vec::new() });
    }
    out
}

/// The records of `s` that `query` finds, in the map's order (the hero starts, buildings,
/// armies, points, events, objects), the records whose id is the query (a number) before the
/// others; `#N` finds only the records with id N. At most [`MAX_HITS`]; an empty query finds
/// nothing.
pub fn find(s: &Scenario, names: &Names, query: &str) -> Vec<Hit> {
    if search::words(query).is_empty() {
        return Vec::new();
    }
    let only_id: Option<u32> = query.trim().strip_prefix('#').and_then(|n| n.trim().parse().ok());
    let number: Option<u32> = only_id.or_else(|| query.trim().parse().ok());
    let mut exact = Vec::new();
    let mut rest = Vec::new();
    for r in records(s, names) {
        if let Some(n) = only_id {
            if r.id == n && n != 0 {
                let label = if r.name.is_empty() { r.head.clone() } else { format!("{}: {}", r.head, r.name) };
                exact.push(Hit { place: r.place, label, marked: None, detail: None });
            }
            continue;
        }
        let id = r.id.to_string();
        let mut others: Vec<&str> = r.fields.iter().map(|(_, v)| v.as_str()).collect();
        if r.id != 0 {
            others.push(&id);
        }
        others.push(&r.head);
        let Some(Match { name_range }) = search::matches(query, &r.name, &others) else { continue };
        let label = if r.name.is_empty() { r.head.clone() } else { format!("{}: {}", r.head, r.name) };
        let shift = label.len() - r.name.len();
        let marked = name_range.filter(|_| !r.name.is_empty()).map(|m| m.start + shift..m.end + shift);
        // The field the query stands in, when the name does not hold all of it.
        let detail = r
            .fields
            .iter()
            .filter(|(_, v)| !v.trim().is_empty())
            .find(|(_, v)| search::words(query).iter().any(|w| search::find(&r.name, w).is_none() && search::find(v, w).is_some()))
            .map(|(k, v)| format!("{k}: {}", v.split_whitespace().collect::<Vec<_>>().join(" ")));
        let hit = Hit { place: r.place, label, marked, detail };
        if number.is_some_and(|n| n == r.id && r.id != 0) && !matches!(r.place, Place::Object(_)) {
            exact.push(hit);
        } else {
            rest.push(hit);
        }
    }
    exact.extend(rest);
    exact.truncate(MAX_HITS);
    exact
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Army, Building, Event, MapObject};
    use crate::editor::palette::Choice;

    fn names() -> Names {
        Names {
            units: vec![Choice { id: 5, name: "Лучник".into() }, Choice { id: 6, name: "Мечник".into() }],
            artefacts: vec![Choice { id: 9, name: "Ёлочный амулет".into() }],
            spells: vec![Choice { id: 1, name: "Огненный шар".into() }],
        }
    }

    fn map() -> Scenario {
        let mut s = crate::editor::EditorDoc::new_map(crate::editor::NewMap { width: 20, height: 20, fill: 6 }).scenario;
        let mut castle = Building { name: "Замок Черной скалы".into(), owner_name: "Барон".into(), ..Building::default() };
        castle.garrison[0] = Troop { unit: 5, level: 1, count: 3 };
        castle.artifact_slots[0] = 9;
        castle.spells_for_sale[0] = 1;
        let fort = Building { name: "Форт у реки".into(), ..Building::default() };
        s.buildings = vec![castle, fort];
        let mut gang = Army { name: "Морские разбойники".into(), leader_name: "Одноглазый".into(), leader_unit: 6, ..Army::default() };
        gang.troops[0] = Troop { unit: 5, level: 0, count: 2 };
        s.armies = vec![gang];
        let quest = Event { title: "Сообщение посыльного%+Письмо".into(), message: "Барон ждёт вас в замке".into(), ..Event::default() };
        s.events = vec![quest];
        s.objects = vec![MapObject { x: 1, y: 1, sprite: 3, class: 9 }];
        s
    }

    fn places(q: &str) -> Vec<Place> {
        find(&map(), &names(), q).into_iter().map(|h| h.place).collect()
    }

    #[test]
    fn finds_records_by_name_in_any_case() {
        assert_eq!(places("барон"), [Place::Building(1), Place::Event(1)], "the castle's owner, and the event that names him");
        assert_eq!(places("РАЗБОЙ"), [Place::Army(1)]);
        assert_eq!(places("посыльн"), [Place::Event(1)], "the event's name, not its flag script");
        assert_eq!(places("письмо"), Vec::<Place>::new());
        assert_eq!(places("черной"), [Place::Building(1)]);
        assert!(places("").is_empty() && places("   ").is_empty());
        let hit = &find(&map(), &names(), "черн")[0];
        assert_eq!(&hit.label[hit.marked.clone().unwrap()], "Черн", "the name's match is lit in the label");
    }

    #[test]
    fn finds_units_items_and_spells_on_the_map() {
        assert_eq!(places("лучник"), [Place::Building(1), Place::Army(1)], "a garrison and a troop");
        assert_eq!(places("мечник"), [Place::Army(1)], "the leader's unit");
        assert_eq!(places("елочный"), [Place::Building(1)], "goods, Ё as Е");
        assert_eq!(places("огненный"), [Place::Building(1)], "spells for sale");
        let hit = &find(&map(), &names(), "лучник")[0];
        assert_eq!(hit.detail.as_deref(), Some("garrison: Лучник"));
        assert_eq!(places("Trees"), [Place::Object(0)], "objects by class");
    }

    #[test]
    fn an_id_comes_first() {
        let p = places("2");
        assert_eq!(p[0], Place::Building(2), "building 2 by its id");
        assert_eq!(places("#1")[..4], [Place::Building(1), Place::Army(1), Place::Event(1), Place::Object(0)][..], "#1: every record 1, the object too");
        assert_eq!(places("army 1"), [Place::Army(1)], "by its label");
    }
}
