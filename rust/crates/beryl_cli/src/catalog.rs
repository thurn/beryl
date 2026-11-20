//! The cards a command plays with, passed in by the binary that links them.
//!
//! This library names no card crate: `beryl_cards`, the registry over every set crate, is a
//! dependency of the binaries alone (`src/main.rs`, Beryl 3D's recorder). Each builds a
//! [`Catalog`] from it at startup and hands it to the commands, so an edit to a card file
//! recompiles the set crate and relinks the binaries, and recompiles no code of this library.
//!
//! The decklists and card names come from `beryl_card_names`, which changes only when a card
//! is added, and the props from `beryl_card_shared`; only what depends on the card files'
//! bodies is here.

use beryl_engine::CardTable;
use beryl_engine::card::CardDef;
use beryl_scenario::Scenario;

/// A decklist as `beryl_engine::GameTable::new` takes it: each card's count and name.
pub type Decklist = Vec<(u8, &'static str)>;

/// The implemented cards and what is built from their files: their scenarios and the fuzz
/// decks dealt from them.
///
/// The lookups are function pointers rather than data because each is built on demand
/// (scenarios are gathered or generated on first use, which costs seconds), and a binary
/// declares its catalog as a `static` of plain function items. Every field is filled from
/// the same-named item of `beryl_cards`.
pub struct Catalog {
    /// Every implemented card (`beryl_cards::CARDS`).
    pub cards: &'static CardTable,
    /// Every card file's hand-written scenarios by card name, in the order of
    /// [`CardTable::cards`] (`beryl_cards::scenarios`).
    pub scenarios: fn() -> &'static [(&'static str, &'static [Scenario])],
    /// The rules library's scenarios by the card that has the effect
    /// (`beryl_cards::library::by_card`, collected).
    pub library_scenarios: fn() -> Vec<(&'static str, &'static [Scenario])>,
    /// A card's generated scenarios (`beryl_cards::derive::for_card`'s `scenarios`).
    pub derived_scenarios: fn(&'static CardDef) -> Vec<Scenario>,
    /// A 40-card fuzz deck dealt from the given cards by a seed
    /// (`beryl_cards::decks::fuzz_deck_from`).
    pub fuzz_deck_from: fn(u64, &[&CardDef]) -> Decklist,
    /// The cards of the named keystone family, with every basic land
    /// (`beryl_cards::decks::keystone_family`).
    pub keystone_family: fn(&str) -> Option<Vec<&'static CardDef>>,
}

impl Catalog {
    /// A fuzz deck dealt by `seed` from every implemented card (`beryl_cards::decks::fuzz_deck`).
    #[must_use]
    pub fn fuzz_deck(&self, seed: u64) -> Decklist {
        (self.fuzz_deck_from)(seed, self.cards.cards())
    }
}

/// The catalog of every implemented card, for this crate's unit tests.
#[cfg(test)]
pub(crate) static TEST: Catalog = Catalog {
    cards: &beryl_cards::CARDS,
    scenarios: beryl_cards::scenarios,
    library_scenarios: || beryl_cards::library::by_card().collect(),
    derived_scenarios: |def| beryl_cards::derive::for_card(def).scenarios,
    fuzz_deck_from: beryl_cards::decks::fuzz_deck_from,
    keystone_family: beryl_cards::decks::keystone_family,
};
