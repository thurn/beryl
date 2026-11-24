//! The kinds of event that a card's abilities may react to, as typed 64-bit sets: what its
//! triggered abilities listen to ([`Listens`]) and what its static abilities replace
//! ([`Replaces`]).
//!
//! Implements: none. Indexes events by kind for trigger and replacement lookups.

use beryl_card::Zone;

use crate::layout::Layout;

use super::{Event, Reason};

/// A kind of event a triggered ability can listen to (§6.4). A card's [`Listens`] has one
/// bit for each, so there may be up to 64.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EventKind {
    Enters,
    Attack,
    Damage,
    Step,
    Tokens,
    Cast,
    Dies,
    Discard,
    Draw,
    /// A permanent leaving the battlefield for any zone: matched, as "dies" is, before the
    /// permanents of the event move (see [CR 603.10a]).
    ///
    /// [CR 603.10a]: https://yawgatog.com/resources/magic-rules/#R60310a
    Leaves,
    /// No event: a state trigger, matched as state-based actions are checked (see [CR 603.8]):
    /// "Some triggered abilities trigger when a game state ... is true, rather than triggering
    /// when an event occurs."
    ///
    /// [CR 603.8]: https://yawgatog.com/resources/magic-rules/#R6038
    State,
    /// An object becoming the target of a spell or ability ([`crate::event::Event::Targeted`]).
    Targeted,
    /// A mutating creature spell merging with a creature ([`crate::event::Event::Mutated`]).
    Mutated,
    TurnedFaceUp,
    /// Lore added to the source of a chapter ability, matched by the lore placement procedure.
    Chapter,
    ClassLevel,
}

impl EventKind {
    const fn bit(self) -> u64 {
        1 << self as u8
    }
}

/// The [`EventKind`]s that a card's triggered abilities listen to, as one 64-bit mask, at
/// three levels: every card of the game ([`crate::GameTable::listens_any`]), the permanents
/// on the battlefield now ([`crate::State::listening`]) and the face of one card
/// ([`crate::Copiable::may_listen`]). A kind that no card listens to costs one AND.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Listens(u64);

impl Listens {
    pub const NONE: Listens = Listens(0);

    /// The mask of one kind of event.
    #[inline]
    pub const fn event(kind: EventKind) -> Listens {
        Listens(kind.bit())
    }

    /// Whether it listens to events of `kind`.
    #[inline]
    pub const fn hears(self, kind: EventKind) -> bool {
        self.0 & Listens::event(kind).0 != 0
    }

