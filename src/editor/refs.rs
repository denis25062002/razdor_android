//! Keeping ids consistent when a record is removed.
//!
//! Buildings, armies, points and named characters have 1-based ids in file order, and other
//! records refer to them by those ids (`docs/reference/dtm-format.md` §6–10). Removing
//! record `k` shifts every later id down by one; references to `k` itself are cleared
//! (0 = none; a building's owner army becomes 0xFF, the neutral owner). The fields remapped
//! here are every reference the format doc lists. Arguments of the Community opcodes
//! (events whose "no meeting" switch carries an opcode) are not ids of this kind and are
//! left alone, except that the patrol-army byte of such events is not treated as an army,
//! and that the relative shifts of the "edit event" opcodes 1–5 follow a removed event.

use crate::dt::dtm::Scenario;
use crate::rules::events::{extension, Extension};

/// `old` after removing id `removed`: 0 stays 0, `removed` becomes 0, later ids shift down.
fn shift(old: u32, removed: u32) -> u32 {
    match old {
        0 => 0,
        o if o == removed => 0,
        o if o > removed => o - 1,
        o => o,
    }
}

fn shift_u8(v: &mut u8, removed: u32) {
    *v = shift(*v as u32, removed) as u8;
}

fn shift_u16(v: &mut u16, removed: u32) {
    *v = shift(*v as u32, removed) as u16;
}

/// Removes building `id` (1-based) and remaps: army home buildings, buildings' linked
/// buildings, hero presets' start buildings, event building conditions.
pub fn remove_building(s: &mut Scenario, id: u16) -> bool {
    let Some(i) = (id as usize).checked_sub(1).filter(|i| *i < s.buildings.len()) else { return false };
    s.buildings.remove(i);
    let r = id as u32;
    for a in &mut s.armies {
        shift_u8(&mut a.home_building, r);
    }
    for b in &mut s.buildings {
        shift_u8(&mut b.linked_building, r);
    }
    for h in &mut s.header.heroes {
        shift_u8(&mut h.start_building, r);
    }
    for e in &mut s.events {
        for b in &mut e.conditions.buildings {
            shift_u8(b, r);
        }
    }
    true
}

/// Removes army `id` and renumbers the rest (the army id is its 1-based index). Remaps
/// building owners (1..=254; 0 and 0xFF are kept) and every army reference of the events.
pub fn remove_army(s: &mut Scenario, id: u8) -> bool {
    let Some(i) = (id as usize).checked_sub(1).filter(|i| *i < s.armies.len()) else { return false };
    s.armies.remove(i);
    for (k, a) in s.armies.iter_mut().enumerate() {
        a.id = (k + 1) as u8;
    }
    let r = id as u32;
    for b in &mut s.buildings {
        if b.owner_army != 0 && b.owner_army != 0xFF {
            b.owner_army = match shift(b.owner_army as u32, r) {
                0 => 0xFF,
                n => n as u8,
            };
        }
    }
    for e in &mut s.events {
        let opcode = matches!(extension(e), Some(Extension::Opcode(_)));
        let c = &mut e.conditions;
        for a in c.defeated_armies.iter_mut().chain(c.beaten_armies.iter_mut()) {
            shift_u8(a, r);
        }
        for a in [&mut c.meet_army, &mut c.army_inactive, &mut c.army_active, &mut c.army_at_home] {
            shift_u8(a, r);
        }
        let x = &mut e.results;
        for a in x.activate_armies.iter_mut() {
            shift_u8(a, r);
        }
        for a in [&mut x.deactivate_army, &mut x.removed_units_to_army, &mut x.units_from_army, &mut x.show_army, &mut x.start_battle_with] {
            shift_u8(a, r);
        }
        if !opcode {
            shift_u8(&mut x.patrol_army, r);
        }
    }
    true
}

/// Removes point `id` and renumbers the rest; remaps the lanterns events light.
pub fn remove_point(s: &mut Scenario, id: u8) -> bool {
    let Some(i) = (id as usize).checked_sub(1).filter(|i| *i < s.points.len()) else { return false };
    s.points.remove(i);
    for (k, p) in s.points.iter_mut().enumerate() {
        p.id = (k + 1) as u8;
    }
    for e in &mut s.events {
        for l in e.results.light_lanterns.iter_mut() {
            shift_u16(l, id as u32);
        }
    }
    true
}

/// Removes named character `index` (1-based) and remaps armies' named characters and the
/// named-unit fields of events.
pub fn remove_named_character(s: &mut Scenario, index: u8) -> bool {
    let Some(i) = (index as usize).checked_sub(1).filter(|i| *i < s.named_characters.len()) else { return false };
    s.named_characters.remove(i);
    // Keep the stored slots in step (the writer refreshes the used ones).
    let slots = &mut s.header.named_character_slots;
    slots.copy_within(i + 1.., i);
    slots[31] = 0;
    let r = index as u32;
    for a in &mut s.armies {
        shift_u8(&mut a.named_character, r);
    }
    for e in &mut s.events {
        for n in e.conditions.units_named.iter_mut().chain(e.results.units_add_named.iter_mut()).chain(e.results.units_remove_named.iter_mut()) {
            shift_u8(n, r);
        }
    }
    true
}

