//! The original's random numbers (engine.md §3).
//!
//! [`Rng`] is the game's one generator: the C runtime's `rand()` stream (`S × 214013 +
//! 2531011`, 15 bits out). It is not saved: a new map sets it to 1 (so the markets of a fresh
//! map are always the same), and a loaded game starts from what the load sequence leaves
//! ([`Rng::jitter_plants`], one draw per army, the world music's draw; `rules::save`).
//! [`EventRng`] is the Community event generator, seeded from the clock.

/// The game's generator: one 32-bit state, `0` when the program starts.
#[derive(Clone, Debug, Default)]
pub struct Rng(u32);

/// The draw the world music makes when a map's world screen starts (engine.md §3.2, §9):
/// the time to the next track change, `Random(90000)`. The music itself is the interface's.
pub const WORLD_MUSIC_DRAW: i32 = 90_000;
/// The idle-animation offset each AI army draws at a map or save load (ms).
pub const ARMY_IDLE_DRAW: i32 = 3000;

impl Rng {
    /// A generator whose state is `state` (the original sets 1 at every map load).
    pub fn new(state: u32) -> Self {
        Rng(state)
    }

    /// The state as a map load leaves it, before the markets are stocked.
    pub fn map_load() -> Self {
        Rng(1)
    }

    pub fn state(&self) -> u32 {
        self.0
    }

    /// The original's `Random(n)`: the state always steps, even for `n = 0` (which gives 0);
    /// then the top 15 bits mod `n`. So `n` above 32768 never gives more than 32767, and a
    /// negative `n` acts as `|n|` (the original's behaviour, kept).
    pub fn random(&mut self, n: i32) -> i32 {
        self.0 = self.0.wrapping_mul(214_013).wrapping_add(2_531_011);
        if n == 0 {
            return 0;
        }
        ((self.0 >> 16) & 0x7fff) as i32 % n
    }

    /// `lo..=hi` from one `Random(hi − lo + 1)`, for Razdor's own rolls (rules the original
    /// does not have); one draw even when the range is empty, which gives `lo`.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let n = (hi as i64 - lo as i64 + 1).clamp(0, i32::MAX as i64) as i32;
        lo.saturating_add(self.random(n))
    }

    /// The plant jitter of a map or save load (engine.md §3.3): for every cell in row-major
    /// order whose object word (`cells`, `w` per row, class in the high byte) is a plant
    /// (classes 9–11), the state is set from the cell's hash, then the x offset, the y offset
    /// and the sway phase are drawn. Only the stream is kept (the offsets are not drawn on the
    /// map yet). Returns whether any plant re-seeded it.
    pub fn jitter_plants(&mut self, w: i32, cells: &[u16]) -> bool {
        let mut any = false;
        for (l, &word) in cells.iter().enumerate() {
            if !(9..=11).contains(&(word >> 8)) {
                continue;
            }
            let (x, y) = (l as i32 % w.max(1), l as i32 / w.max(1));
            self.0 = plant_hash(x, y, word);
            // x offset: 4 + Random(16) on odd rows, 28 − Random(16) on even ones; then y
            // offset Random(11), sway phase Random(1000).
            self.random(16);
            self.random(11);
            self.random(1000);
            any = true;
        }
        any
    }

    /// The generator after a save load (engine.md §3.2, 0x4b771c): it is not saved, so the
    /// plant jitter re-seeds it from the map's last plant, every army of the map file draws
    /// its idle offset, and the world music draws. A map without plants keeps the state the
    /// program had *(guess: 0, the state at start; the original keeps the last session's)*.
    pub fn save_load(w: i32, plants: &[u16], armies: usize) -> Rng {
        let mut r = Rng::default();
        r.jitter_plants(w, plants);
        for _ in 0..armies {
            r.random(ARMY_IDLE_DRAW);
        }
        r.random(WORLD_MUSIC_DRAW);
        r
    }
}

