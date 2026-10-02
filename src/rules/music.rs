//! The map music's draws from the game's generator (interface.md §13, engine.md §9). The
//! original picks the world's tracks with its one generator, so every change of track shifts
//! the rolls that follow; Razdor draws them the same way. Which track plays and when is the
//! interface's (`ui::jukebox`); these are only the draws.

use super::rng::Rng;

/// The tracks of the map rotation by the original's pick number: the seven map themes and,
/// at 3, the credits theme.
pub const ROTATION: [&str; 8] = ["BkgMap1", "BkgMap2", "BkgMap3", "BkgAuthors", "BkgMap4", "BkgMap5", "BkgMap6", "BkgMap7"];

/// The pick a map start or a load sets: the world theme, `BkgMap2`.
pub const WORLD_THEME: usize = 1;

/// The milliseconds until the next change after pick `pick` (0x49d7f8): a base and a draw,
/// 80 s for `BkgMap5` with no draw. The draws are `Random(50000)`, `Random(90000)` and
/// `Random(60000)`, which never pass 32767 (the original's 15-bit generator).
pub fn next_change(pick: usize, rng: &mut Rng) -> u32 {
    let (base, draw) = match pick {
        0 | 2 | 7 => (40_000, 50_000),
        1 | 3 | 4 => (90_000, 90_000),
        6 => (60_000, 60_000),
        _ => (80_000, 0),
    };
    base + if draw > 0 { rng.random(draw) as u32 } else { 0 }
}

/// A change of track when it is due (0x49d7f8): `Random(8)` drawn again until it differs from
/// the last pick, then the time to the next change. Returns the pick and that time (ms).
pub fn rotate(last: usize, rng: &mut Rng) -> (usize, u32) {
    let pick = loop {
        let k = rng.random(8) as usize;
        if k != last {
            break k;
        }
    };
    (pick, next_change(pick, rng))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_redraws_the_last_pick_and_draws_its_time() {
        // The stream from 1 gives 41, 18467, 6334: Random(8) is 1, then 3.
        let mut r = Rng::new(1);
        let (pick, ms) = rotate(1, &mut r);
        assert_eq!(pick, 3, "the 1 equals the last pick and is drawn again: the credits theme");
        assert_eq!(ms, 90_000 + 6334);
        let mut r = Rng::new(1);
        assert_eq!(rotate(0, &mut r), (1, 90_000 + 18467), "BkgMap2 draws from 90 000");
    }

    #[test]
    fn the_times_by_pick() {
        for (pick, lo, hi) in [(0, 40_000, 72_767), (1, 90_000, 122_767), (2, 40_000, 72_767), (3, 90_000, 122_767), (4, 90_000, 122_767), (5, 80_000, 80_000), (6, 60_000, 92_767), (7, 40_000, 72_767)] {
            let mut r = Rng::new(7);
            for _ in 0..200 {
                let ms = next_change(pick, &mut r);
                assert!((lo..=hi).contains(&ms), "{pick}: {ms}");
            }
        }
        let mut r = Rng::new(5);
        next_change(5, &mut r);
        assert_eq!(r.state(), 5, "BkgMap5 draws nothing");
        assert_eq!(ROTATION[3], "BkgAuthors");
    }
}
