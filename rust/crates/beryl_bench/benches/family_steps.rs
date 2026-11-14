//! The present-cost benchmark's pairing (`beryl_bench::family`), run natively: the steps each
//! family's rollouts paired with the control's and the steps they left out, one JSON line per
//! family, for `just count-gate --family`, which reads them beside `family_counts`' counts.
//! `cargo bench --bench family_steps -- <family>...`; every family with no names.

use beryl_bench::family::{FAMILIES, FamilyWorkload, roll_paired};

fn main() {
    let names: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    for family in FAMILIES {
        if !names.is_empty() && !names.iter().any(|n| n == family.name) {
            continue;
        }
        let w = FamilyWorkload::new(family.name);
        let p = roll_paired(&w);
        println!(
            "{{\"family\": \"{}\", \"positions\": {}, \"paired\": {}, \"skipped\": {}}}",
            family.name,
            w.positions.len(),
            p.paired,
            p.skipped
        );
    }
}
