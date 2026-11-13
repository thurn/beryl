//! The parameters an entry has of its own: where, in what order and on what terms.

#[cfg(doc)]
use crate::continuous::Duration;
#[cfg(doc)]
use crate::effect::Effect;
use crate::filter::Filter;

/// Where in a library a card is put.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LibraryPosition {
    /// "On top of".
    Top,
    /// "On the bottom of".
    Bottom,
    /// "On their choice of the top or bottom of": each card's owner chooses as the
    /// instruction is reached (CR 608.2d).
    TopOrBottom,
    /// "Nth from the top", the top card being first
    /// ([CR 401.7](https://yawgatog.com/resources/magic-rules/#R4017)): "If an effect causes
    /// a player to put a card into a library 'Nth from the top,' and that library has fewer
    /// than N cards in it, the player puts that card on the bottom of that library."
    FromTop(u8),
}

/// What a permission to cast says of a spell cast by it ([`Effect::PermitCast`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CastTerms {
    /// "A spell cast this way costs {N} more to cast": a cost increase
    /// ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f)): the total cost is
    /// the mana cost "plus all additional costs and cost increases". Generic mana.
    pub costs_more: u8,
    /// "If that spell would be put into your graveyard, exile it instead": a replacement
    /// effect on the spell
    /// ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)): "Effects that use
    /// the word 'instead' are replacement effects."
    pub exiled_instead: bool,
    /// "May play it" rather than "may cast it"
    /// ([CR 701.18b](https://yawgatog.com/resources/magic-rules/#R70118b)): "To play a card
    /// means to play that card as a land or to cast that card as a spell, whichever is
    /// appropriate." For a nonland card the two are the same. The engine has no permission
    /// to play a land from outside a hand, and stops on one (`no_primitive`).
    pub played: bool,
}

impl CastTerms {
    /// A permission that says nothing more of the spell.
    pub const PLAIN: CastTerms = CastTerms {
        costs_more: 0,
        exiled_instead: false,
        played: false,
    };

    /// These terms, with "a spell cast this way costs {`generic`} more to cast".
    #[must_use]
    pub const fn costing_more(self, generic: u8) -> CastTerms {
        CastTerms {
            costs_more: generic,
            ..self
        }
    }

    /// These terms, worded "may play it" (CR 701.18b).
    #[must_use]
    pub const fn played(self) -> CastTerms {
        CastTerms {
            played: true,
            ..self
        }
    }

    /// These terms, with "if that spell would be put into your graveyard, exile it instead".
    #[must_use]
    pub const fn exiled_instead(self) -> CastTerms {
        CastTerms {
            exiled_instead: true,
            ..self
        }
    }
}

/// How a copy differs from what it copies
/// ([CR 707.9](https://yawgatog.com/resources/magic-rules/#R7079)): "Copy effects may include
/// modifications or exceptions to the copying process."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CopyException {
    /// An exact copy.
    None,
    /// "Except it's [power]/[toughness]"
    /// ([CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b)): "Some copy effects
    /// modify a characteristic as part of the copying process. The final set of values for
    /// that characteristic becomes part of the copiable values of the copy."
    PowerToughness(i8, i8),
}

/// Which of the cards a player looks at they take into their hand ([`Effect::Dig`]).
#[derive(Clone, Copy, Debug)]
pub enum Taken {
    /// "Put [n] of them into your hand": that many, of the player's choice, or all of them
    /// if there are no more (CR 609.3).
    Put(u8),
    /// "You may reveal a [filter] card from among them and put it into your hand": one the
    /// filter accepts, or none
    /// ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a
    /// card, show that card to all players for a brief time."
    MayReveal(Filter),
}

/// The order cards put on the bottom of a library at the same time go there in
/// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "If an effect puts two or
/// more cards in a specific position in a library at the same time, the owner of those cards
/// may arrange them in any order. That library's owner doesn't reveal the order in which the
/// cards go into the library."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LibraryOrder {
    /// "In any order": the player chooses.
    Any,
    /// "In a random order".
    Random,
}

/// Where the cards a search finds go ([`Effect::Search`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Found {
    /// "Reveal it, put it into your hand"
    /// ([CR 701.23e](https://yawgatog.com/resources/magic-rules/#R70123e)).
    RevealToHand,
    /// "Put it onto the battlefield tapped", under the control of the player searching
    /// ([CR 110.2a](https://yawgatog.com/resources/magic-rules/#R1102a)).
    OntoBattlefieldTapped,
}
