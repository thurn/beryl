//! How a game ended, and the abilities that name its cause.

use serde::{Deserialize, Serialize};

use super::{Side, Stall};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OutcomeView {
    Win {
        winner: Side,
        /// Why the other player lost.
        reason: LossReason,
        /// The card that made them lose, when one did and the session saw it.
        by: Option<Culprit>,
    },
    Draw,
    /// The engine gave up when a budget or capacity ran out; not a rules outcome.
    Stalled {
        reason: Stall,
    },
}

/// Why a player lost the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LossReason {
    /// [CR 704.5a](https://yawgatog.com/resources/magic-rules/#R7045a): "If a player has 0 or
    /// less life, that player loses the game."
    Life,
    /// [CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b): "If a player attempted
    /// to draw a card from a library with no cards in it since the last time state-based
    /// actions were checked, that player loses the game."
    Library,
    /// An effect had the other player win
    /// ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)): "An effect may state
    /// that a player wins the game."
    Effect,
    /// [CR 104.3a](https://yawgatog.com/resources/magic-rules/#R1043a): "A player can concede
    /// the game at any time."
    Concede,
}

/// The card that ended the game: the one whose effect had a player win or lose, or the source
/// of the damage that took a player's last life.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Culprit {
    /// The name of its face.
    pub name: String,
    /// Which of its abilities did it, as its card's text orders them; none for a spell, or
    /// for damage.
    pub ability: Option<AbilityRef>,
    /// The ability's rules text, when the printed text of its face has no paragraph that
    /// says it ([`StackItem::Ability`](super::StackItem::Ability)'s `text`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// One of a face's abilities, as its rules text finds it: the `nth` of its `kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbilityRef {
    pub kind: AbilityClass,
    pub nth: u8,
}

/// The kinds of ability a face's rules text tells apart
/// ([CR 113.3](https://yawgatog.com/resources/magic-rules/#R1133)): "There are four general
/// categories of abilities", of which spell abilities have no paragraph of their own and mana
/// abilities are written as activated ones are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AbilityClass {
    Triggered,
    Activated,
    /// A static ability, one that creates a replacement effect included.
    Static,
}
