//! Beryl's cards: one file per card under color directories (by the cube list's color
//! groups), the generated [`names`], the static [`CARDS`] table and the [`decks`].

pub mod azorius;
pub mod blue;
pub mod decks;
pub mod names;
pub mod white;

use beryl_engine::CardTable;
use beryl_scenario::Scenario;

/// Every card Beryl implements.
pub static CARDS: CardTable = CardTable::new(&[
    &white::plains::CARD,
    &blue::island::CARD,
    &azorius::tundra::CARD,
]);

/// Every card's scenarios (architecture §17.1), by card name.
pub static SCENARIOS: &[(&str, &[Scenario])] = &[
    (names::PLAINS, white::plains::SCENARIOS),
    (names::ISLAND, blue::island::SCENARIOS),
    (names::TUNDRA, azorius::tundra::SCENARIOS),
];

#[cfg(test)]
mod tests;
