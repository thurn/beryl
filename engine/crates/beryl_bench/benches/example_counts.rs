//! Instruction counts with Gungraun (Valgrind's Callgrind): exact, and the same on every run
//! however busy the machine is. Valgrind needs Linux: `just bench-count` runs this in the
//! Lima VM. Elsewhere it compiles to a stub, so `cargo bench` on the Mac skips it.
//!
//! `compare_by_id` prints `new` against `old` (same `#[bench::pools]` id); each run is also
//! compared with the previous run of the same benchmark.

#[cfg(target_os = "linux")]
use {beryl_bench::example, gungraun::prelude::*, std::hint::black_box};

// The argument is built before measurement starts, so its cost isn't counted.
// (Gungraun's macros reject doc comments here.)
#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::pools(example::sample_pools(example::SEED, example::POOLS))]
fn old(pools: Vec<Vec<u8>>) -> u32 {
    black_box(example::over_pools(
        black_box(&pools),
        example::mana_curve_old,
    ))
}

#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::pools(example::sample_pools(example::SEED, example::POOLS))]
fn new(pools: Vec<Vec<u8>>) -> u32 {
    black_box(example::over_pools(
        black_box(&pools),
        example::mana_curve_new,
    ))
}

#[cfg(target_os = "linux")]
library_benchmark_group!(
    name = mana_curve,
    compare_by_id = true,
    benchmarks = [old, new]
);

#[cfg(target_os = "linux")]
main!(library_benchmark_groups = mana_curve);

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("example_counts: Gungraun needs Valgrind, on Linux; run `just bench-count`");
}
