//! Benchmark inputs come from a fixed seed through a fixed algorithm. `rand`'s `StdRng` and
//! `thread_rng` don't qualify: their algorithm may change between releases, and a dependency
//! bump would quietly change what every benchmark measures.

/// `SplitMix64`: tiny, fast, and its output for a seed never changes.
#[derive(Clone, Debug)]
pub struct SplitMix64(u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n`, by multiply-shift (biased by at most `n / 2^64`).
    pub fn below(&mut self, n: u64) -> u64 {
        ((u128::from(self.next_u64()) * u128::from(n)) >> 64) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::SplitMix64;

    #[test]
    fn output_matches_the_reference_implementation() {
        let mut rng = SplitMix64::new(0);
        assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
    }

    #[test]
    fn below_stays_in_range() {
        let mut rng = SplitMix64::new(7);
        assert!((0..1000).all(|_| rng.below(6) < 6));
    }
}
