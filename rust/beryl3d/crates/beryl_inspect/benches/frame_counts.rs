//! Instruction counts of the gated boards' frames with Gungraun: `just b3d gate --counts`
//! (docs/beryl-3d.md, "Finding the worst frame"). Valgrind needs Linux; elsewhere this is a
//! stub.
//!
//! For each board in `gate::BOARDS`:
//!
//! - `total` counts the whole playback, every frame's `handle` and `frame` calls.
//! - `ranked` counts one frame of the `gate::RANKED` that `gate::rank` picks, with Callgrind
//!   collecting only inside `gate::frame_counted`. Run `n` counts the frame in place `n` of the
//!   ranking, and the board's worst frame is the largest of them. Collection toggled on and off
//!   around several frames in one run would give their sum, not the largest.
//! - `stretch` counts each frame of `gate::stretch` the same way, so that the gate can check the
//!   ranking: the frame of the stretch with the most instructions must be among the ranked.
//!
//! Adding a board: a `#[benches::<name>(...)]` on each function, with setup functions for it.

#[cfg(target_os = "linux")]
use {
    beryl_inspect::gate::{self, Counted, Playback},
    gungraun::prelude::*,
    gungraun::{Callgrind, EntryPoint},
    std::hint::black_box,
};

// The board is built before counting starts, so only its playback is counted. (Gungraun's
// macros reject doc comments here.)
#[cfg(target_os = "linux")]
#[library_benchmark]
#[bench::stress_255(Playback::new("stress-255").expect("a gated board"))]
#[bench::stress_1024(Playback::new("stress-1024").expect("a gated board"))]
#[bench::verbs_zones(Playback::new("verbs-zones").expect("a gated board"))]
#[bench::pay_default(Playback::new("pay-default").expect("a gated board"))]
fn total(playback: Playback) -> Playback {
    black_box(gate::play_all(black_box(playback)))
}

// Callgrind collects only inside `frame_counted`, which the one frame `Counted` names goes
// through.
#[cfg(target_os = "linux")]
#[library_benchmark(
    config = LibraryBenchmarkConfig::default().tool(
        Callgrind::default().entry_point(EntryPoint::Custom(
            "beryl_inspect::gate::frame_counted".to_owned()
        ))
    )
)]
#[benches::stress_255(iter = 0..gate::RANKED, setup = ranked_255)]
#[benches::stress_1024(iter = 0..gate::RANKED, setup = ranked_1024)]
#[benches::verbs_zones(iter = 0..gate::RANKED, setup = ranked_verbs_zones)]
#[benches::pay_default(iter = 0..gate::RANKED, setup = ranked_pay_default)]
fn ranked(counted: Counted) -> Playback {
    black_box(counted.play())
}

#[cfg(target_os = "linux")]
#[library_benchmark(
    config = LibraryBenchmarkConfig::default().tool(
        Callgrind::default().entry_point(EntryPoint::Custom(
            "beryl_inspect::gate::frame_counted".to_owned()
        ))
    )
)]
#[benches::stress_255(iter = 0..gate::STRETCH_FRAMES, setup = stretch_255)]
#[benches::stress_1024(iter = 0..gate::STRETCH_FRAMES, setup = stretch_1024)]
#[benches::verbs_zones(iter = 0..gate::STRETCH_FRAMES, setup = stretch_verbs_zones)]
#[benches::pay_default(iter = 0..gate::STRETCH_FRAMES, setup = stretch_pay_default)]
fn stretch(counted: Counted) -> Playback {
    black_box(counted.play())
}

// Run `n` of a board's `ranked` counts its `n`th ranked frame, and run `n` of its `stretch` the
// `n`th frame of its stretch.
#[cfg(target_os = "linux")]
fn stretched(name: &str, place: usize) -> Counted {
    let frames = gate::stretch(name).expect("a gated board");
    Counted::frame(name, frames.start + place)
}

#[cfg(target_os = "linux")]
fn ranked_255(place: usize) -> Counted {
    Counted::ranked("stress-255", place)
}

#[cfg(target_os = "linux")]
fn ranked_1024(place: usize) -> Counted {
    Counted::ranked("stress-1024", place)
}

#[cfg(target_os = "linux")]
fn ranked_verbs_zones(place: usize) -> Counted {
    Counted::ranked("verbs-zones", place)
}

#[cfg(target_os = "linux")]
fn ranked_pay_default(place: usize) -> Counted {
    Counted::ranked("pay-default", place)
}

#[cfg(target_os = "linux")]
fn stretch_255(place: usize) -> Counted {
    stretched("stress-255", place)
}

#[cfg(target_os = "linux")]
fn stretch_1024(place: usize) -> Counted {
    stretched("stress-1024", place)
}

#[cfg(target_os = "linux")]
fn stretch_verbs_zones(place: usize) -> Counted {
    stretched("verbs-zones", place)
}

#[cfg(target_os = "linux")]
fn stretch_pay_default(place: usize) -> Counted {
    stretched("pay-default", place)
}

// Gungraun clears the environment of the benchmark it runs, but the recorded boards are read
// from the recordings pack in `$BERYL_CACHE`, which the gate points at the copy it sends into
// the machine.
#[cfg(target_os = "linux")]
library_benchmark_group!(
    name = frames,
    config = LibraryBenchmarkConfig::default().pass_through_env("BERYL_CACHE"),
    benchmarks = [total, ranked, stretch]
);

#[cfg(target_os = "linux")]
main!(library_benchmark_groups = frames);

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("frame_counts: Gungraun needs Valgrind, on Linux; run `just b3d gate --counts`");
}
