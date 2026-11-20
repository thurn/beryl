//! `record`: builds the recordings pack for the present sources if it is missing, and prints
//! its path. It is the one program of Beryl 3D that plays the engine, kept apart from `b3d` so
//! that `b3d` links neither the engine nor the cards (docs/beryl-3d.md, "Edit-to-output time").
//! `just b3d` runs it when `b3d` exits with [`beryl_inspect::recordings::MISSING`], and before
//! the tests and the gate.

mod recorder;

use std::process::ExitCode;

use beryl_cli::Catalog;

/// Every implemented card and what is built from their files, which the recorders play with.
/// This binary links the cards and hands them to `beryl_cli`'s library, which names no card
/// crate, so an edit to a card file recompiles none of that library.
static CATALOG: Catalog = Catalog {
    cards: &beryl_cards::CARDS,
    scenarios: beryl_cards::scenarios,
    library_scenarios: || beryl_cards::library::by_card().collect(),
    derived_scenarios: |def| beryl_cards::derive::for_card(def).scenarios,
    fuzz_deck_from: beryl_cards::decks::fuzz_deck_from,
    keystone_family: beryl_cards::decks::keystone_family,
};

fn main() -> ExitCode {
    let root = beryl_inspect::recordings::root();
    match beryl_inspect::recordings::build(&recorder::Cli {
        root,
        catalog: &CATALOG,
    }) {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(why) => {
            eprintln!("record: {why}");
            ExitCode::FAILURE
        }
    }
}