/// The seed of a plant cell: `Trunc(sin(800y + x) × 10⁶ + cos(600x + y) × 10⁴ + word)`, its
/// low 32 bits. The original evaluates it with the x87 `fsin`/`fcos`; this takes the f64
/// ones *(guess: the FPU precision the game runs with is unknown, engine.md §11)*.
pub fn plant_hash(x: i32, y: i32, word: u16) -> u32 {
    let (x, y) = (x as f64, y as f64);
    let v = (800.0 * y + x).sin() * 1e6 + (600.0 * x + y).cos() * 1e4 + word as f64;
    v.trunc() as i64 as u32
}

/// The cells' plant layer of a map: the object word (`class << 8 | sprite`) of the last
/// object of class 0 or 9 and above on each cell, objects at `y × w + x` in file order (an
/// object past the row's end lands on the next row, as in the original).
pub fn plant_layer(w: i32, h: i32, objects: impl IntoIterator<Item = (i32, i32, u8, u8)>) -> Vec<u16> {
    let mut cells = vec![0u16; (w.max(0) * h.max(0)) as usize];
    for (x, y, class, sprite) in objects {
        if (1..=8).contains(&class) || x < 0 || y < 0 {
            continue;
        }
        if let Some(c) = cells.get_mut((y * w + x) as usize) {
            *c = (class as u16) << 8 | sprite as u16;
        }
    }
    cells
}

/// The Community event generator (engine.md §3.5): its own 32-bit state, seeded from the CPU
/// clock whenever a map or a save is loaded, never saved. Used by event opcode 18 only.
#[derive(Clone, Debug, Default)]
pub struct EventRng(u32);

impl EventRng {
    pub fn new(state: u32) -> Self {
        EventRng(state)
    }

    /// Seeded from the clock, as the original does from the time-stamp counter.
    pub fn from_clock() -> Self {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        EventRng(t as u32 ^ (t >> 32) as u32)
    }

    fn step(&mut self) -> u32 {
        // Numerical Recipes' constants, typed as hexadecimal in the original.
        self.0 = self.0.wrapping_mul(0x0166_4525).wrapping_add(0x1390_4223);
        self.0
    }

