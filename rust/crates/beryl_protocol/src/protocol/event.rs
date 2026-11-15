//! The game events a view carries.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Counter, Mana, ObjectId, Side, StackItem, Step, TargetRef, Why, ZoneName};

/// One thing that happened (architecture §8.1), by public slot and relative to the viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GameEvent {
    /// The object now in `slot` arrived from another zone.
    ZoneChange {
        slot: ObjectId,
        from: ZoneName,
        to: ZoneName,
        reason: Why,
        /// Whether a spell went to the top or the bottom of its owner's library.
        #[serde(rename = "libraryTop", skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        library_top: Option<bool>,
    },
    Damage {
        source: ObjectId,
        to: TargetRef,
        amount: u8,
        combat: bool,
    },
    Draw {
        player: Side,
        cards: u8,
        /// The public slots drawn, top first. Older recordings carry only the count.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        slots: Option<Vec<ObjectId>>,
    },
    Scry {
        player: Side,
        top: u8,
        bottom: u8,
    },
    /// A player put a card on the top or the bottom of their library by their own choice.
    TopOrBottom {
        player: Side,
        top: bool,
    },
    LibraryShuffled {
        player: Side,
    },
    /// The card in `slot` was revealed to every player
    /// ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a card,
    /// show that card to all players for a brief time."
    Reveal {
        /// The player whose effect revealed it.
        player: Side,
        slot: ObjectId,
    },
    LifeGain {
        player: Side,
        amount: u8,
    },
    LifeLoss {
        player: Side,
        amount: u8,
    },
    CountersAdded {
        slot: ObjectId,
        counter: Counter,
        amount: u8,
    },
    CountersRemoved {
        slot: ObjectId,
        counter: Counter,
        amount: u8,
    },
    Tap {
        slot: ObjectId,
    },
    Untap {
        slot: ObjectId,
    },
    /// Mana added to a player's mana pool
    /// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
    /// instructs a player to add mana, that mana goes into a player's mana pool." The UI
    /// shows it flow from its source to the spell or ability it pays for.
    Mana {
        player: Side,
        /// The mana ability's permanent, or the spell or ability that added it, in the slot
        /// it had then: a source sacrificed for mana has left it by the time the view shows.
        source: Option<ObjectId>,
        color: Mana,
        amount: u8,
    },
    /// Mana paid into the named resolving spell or ability.
    ResolutionPayment {
        player: Side,
        item: StackItem,
    },
    /// The top ability finished resolving and left the stack.
    AbilityFinished,
    TokensCreated {
        controller: Side,
        tokens: Vec<ObjectId>,
    },
    Cast {
        slot: ObjectId,
        controller: Side,
        /// The spell as it went on the stack: an opponent's spell that resolves unanswered
        /// is gone from the stack by the next view, which shows its cast and resolution
        /// together.
        item: StackItem,
    },
    /// An ability was activated, or a triggered one with targets put on the stack, as the
    /// stack then showed it.
    Ability {
        item: StackItem,
        /// A loyalty ability's cost: the loyalty counters it added or removed, which the
        /// activation says rather than the counters
        /// ([CR 606.4](https://yawgatog.com/resources/magic-rules/#R6064)): "The cost to activate
        /// a loyalty ability of a permanent is to put on or remove from that permanent a certain
        /// number of loyalty counters, as shown by the loyalty symbol in the ability's cost."
        loyalty: Option<i8>,
    },
    Attack {
        player: Side,
        attackers: Vec<ObjectId>,
    },
    /// `player` declared the creature in `blocker` a blocker of the one in `attacker`
    /// ([CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a): "For each of the
    /// chosen creatures, the defending player chooses one creature for it to block"): one
    /// event for each blocker of a declaration.
    Block {
        player: Side,
        blocker: ObjectId,
        attacker: ObjectId,
    },
    StepBegins {
        step: Step,
        active: Side,
    },
    GainControl {
        slot: ObjectId,
        player: Side,
    },
    CoinFlip {
        player: Side,
        won: bool,
        /// The permanent whose ability flipped it.
        source: ObjectId,
    },
    Lose {
        player: Side,
    },
    Win {
        player: Side,
    },
}