/// Removes event `id` (1-based) and remaps every event reference: other events'
/// conditions (happened with yes / no, not happened) and results (relative, quest
/// completed, chained); buildings' and points' local lists (the entry is taken out and the
/// list closes up); the victory and defeat events; and the relative shifts of the
/// Community "edit event" opcodes 1–5 (a shift pointing at the removed event is kept, as
/// nothing can replace it).
pub fn remove_event(s: &mut Scenario, id: u16) -> bool {
    let Some(i) = (id as usize).checked_sub(1).filter(|i| *i < s.events.len()) else { return false };
    // The relative shifts first, while the ids are the old ones.
    let n = s.events.len() as i64;
    for (j, e) in s.events.iter_mut().enumerate() {
        let from = j as i64 + 1;
        if from == id as i64 {
            continue;
        }
        let Some(Extension::Opcode(1..=5)) = extension(e) else { continue };
        let second = (1..=5).contains(&e.conditions.squad_count);
        let new_from = from - (from > id as i64) as i64;
        let fix = |shift: &mut i16| {
            let target = from + *shift as i64;
            if *shift == 0 || target == id as i64 || !(1..=n).contains(&target) {
                // Itself, the removed event or no event: the new position keeps the meaning
                // of "itself"; the others stay as they are.
                return;
            }
            let new_target = target - (target > id as i64) as i64;
            *shift = (new_target - new_from).clamp(i16::MIN as i64, i16::MAX as i64) as i16;
        };
        fix(&mut e.results.experience);
        if second {
            fix(&mut e.conditions.gold);
        }
    }
    s.events.remove(i);
    let r = id as u32;
    for e in &mut s.events {
        let c = &mut e.conditions;
        for x in c.happened_yes.iter_mut().chain(c.happened_no.iter_mut()).chain(c.not_happened.iter_mut()) {
            shift_u16(x, r);
        }
        let x = &mut e.results;
        for v in [&mut x.relative_event, &mut x.completes_quest, &mut x.chained_event] {
            shift_u16(v, r);
        }
    }
    for b in &mut s.buildings {
        remap_list(&mut b.event_slots, &mut b.event_count, id);
    }
    for p in &mut s.points {
        remap_list(&mut p.event_slots, &mut p.event_count, id);
    }
    shift_u16(&mut s.header.victory_event, r);
    shift_u16(&mut s.header.defeat_event, r);
    true
}

