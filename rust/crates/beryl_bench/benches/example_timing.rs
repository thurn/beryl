//! Wall-clock timing with Criterion: absolute times per call, and regressions against a
//! saved baseline. `just bench -p beryl_bench --bench example_timing`.
//!
//! Criterion times `old` to completion and then `new`, so drift between the two lands on
//! one side; `example_abba` is the better old-versus-new comparison.

use std::hint::black_box;
use std::time::Duration;

use beryl_bench::{cores, example};
use criterion::{Criterion, criterion_group};

fn mana_curve(c: &mut Criterion) {
    let pools = example::sample_pools(example::SEED, example::POOLS);
    let mut group = c.benchmark_group("mana_curve");
    group.bench_function("old", |b| {
        b.iter(|| example::over_pools(black_box(&pools), example::mana_curve_old));
    });
    group.bench_function("new", |b| {
        b.iter(|| example::over_pools(black_box(&pools), example::mana_curve_new));
    });
    group.finish();
}

/// Longer and stricter than Criterion's defaults (3 s warm-up, 5 s measurement, 100 samples,
/// changes under 1% called noise, 5% significance).
fn config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(5))
        .measurement_time(Duration::from_secs(20))
        .sample_size(200)
        .noise_threshold(0.02)
        .significance_level(0.01)
}

criterion_group! {
    name = benches;
    config = config();
    targets = mana_curve
}

// `criterion_main!`, plus a request for the fastest cores before anything is timed.
fn main() {
    cores::prefer_fastest_cores();
    benches();
    Criterion::default().configure_from_args().final_summary();
}
