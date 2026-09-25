//! Small edits inside a record that the property panels share: slot lists with a count
//! (local events of buildings and points) and the market goods.

use crate::dt::dtm::{Army, Building};
use crate::rules::content::{Content, UnitId};

/// Appends event `id` to a slot list whose first `count` slots are used. False if full or
/// already listed.
pub fn add_event(slots: &mut [u16], count: &mut u8, id: u16) -> bool {
    let n = (*count as usize).min(slots.len());
    if id == 0 || n >= slots.len() || slots[..n].contains(&id) {
        return false;
    }
    slots[n] = id;
    *count = n as u8 + 1;
    true
}

/// Removes the event in used slot `index`, closing the gap.
pub fn remove_event(slots: &mut [u16], count: &mut u8, index: usize) -> bool {
    let n = (*count as usize).min(slots.len());
    if index >= n {
        return false;
    }
    slots.copy_within(index + 1..n, index);
    slots[n - 1] = 0;
    *count = n as u8 - 1;
    true
}

/// The used event ids of a slot list, deleted (0) slots included, in order.
pub fn used_events(slots: &[u16], count: u8) -> &[u16] {
    &slots[..(count as usize).min(slots.len())]
}

/// Market goods / ruin treasure: the first six artefact slots.
pub const GOODS: usize = 6;

/// Sets goods slot `k` and keeps the byte copy at offset 296 in step (the original editor
/// writes one; the game's use of it is unknown).
pub fn set_goods(b: &mut Building, k: usize, artefact: u16) {
    if k < GOODS {
        b.artifact_slots[k] = artefact;
        b.stale_artifacts[k] = artefact as u8;
    }
}

/// The army's strength as the game sums it: the tactical cost of its leader and of every
/// unit of its troops at their levels (`docs/reference/original-mechanics/experience.md` §1).
/// Unknown units count 0.
pub fn army_strength(a: &Army, content: &Content) -> i64 {
    let cost = |unit: u8, level: u8| {
        let id = UnitId(unit as u32);
        if unit == 0 || content.try_unit(id).is_none() {
            0
        } else {
            content.tactical_cost(id, level as i32 + 1) as i64
        }
    };
    let troops: i64 = a.troops.iter().map(|t| cost(t.unit, t.level) * t.count as i64).sum();
    cost(a.leader_unit, a.leader_level) + troops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_slot_lists() {
        let mut slots = [0u16; 5];
        let mut n = 0;
        assert!(add_event(&mut slots, &mut n, 7));
        assert!(add_event(&mut slots, &mut n, 3));
        assert!(!add_event(&mut slots, &mut n, 7), "no duplicates");
        assert!(!add_event(&mut slots, &mut n, 0));
        assert!(add_event(&mut slots, &mut n, 9));
        assert_eq!((used_events(&slots, n), n), (&[7, 3, 9][..], 3));
        assert!(remove_event(&mut slots, &mut n, 0));
        assert_eq!((slots, n), ([3, 9, 0, 0, 0], 2));
        assert!(!remove_event(&mut slots, &mut n, 2));
        for id in 10..13 {
            add_event(&mut slots, &mut n, id);
        }
        assert!(!add_event(&mut slots, &mut n, 99), "full");
        assert_eq!(n, 5);
    }

    #[test]
    fn goods_keep_the_byte_copy() {
        let mut b = Building::default();
        set_goods(&mut b, 2, 300);
        assert_eq!((b.artifact_slots[2], b.stale_artifacts[2]), (300, 44));
        set_goods(&mut b, 6, 1);
        assert_eq!(b.artifact_slots[6], 0);
    }

    #[test]
    fn strength_sums_tactical_costs() {
        let c = Content::builtin();
        let mut a = Army { leader_unit: 1, ..Army::default() };
        let leader = army_strength(&a, &c);
        assert!(leader > 0);
        a.troops[0] = crate::dt::dtm::Troop { unit: 1, level: 0, count: 2 };
        assert_eq!(army_strength(&a, &c), 3 * leader);
        a.troops[1] = crate::dt::dtm::Troop { unit: 250, level: 0, count: 2 };
        assert_eq!(army_strength(&a, &c), 3 * leader);
    }
}
