//! Instruction counts of the present-cost benchmark (`beryl_bench::family`) with Gungraun:
//! `just count-gate --family <name>`. Both benchmarks run every family's paired rollouts in
//! full; `present` counts only the rollout's own transitions (`family::present_step`) and
//! `control` only the same transitions on the control states (`family::control_step`), so
//! building each control state and matching the choice in it are counted by neither. Their
//! ratio is the family's present cost. Valgrind needs Linux; elsewhere this is a stub.
//!
//! Adding a family: its name as a `#[bench::<name>(...)]` on both functions, matching
//! `family::FAMILIES`.

#[cfg(target_os = "linux")]
use {
    beryl_bench::family::{self, FamilyWorkload},
    gungraun::prelude::*,
    gungraun::{Callgrind, EntryPoint},
    std::hint::black_box,
};

// Each counts from one function's entry: Callgrind collects only while it runs, with what it
// calls. (Gungraun's macros reject doc comments here.)
#[cfg(target_os = "linux")]
#[library_benchmark(
    config = LibraryBenchmarkConfig::default().tool(
        Callgrind::default().entry_point(EntryPoint::Custom(
            "beryl_bench::family::present_step".to_owned()
        ))
    )
)]
#[bench::blank(FamilyWorkload::new("blank"))]
#[bench::attachments(FamilyWorkload::new("attachments"))]
fn present(w: FamilyWorkload) -> u64 {
    black_box(family::roll_paired(black_box(&w)).paired)
}

#[cfg(target_os = "linux")]
#[library_benchmark(
    config = LibraryBenchmarkConfig::default().tool(
        Callgrind::default().entry_point(EntryPoint::Custom(
            "beryl_bench::family::control_step".to_owned()
        ))
    )
)]
#[bench::blank(FamilyWorkload::new("blank"))]
#[bench::attachments(FamilyWorkload::new("attachments"))]
fn control(w: FamilyWorkload) -> u64 {
    black_box(family::roll_paired(black_box(&w)).paired)
}

#[cfg(target_os = "linux")]
library_benchmark_group!(name = present_cost, benchmarks = [present, control]);

#[cfg(target_os = "linux")]
main!(library_benchmark_groups = present_cost);

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("family_counts: Gungraun needs Valgrind, on Linux; run `just count-gate --family`");
}
