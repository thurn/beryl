//! Instruction counts of a rollout step inside a subgame and in a plain game, both in the
//! `Large` layout (`beryl_bench::subgame`), with Gungraun: `just count-gate --subgame`.
//! Valgrind needs Linux; elsewhere this is a stub.

#[cfg(target_os = "linux")]
use {
    beryl_bench::subgame::{self, Side, SubgameWorkload},
    gungraun::prelude::*,
    gungraun::{Callgrind, EntryPoint},
    std::hint::black_box,
};

#[cfg(target_os = "linux")]
#[library_benchmark(
    config = LibraryBenchmarkConfig::default().tool(
        Callgrind::default().entry_point(EntryPoint::Custom(
            "beryl_bench::subgame::subgame_step".to_owned()
        ))
    )
)]
#[bench::steps(SubgameWorkload::new())]
fn in_subgame(w: SubgameWorkload) -> u64 {
    black_box(subgame::roll(black_box(&w), Side::Subgame))
}

#[cfg(target_os = "linux")]
#[library_benchmark(
    config = LibraryBenchmarkConfig::default().tool(
        Callgrind::default().entry_point(EntryPoint::Custom(
            "beryl_bench::subgame::plain_step".to_owned()
        ))
    )
)]
#[bench::steps(SubgameWorkload::new())]
fn in_plain_game(w: SubgameWorkload) -> u64 {
    black_box(subgame::roll(black_box(&w), Side::Plain))
}

#[cfg(target_os = "linux")]
library_benchmark_group!(
    name = subgame_cost,
    benchmarks = [in_subgame, in_plain_game]
);

#[cfg(target_os = "linux")]
main!(library_benchmark_groups = subgame_cost);

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("subgame_counts: Gungraun needs Valgrind, on Linux; run `just count-gate --subgame`");
}
