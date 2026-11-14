//! The kinds of event that a card's abilities may react to, as typed 64-bit sets: what its
//! triggered abilities listen to ([`Listens`]) and what its static abilities replace
//! ([`Replaces`]).

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
        Listens(1 << kind as u8)
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

/// A kind of event that a static ability can replace ([CR 614.1]): "Some continuous effects
/// are replacement effects." A card's [`Replaces`] has one bit for each, so there may be up
/// to 63; the last bit is [`Replaces::PLAYER_RULE_CHANGE`].
///
/// [CR 614.1]: https://yawgatog.com/resources/magic-rules/#R6141
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ReplacementKind {
    /// Its controller's draws (`beryl_card`'s `Replacement::WouldDraw`).
    Draw,
}

/// What a card's static abilities do that the layers' pass over the battlefield collects:
/// the [`ReplacementKind`]s they replace, and one fact that is no replacement,
/// [`Replaces::PLAYER_RULE_CHANGE`]. It has the same three levels as [`Listens`]: every card
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

    /// The mask of one kind of replacement.
    #[inline]
    pub const fn kind(kind: ReplacementKind) -> Replaces {
        Replaces(1 << kind as u8)
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
