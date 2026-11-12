//! `Effect`: what a spell or ability does as it resolves, as data (plan K1).
//!
//! An effect is a tree of instructions, each one the Comprehensive Rules define, over four
//! kinds of expression: which objects ([`Objects`]), which players ([`Players`]), how many
//! ([`Amount`]) and whether ([`Condition`]). Every node is `Copy` and built in `const`
//! context, with children behind `&'static` references, so a definition is a `static`.
//!
//! # Saying what the card says
//!
//! An entry is designed from its rule and says the card's words, not what they come to in a
//! two-player game (plan K7): "each opponent" is [`Players::EachOpponent`] and "target
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
pub mod build;
mod condition;
mod reference;

pub use amount::Amount;
pub use condition::{Comparison, Condition};
pub use reference::{Objects, Players};

use crate::continuous::{Duration, Modification};
use crate::counter::CounterKind;
use crate::definition::CardDef;
use crate::index::SpecIx;
use crate::mana::{ManaCost, ManaSet};
use crate::target::Takes;

/// One instruction, or several composed
/// ([CR 609.1](https://yawgatog.com/resources/magic-rules/#R6091)): "An effect is something
/// that happens in the game as a result of a spell or ability."
///
/// An instruction that cannot be done in full does what it can
/// ([CR 609.3](https://yawgatog.com/resources/magic-rules/#R6093)): "If an effect attempts to
/// do something impossible, it does only as much as possible." One whose target has become
/// illegal does nothing to that target (CR 608.2b), so [`Objects::Target`] then names
/// nothing.
#[derive(Clone, Copy, Debug)]
pub enum Effect {
    /// Each instruction in turn
    /// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "The controller of
    /// the spell or ability follows its instructions in the order written." None is no
    /// instruction at all ([`Effect::NOTHING`]).
    Sequence(&'static [Effect]),
    /// "If [condition], [then]. Otherwise, [otherwise]." The condition is read as the
    /// instruction is reached
    /// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "If an effect
    /// requires information from the game ..., the answer is determined only once, when the
    /// effect is applied."
    If {
        condition: Condition,
        then: &'static Effect,
        otherwise: Option<&'static Effect>,
    },
    /// "[Player] may [effect]": the player chooses as the instruction is reached
    /// ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)): "If an effect of a
    /// spell or ability offers any choices ..., the player announces these while applying the
    /// effect."
    May {
        player: Players,
        effect: &'static Effect,
    },
    /// "[Effect] unless [player] pays [cost]"
    /// ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)): "This means the
    /// same thing as '[A player may do something else]. If [that player doesn't], [do
    /// something].'" A player who cannot pay is not asked
    /// ([CR 118.3](https://yawgatog.com/resources/magic-rules/#R1183)).
    Unless {
        effect: &'static Effect,
        player: Players,
        cost: ManaCost,
    },
    /// "Each [player] [effect]", where the effect differs by player: within it,
    /// [`Players::ThatPlayer`] is the player it is being done for. The players take their
    /// turns at it in turn order from the active player
    /// ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)). An instruction that
    /// is the same for every player takes the players as its operand instead.
    ForEachPlayer {
        players: Players,
        effect: &'static Effect,
    },
    /// "Counter [spell]"
    /// ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a)): "To counter a spell
    /// or ability means to cancel it, removing it from the stack. It doesn't resolve and none
    /// of its effects occur. A countered spell is put into its owner's graveyard."
    Counter(Objects),
    /// "Destroy [permanents]"
    /// ([CR 701.8a](https://yawgatog.com/resources/magic-rules/#R7018a)): "To destroy a
    /// permanent, move it from the battlefield to its owner's graveyard."
    Destroy(Objects),
    /// "Exile [objects]"
    /// ([CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a)): "To exile an
    /// object, move it to the exile zone from wherever it is."
    Exile(Objects),
    /// "Exile [objects] until this leaves the battlefield"
    /// ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)): "Some one-shot
    /// effects cause an object to change zones 'until' a specified event occurs. A second
    /// one-shot effect is created immediately after the specified event. This second one-shot
    /// effect returns the object to its previous zone."
    ExileUntilThisLeaves(Objects),
    /// "Sacrifice [permanents]": each one's controller does
    /// ([CR 701.21a](https://yawgatog.com/resources/magic-rules/#R70121a)): "To sacrifice a
    /// permanent, its controller moves it from the battlefield directly to its owner's
    /// graveyard."
    Sacrifice(Objects),
    /// "Return [objects] to their owners' hands"
    /// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object would
    /// go to any library, graveyard, or hand other than its owner's, it goes to its owner's
    /// corresponding zone."
    ReturnToHand(Objects),
    /// "Put [objects] into their owners' libraries [position]" (CR 400.3).
    PutIntoLibrary {
        objects: Objects,
        position: LibraryPosition,
    },
    /// "[Player] draws [count] cards"
    /// ([CR 121.1](https://yawgatog.com/resources/magic-rules/#R1211)): "A player draws a
    /// card by putting the top card of their library into their hand", one at a time
    /// ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)).
    Draw { player: Players, count: Amount },
    /// "[Player] discards [count] cards", of that player's choice
    /// ([CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a)): "To discard a card,
    /// move it from its owner's hand to that player's graveyard."
    /// [CR 701.9b](https://yawgatog.com/resources/magic-rules/#R7019b): "By default, effects
    /// that cause a player to discard a card allow the affected player to choose which card
    /// to discard."
    Discard { player: Players, count: Amount },
    /// "[Player] mills [count] cards"
    /// ([CR 701.17a](https://yawgatog.com/resources/magic-rules/#R70117a)): "that player puts
    /// that many cards from the top of their library into their graveyard."
    Mill { player: Players, count: Amount },
    /// "[Player] scries [count]"
    /// ([CR 701.22a](https://yawgatog.com/resources/magic-rules/#R70122a)): "To 'scry N'
    /// means to look at the top N cards of your library, then put any number of them on the
    /// bottom of your library in any order and the rest on top of your library in any order."
    Scry { player: Players, count: Amount },
    /// "[Player] gains [amount] life"
    /// ([CR 119.3](https://yawgatog.com/resources/magic-rules/#R1193)): "If an effect causes
    /// a player to gain life or lose life, that player's life total is adjusted accordingly."
    GainLife { player: Players, amount: Amount },
    /// "[Player] loses [amount] life" (CR 119.3).
    LoseLife { player: Players, amount: Amount },
    /// "[Player] creates [count] [token] tokens"
    /// ([CR 701.7a](https://yawgatog.com/resources/magic-rules/#R7017a)): "To create one or
    /// more tokens with certain characteristics, put the specified number of tokens with the
    /// specified characteristics onto the battlefield."
    /// [CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112): "The player who creates
    /// a token is its owner. The token enters the battlefield under that player's control."
    /// The token's definition is its text
    /// ([CR 111.3](https://yawgatog.com/resources/magic-rules/#R1113)).
    CreateTokens {
        player: Players,
        token: &'static CardDef,
        count: Amount,
    },
    /// "[Player] creates [count] tokens that are copies of [object]"
    /// ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)): "When copying an
    /// object, the copy acquires the copiable values of the original object's
    /// characteristics".
    CreateCopies {
        player: Players,
        of: Objects,
        count: Amount,
        except: CopyException,
    },
    /// "Put [count] [kind] counters on [objects]"
    /// ([CR 122.1](https://yawgatog.com/resources/magic-rules/#R1221)): "A counter is a
    /// marker placed on an object or player that modifies its characteristics and/or
    /// interacts with a rule, ability, or effect."
    AddCounters {
        objects: Objects,
        kind: CounterKind,
        count: Amount,
    },
    /// "Remove [count] [kind] counters from [objects]" (CR 122.1), or as many as each has
    /// (CR 609.3).
    RemoveCounters {
        objects: Objects,
        kind: CounterKind,
        count: Amount,
    },
    /// "[Player] adds [mana]"
    /// ([CR 106.3](https://yawgatog.com/resources/magic-rules/#R1063)): "A spell or ability
    /// that produces mana instructs a player to add that mana." Where the set has several
    /// kinds, the player chooses one of them.
    AddMana { player: Players, mana: ManaSet },
    /// "[Objects] get, gain or become [modification] [duration]": a continuous effect from a
    /// resolving spell or ability
    /// ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)): it "lasts as long as
    /// stated by the spell or ability creating it (such as 'until end of turn')." What it
    /// affects is settled as it begins
    /// ([CR 611.2c](https://yawgatog.com/resources/magic-rules/#R6112c)): "the set of objects
    /// it affects is determined when that continuous effect begins. After that point, the set
    /// won't change."
    Modify {
        objects: Objects,
        modification: Modification,
        duration: Duration,
    },
}

impl Effect {
    /// No instruction: what an ability that never resolves does, such as a static ability.
    pub const NOTHING: Effect = Effect::Sequence(&[]);