/// A local event list after removing event `id`: its entries are taken out, later ids
/// shift down; the used part is the first `count` slots.
fn remap_list(slots: &mut [u16], count: &mut u8, id: u16) {
    let n = (*count as usize).min(slots.len());
    let mut k = 0;
    while k < (*count as usize).min(slots.len()) {
        if slots[k] == id {
            super::records::remove_event(slots, count, k);
        } else {
            k += 1;
        }
    }
    for v in slots[..n].iter_mut() {
        if *v > id {
            *v -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Army, Building, Event, NamedCharacter, Point};

    fn army(id: u8) -> Army {
        Army { id, ..Army::default() }
    }

    fn point(id: u8) -> Point {
        Point { id, model: 9, serial: id as u16, ..Point::default() }
    }

    fn scenario() -> Scenario {
        Scenario {
            buildings: (0..4).map(|_| Building::default()).collect(),
            armies: (1..=4).map(army).collect(),
            points: (1..=3).map(point).collect(),
            events: vec![Event::default()],
            ..Scenario::default()
        }
    }

    #[test]
    fn removing_a_building_remaps_references() {
        let mut s = scenario();
        s.armies[0].home_building = 3;
        s.armies[1].home_building = 2;
        s.armies[2].home_building = 1;
        s.buildings[3].linked_building = 4;
        s.header.heroes[0].start_building = 3;
        s.header.heroes[1].start_building = 2;
        s.events[0].conditions.buildings = [1, 2, 4];
        assert!(remove_building(&mut s, 2));
        assert_eq!(s.buildings.len(), 3);
        assert_eq!([s.armies[0].home_building, s.armies[1].home_building, s.armies[2].home_building], [2, 0, 1]);
        assert_eq!(s.buildings[2].linked_building, 3);
        assert_eq!([s.header.heroes[0].start_building, s.header.heroes[1].start_building], [2, 0]);
        assert_eq!(s.events[0].conditions.buildings, [1, 0, 3]);
        assert!(!remove_building(&mut s, 9));
        assert!(!remove_building(&mut s, 0));
    }

    #[test]
    fn removing_an_army_renumbers_and_remaps() {
        let mut s = scenario();
        s.buildings[0].owner_army = 3;
        s.buildings[1].owner_army = 2;
        s.buildings[2].owner_army = 0xFF;
        s.buildings[3].owner_army = 0;
        let e = &mut s.events[0];
        e.conditions.defeated_armies = [2, 4];
        e.conditions.meet_army = 3;
        e.conditions.army_at_home = 2;
        e.results.activate_armies = [4, 1];
        e.results.start_battle_with = 3;
        e.results.patrol_army = 4;
        assert!(remove_army(&mut s, 2));
        assert_eq!(s.armies.iter().map(|a| a.id).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(s.buildings.iter().map(|b| b.owner_army).collect::<Vec<_>>(), [2, 0xFF, 0xFF, 0]);
        let e = &s.events[0];
        assert_eq!(e.conditions.defeated_armies, [0, 3]);
        assert_eq!((e.conditions.meet_army, e.conditions.army_at_home), (2, 0));
        assert_eq!(e.results.activate_armies, [3, 1]);
        assert_eq!((e.results.start_battle_with, e.results.patrol_army), (2, 3));
    }

    #[test]
    fn opcode_events_keep_their_patrol_byte() {
        let mut s = scenario();
        let e = &mut s.events[0];
        (e.results.no_meeting, e.results.patrol_delta, e.results.patrol_army) = (1, 6, 4);
        remove_army(&mut s, 1);
        assert_eq!(s.events[0].results.patrol_army, 4);
    }

    #[test]
    fn removing_a_point_remaps_lanterns() {
        let mut s = scenario();
        s.events[0].results.light_lanterns = [1, 2, 3, 0];
        assert!(remove_point(&mut s, 2));
        assert_eq!(s.points.iter().map(|p| p.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(s.events[0].results.light_lanterns, [1, 0, 2, 0]);
    }

    #[test]
    fn removing_an_event_remaps_every_reference() {
        let mut s = scenario();
        s.events = vec![Event::default(); 5];
        let e = &mut s.events[0];
        e.conditions.happened_yes = [2, 4];
        e.conditions.happened_no = [5, 3];
        e.conditions.not_happened = [3, 1];
        e.results.relative_event = 4;
        e.results.completes_quest = 3;
        e.results.chained_event = 5;
        s.buildings[0].event_count = 3;
        s.buildings[0].event_slots[..3].copy_from_slice(&[3, 4, 1]);
        s.points[1].event_count = 2;
        s.points[1].event_slots[..2].copy_from_slice(&[5, 3]);
        s.header.victory_event = 5;
        s.header.defeat_event = 3;
        assert!(remove_event(&mut s, 3));
        assert_eq!(s.events.len(), 4);
        let e = &s.events[0];
        assert_eq!((e.conditions.happened_yes, e.conditions.happened_no, e.conditions.not_happened), ([2, 3], [4, 0], [0, 1]));
        assert_eq!((e.results.relative_event, e.results.completes_quest, e.results.chained_event), (3, 0, 4));
        assert_eq!((&s.buildings[0].event_slots[..3], s.buildings[0].event_count), (&[3, 1, 0][..], 2));
        assert_eq!((&s.points[1].event_slots[..2], s.points[1].event_count), (&[4, 0][..], 1));
        assert_eq!((s.header.victory_event, s.header.defeat_event), (4, 0));
        assert!(!remove_event(&mut s, 0));
        assert!(!remove_event(&mut s, 5));
    }

    #[test]
    fn removing_an_event_keeps_opcode_targets() {
        use crate::editor::events::{set_opcode, set_opcode_args};
        let mut s = scenario();
        s.events = vec![Event::default(); 6];
        // Event 2 edits event 5 (+3); event 6 edits event 1 (-5) and, in its second
        // setting, event 4 (-2); event 5 edits itself (0); event 1 edits event 3 (+2), the
        // one removed.
        for (i, shift) in [(1, 3), (5, -5), (4, 0), (0, 2)] {
            set_opcode(&mut s.events[i], Some(1));
            set_opcode_args(&mut s.events[i], [shift, 85, 1]);
        }
        s.events[5].conditions.squad_count = 2;
        s.events[5].conditions.gold = -2;
        assert!(remove_event(&mut s, 3));
        assert_eq!(s.events[1].results.experience, 2, "2 -> 4 (was 5)");
        assert_eq!(s.events[4].results.experience, -4, "5 -> 1");
        assert_eq!(s.events[4].conditions.gold, -2, "both after the removed one: the distance stays");
        assert_eq!(s.events[3].results.experience, 0);
        assert_eq!(s.events[0].results.experience, 2, "a shift to the removed event stays");
    }

    #[test]
    fn removing_a_named_character_remaps() {
        let mut s = scenario();
        s.named_characters = (0..3).map(|k| NamedCharacter { unit: 10 + k, name: format!("n{k}") }).collect();
        s.header.named_character_slots[..3].copy_from_slice(&[10, 11, 12]);
        s.armies[0].named_character = 3;
        s.armies[1].named_character = 1;
        s.events[0].results.units_add_named = [1, 2, 3, 0];
        assert!(remove_named_character(&mut s, 2));
        assert_eq!(s.named_characters.iter().map(|n| n.unit).collect::<Vec<_>>(), [10, 12]);
        assert_eq!(&s.header.named_character_slots[..3], &[10, 12, 0]);
        assert_eq!((s.armies[0].named_character, s.armies[1].named_character), (2, 1));
        assert_eq!(s.events[0].results.units_add_named, [1, 0, 2, 0]);
    }
}
