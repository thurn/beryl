//! Wall-clock timing of the cube rollout benchmark (`beryl_bench::rollout`) with Criterion:
//! `just bench -p beryl_bench --bench rollout_timing`, which builds the `search` profile.
//! Throughputs are per copy, per decision, per sampled decision and per rollout, so
//! `rollouts` reads as rollouts per core-second.

use std::hint::black_box;
use std::time::Duration;

use beryl_bench::cores;
use beryl_bench::rollout::{self, Workload};
use criterion::{Criterion, Throughput, criterion_group};

fn cube_rollout(c: &mut Criterion) {
    let w = Workload::new();
    let summary = w.summary;
    println!(
        "cube rollout: {} positions, {} rollouts, {} decisions ({} sampled for the policy); \
         {:.1} decisions per rollout, {:.2} per turn, {:.2} inline questions per decision",
        w.starts.len() as u64 / rollout::ROLLOUTS,
        summary.rollouts,
        summary.decisions,
        w.samples.len(),
        summary.decisions_per_rollout(),
        summary.decisions_per_turn(),
        summary.inline as f64 / summary.decisions as f64,
    );
    let (mut x, mut s) = w.memory();
    let mut group = c.benchmark_group("cube_rollout");
    group.throughput(Throughput::Elements(w.starts.len() as u64));
    group.bench_function("copy", |b| {
        b.iter(|| rollout::copy_states(black_box(&w.starts), &mut s));
    });
    group.throughput(Throughput::Elements(summary.decisions));
    group.bench_function("next_apply", |b| {
        b.iter(|| rollout::replay(&w.table, black_box(&w.scripts), &mut s, &mut x));
    });
    group.throughput(Throughput::Elements(w.samples.len() as u64));
    group.bench_function("policy", |b| {
        b.iter(|| rollout::decide(&w.table, black_box(&w.samples), &w.policy));
    });
    group.throughput(Throughput::Elements(summary.rollouts));
    group.bench_function("rollouts", |b| {
        b.iter(|| rollout::roll(&w.table, black_box(&w.starts), &w.policy, &mut s, &mut x));
    });
    group.finish();
}

/// As `example_timing`: longer and stricter than Criterion's defaults.
fn config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(5))
        .measurement_time(Duration::from_secs(20))
        .sample_size(100)
        .noise_threshold(0.02)
        .significance_level(0.01)
}

criterion_group! {
    name = benches;
    config = config();
    targets = cube_rollout
}

// `criterion_main!`, plus a request for the fastest cores before anything is timed.
fn main() {
    cores::prefer_fastest_cores();
    benches();
    Criterion::default().configure_from_args().final_summary();
}
