//! The listeners of a [`GameTable`]: each card's triggered abilities under the kind of event they
//! listen to, those that work on the battlefield apart from those that work elsewhere
//! ([`crate::listener::AWAY`]).
//!
//! Implements: none. Indexes each game's triggered abilities by event kind and zone.

use beryl_card::{AbilityIx, AbilityKind, CardDef, Face, FaceIx, GrantIx, ZoneSet};

use super::{GameTable, TableError};
use crate::event::{EventKind, Listens};
use crate::ids::CardId;
use crate::listener::{self, Listener};

/// Each card's listeners of one list of a [`GameTable`], one card after another, where each card's
/// start, and for each card the kinds of event they listen to and the zones they work in but the
/// battlefield.
#[derive(Clone, Debug, Default)]
pub(super) struct Indexed {
    pub(super) listeners: Vec<Listener>,
    pub(super) from: Vec<u32>,
    pub(super) kinds: Vec<Listens>,
    pub(super) zones: Vec<ZoneSet>,
    /// Every card's kinds together, once the lists are finished.
    pub(super) all: Listens,
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
        self.all = self.kinds.iter().fold(Listens::NONE, |a, &b| a | b);
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
    /// nothing else of the face is read. A face-down permanent's face has none (see [CR 708.2a]).
    ///
    /// [CR 708.2a]: https://yawgatog.com/resources/magic-rules/#R7082a
    #[inline]
    pub fn listeners(
        &self,
        card: impl CardId,
        face: FaceIx,
        kind: EventKind,
    ) -> impl Iterator<Item = &Listener> {
        of_face(&self.listeners, &self.listeners_from, card, face, kind)
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
        of_face(&self.away_listeners, &self.away_from, card, face, kind)
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

impl GameTable {
    /// Every kind of event a triggered ability that a card of the game grants other
    /// permanents listens to ([`super::Capability::GrantsTriggers`]): among
    /// [`GameTable::listens_any`] and [`GameTable::listens_away`], and matched apart from the
    /// faces of the battlefield's definitions, from the rows of the table of grants
    /// (`beryl_rules`' `trigger`). They are not among what the layer results say the
    /// battlefield listens to ([`crate::State::listening`]), so that a game with no such card
    /// pays nothing for them as the layers run.
    #[inline]
    pub fn listens_granted(&self) -> Listens {
        self.granted.all
    }

    /// The triggered ability at `grant` among the grants of the front face of the card at
    /// `card`, if a continuous effect grants it and the engine matches it as a granted one
    /// ([`listener::granted_kind`]): its trigger and the kind of event it listens to. The
    /// listener's [`Listener::ability`] is the index of the grant.
    #[inline]
    pub fn granted_trigger(&self, card: impl CardId, grant: GrantIx) -> Option<&Listener> {
        let offset =
            |i: usize| usize::try_from(self.granted.from[i]).expect("a listener offset fits");
        self.granted.listeners[offset(card.index())..offset(card.index() + 1)]
            .iter()
            .find(|l| l.ability.index() == grant.index())
    }
}

/// The triggered abilities among the grants of `def`'s front face that a continuous effect
/// grants and the engine matches from the table of grants ([`listener::granted_kind`]), each
/// with the index of its grant as its [`Listener::ability`].
pub(super) fn granted(def: &CardDef) -> Vec<Listener> {
    let Some(face) = def.faces.first() else {
        return Vec::new();
    };
    (0..face.grants.len())
        .filter(|&i| super::capability::granted_trigger(face, i))
        .filter_map(|i| {
            let ability = &face.grants[i];
            Some(Listener {
                kind: listener::granted_kind(ability)?,
                face: FaceIx::FRONT,
                ability: AbilityIx::new(u8::try_from(i).expect("a face of at most 64 grants")),
                trigger: ability.trigger()?,
                zones: ability.zones(),
            })
        })
        .collect()
}

/// The listeners of `face` of the card at `card` that listen to `kind`, from one list of a
/// [`GameTable`] and the index of where each card's start in it ([`Indexed`]).
#[inline]
fn of_face<'a>(
    listeners: &'a [Listener],
    from: &[u32],
    card: impl CardId,
    face: FaceIx,
    kind: EventKind,
) -> impl Iterator<Item = &'a Listener> {
    let offset = |i: usize| usize::try_from(from[i]).expect("a listener offset fits in usize");
    listeners[offset(card.index())..offset(card.index() + 1)]
        .iter()
        .filter(move |l| l.kind == kind && l.face == face)
}

/// Whether the engine finds each ability of `def`, and each it grants, wherever it says it
/// works ([`listener::found_where_it_works`]), and reads each it grants
/// ([`listener::read_when_granted`]).
pub(super) fn found_where_they_work(def: &CardDef) -> Result<(), TableError> {
    let card = def.name;
    let mut faces = def.faces.iter();
    let mut abilities = faces
        .clone()
        .flat_map(|f| f.abilities.iter().chain(f.grants));
    if !abilities.all(listener::found_where_it_works) {
        return Err(TableError::Zone { card });
    }
    // A triggered ability a copy exception gives is matched from the copy's copiable values
    // (`listener::given`), not from what a continuous effect grants.
    let read = |f: &Face| {
        (0..f.grants.len()).all(|i| {
            let ability = &f.grants[i];
            listener::read_when_granted(ability)
                || (ability.kind() == AbilityKind::Triggered && listener::given_by_exception(f, i))
        })
    };
    if !faces.all(read) {
        return Err(TableError::Granted { card });
    }
    Ok(())
}
