//! The static card table and the per-game table built from two decklists (architecture §6.4,
//! decision D20).

use crate::card::{CardDef, Trigger};
use crate::event::kind;
use crate::ids::{CardIx, Seat};
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
    DeckTooLarge { player: Seat, cards: usize },
    /// The decks and their tokens need more game indices than [`CardIx`] has.
    TooManyCards,
}

/// The cards one game can contain, under dense game-local indices: the [`CardIx`] that
/// `State` stores for an object's card. Built once per game and shared, read-only, by every thread
/// searching it, or cloned into an agent that outlives the borrow.
#[derive(Clone, Debug)]
pub struct GameTable {
    cards: &'static CardTable,
    defs: Vec<&'static CardDef>,
    decks: [Vec<CardIx>; 2],
    /// Per game index, the event kinds its triggered abilities listen to.
    listens: Vec<u16>,
    /// Every kind any card of the game listens to.
    listens_any: u16,
}

impl GameTable {
    /// Assigns game indices in order of first appearance, the first deck before the second,
    /// then the tokens and emblems those cards can create, so the same decklists always give
    /// the same table.
    pub fn new(cards: &'static CardTable, decks: [&Decklist; 2]) -> Result<Self, TableError> {
        let mut defs: Vec<&'static CardDef> = Vec::new();
        let mut indices: [Vec<CardIx>; 2] = [Vec::new(), Vec::new()];
        for (player, list) in Seat::BOTH.into_iter().zip(decks) {
            for &(count, name) in list {
                let def = cards.get(name).ok_or(TableError::UnknownCard(name))?;
                let index = if let Some(i) = defs.iter().position(|d| std::ptr::eq(*d, def)) {
                    i
                } else {
                    defs.push(def);
                    defs.len() - 1
                };
                let index = u8::try_from(index)
                    .ok()
                    .and_then(CardIx::try_new)
                    .ok_or(TableError::TooManyCards)?;
                for _ in 0..count {
                    indices[player.index()].push(index);
                }
            }
            if indices[player.index()].len() > DECK_SLOTS {
                return Err(TableError::DeckTooLarge {
                    player,
                    cards: indices[player.index()].len(),
                });
            }
        }
        let mut i = 0;
        while i < defs.len() {
            for &token in defs[i].tokens {
                if !defs.iter().any(|d| std::ptr::eq(*d, token)) {
                    defs.push(token);
                }
            }
            i += 1;
        }
        if defs.len() > CardIx::COUNT {
            return Err(TableError::TooManyCards);
        }
        let listens: Vec<u16> = defs.iter().map(|d| listens(d)).collect();
        let listens_any = listens.iter().fold(0, |a, &b| a | b);
        Ok(Self {
            cards,
            defs,
            decks: indices,
            listens,
            listens_any,
        })
    }

    /// The game index of a card or token, if the game can contain it.
    pub fn index_of(&self, def: &CardDef) -> Option<CardIx> {
        self.defs
            .iter()
            .position(|d| std::ptr::eq(*d, def))
            .map(|i| CardIx::new(i as u8))
    }

    /// The event kinds the triggered abilities of the card at `card` listen to.
    pub(crate) fn listens(&self, card: CardIx) -> u16 {
        self.listens[card.index()]
    }

    /// The event kinds any card of this game listens to.
    pub(crate) fn listens_any(&self) -> u16 {
        self.listens_any
    }

    pub fn cards(&self) -> &'static CardTable {
        self.cards
    }

    /// The definition behind a game index.
    pub fn def(&self, card: CardIx) -> &'static CardDef {
        self.defs[card.index()]
    }

    /// How many distinct cards the game has.
    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// A player's deck as game indices, in decklist order.
    pub fn deck(&self, player: Seat) -> &[CardIx] {
        &self.decks[player.index()]
    }
}

/// The event kinds a card's triggered abilities listen to, on any face.
fn listens(def: &CardDef) -> u16 {
    let mut mask = 0;
    for face in def.faces {
        for ability in face.abilities {
            mask |= match ability.trigger {
                Trigger::None | Trigger::Delayed => 0,
                Trigger::Enters => kind::ENTERS,
                Trigger::Dies => kind::DIES,
                Trigger::Attacks | Trigger::YouAttack => kind::ATTACK,
                Trigger::CombatDamageToPlayer => kind::DAMAGE,
                Trigger::Step(..) => kind::STEP,
                Trigger::YouCreateTokens(_) => kind::TOKENS,
                Trigger::YouCast => kind::CAST,
            };
        }
    }
    mask
}
