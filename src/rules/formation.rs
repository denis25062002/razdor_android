//! The card formation each side fights in (mechanics.md 2.1).
//!
//! Rows × columns are configurable: the Community Update's "wide row" is 2 rows of 6 (the
//! default, as in the player's install and the gameplay video); vanilla is 3 rows of 4, the
//! third being the reserve. Both hold 12 units.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Row {
    /// Row 1: fights in melee, screens the back row.
    Front,
    /// Row 2: shooters and mages; +`Row2Def` defence against shots.
    Back,
    /// Row 3 (vanilla only): cannot act except to move out, cannot be targeted.
    Reserve,
}

impl Row {
    /// 1-based row number as the original counts it (front = 1).
    pub fn number(self) -> i32 {
        match self {
            Row::Front => 1,
            Row::Back => 2,
            Row::Reserve => 3,
        }
    }

    /// Front or back: the rows that fight.
    pub fn is_active(self) -> bool {
        self != Row::Reserve
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Slot {
    pub row: Row,
    pub col: u8,
}

impl Slot {
    pub const fn new(row: Row, col: u8) -> Self {
        Slot { row, col }
    }
}

/// Shape of a side's formation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Formation {
    pub cols: u8,
    /// A third, reserve row behind the back row.
    pub reserve: bool,
}

impl Default for Formation {
    fn default() -> Self {
        Formation::WIDE
    }
}

impl Formation {
    /// Community "wide front row": 2 × 6, no reserve.
    pub const WIDE: Formation = Formation { cols: 6, reserve: false };
    /// Vanilla: 3 × 4, the third row being the reserve.
    pub const VANILLA: Formation = Formation { cols: 4, reserve: true };

    pub fn rows(&self) -> &'static [Row] {
        if self.reserve {
            &[Row::Front, Row::Back, Row::Reserve]
        } else {
            &[Row::Front, Row::Back]
        }
    }

    pub fn slots(&self) -> impl Iterator<Item = Slot> {
        let cols = self.cols;
        self.rows().iter().flat_map(move |&row| (0..cols).map(move |col| Slot { row, col }))
    }

    pub fn contains(&self, s: Slot) -> bool {
        s.col < self.cols && self.rows().contains(&s.row)
    }

    /// Units a side can field (12 for both shapes).
    pub fn capacity(&self) -> usize {
        self.rows().len() * self.cols as usize
    }

    /// Columns from the centre outwards, as the original auto-places
    /// (3,2,4,1 for 4 columns and 4,3,5,2,6,1 for 6, 1-based).
    pub fn col_order(&self) -> Vec<u8> {
        let n = self.cols as i32;
        let mut out = Vec::with_capacity(n as usize);
        let mid = n / 2;
        out.push(mid);
        for d in 1..=n {
            for c in [mid - d, mid + d] {
                if (0..n).contains(&c) && out.len() < n as usize {
                    out.push(c);
                }
            }
        }
        out.into_iter().map(|c| c as u8).collect()
    }

    /// First free cell: the preferred row first, then the other fighting row, then the reserve.
    pub fn free_slot(&self, occupied: &[Slot], preferred: Row) -> Option<Slot> {
        let mut rows = vec![preferred];
        rows.extend(self.rows().iter().copied().filter(|r| *r != preferred));
        let order = self.col_order();
        rows.into_iter()
            .filter(|r| self.rows().contains(r))
            .flat_map(|row| order.iter().map(move |&col| Slot { row, col }))
            .find(|s| !occupied.contains(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_order_is_centre_out() {
        assert_eq!(Formation::WIDE.col_order(), vec![3, 2, 4, 1, 5, 0]);
        assert_eq!(Formation::VANILLA.col_order(), vec![2, 1, 3, 0]);
    }

    #[test]
    fn both_shapes_hold_twelve() {
        assert_eq!(Formation::WIDE.capacity(), 12);
        assert_eq!(Formation::VANILLA.capacity(), 12);
        assert_eq!(Formation::WIDE.slots().count(), 12);
        assert!(!Formation::WIDE.contains(Slot::new(Row::Reserve, 0)));
        assert!(Formation::VANILLA.contains(Slot::new(Row::Reserve, 3)));
        assert!(!Formation::VANILLA.contains(Slot::new(Row::Front, 4)));
    }

    #[test]
    fn free_slot_prefers_row_and_centre() {
        let f = Formation::WIDE;
        assert_eq!(f.free_slot(&[], Row::Back), Some(Slot::new(Row::Back, 3)));
        let full_front: Vec<_> = (0..6).map(|c| Slot::new(Row::Front, c)).collect();
        assert_eq!(f.free_slot(&full_front, Row::Front), Some(Slot::new(Row::Back, 3)));
        let all: Vec<_> = f.slots().collect();
        assert_eq!(f.free_slot(&all, Row::Front), None);
        let v = Formation::VANILLA;
        let active: Vec<_> = v.slots().filter(|s| s.row.is_active()).collect();
        assert_eq!(v.free_slot(&active, Row::Front), Some(Slot::new(Row::Reserve, 2)));
    }
}
