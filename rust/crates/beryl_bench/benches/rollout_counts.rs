//! Instruction counts of the cube rollout benchmark (`beryl_bench::rollout`) with Gungraun:
//! `just bench-count`. Each benchmark runs its subject over the whole corpus; divide by the
//! summary `rollout_timing` prints (copies are one per start, samples are the policy's) for
//! the count per copy, decision or rollout. Valgrind needs Linux; elsewhere this is a stub.

#[cfg(target_os = "linux")]
use {
    beryl_bench::rollout::{self, Workload},
    gungraun::prelude::*,
    gungraun::{Callgrind, EntryPoint},
    std::hint::black_box,
};

// The workload is built before counting starts, so only the subject is counted. (Gungraun's
// macros reject doc comments here.)
#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::corpus(Workload::new())]
fn copy(w: Workload) -> u64 {
    let (_, mut s) = w.memory();
    black_box(rollout::copy_states(black_box(&w.starts), &mut s))
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::corpus(Workload::new())]
fn next_apply(w: Workload) -> u64 {
    let (mut x, mut s) = w.memory();
    black_box(rollout::replay(
        &w.table,
        black_box(&w.scripts),
        &mut s,
        &mut x,
    ))
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::corpus(Workload::new())]
fn policy(w: Workload) -> u64 {
    black_box(rollout::decide(&w.table, black_box(&w.samples), &w.policy))
}

// Counted from `counted_rollout`, not this function: the setup's own rollouts leave Callgrind's
// call stack thinking a `rollout` is still running, so entering one here would unwind past
// this function and stop collection. Not from generic `rollout` either, whose other copy the
// setup runs. The copies left out are `copy`'s.
#[cfg(target_os = "linux")]
#[library_benchmark(
    config = LibraryBenchmarkConfig::default().tool(
        Callgrind::default().entry_point(EntryPoint::Custom(
            "beryl_bench::rollout::counted_rollout".to_owned()
        ))
    )
)]
#[bench::corpus(Workload::new())]
fn rollouts(w: Workload) -> u64 {
    let (mut x, mut s) = w.memory();
    black_box(rollout::roll(
        &w.table,
        black_box(&w.starts),
        &w.policy,
        &mut s,
        &mut x,
    ))
}

#[cfg(target_os = "linux")]
library_benchmark_group!(
    name = cube_rollout,
    benchmarks = [copy, next_apply, policy, rollouts]
);

#[cfg(target_os = "linux")]
main!(library_benchmark_groups = cube_rollout);

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("rollout_counts: Gungraun needs Valgrind, on Linux; run `just bench-count`");
}
