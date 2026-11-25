//! The steps and positions of the subgame benchmark (`beryl_bench::subgame`), run natively,
//! as one JSON line, for `just count-gate --subgame`, which divides the counts by them.

use beryl_bench::subgame::{Side, SubgameWorkload, roll};

fn main() {
    let w = SubgameWorkload::new();
    println!(
        "{{\"subgame_positions\": {}, \"plain_positions\": {}, \"subgame_steps\": {}, \"plain_steps\": {}}}",
        w.subgame.len(),
        w.plain.len(),
        roll(&w, Side::Subgame),
        roll(&w, Side::Plain)
    );
}