    /// `lo..=hi`: draws until the state falls below the largest multiple of `n = hi − lo + 1`
    /// (unsigned), then `lo + state mod n`. The original's retry loop jumps back one step too
    /// far, so after a rejection the limit becomes the rejected state × n (kept). A range of
    /// 2³² values (n = 0) divides by zero there; this returns `lo` *(guess)*.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let n = (hi.wrapping_sub(lo) as u32).wrapping_add(1);
        if n == 0 {
            return lo;
        }
        let mut q = u32::MAX / n;
        loop {
            let limit = q.wrapping_mul(n);
            let s = self.step();
            if s < limit {
                return lo.wrapping_add((s % n) as i32);
            }
            q = s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(r: &mut Rng, k: usize) -> Vec<i32> {
        (0..k).map(|_| r.random(32768)).collect()
    }

    #[test]
    fn the_stream_is_the_c_runtimes_rand() {
        assert_eq!(raw(&mut Rng::new(1), 8), [41, 18467, 6334, 26500, 19169, 15724, 11478, 29358]);
        assert_eq!(raw(&mut Rng::default(), 5), [38, 7719, 21238, 2437, 8855], "0 at program start");
        assert_eq!(Rng::map_load().state(), 1);
    }

    #[test]
    fn random_n_is_the_15_bits_mod_n_and_always_draws() {
        let mut r = Rng::new(1);
        assert_eq!(r.random(10), 1, "41 mod 10");
        assert_eq!(r.random(0), 0, "Random(0) is 0 but steps the state");
        assert_eq!(r.random(1), 0);
        assert_eq!(r.random(100), 0, "26500 mod 100");
        // Ranges above 32768 never reach past 32767 (the music's 50 000 – 90 000).
        let mut r = Rng::new(1);
        assert_eq!(raw(&mut r.clone(), 8), (0..8).map(|_| r.random(90_000)).collect::<Vec<_>>());
        // A negative n acts as its absolute value.
        assert_eq!(Rng::new(1).random(-10), 1);
    }

    #[test]
    fn range_is_one_random_draw() {
        let mut r = Rng::new(1);
        assert_eq!(r.range(3, 7), 3 + 41 % 5);
        assert_eq!(r.range(5, 5), 5);
        assert_eq!(r.range(6, 2), 6, "an empty range gives lo");
        assert_eq!(r.state(), Rng::new(1).tap(3).state(), "three draws");
        for _ in 0..1000 {
            assert!((-4..=4).contains(&r.range(-4, 4)));
        }
    }

    impl Rng {
        fn tap(mut self, k: usize) -> Rng {
            (0..k).for_each(|_| {
                self.random(0);
            });
            self
        }
    }

    #[test]
    fn plants_reseed_the_stream_from_their_cell() {
        // Cell (0, 0): sin 0 = 0, cos 0 = 1, so the seed is 10 000 + the word.
        let tree = 9 << 8 | 3;
        assert_eq!(plant_hash(0, 0, tree), 10_000 + tree as u32);
        // Cell (0, 2): sin 1600 × 10⁶ + cos 2 × 10⁴ + 2 = −805 384.26; the cut is toward
        // zero, and S keeps the low 32 bits.
        assert_eq!(plant_hash(0, 2, 2), (-805_384i32) as u32);
        // Three draws after the last plant's seed; the cells before it do not matter.
        let cells = plant_layer(3, 2, [(1, 0, 10, 5), (2, 1, 9, 7), (0, 1, 3, 1)]);
        assert_eq!(cells, [0, 10 << 8 | 5, 0, 0, 0, 9 << 8 | 7]);
        let mut r = Rng::new(77);
        assert!(r.jitter_plants(3, &cells));
        assert_eq!(r.state(), Rng::new(plant_hash(2, 1, 9 << 8 | 7)).tap(3).state());
        // No plant (a rock of class 12 or a massif): the state is left alone.
        let mut r = Rng::new(77);
        assert!(!r.jitter_plants(3, &plant_layer(3, 2, [(0, 0, 12, 1), (1, 1, 5, 0)])));
        assert_eq!(r.state(), 77);
    }

    #[test]
    fn a_save_load_starts_the_stream_from_the_last_plant() {
        let cells = plant_layer(4, 3, [(1, 0, 9, 2), (3, 1, 11, 4), (0, 2, 5, 1)]);
        // The last plant's seed, its three jitter draws, one draw per army, the music's.
        let r = Rng::save_load(4, &cells, 2);
        assert_eq!(r.state(), Rng::new(plant_hash(3, 1, 11 << 8 | 4)).tap(3 + 2 + 1).state());
        assert_eq!(Rng::save_load(4, &cells, 2).state(), r.state(), "the same save replays the same stream");
        assert_eq!(Rng::save_load(4, &[0; 12], 0).state(), Rng::default().tap(1).state(), "no plants, no armies");
    }

    #[test]
    fn the_plant_layer_keeps_the_last_object_and_wraps_past_the_row() {
        let cells = plant_layer(2, 2, [(0, 0, 9, 1), (0, 0, 11, 2), (0, 0, 4, 9), (2, 0, 10, 3)]);
        assert_eq!(cells, [11 << 8 | 2, 0, 10 << 8 | 3, 0], "a massif keeps its own layer; x = 2 is the next row");
    }

    #[test]
    fn the_event_generator_and_its_retry_slip() {
        let step = |s: u32| s.wrapping_mul(0x0166_4525).wrapping_add(0x1390_4223);
        let mut e = EventRng::new(5);
        let s1 = step(5);
        assert_eq!(e.range(10, 19), 10 + (s1 % 10) as i32);
        // n = 2³¹ + 1: the limit is n itself, so a state at or above it is rejected and the
        // next limit is that state × n.
        // A state that the true limit would reject again but the slipped one accepts.
        let n = 0x8000_0001u32;
        let seed = (0u32..)
            .find(|&s| {
                let (r, t) = (step(s), step(step(s)));
                r >= n && t >= n && t < r.wrapping_mul(n)
            })
            .unwrap();
        let mut e = EventRng::new(seed);
        assert_eq!(e.range(0, i32::MIN), (step(step(seed)) - n) as i32);
        assert_eq!(e.0, step(step(seed)), "two draws, not three");
        assert_eq!(EventRng::new(9).range(4, 3), 4, "n = 0: lo");
    }
}
