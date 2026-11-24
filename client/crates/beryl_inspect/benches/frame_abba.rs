//! One host frame of the gated boards, old against new, interleaved A B B A
//! (docs/benchmarking.md, "Old against new: the ABBA harness"):
//! `just client cargo bench -p beryl_inspect --bench frame_abba` (`-- --quick` for a fast look).
//! Until an optimization puts its old version in A, both sides run the current code: a null
//! comparison whose interval shows the noise floor on this machine.
//!
//! A call is one frame of a host's work on a gated board (`gate::Playback`): the inputs due
//! before the frame, then `Client::frame`. Each side plays its own copy of the board, and starts
//! it again when its script has played out, so the time per call includes a board's setup
//! spread over its frames, the same on both sides. The instruction-count gate holds the work
//! itself; this times it on the Mac, where clocks and caches count.

use std::hint::black_box;

use beryl_inspect::gate::Playback;

// The main workspace's ABBA harness and its request for the fastest cores, compiled in here
// because `beryl_bench`, the crate they belong to, links the engine, which `beryl_inspect` must
// not (client/AGENTS.md).
#[allow(
    dead_code,
    unused_imports,
    reason = "the harness's report fields and its tests, which `cargo bench` compiles but a \
              bench with no harness never runs, are used by the main workspace"
)]
#[path = "../../../../engine/crates/beryl_bench/src/abba.rs"]
mod abba;
#[path = "../../../../engine/crates/beryl_bench/src/cores.rs"]
mod cores;

/// A gated board played for ever: its playback, started again each time its script ends.
struct Looping {
    name: &'static str,
    playback: Playback,
}

impl Looping {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            playback: Playback::new(name).expect("a gated board"),
        }
    }

    /// One frame of the host's work.
    fn frame(&mut self) {
        if self.playback.moment().is_none() {
            self.playback = Playback::new(self.name).expect("a gated board");
        }
        self.playback.handle();
        black_box(self.playback.frame());
    }
}

fn main() {
    if !cores::prefer_fastest_cores() && cfg!(target_os = "macos") {
        eprintln!("frame_abba: couldn't request the fastest cores");
    }
    let config = abba::Config::from_args(std::env::args().skip(1));
    for name in ["stress-255", "stress-1024", "verbs-zones", "pay-default"] {
        let (mut a, mut b) = (Looping::new(name), Looping::new(name));
        let report = abba::compare(
            &format!("{name} frame (A old, B new)"),
            &config,
            || a.frame(),
            || b.frame(),
        );
        print!("{report}");
    }
}
