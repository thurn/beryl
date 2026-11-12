//! What a step does: one variant per [`Verb`], carrying all that it changes on the table.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Arrow, Mark, Span, Subject, Verb};
use crate::protocol::{
    CardHandling, Counter, Mana, ManaPool, ObjectView, Side, StackItem, Step as TurnStep, Why,
    ZoneName,
};

/// A verb with what it is applied to. An `object` is always the object's whole record once
/// the step is done, and replaces the one the table held.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "verb", rename_all = "camelCase")]
pub enum Act {
    /// The object travels to where its record puts it: another zone, another controller's
    /// side, or onto the permanent it is now attached to.
    Fly {
        from: ZoneName,
        why: Why,
        object: ObjectView,
        /// Where in its zone's order it goes, counted from the bottom of a pile or the first
        /// permanent: none is the end, in a new zone, and where it was, in the same one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        at: Option<u16>,
        /// The spell it becomes, when it goes to the stack.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        item: Option<StackItem>,
    },
    /// The object is at its place with no flight: a new one (a token, a copy) from
    /// [`ZoneName::Nowhere`], or a card from another zone. `at` and `item` are a `fly`'s.
    Appear {
        from: ZoneName,
        why: Why,
        object: ObjectView,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        at: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        item: Option<StackItem>,
    },
    /// The card or ability goes from where it is with no flight, as `why` says: destroyed,
    /// exiled, sacrificed, countered, returned, resolved. A card is then where `object` puts
    /// it. With no `object`, the subject is no more: an ability leaves the stack, or a token
    /// or copy ceases to exist and frees its slot.
    Vanish {
        subject: Subject,
        from: ZoneName,
        why: Why,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        object: Option<ObjectView>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        at: Option<u16>,
    },
    /// The ability `item` goes on top of the stack, out of its source if it names one, and is
    /// ability `id` from here on.
    Emerge {
        id: u32,
        item: StackItem,
    },
    /// The object turns over in place: its record's `facing`, or whether it has a name, says
    /// which way.
    Flip {
        object: ObjectView,
    },
    /// The permanent taps or untaps, as its record's `tapped` says.
    Tap {
        object: ObjectView,
    },
    /// The creature in `source` deals `amount` combat damage to `to`, a player or the
    /// creature or planeswalker whose record `object` is.
    Strike {
        source: u16,
        to: Subject,
        amount: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        object: Option<ObjectView>,
    },
    /// A streak from `from` lands on `to`: mana on what it pays for, or noncombat damage on
    /// what it hits, whose record `object` is. Mana that stays in a pool is a `count`.
    Streak {
        /// A player, for mana out of their pool. None when the source has no place on the
        /// table.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        from: Option<Subject>,
        to: Subject,
        stream: Stream,
        amount: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        object: Option<ObjectView>,
    },
    /// A number on the table is now this.
    Count {
        tally: Tally,
    },
    /// A part of the table is now this, and nothing moves.
    Set {
        fact: Fact,
    },
    /// The object is held up to be read; its record has the face shown.
    Showcase {
        object: ObjectView,
    },
    /// The spell or ability is left to be read.
    Dwell {
        subject: Subject,
    },
    Announce {
        notice: Notice,
    },
    /// An arrow from `from` to `to` begins or ends.
    Point {
        span: Span,
        arrow: Arrow,
        from: Subject,
        to: Subject,
    },
    /// A mark on `on` begins or ends.
    Mark {
        span: Span,
        mark: Mark,
        on: Subject,
    },
    /// The opponent begins or ends handling cards of a library or hand.
    Lift {
        span: Span,
        handling: CardHandling,
    },
}

impl Act {
    /// The object whose record the step carries, if it carries one.
    pub fn object(&self) -> Option<&ObjectView> {
        match self {
            Act::Fly { object, .. }
            | Act::Appear { object, .. }
            | Act::Flip { object }
            | Act::Tap { object }
            | Act::Showcase { object }
            | Act::Count {
                tally: Tally::Counters { object, .. } | Tally::Stats { object },
            }
            | Act::Set {
                fact: Fact::Object { object },
            } => Some(object),
            Act::Vanish { object, .. }
            | Act::Strike { object, .. }
            | Act::Streak { object, .. } => object.as_ref(),
            _ => None,
        }
    }

    pub(super) fn object_mut(&mut self) -> Option<&mut ObjectView> {
        match self {
            Act::Fly { object, .. }
            | Act::Appear { object, .. }
            | Act::Flip { object }
            | Act::Tap { object }
            | Act::Showcase { object }
            | Act::Count {
                tally: Tally::Counters { object, .. } | Tally::Stats { object },
            }
            | Act::Set {
                fact: Fact::Object { object },
            } => Some(object),
            Act::Vanish { object, .. }
            | Act::Strike { object, .. }
            | Act::Streak { object, .. } => object.as_mut(),
            _ => None,
        }
    }

    pub const fn verb(&self) -> Verb {
        match self {
            Act::Fly { .. } => Verb::Fly,
            Act::Appear { .. } => Verb::Appear,
            Act::Vanish { .. } => Verb::Vanish,
            Act::Emerge { .. } => Verb::Emerge,
            Act::Flip { .. } => Verb::Flip,
            Act::Tap { .. } => Verb::Tap,
            Act::Strike { .. } => Verb::Strike,
            Act::Streak { .. } => Verb::Streak,
            Act::Count { .. } => Verb::Count,
            Act::Set { .. } => Verb::Set,
            Act::Showcase { .. } => Verb::Showcase,
            Act::Dwell { .. } => Verb::Dwell,
            Act::Announce { .. } => Verb::Announce,
            Act::Point { .. } => Verb::Point,
            Act::Mark { .. } => Verb::Mark,
            Act::Lift { .. } => Verb::Lift,
        }
    }
}

/// What a streak is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Stream {
    Mana { color: Mana },
    Damage,
}

/// A number on the table, at its new value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Tally {
    Life {
        player: Side,
        life: i16,
    },
    /// A player's unspent mana.
    Pool {
        player: Side,
        mana: ManaPool,
    },
    /// The object's counters of one kind, as its record has them.
    Counters {
        counter: Counter,
        object: ObjectView,
    },
    /// The object's power and toughness.
    Stats {
        object: ObjectView,
    },
}

/// A part of the table that changes with no motion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Fact {
    /// A step of the turn began.
    Turn {
        number: u16,
        active: Side,
        step: TurnStep,
    },
    /// An object's record, where no verb shows the change: summoning sickness or marked
    /// damage wearing off, a characteristic an effect changed.
    Object { object: ObjectView },
    /// A library's order, bottom first, after a shuffle. Its cards' records change by steps
    /// of their own.
    Library { player: Side, order: Vec<u16> },
    /// A spell or ability on the stack as it now is, its targets changed.
    Item { subject: Subject, item: StackItem },
}

/// What an `announce` says. The words are the stage's, and Arena's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Notice {
    /// "Your Turn" or "Opponent's Turn".
    Turn { active: Side },
    /// "Scry: 1 top, 1 bottom."
    Scry { player: Side, top: u8, bottom: u8 },
    /// A coin an ability of the permanent in `source` flipped: the card the banner names. The
    /// step's cause is the ability, as it resolves.
    Coin {
        player: Side,
        won: bool,
        source: u16,
    },
}