    /// Whether it has any kind of `other`.
    #[inline]
    pub const fn intersects(self, other: Listens) -> bool {
        self.0 & other.0 != 0
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for Listens {
    type Output = Listens;

    #[inline]
    fn bitor(self, other: Listens) -> Listens {
        Listens(self.0 | other.0)
    }
}

impl std::ops::BitOr<EventKind> for Listens {
    type Output = Listens;

    #[inline]
    fn bitor(self, kind: EventKind) -> Listens {
        self | Listens::event(kind)
    }
}

impl std::ops::BitOrAssign for Listens {
    #[inline]
    fn bitor_assign(&mut self, other: Listens) {
        self.0 |= other.0;
    }
}

/// A kind of event that a static ability can replace (see [CR 614.1]): "Some continuous effects
/// are replacement effects." A card's [`Replaces`] has one bit for each, so there may be up
/// to 62; the last two bits are [`Replaces::PROTECTION`] and
/// [`Replaces::PLAYER_RULE_CHANGE`].
///
/// [CR 614.1]: https://yawgatog.com/resources/magic-rules/#R6141
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ReplacementKind {
    /// Draws (`beryl_card`'s `Replaced::Draw`).
    Draw,
    /// Draws replaced by instructions the rules carry out (`beryl_rules`' `Instead::Instructions`),
    /// which may ask a player a question in the middle of a step. Each such card also has
    /// [`ReplacementKind::Draw`].
    DrawInstructions,
    /// An object being put into a zone (`beryl_card`'s `Replaced::PutInto`), or entering the
    /// battlefield (`Replaced::Enters`).
    Move,
    /// Damage (`beryl_card`'s `Replaced::Damage`).
    Damage,
    /// A player beginning a turn (`beryl_card`'s `Replaced::BeginTurn`).
    BeginTurn,
}

impl ReplacementKind {
    const fn bit(self) -> u64 {
        1 << self as u8
    }
}

/// What a card's static abilities do that the layers' pass over the battlefield collects:
/// the [`ReplacementKind`]s they replace, one fact that is no replacement,
/// [`Replaces::PLAYER_RULE_CHANGE`], and, at the state's level only, whether a permanent may
/// have protection ([`Replaces::PROTECTION`]). It has the same three levels as [`Listens`]: every card
/// of the game ([`crate::GameTable::replaces_any`]), the permanents on the battlefield now
/// ([`crate::State::replacing`]) and the face of one card
/// ([`crate::Copiable::may_replace`]). Kept apart from [`Listens`] so that the gate of a
/// replacement is its own AND, and the 64 trigger kinds keep every bit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Replaces(u64);

impl Replaces {
    pub const NONE: Replaces = Replaces(0);
    /// Not a replacement: a static ability of it changes a rule for players
    /// ([`crate::rule_changes::RuleKind::of_players`]). The layers' pass collects it with the
    /// replacement kinds, so it learns in the same read of each permanent whether any
    /// permanent changes a rule ([`crate::derive::Rederive::finish`]).
    pub const PLAYER_RULE_CHANGE: Replaces = Replaces(1 << 63);
    /// Not a replacement but a prevention effect: a continuous effect gave a permanent
    /// protection (see [CR 702.16e]): "Any damage that would be dealt by sources that have the
    /// stated quality to a permanent or player with protection is prevented." Only the state's
    /// level has it, set by the layers' pass when an effect that gives protection applied, so
    /// that damage, targets, blocks and attachments ask a permanent's protection only then.
    /// An effect that removed the protection again leaves it set, which costs those reads.
    ///
    /// [CR 702.16e]: https://yawgatog.com/resources/magic-rules/#R70216e
    pub const PROTECTION: Replaces = Replaces(1 << 62);

    /// The mask of one kind of replacement.
    #[inline]
    pub const fn kind(kind: ReplacementKind) -> Replaces {
        Replaces(kind.bit())
    }

    /// Whether it replaces events of `kind`.
    #[inline]
    pub const fn replaces(self, kind: ReplacementKind) -> bool {
        self.0 & Replaces::kind(kind).0 != 0
    }

    /// Whether a static ability of it changes a rule for players.
    #[inline]
    pub const fn changes_player_rules(self) -> bool {
        self.0 & Replaces::PLAYER_RULE_CHANGE.0 != 0
    }

    /// Whether a permanent may have protection ([`Replaces::PROTECTION`]).
    #[inline]
    pub const fn protects(self) -> bool {
        self.0 & Replaces::PROTECTION.0 != 0
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for Replaces {
    type Output = Replaces;

    #[inline]
    fn bitor(self, other: Replaces) -> Replaces {
        Replaces(self.0 | other.0)
    }
}

impl std::ops::BitOr<ReplacementKind> for Replaces {
    type Output = Replaces;

    #[inline]
    fn bitor(self, kind: ReplacementKind) -> Replaces {
        self | Replaces::kind(kind)
    }
}

impl std::ops::BitOrAssign for Replaces {
    #[inline]
    fn bitor_assign(&mut self, other: Replaces) {
        self.0 |= other.0;
    }
}

impl<L: Layout> Event<L> {
    /// The kind of event a trigger may listen to this as, or `None` if no trigger listens to
    /// it.
    pub fn kind(&self) -> Option<EventKind> {
        Some(match self {
            Event::ZoneChange {
                to: Zone::Battlefield,
                ..
            } => EventKind::Enters,
            Event::ZoneChange {
                reason: Reason::Discard,
                ..
            } => EventKind::Discard,
            Event::Attack { .. } => EventKind::Attack,
            Event::Damage { .. } => EventKind::Damage,
            Event::StepBegins { .. } => EventKind::Step,
            Event::TokensCreated { .. } => EventKind::Tokens,
            Event::Cast { .. } => EventKind::Cast,
            Event::Draw { .. } => EventKind::Draw,
            Event::Targeted { .. } => EventKind::Targeted,
            Event::Mutated { .. } => EventKind::Mutated,
            Event::TurnedFaceUp { .. } => EventKind::TurnedFaceUp,
            Event::ClassLevel { .. } => EventKind::ClassLevel,
            Event::ZoneChange { .. }
            | Event::DrawCard { .. }
            | Event::Scry { .. }
            | Event::TopOrBottom { .. }
            | Event::LibraryShuffled { .. }
            | Event::Reveal { .. }
            | Event::LifeGain { .. }
            | Event::LifeLoss { .. }
            | Event::CountersAdded { .. }
            | Event::CountersRemoved { .. }
            | Event::Tap { .. }
            | Event::Untap { .. }
            | Event::Mana { .. }
            | Event::ResolutionPayment { .. }
            | Event::AbilityFinished
            | Event::GameRestarted
            | Event::StackArrival { .. }
            | Event::StackDeparture { .. }
            | Event::Ability { .. }
            | Event::Block { .. }
            | Event::TurnPlanChanged(_)
            | Event::GainControl { .. }
            | Event::CoinFlip { .. }
            | Event::Lose { .. }
            | Event::Win { .. } => return None,
        })
    }
}