    /// Whether carrying this out may ask a player to choose
    /// ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)): "If an effect of a
    /// spell or ability offers any choices other than choices already made as part of
    /// casting the spell, activating the ability, or otherwise putting the spell or ability
    /// on the stack, the player announces these while applying the effect." The kind of mana
    /// a mana ability adds is chosen as it is activated, so [`Effect::AddMana`] asks nothing.
    pub const fn asks(&self) -> bool {
        match *self {
            Effect::May { .. }
            | Effect::Unless { .. }
            | Effect::Discard { .. }
            | Effect::Scry { .. } => true,
            Effect::Sequence(effects) => {
                let mut i = 0;
                while i < effects.len() {
                    if effects[i].asks() {
                        return true;
                    }
                    i += 1;
                }
                false
            }
            Effect::If {
                then, otherwise, ..
            } => {
                then.asks()
                    || match otherwise {
                        Some(otherwise) => otherwise.asks(),
                        None => false,
                    }
            }
            Effect::ForEachPlayer { effect, .. } => effect.asks(),
            Effect::Counter(_)
            | Effect::Destroy(_)
            | Effect::Exile(_)
            | Effect::ExileUntilThisLeaves(_)
            | Effect::Sacrifice(_)
            | Effect::ReturnToHand(_)
            | Effect::PutIntoLibrary { .. }
            | Effect::Draw { .. }
            | Effect::Mill { .. }
            | Effect::GainLife { .. }
            | Effect::LoseLife { .. }
            | Effect::CreateTokens { .. }
            | Effect::CreateCopies { .. }
            | Effect::AddCounters { .. }
            | Effect::RemoveCounters { .. }
            | Effect::AddMana { .. }
            | Effect::Modify { .. } => false,
        }
    }

