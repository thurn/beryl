//! The cube rollout benchmark's subjects old against new, interleaved A B B A
//! (`beryl_bench::abba`): `just bench -p beryl_bench --bench rollout_abba` (`-- --quick` for
//! a fast look). Until an optimization puts its old version in A, both sides run the
//! current code: a null comparison whose interval shows the noise floor on this machine.

use std::hint::black_box;

use beryl_bench::rollout::{self, Workload};
use beryl_bench::{abba, cores};

fn main() {
    if !cores::prefer_fastest_cores() && cfg!(target_os = "macos") {
        eprintln!("rollout_abba: couldn't request the fastest cores");
    }
    let config = abba::Config::from_args(std::env::args().skip(1));
    let w = Workload::new();
    let ((mut xa, mut sa), (mut xb, mut sb)) = (w.memory(), w.memory());

    let report = abba::compare(
        "copy (A old, B new)",
        &config,
        || rollout::copy_states(black_box(&w.starts), &mut sa),
        || rollout::copy_states(black_box(&w.starts), &mut sb),
    );
    print!("{report}");
    let report = abba::compare(
        "next_apply (A old, B new)",
        &config,
        || rollout::replay(&w.table, black_box(&w.scripts), &mut sa, &mut xa),
        || rollout::replay(&w.table, black_box(&w.scripts), &mut sb, &mut xb),
    );
    print!("{report}");
    let report = abba::compare(
        "policy (A old, B new)",
        &config,
        || rollout::decide(&w.table, black_box(&w.samples), &w.policy),
        || rollout::decide(&w.table, black_box(&w.samples), &w.policy),
    );
    print!("{report}");
    let report = abba::compare(
        "rollouts (A old, B new)",
        &config,
        || rollout::roll(&w.table, black_box(&w.starts), &w.policy, &mut sa, &mut xa),
        || rollout::roll(&w.table, black_box(&w.starts), &w.policy, &mut sb, &mut xb),
    );
    print!("{report}");
}
