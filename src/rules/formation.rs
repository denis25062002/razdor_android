//! The 2×6 card formation each side fights in.

pub const COLS: u8 = 6;
/// Column fill order when auto-placing: centre first.
const COL_ORDER: [u8; COLS as usize] = [2, 3, 1, 4, 0, 5];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Row {
    Front,
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Slot {
    pub row: Row,
    pub col: u8,
}

impl Slot {
    pub const fn new(row: Row, col: u8) -> Self {
        Slot { row, col }
    }

    pub fn all() -> impl Iterator<Item = Slot> {
        [Row::Front, Row::Back]
            .into_iter()
            .flat_map(|row| (0..COLS).map(move |col| Slot { row, col }))
    }
}

/// First free cell, trying the preferred row before the other one.
pub fn free_slot(occupied: &[Slot], preferred: Row) -> Option<Slot> {
    let other = if preferred == Row::Front { Row::Back } else { Row::Front };
    [preferred, other]
        .into_iter()
        .flat_map(|row| COL_ORDER.iter().map(move |&col| Slot { row, col }))
        .find(|s| !occupied.contains(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_slot_prefers_row_and_centre() {
        assert_eq!(free_slot(&[], Row::Back), Some(Slot::new(Row::Back, 2)));
        let full_front: Vec<_> = (0..COLS).map(|c| Slot::new(Row::Front, c)).collect();
        assert_eq!(free_slot(&full_front, Row::Front), Some(Slot::new(Row::Back, 2)));
        let all: Vec<_> = Slot::all().collect();
        assert_eq!(all.len(), 12);
        assert_eq!(free_slot(&all, Row::Front), None);
    }
}
