//! `Effect`: what a spell or ability does as it resolves, as data.
//!
//! An effect is a tree of instructions, each one the Comprehensive Rules define, over four
//! kinds of expression: which objects ([`Objects`]), which players ([`Players`]), how many
//! ([`Amount`]) and whether ([`Condition`]). Every node is `Copy` and built in `const`
//! context, with children behind `&'static` references, so a definition is a `static`.
//!
//! # Saying what the card says
//!
//! An entry is designed from its rule and says the card's words, not what they come to in a
//! two-player game: "each opponent" is [`Players::EachOpponent`] and "target
//! opponent" is [`Players::Target`], though both name the same player today. Which players a
//! target may be is its [`crate::TargetSpec`]'s to say, in the same words
//! ([`crate::Filter::Whose`]).
//!
//! # What the instruction before did
//!
//! Card text refers back to what it just did: "exile it", "that many", "if you do". Here
//! those are [`Objects::Those`], [`Amount::ThatMany`] and [`Condition::Did`], and each means
//! the outcome of the instruction before the one that reads it: the objects it acted on, as
//! they are now ([CR 400.7j](https://yawgatog.com/resources/magic-rules/#R4007j): "If an
//! effect causes an object to move to a public zone, other parts of that effect can find that
//! object"), how many or how much it did, and whether it did anything. The outcome of a
//! [`Effect::Sequence`] is its last instruction's; of an [`Effect::If`], [`Effect::May`] or
//! [`Effect::Unless`], what it ran, or nothing when it ran nothing; of an
//! [`Effect::ForEachPlayer`], every player's together. The first instruction of a nested
//! effect reads what the enclosing one would. Nothing is named and nothing is looked up by
//! name: a reference is a value of its expression type.

mod amount;
mod asks;
mod enters;
pub mod build;
mod condition;
mod instruction;
mod operands;
mod reference;
mod takes;
mod terms;
mod traversal;

pub use amount::Amount;
pub use condition::{Comparison, Condition};
pub use instruction::Effect;
pub use operands::Operands;
pub use reference::{Objects, Players, Recipients};
pub use terms::{
    CastFace, CastTerms, CopyChange, CopyException, ExileMatch, Found, LibraryOrder,
    LibraryPosition, Prevention, PutOnto, Regeneration, Taken,
};
pub(crate) use traversal::Branches;
