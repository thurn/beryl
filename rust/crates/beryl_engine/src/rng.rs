//! The game's randomness: two `SplitMix64` streams inside `State` (architecture §14).

/// `SplitMix64`: tiny, fast, and its output for a seed never changes, so a game is a pure
/// function of its decks, seed and choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    /// Shuffles `items` uniformly (Fisher-Yates).
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

/// Two streams, so shuffle outcomes stay stable across replays whose other random events
/// differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rng {
    /// Shuffles and the deck slot assignment.
    pub shuffle: SplitMix64,
    /// Everything else: coin flips, random discards, random bottom orders.
    pub other: SplitMix64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut seeder = SplitMix64::new(seed);
        Self {
            shuffle: SplitMix64::new(seeder.next_u64()),
            other: SplitMix64::new(seeder.next_u64()),
        }
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
    fn shuffle_is_a_permutation() {
        let mut items: Vec<u8> = (0..40).collect();
        SplitMix64::new(3).shuffle(&mut items);
        assert_ne!(items, (0..40).collect::<Vec<_>>());
        items.sort_unstable();
        assert_eq!(items, (0..40).collect::<Vec<_>>());
    }
}
