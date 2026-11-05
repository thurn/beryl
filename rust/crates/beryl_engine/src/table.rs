//! The static card table and the per-game table built from two decklists (architecture §6.4,
//! decision D20).

use crate::card::CardDef;
use crate::state::DECK_SLOTS;

/// Every card the program knows, built once and shared by every thread as a `&'static`.
#[derive(Debug)]
pub struct CardTable {
    cards: &'static [&'static CardDef],
}

impl CardTable {
    pub const fn new(cards: &'static [&'static CardDef]) -> Self {
        Self { cards }
    }

    pub fn get(&self, name: &str) -> Option<&'static CardDef> {
        self.cards.iter().copied().find(|c| c.name == name)
    }

    pub fn cards(&self) -> &'static [&'static CardDef] {
        self.cards
    }
}

/// A decklist: how many of each card, by name.
pub type Decklist = [(u8, &'static str)];

/// Why a [`GameTable`] could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableError {
    /// A decklist names a card the [`CardTable`] does not have.
    UnknownCard(&'static str),
    /// A deck has more cards than the engine reserves slots for.
    DeckTooLarge { player: u8, cards: usize },
}

/// The cards one game can contain, under dense game-local indices: the `u8` that `State`
/// stores for an object's card. Built once per game and shared, read-only, by every thread
/// searching it.
#[derive(Debug)]
pub struct GameTable {
    cards: &'static CardTable,
    defs: Vec<&'static CardDef>,
    decks: [Vec<u8>; 2],
}

impl GameTable {
    /// Assigns game indices in order of first appearance, the first deck before the second,
    /// so the same decklists always give the same table.
    pub fn new(cards: &'static CardTable, decks: [&Decklist; 2]) -> Result<Self, TableError> {
        let mut defs: Vec<&'static CardDef> = Vec::new();
        let mut indices: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
        for (player, list) in decks.iter().enumerate() {
            for &(count, name) in *list {
                let def = cards.get(name).ok_or(TableError::UnknownCard(name))?;
                let index = if let Some(i) = defs.iter().position(|d| std::ptr::eq(*d, def)) {
                    i
                } else {
                    defs.push(def);
                    defs.len() - 1
                };
                for _ in 0..count {
                    indices[player].push(index as u8);
                }
            }
            if indices[player].len() > DECK_SLOTS {
                return Err(TableError::DeckTooLarge {
                    player: player as u8,
                    cards: indices[player].len(),
                });
            }
        }
        Ok(Self {
            cards,
            defs,
            decks: indices,
        })
    }

    pub fn cards(&self) -> &'static CardTable {
        self.cards
    }

    /// The definition behind a game index.
    pub fn def(&self, card: u8) -> &'static CardDef {
        self.defs[usize::from(card)]
    }

    /// How many distinct cards the game has.
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// A player's deck as game indices, in decklist order.
    pub fn deck(&self, player: u8) -> &[u8] {
        &self.decks[usize::from(player)]
    }
}
