//! The listeners of a [`GameTable`]: each card's triggered abilities under the kind of event they
//! listen to, those that work on the battlefield apart from those that work elsewhere
//! ([`crate::listener::AWAY`]).

use beryl_card::{FaceIx, ZoneSet};

use super::GameTable;
use crate::event::{EventKind, Listens};
use crate::ids::CardId;
use crate::listener::Listener;

/// Each card's listeners of one list of a [`GameTable`], one card after another, where each card's
/// start, and for each card the kinds of event they listen to and the zones they work in but the
/// battlefield.
#[derive(Default)]
pub(super) struct Indexed {
    pub(super) listeners: Vec<Listener>,
    pub(super) from: Vec<u32>,
    pub(super) kinds: Vec<Listens>,
    pub(super) zones: Vec<ZoneSet>,
}

impl Indexed {
    /// The next card's listeners.
    pub(super) fn push<'a>(&mut self, of: impl Iterator<Item = &'a Listener>) {
        self.from
            .push(u32::try_from(self.listeners.len()).expect("fewer listeners"));
        let (mut kinds, mut zones) = (Listens::NONE, ZoneSet::NONE);
        for l in of {
            kinds |= Listens::event(l.kind);
            zones = zones.union(l.zones.without(ZoneSet::BATTLEFIELD));
            self.listeners.push(*l);
        }
        self.kinds.push(kinds);
        self.zones.push(zones);
    }

    /// The lists, with the end of the last card's listeners after it.
    pub(super) fn finish(mut self) -> Indexed {
        self.from
            .push(u32::try_from(self.listeners.len()).expect("fewer listeners"));
        self
    }
}

impl GameTable {
    /// The event kinds any card of this game listens to.
    #[inline]
    pub fn listens_any(&self) -> Listens {
        self.listens_any
    }

    /// The event kinds the triggered abilities of the card at `card` listen to. The crate's
    /// own: others ask [`crate::Copiable::may_listen`], which is nothing for a face-down
    /// permanent whatever its hidden card listens to.
    #[inline]
    pub(crate) fn listens(&self, card: impl CardId) -> Listens {
        self.listens[card.index()]
    }

    /// The triggered abilities of the face `face` of the card at `card` that listen to events of
    /// `kind`, in the order the face lists them: what an event of that kind is matched against, and
    /// nothing else of the face is read. A face-down permanent's face has none ([CR 708.2a]).
    ///
    /// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
    #[inline]
    pub fn listeners(
        &self,
        card: impl CardId,
        face: FaceIx,
        kind: EventKind,
    ) -> impl Iterator<Item = &Listener> {
        let from = self.listeners_from[card.index()] as usize;
        let to = self.listeners_from[card.index() + 1] as usize;
        self.listeners[from..to]
            .iter()
            .filter(move |l| l.kind == kind && l.face == face)
    }

    /// The triggered abilities of the face `face` of the card at `card` that listen to events of
    /// `kind` and work in a zone of [`listener::AWAY`], in the order the face lists them: what an
    /// event is matched against for a card in such a zone, which must be one its ability works in
    /// ([`Listener::zones`]).
    #[inline]
    pub fn away_listeners(
        &self,
        card: impl CardId,
        face: FaceIx,
        kind: EventKind,
    ) -> impl Iterator<Item = &Listener> {
        let from = self.away_from[card.index()] as usize;
        let to = self.away_from[card.index() + 1] as usize;
        self.away_listeners[from..to]
            .iter()
            .filter(move |l| l.kind == kind && l.face == face)
    }

    /// The zones of [`listener::AWAY`] in which some triggered ability of the card at `card`
    /// works.
    #[inline]
    pub fn works_away(&self, card: impl CardId) -> ZoneSet {
        self.works_away[card.index()]
    }

    /// Every zone of [`listener::AWAY`] in which a triggered ability of some card of the game
    /// works: none in most games, which then never look outside the battlefield.
    #[inline]
    pub fn zones_away(&self) -> ZoneSet {
        self.zones_away
    }

    /// Every kind of event a triggered ability that works outside the battlefield listens to.
    #[inline]
    pub fn listens_away(&self) -> Listens {
        self.listens_away
    }

    /// Every kind of event any triggered ability of the game listens to, wherever it works:
    /// [`GameTable::listens_any`] and [`GameTable::listens_away`] together.
    #[inline]
    pub fn listens_anywhere(&self) -> Listens {
        self.listens_anywhere
    }
}
