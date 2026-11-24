//! Old versus new, interleaved A B B A in one executable (`beryl_bench::abba`).
//! `just bench -p beryl_bench --bench example_abba` (`-- --quick` for a fast look).

use std::hint::black_box;

use beryl_bench::{abba, cores, example};

fn main() {
    if !cores::prefer_fastest_cores() && cfg!(target_os = "macos") {
        eprintln!("example_abba: couldn't request the fastest cores");
    }
    let config = abba::Config::from_args(std::env::args().skip(1));
    let pools = example::sample_pools(example::SEED, example::POOLS);
    let report = abba::compare(
        "mana_curve (A old, B new)",
        &config,
        || example::over_pools(black_box(&pools), example::mana_curve_old),
        || example::over_pools(black_box(&pools), example::mana_curve_new),
    );
    print!("{report}");
}
