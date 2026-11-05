//! Beryl's cards: one file per card under color directories (by the cube list's color
//! groups), the generated [`names`], the static [`CARDS`] table and the [`decks`].

pub mod azorius;
pub mod blue;
pub mod decks;
pub mod names;
pub mod white;

use beryl_engine::CardTable;

/// Every card Beryl implements.
pub static CARDS: CardTable = CardTable::new(&[
    &white::plains::CARD,
    &blue::island::CARD,
    &azorius::tundra::CARD,
]);

#[cfg(test)]
mod tests;
