/// Small deterministic xorshift RNG so battles are reproducible in tests.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform integer in `lo..=hi`.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo + 1) as u64) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_stays_in_bounds() {
        let mut r = Rng::new(42);
        for _ in 0..1000 {
            let v = r.range(3, 7);
            assert!((3..=7).contains(&v));
        }
    }
}
