//! The sample subject for the `example_*` benchmarks: a small optimization, with the old and
//! new versions side by side so one executable can measure both.
//!
//! A drafting AI looks at the mana curve of a pool over and over. [`mana_curve_old`] counts
//! into a heap-allocated `Vec`; [`mana_curve_new`] into an array on the stack. Same answer,
//! one allocation fewer per call.

use crate::rng::SplitMix64;

/// Benchmarks seed their inputs with this, so every run measures the same pools.
pub const SEED: u64 = 0xBE41_2026;
/// Cards in a cube draft pool: 3 packs of 15.
pub const POOL_CARDS: usize = 45;
/// Pools per measured call, so one call covers varied inputs.
pub const POOLS: usize = 64;

/// Cards with mana value 0, 1, ... 6, and 7 or more.
pub type Curve = [u32; 8];

pub fn mana_curve_old(mana_values: &[u8]) -> Curve {
    let mut counts = vec![0_u32; 8];
    for &mv in mana_values {
        counts[usize::from(mv.min(7))] += 1;
    }
    counts.try_into().expect("eight buckets")
}

pub fn mana_curve_new(mana_values: &[u8]) -> Curve {
    let mut counts = [0_u32; 8];
    for &mv in mana_values {
        counts[usize::from(mv.min(7))] += 1;
    }
    counts
}

/// Runs `curve` over every pool and folds the results into one number, which the benchmark
/// returns so the compiler can't skip the work.
pub fn over_pools(pools: &[Vec<u8>], curve: impl Fn(&[u8]) -> Curve) -> u32 {
    pools
        .iter()
        .map(|pool| {
            curve(pool)
                .iter()
                .zip(1..)
                .map(|(n, weight)| n * weight)
                .sum::<u32>()
        })
        .fold(0, u32::wrapping_add)
}

/// `count` pools of mana values, shaped roughly like a cube's curve.
pub fn sample_pools(seed: u64, count: usize) -> Vec<Vec<u8>> {
    // Relative frequency of mana values 0..=8.
    const WEIGHTS: [u64; 9] = [4, 16, 24, 22, 15, 9, 5, 3, 2];
    let total: u64 = WEIGHTS.iter().sum();
    let mut rng = SplitMix64::new(seed);
    let mut card = move || {
        let mut roll = rng.below(total);
        let mut mv = 0;
        while roll >= WEIGHTS[mv] {
            roll -= WEIGHTS[mv];
            mv += 1;
        }
        mv as u8
    };
    (0..count)
        .map(|_| (0..POOL_CARDS).map(|_| card()).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{POOLS, SEED, mana_curve_new, mana_curve_old, over_pools, sample_pools};

    #[test]
    fn old_and_new_agree() {
        let pools = sample_pools(SEED, POOLS);
        for pool in &pools {
            assert_eq!(mana_curve_old(pool), mana_curve_new(pool));
        }
    }

    #[test]
    fn inputs_never_change() {
        // If this fails, every saved baseline is measuring different inputs: re-baseline.
        let pools = sample_pools(SEED, POOLS);
        assert_eq!(over_pools(&pools, mana_curve_new), 11_586);
        assert_eq!(pools[0][..8], [4, 8, 4, 3, 0, 3, 4, 4]);
    }
}
