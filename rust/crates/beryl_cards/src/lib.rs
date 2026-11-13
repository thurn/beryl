//! Beryl's cards: the registry over every set crate under `rust/cards`, the generated
//! [`names`] and the [`decks`].
//!
//! A card's file is `rust/cards/<crate>/<set>/<card>.rs`, by its first printing
//! (`data/set-groups.json`; `just card path <name>` prints it). No list of them is kept by
//! hand: each set crate's build script writes its tables from its directories, and this
//! crate's writes [`CARDS`] and [`scenarios`] over those, crate by crate, then set, then file
//! (`beryl_card_build`, plan K8). The cards complete from the card store, single-faced
//! creatures with no abilities but keywords the engine has, have no file: each set crate's build
//! writes their definitions from their records. Nothing may depend on that order: a game numbers its cards
//! from its decklists (`GameTable`), and everything else looks a card up by name.

pub mod decks;
#[cfg(feature = "scenarios")]
pub mod derive;
#[cfg(feature = "scenarios")]
pub mod generate;
#[cfg(feature = "scenarios")]
pub mod library;

pub use beryl_card_shared::{props, tokens};

use beryl_card::CardDef;
use beryl_engine::CardTable;

/// Every card name as a constant, so a misspelled card is a compile error: the cards of the
/// decklists in `docs/sample-decks`, the basic lands, every card with a file, every card
/// complete from the card store and each card this crate's sources name (`beryl_card_build`'s
/// `names_source`).
pub mod names {
    include!(concat!(env!("OUT_DIR"), "/names.rs"));
}

// `SET_CARDS` and `SET_SCENARIOS`: each set crate's table.
include!(concat!(env!("OUT_DIR"), "/tables.rs"));

/// Every card Beryl implements: those written in files, which [`CardTable::cards`] lists and
/// tests and sweeps walk, and those complete from the card store, which have no file and which
/// [`CardTable::get`] finds by name (`beryl_card_build`'s `complete`).
pub static CARDS: CardTable = CardTable::of_parts(SET_CARDS, SET_STORE_CARDS);

/// Every card's scenarios (architecture §17.1), by card name, in the order of [`CARDS`].
///
/// They are gathered from the set crates' tables when first asked for, as [`CARDS`]' cards
/// are. Gathering them in a static made the compiler read every scenario of every set crate
/// while it checked this crate, on one thread after the set crates had finished: 11 of the 14
/// CPU-seconds of this crate's check at 50,000 cards (the scale rehearsal, `just scale`). Only
/// tests and tools build scenarios, so the app never pays for gathering them.
#[cfg(feature = "scenarios")]
pub fn scenarios() -> &'static [(&'static str, &'static [beryl_scenario::Scenario])] {
    static GATHERED: std::sync::OnceLock<Vec<(&str, &[beryl_scenario::Scenario])>> =
        std::sync::OnceLock::new();
    GATHERED.get_or_init(|| {
        SET_SCENARIOS
            .iter()
            .flat_map(|part| part.iter().copied())
            .collect()
    })
}

#[cfg(test)]
mod tests;

/// Every ability, granted ability and casting option of the cards and their tokens, each
/// named for a report and with the address `beryl_engine::coverage` records it by: what a
/// game can exercise (architecture §17.3).
pub fn parts() -> Vec<(String, usize)> {
    use beryl_engine::coverage::address;

    let mut defs: Vec<&'static CardDef> = Vec::new();
    for &def in CARDS.cards() {
        for &d in std::iter::once(&def).chain(def.tokens) {
            if !defs.iter().any(|&known| std::ptr::eq(known, d)) {
                defs.push(d);
            }
        }
    }
    let mut parts = Vec::new();
    for def in defs {
        for (f, face) in def.faces.iter().enumerate() {
            let name = if f == 0 { def.name } else { face.name };
            for (i, a) in face.abilities.iter().enumerate() {
                parts.push((format!("{name}: ability {i} ({:?})", a.kind()), address(a)));
            }
            for (i, a) in face.grants.iter().enumerate() {
                let what = format!("{name}: granted ability {i} ({:?})", a.kind());
                parts.push((what, address(a)));
            }
            for (i, option) in face.casting.iter().enumerate() {
                let what = format!("{name}: casting option {i} ({option:?})");
                parts.push((what, address(option)));
            }
        }
    }
    parts
}