    /// Whether this takes what was chosen for the `target`th instance of the word "target"
    /// from where it is in the way `how` names, whatever else it does and whatever it asks
    /// first: a removal destroys, exiles, sacrifices or returns it, or puts it into a
    /// library, and a counter counters it. A player is told as they choose the target
    /// ([`crate::Ability::takes_target`]).
    pub const fn takes(&self, target: SpecIx, how: Takes) -> bool {
        match *self {
            Effect::Destroy(objects)
            | Effect::Exile(objects)
            | Effect::ExileUntilThisLeaves(objects)
            | Effect::Sacrifice(objects)
            | Effect::ReturnToHand(objects)
            | Effect::PutIntoLibrary { objects, .. } => {
                matches!(how, Takes::Removal) && is_target(objects, target)
            }
            Effect::Counter(objects) => matches!(how, Takes::Counter) && is_target(objects, target),
            Effect::Sequence(effects) => {
                let mut i = 0;
                while i < effects.len() {
                    if effects[i].takes(target, how) {
                        return true;
                    }
                    i += 1;
                }
                false
            }
            Effect::If {
                then, otherwise, ..
            } => {
                then.takes(target, how)
                    || match otherwise {
                        Some(otherwise) => otherwise.takes(target, how),
                        None => false,
                    }
            }
            Effect::May { effect, .. }
            | Effect::Unless { effect, .. }
            | Effect::ForEachPlayer { effect, .. } => effect.takes(target, how),
            Effect::Draw { .. }
            | Effect::Discard { .. }
            | Effect::Mill { .. }
            | Effect::Scry { .. }
            | Effect::GainLife { .. }
            | Effect::LoseLife { .. }
            | Effect::CreateTokens { .. }
            | Effect::CreateCopies { .. }
            | Effect::AddCounters { .. }
            | Effect::RemoveCounters { .. }
            | Effect::AddMana { .. }
            | Effect::Modify { .. } => false,
        }
    }
}

/// Whether `objects` is what was chosen for the `target`th instance of the word "target".
const fn is_target(objects: Objects, target: SpecIx) -> bool {
    matches!(objects, Objects::Target(n) if n.get() == target.get())
}

/// Where in a library a card is put.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LibraryPosition {
    /// "On top of".
    Top,
    /// "On the bottom of".
    Bottom,
    /// "Nth from the top", the top card being first
    /// ([CR 401.7](https://yawgatog.com/resources/magic-rules/#R4017)): "If an effect causes
    /// a player to put a card into a library 'Nth from the top,' and that library has fewer
    /// than N cards in it, the player puts that card on the bottom of that library."
    FromTop(u8),
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
