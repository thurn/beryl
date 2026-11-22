//! The Memos store: what an object remembers of how it was cast or entered.
//!
//! A memo is written once, with [`State::remember`], and read by the object's own abilities
//! through a query by kind: [`State::paid`], [`State::x_of`]. It ends with its object,
//! inside the zone move
//! (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): "An object that moves
//! from one zone to another becomes a new object with no memory of, or relation to, its
//! previous existence."), so an entry names a slot and is always about the object now in
//! it. A kind with nothing live costs its reader one AND.
//!
//! Built so far: what a permanent remembers of the spell it was, the costs paid for it and
//! the value of X (by-w8gv.7.6); and what exiled a card, a memo of the exiled card naming
//! the slot of what exiled it (by-w8gv.7.7). Such a memo ends with the card as every memo
//! does, and one of the two also as a new object arrives in the slot it names
//! ([`Memo::ExiledWith`]); and the value a permanent chose as it entered
//! ([`Memo::Chosen`]).
//!
//! Implements: none. Stores facts about objects for their abilities to read later.

use beryl_card::{CastingKind, CastingKinds};

use crate::bitboard::Bitboard;
use crate::chosen::ChosenValue;
use crate::fit::{Misfit, room};
use crate::fixed::Full;
use crate::ids::{Seat, SlotId};
use crate::invariants::{At, Violation, check};
use crate::layout::{Layout, Queue, Small};
use crate::observation::Observation;
use crate::player_set::PlayerSet;
use crate::relayout::{Relayout, queue, slot};
use crate::state::{State, Zone};
use crate::store::{Duration, Ended, Store};
use crate::table::GameTable;

/// The memos a [`Small`] game holds at once. One past it stalls the game into
/// [`crate::Large`], whose store grows.
pub const MEMOS: usize = 32;

/// One thing an object remembers. A new kind is a variant here, its arm in each of the
/// methods below and its reader: the store's own code names no kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Memo<L: Layout = Small> {
    /// The permanent's spell was cast with these kinds of cost paid, a kicker cost among
    /// them or an evoke cost
    /// (see [CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d): "If a spell's
    /// controller declares the intention to pay any of that spell's kicker costs, that
    /// spell has been 'kicked.'"). Never the empty set: a permanent whose spell paid none
    /// remembers nothing.
    Paid(CastingKinds),
    /// The value of X the permanent's spell was cast with, for its enters abilities
    /// (see [CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)): "the value of X
    /// for that ability is the same as the value of X for that spell". Never 0, which is
    /// what a permanent with no such memo answers.
    X(u8),
    /// The card in exile was exiled by an ability of the object in this slot, whose linked
    /// ability refers to the cards "exiled with" it
    /// (see [CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "If an object has
    /// an activated or triggered ability printed on it that instructs a player to exile one
    /// or more cards and an ability printed on it that refers either to 'the exiled cards'
    /// or to cards 'exiled with [this object],' these abilities are linked."
    ///
    /// It names the slot and not one incarnation of it: the card stays exiled with that
    /// object after the object has left its zone, for an ability of it still on the stack,
    /// until a new object arrives in the slot on the battlefield or the slot of a token is
    /// freed ([`State::forget_links_to`]).
    ExiledWith(L::Slot),
    /// The card in exile was exiled until the permanent in this slot leaves the
    /// battlefield, which returns it
    /// (see [CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)): "Some one-shot
    /// effects cause an object to change zones 'until' a specified event occurs."
    ExiledUntil(L::Slot),
    /// The value the permanent chose as it entered: a color, a creature type or a card name
    /// (see [CR 614.12a](https://yawgatog.com/resources/magic-rules/#R61412a)): "If a replacement
    /// effect that modifies how a permanent enters the battlefield requires a choice, that
    /// choice is made before the permanent enters the battlefield." Not a copiable value
    /// (see [CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)): "Other effects
    /// (including type-changing and text-changing effects), status, counters, and stickers
    /// are not copied." A copy of the permanent makes its own choice as it enters, and a
    /// permanent that becomes a copy keeps this one.
    Chosen(ChosenValue<L>),
    /// The card the permanent really is, at this game index, while a copy effect gives it the
    /// values of another ([`crate::effects::copy`]): its definition field names the one it
    /// copies, so that every reader of its values reads them as it would of any object, and
    /// this is what it is again once it leaves the battlefield (see [CR 707.2]): "When copying an
    /// object, the copy acquires the copiable values of the original object's characteristics".
    /// A token has none: a token copy is what it copied wherever it is (see CR 111.3).
    ///
    /// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
    Really(L::Card),
    /// The permanent was put onto the battlefield by an ability of the Aura in this slot,
    /// which says so of it: Animate Dead's "enchant creature put onto the battlefield with
    /// this Aura", and its "that creature" once the Aura has left. It names the slot as
    /// [`Memo::ExiledWith`] does, for an ability of the Aura still on the stack after the
    /// Aura left, until a new object arrives in that slot ([`State::forget_links_to`]).
    PutOntoBattlefieldWith(L::Slot),
    /// The phased-out permanent phased out under this player's control, who is not the player
    /// who put it onto the battlefield: it phases in during that player's untap step
    /// (see [CR 702.26a]): "all phased-out permanents that had phased out under that player's
    /// control “phase in.”" A permanent without it phased out under its base controller's
    /// control ([`crate::phasing`]).
    ///
    /// [CR 702.26a]: https://yawgatog.com/resources/magic-rules/#R70226a
    PhasedOutUnder(Seat),
    /// The card is one of the cards of the merged permanent in this slot, other than the
    /// card in the permanent's own slot (see [CR 731.2]): "That permanent becomes a merged
    /// permanent represented by the card or copy that represented that object in addition to
    /// any other components that were representing it." It waits in [`Zone::Merged`], in no
    /// battlefield list, and goes where the permanent goes as it leaves the battlefield
    /// ([`State::pile`]). Which card is on top is the permanent's copiable values' to say
    /// ([`crate::query::Copiable::merged_with`]).
    ///
    /// [CR 731.2]: https://yawgatog.com/resources/magic-rules/#R7312
    PartOf(L::Slot),
    /// The card the permanent really is ([`Memo::Really`]) was face down when a card merged
    /// over it, and stays a face-down component of the merged permanent, which only these
    /// players know (see [CR 731.2e]): "If a merged permanent contains face-up and face-down
    /// components, the permanent's status is determined by its topmost component." The
    /// merged permanent itself is face up and every player knows it, so its slot is known to
    /// all ([`State::knows`]); this says who knows the card behind it
    /// ([`State::knows_card_really_is`]). It is revealed as the permanent leaves the
    /// battlefield (see [CR 708.9]): "If a face-down permanent or a face-down component of a
    /// merged permanent moves from the battlefield to any other zone, its owner must reveal
    /// it to all players as they move it."
    ///
    /// [CR 731.2e]: https://yawgatog.com/resources/magic-rules/#R7312e
    /// [CR 708.9]: https://yawgatog.com/resources/magic-rules/#R7089
    FaceDownUnder(PlayerSet),
    /// This permanent entered by resolving a face-down spell cast for morph (see CR 702.37c).
    Morphed,
}

impl<L: Layout> Memo<L> {
    /// The bits of its kinds in a store's [`Kinds`].
    #[inline]
    fn kinds(self) -> u32 {
        match self {
            Memo::Paid(paid) => Kinds::paid(paid),
            Memo::X(_) => Kinds::X,
            Memo::ExiledWith(_) => Kinds::EXILED_WITH,
            Memo::ExiledUntil(_) => Kinds::EXILED_UNTIL,
            Memo::Chosen(_) => Kinds::CHOSEN,
            Memo::Really(_) => Kinds::REALLY,
            Memo::PutOntoBattlefieldWith(_) => Kinds::PUT_WITH,
            Memo::PhasedOutUnder(_) => Kinds::PHASED_OUT_UNDER,
            Memo::PartOf(_) => Kinds::PART_OF,
            Memo::FaceDownUnder(_) => Kinds::FACE_DOWN_UNDER,
            Memo::Morphed => Kinds::MORPHED,
        }
    }

    /// The zone of an object that remembers it: a permanent remembers its spell, a card in
    /// exile what exiled it.
    #[inline]
    fn zone(self) -> Zone {
        match self {
            Memo::Paid(_)
            | Memo::X(_)
            | Memo::Chosen(_)
            | Memo::Really(_)
            | Memo::PutOntoBattlefieldWith(_)
            | Memo::PhasedOutUnder(_)
            | Memo::FaceDownUnder(_)
            | Memo::Morphed => Zone::Battlefield,
            Memo::ExiledWith(_) | Memo::ExiledUntil(_) => Zone::Exile,
            Memo::PartOf(_) => Zone::Merged,
        }
    }

    /// Whether the layers read it: a static ability may apply only while a cost was paid,
    /// as one with impending does
    /// (see [CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a)).
    #[inline]
    fn layer_input(self) -> bool {
        matches!(self, Memo::Paid(_))
    }

    /// The slot it names beside the object that remembers it: what exiled the card. A
    /// kind that names one has its bit in [`Kinds::NAMING`], which the store's invariant
    /// holds it to.
    #[inline]
    fn names(self) -> Option<L::Slot> {
        match self {
            Memo::Paid(_)
            | Memo::X(_)
            | Memo::Chosen(_)
            | Memo::Really(_)
            | Memo::PhasedOutUnder(_)
            | Memo::FaceDownUnder(_)
            | Memo::Morphed => None,
            Memo::ExiledWith(by)
            | Memo::ExiledUntil(by)
            | Memo::PutOntoBattlefieldWith(by)
            | Memo::PartOf(by) => Some(by),
        }
    }

    /// Whether it says something, and of an exiler that is where such a memo needs it:
    /// what the store's invariant holds each memo to, given the zone of the object in a
    /// slot.
    fn holds(self, zone: impl Fn(L::Slot) -> Zone) -> bool {
        match self {
            Memo::X(x) => x > 0,
            Memo::Paid(paid) => !paid.is_empty(),
            Memo::Chosen(value) => value.holds(),
            Memo::Really(_) | Memo::PhasedOutUnder(_) | Memo::Morphed => true,
            // Somebody knew the face-down card: at least its controller (see CR 708.5).
            Memo::FaceDownUnder(knew) => !knew.is_empty(),
            // What exiled it left a card or a token in its slot.
            Memo::ExiledWith(by) | Memo::PutOntoBattlefieldWith(by) => zone(by) != Zone::Nowhere,
            // An object exiled "until" its exiler leaves returns as it leaves (see CR 610.3).
            Memo::ExiledUntil(by) => zone(by) == Zone::Battlefield,
            // A card is part of a permanent only while it is on the battlefield.
            Memo::PartOf(of) => zone(of) == Zone::Battlefield,
        }
    }

    /// The memo in layout `M`: a slot it names at the same number.
    fn relayout<M: Layout>(self) -> Memo<M> {
        match self {
            Memo::Paid(paid) => Memo::Paid(paid),
            Memo::X(x) => Memo::X(x),
            Memo::ExiledWith(by) => Memo::ExiledWith(slot(by)),
            Memo::ExiledUntil(by) => Memo::ExiledUntil(slot(by)),
            Memo::Chosen(value) => Memo::Chosen(value.relayout()),
            Memo::Really(card) => Memo::Really(crate::relayout::card(card)),
            Memo::PutOntoBattlefieldWith(by) => Memo::PutOntoBattlefieldWith(slot(by)),
            Memo::PhasedOutUnder(player) => Memo::PhasedOutUnder(player),
            Memo::PartOf(of) => Memo::PartOf(slot(of)),
            Memo::FaceDownUnder(knew) => Memo::FaceDownUnder(knew),
            Memo::Morphed => Memo::Morphed,
        }
    }

    /// Whether `other` is of the same variant: an object has one memo of each.
    #[inline]
    fn same_variant(self, other: Memo<L>) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }
}

/// The kinds of memo among a store's entries: its index by kind, so that a reader of a kind
/// with nothing live stops at one AND. One bit for the value of X, one for each kind of
/// cost the vocabulary's list has, one for each way a card remembers what exiled it, and one
/// for a chosen value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Kinds(u32);

impl Kinds {
    const X: u32 = 1 << 0;
    const EXILED_WITH: u32 = 1 << 9;
    const EXILED_UNTIL: u32 = 1 << 10;
    const CHOSEN: u32 = 1 << 11;
    const REALLY: u32 = 1 << 12;
    const PUT_WITH: u32 = 1 << 13;
    const PHASED_OUT_UNDER: u32 = 1 << 14;
    const PART_OF: u32 = 1 << 15;
    const FACE_DOWN_UNDER: u32 = 1 << 16;
    const MORPHED: u32 = 1 << 17;
    /// The kinds whose memo names a slot ([`Memo::names`]).
    const NAMING: u32 = Kinds::EXILED_WITH | Kinds::EXILED_UNTIL | Kinds::PUT_WITH | Kinds::PART_OF;
    /// The kinds whose memo ends as a new object arrives in the slot it names
    /// ([`State::forget_links_to`]).
    const LINKS: u32 = Kinds::EXILED_WITH | Kinds::PUT_WITH;

    /// The bits of the kinds of cost in `paid`.
    #[inline]
    fn paid(paid: CastingKinds) -> u32 {
        u32::from(paid.bits()) << 1
    }
}

/// An entry: a memo of the object now in a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Remembered<L: Layout = Small> {
    slot: L::Slot,
    memo: Memo<L>,
}

/// An entry is 3 bytes in [`Small`] and the store 100: the state's room (the crate's
/// contract, "Stores") counts on at most 4 and 136.
const _: () = assert!(size_of::<Remembered<Small>>() <= 4 && size_of::<Memos<Small>>() <= 136);

impl<L: Layout> Default for Remembered<L> {
    fn default() -> Self {
        Remembered {
            slot: L::Slot::default(),
            memo: Memo::X(0),
        }
    }
}

/// The Memos store: the entries in the order they were written, and the kinds among them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Memos<L: Layout = Small> {
    entries: L::Queue<Remembered<L>, MEMOS>,
    kinds: Kinds,
}

impl Copy for Memos<Small> {}

/// The entries alone: the kinds are an index of them, and a hash of a state is a hash of
/// the game.
impl<L: Layout> std::hash::Hash for Memos<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.entries.hash(h);
    }
}

impl<L: Layout> Memos<L> {
    /// The entries, if one of them has a memo of a kind in `kinds`; none otherwise.
    #[inline]
    fn of(&self, kinds: u32) -> &[Remembered<L>] {
        if self.kinds.0 & kinds == 0 {
            return &[];
        }
        self.entries.as_slice()
    }

    /// Ends the entries `keep` refuses and says what that changed.
    #[inline]
    fn end(&mut self, keep: impl Fn(&Remembered<L>) -> bool) -> Ended {
        let mut done = Ended::default();
        let mut kinds = 0;
        self.entries.retain(|e| {
            let kept = keep(e);
            if kept {
                kinds |= e.memo.kinds();
            } else {
                done.any = true;
                done.layer_input |= e.memo.layer_input();
            }
            kept
        });
        self.kinds = Kinds(kinds);
        done
    }

    /// The objects that remember exactly `memo`, if a memo of a kind in `kinds` is live.
    #[inline]
    fn remembering(&self, kinds: u32, memo: Memo<L>) -> Bitboard<L::Slot> {
        self.of(kinds)
            .iter()
            .filter(|e| e.memo == memo)
            .map(|e| e.slot)
            .collect()
    }
}

impl<L: Layout> Store<L> for Memos<L> {
    fn empty() -> Self {
        Memos {
            entries: Default::default(),
            kinds: Kinds::default(),
        }
    }

    fn keys(&self) -> Bitboard<L::Slot> {
        self.entries.iter().map(|e| e.slot).collect()
    }

    /// What exiled each card that remembers it. With no such card in the game, one AND.
    #[inline]
    fn names(&self) -> Bitboard<L::Slot> {
        self.of(Kinds::NAMING)
            .iter()
            .filter_map(|e| e.memo.names())
            .collect()
    }

    fn forget(&mut self, slot: L::Slot) -> Ended {
        self.end(|e| e.slot != slot)
    }

    /// Nothing: a memo lasts as long as its object and no longer.
    #[inline]
    fn expire(&mut self, _: Duration) -> Ended {
        Ended::default()
    }

    fn fits<M: Layout>(&self) -> Result<(), Misfit> {
        room(
            "the memos",
            self.entries.len(),
            <M::Queue<Remembered<M>, MEMOS> as Queue<_>>::CAPACITY,
        )
    }

    fn check(&self, s: &State<L>, _: &GameTable) -> Result<(), Violation> {
        let mut kinds = 0;
        for (i, e) in self.entries.iter().enumerate() {
            kinds |= e.memo.kinds();
            let at = Some(At::Slot(e.slot.index()));
            let zone = |slot: L::Slot| s.object(slot).zone;
            check(
                zone(e.slot) == e.memo.zone(),
                "a memo on an object in no zone to remember it",
                at,
            )?;
            check(
                e.memo.holds(zone),
                "a memo of nothing, or of what exiled it and has gone",
                at,
            )?;
            check(
                e.memo.names().is_none() || e.memo.kinds() & Kinds::NAMING != 0,
                "a memo names a slot and its kind is not among those that do",
                at,
            )?;
            let again = |earlier: &Remembered<L>| {
                earlier.slot == e.slot && earlier.memo.same_variant(e.memo)
            };
            check(
                !self.entries.as_slice()[..i].iter().any(again),
                "an object remembers one thing twice",
                at,
            )?;
        }
        check(
            kinds == self.kinds.0,
            "the kinds of memo disagree with the memos",
            None,
        )
    }

    /// Nothing: a checkpoint of the board keeps the memos read after the turn they were
    /// written, the cost a permanent's static abilities ask about and what exiled a card,
    /// with the permanent and the card. The rest are read by a permanent's enters
    /// abilities alone, which have left the stack by then.
    fn lingering(&self, _: &State<L>, _: &GameTable) -> Option<String> {
        None
    }
}

impl<L: Layout> Relayout for Memos<L> {
    type In<M: Layout> = Memos<M>;

    fn relayout<M: Layout>(&self) -> Memos<M> {
        Memos {
            entries: queue(self.entries.as_slice(), |e| Remembered {
                slot: slot(e.slot),
                memo: e.memo.relayout(),
            }),
            kinds: self.kinds,
        }
    }
}

/// What the Memos store holds about one object, to compare two objects by
/// ([`State::signature`]): equal when the two remember the same things, written in the
/// same order.
#[derive(Clone, Copy, Debug)]
pub struct MemosOf<'a, L: Layout = Small> {
    /// The store's entries, or none where no store has an entry about the object.
    entries: &'a [Remembered<L>],
    slot: L::Slot,
}

impl<L: Layout> MemosOf<'_, L> {
    /// The object's memos, in the order they were written.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = Memo<L>> + '_ {
        let slot = self.slot;
        self.entries
            .iter()
            .filter(move |e| e.slot == slot)
            .map(|e| e.memo)
    }

    /// Whether the object remembers a cost of this kind paid for its spell.
    #[inline]
    pub fn paid(&self, kind: CastingKind) -> bool {
        self.iter()
            .any(|memo| matches!(memo, Memo::Paid(paid) if paid.contains(kind)))
    }

    /// The slot of the object whose ability exiled this card, among the cards "exiled
    /// with" it (see CR 607.2a).
    #[inline]
    pub fn exiled_with(&self) -> Option<L::Slot> {
        self.iter().find_map(|memo| match memo {
            Memo::ExiledWith(by) => Some(by),
            Memo::Paid(_)
            | Memo::X(_)
            | Memo::ExiledUntil(_)
            | Memo::Chosen(_)
            | Memo::Really(_)
            | Memo::PutOntoBattlefieldWith(_)
            | Memo::PhasedOutUnder(_)
            | Memo::PartOf(_)
            | Memo::FaceDownUnder(_)
            | Memo::Morphed => None,
        })
    }

    /// The permanent this card is exiled until the leaving of (see CR 610.3).
    #[inline]
    pub fn exiled_until(&self) -> Option<L::Slot> {
        self.iter().find_map(|memo| match memo {
            Memo::ExiledUntil(by) => Some(by),
            Memo::Paid(_)
            | Memo::X(_)
            | Memo::ExiledWith(_)
            | Memo::Chosen(_)
            | Memo::Really(_)
            | Memo::PutOntoBattlefieldWith(_)
            | Memo::PhasedOutUnder(_)
            | Memo::PartOf(_)
            | Memo::FaceDownUnder(_)
            | Memo::Morphed => None,
        })
    }

    /// The value the permanent chose as it entered (see CR 614.12a).
    #[inline]
    pub fn chosen(&self) -> Option<ChosenValue<L>> {
        self.iter().find_map(|memo| match memo {
            Memo::Chosen(value) => Some(value),
            Memo::Paid(_)
            | Memo::X(_)
            | Memo::ExiledWith(_)
            | Memo::ExiledUntil(_)
            | Memo::Really(_)
            | Memo::PutOntoBattlefieldWith(_)
            | Memo::PhasedOutUnder(_)
            | Memo::PartOf(_)
            | Memo::FaceDownUnder(_)
            | Memo::Morphed => None,
        })
    }
}

impl<L: Layout> PartialEq for MemosOf<'_, L> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl<L: Layout> Eq for MemosOf<'_, L> {}

impl<L: Layout> State<L> {
    /// Has the object in `slot` remember `memo`: a permanent, of the spell it was and what it
    /// chose, as it enters; a card in exile, what exiled it. It remembers until it leaves its zone
    /// (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), and one thing of
    /// each variant of [`Memo`]. The layer results are stale from here if the layers read
    /// the memo. No event: it is part of how the object came to be where it is.
    #[inline]
    pub fn remember(&mut self, slot: L::Slot, memo: Memo<L>) -> Result<(), Full> {
        debug_assert_eq!(self.object(slot).zone, memo.zone());
        debug_assert!(!self.memos(slot).iter().any(|m| m.same_variant(memo)));
        self.memos.entries.push(Remembered { slot, memo })?;
        self.memos.kinds.0 |= memo.kinds();
        self.keyed.insert(slot);
        if memo.layer_input() {
            self.flags.derived_dirty = true;
        }
        Ok(())
    }

    /// Whether the spell that became the permanent in `slot` was cast with a cost of this
    /// kind paid (see [CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d)). With
    /// no permanent in the game that remembers one of the kind, one AND.
    #[inline]
    pub fn paid(&self, slot: L::Slot, kind: CastingKind) -> bool {
        let asked = CastingKinds::NONE.with(kind);
        self.memos
            .of(Kinds::paid(asked))
            .iter()
            .any(|e| e.slot == slot && matches!(e.memo, Memo::Paid(paid) if paid.contains(kind)))
    }

    /// Reveals face-down components under the face-up tops of merged permanents
    /// (see [CR 708.9]): "At the end of each game, all ... face-down components of
    /// merged permanents ... must be revealed to all players."
    ///
    /// [CR 708.9]: https://yawgatog.com/resources/magic-rules/#R7089
    pub fn reveal_face_down_components(&mut self) {
        if self.memos.kinds.0 & Kinds::FACE_DOWN_UNDER == 0 {
            return;
        }
        let players = self.players();
        for row in self.memos.entries.as_mut_slice() {
            if let Memo::FaceDownUnder(knew) = &mut row.memo {
                *knew = players;
            }
        }
    }

    /// Whether the permanent in `slot` entered by resolving a face-down morph spell.
    pub fn morphed(&self, slot: L::Slot) -> bool {
        self.memos
            .of(Kinds::MORPHED)
            .iter()
            .any(|e| e.slot == slot && e.memo == Memo::Morphed)
    }

    /// The value of X the permanent in `slot` was cast with
    /// (see [CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)), 0 if none. With
    /// no permanent in the game that remembers one, one AND.
    #[inline]
    pub fn x_of(&self, slot: L::Slot) -> u8 {
        self.memos
            .of(Kinds::X)
            .iter()
            .find_map(|e| {
                let Memo::X(x) = e.memo else { return None };
                (e.slot == slot).then_some(x)
            })
            .unwrap_or(0)
    }

    /// The value the permanent in `slot` chose as it entered
    /// (see [CR 614.12a](https://yawgatog.com/resources/magic-rules/#R61412a)), if it chose one.
    /// With no permanent in the game that chose one, one AND.
    #[inline]
    pub fn chosen(&self, slot: L::Slot) -> Option<ChosenValue<L>> {
        self.memos.of(Kinds::CHOSEN).iter().find_map(|e| {
            let Memo::Chosen(value) = e.memo else {
                return None;
            };
            (e.slot == slot).then_some(value)
        })
    }

    /// The card the permanent in `slot` really is while a copy effect gives it the values of
    /// another ([`Memo::Really`]), if one does. With no such permanent in the game, one AND.
    #[inline]
    pub fn really(&self, slot: L::Slot) -> Option<L::Card> {
        self.memos.of(Kinds::REALLY).iter().find_map(|e| {
            let Memo::Really(card) = e.memo else {
                return None;
            };
            (e.slot == slot).then_some(card)
        })
    }

    /// Whether `player` knows the card the object in `slot` really is
    /// ([`State::card_really_is`]): for a merged permanent over a face-down component, the
    /// players who knew that card ([`Memo::FaceDownUnder`]); for any other object, whoever
    /// knows the object ([`State::knows`]). With no such permanent in the game, one AND.
    #[inline]
    pub fn knows_card_really_is(&self, player: Seat, slot: L::Slot) -> bool {
        let knew = self.memos.of(Kinds::FACE_DOWN_UNDER).iter().find_map(|e| {
            let Memo::FaceDownUnder(knew) = e.memo else {
                return None;
            };
            (e.slot == slot).then_some(knew)
        });
        knew.map_or_else(|| self.knows(player, slot), |knew| knew.contains(player))
    }

    /// Makes the card the permanent in `slot` really is `card` ([`Memo::Really`]), in a
    /// position dealt for a player who does not know it ([`Observation`]).
    pub(crate) fn deal_card_really_is(&mut self, slot: L::Slot, card: L::Card) {
        for e in self.memos.entries.as_mut_slice() {
            if e.slot == slot && matches!(e.memo, Memo::Really(_)) {
                e.memo = Memo::Really(card);
            }
        }
    }

    /// The cards of the merged permanent in `slot` other than the one in its own slot, in the
    /// order they merged ([`Memo::PartOf`]): none for any other permanent. With no merged
    /// permanent in the game, one AND.
    #[inline]
    pub fn pile(&self, slot: L::Slot) -> impl Iterator<Item = L::Slot> + '_ {
        self.memos
            .of(Kinds::PART_OF)
            .iter()
            .filter(move |e| e.memo == Memo::PartOf(slot))
            .map(|e| e.slot)
    }

    /// The cards exiled with the object in `source` by an ability linked to another of its
    /// abilities (see [CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second
    /// ability refers only to cards in the exile zone that were put there as a result of an
    /// instruction to exile them in the first ability." Kept while that object has left its
    /// zone, until a new object arrives in the slot (see CR 400.7). With no card in the game
    /// exiled this way, one AND.
    #[inline]
    pub fn exiled_with(&self, source: L::Slot) -> Bitboard<L::Slot> {
        self.memos
            .remembering(Kinds::EXILED_WITH, Memo::ExiledWith(source))
    }

    /// The cards exiled until the permanent in `source` leaves the battlefield
    /// (see [CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)). With no card in
    /// the game exiled this way, one AND.
    #[inline]
    pub fn exiled_until(&self, source: L::Slot) -> Bitboard<L::Slot> {
        self.memos
            .remembering(Kinds::EXILED_UNTIL, Memo::ExiledUntil(source))
    }

    /// The permanents put onto the battlefield by an ability of the Aura in `source`
    /// ([`Memo::PutOntoBattlefieldWith`]), whether or not the Aura is still there. With no
    /// permanent in the game that remembers one, one AND.
    #[inline]
    pub fn put_onto_battlefield_with(&self, source: L::Slot) -> Bitboard<L::Slot> {
        self.memos
            .remembering(Kinds::PUT_WITH, Memo::PutOntoBattlefieldWith(source))
    }

    /// Ends the card in `slot`'s memo of what it was exiled until, if it has one: the card
    /// stays in exile as that permanent leaves, as an Aura with nothing it could enchant does
    /// (see [CR 303.4g](https://yawgatog.com/resources/magic-rules/#R3034g)): "If an Aura is
    /// entering the battlefield and there is no legal object or player for it to enchant, the
    /// Aura remains in its current zone". It is exiled until nothing any more.
    #[cold]
    #[inline(never)]
    pub fn end_exiled_until(&mut self, slot: L::Slot) {
        let done = self
            .memos
            .end(|e| !(e.slot == slot && matches!(e.memo, Memo::ExiledUntil(_))));
        self.settle_ended(done);
    }

    /// Unlinks the cards exiled with an earlier object in `slot`, and the permanents put
    /// onto the battlefield with it, as a new object arrives there or a token's slot is
    /// freed: the zone move calls it. The cards stay "exiled with" the object that exiled
    /// them
    /// (see [CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second
    /// ability refers only to cards in the exile zone that were put there as a result of an
    /// instruction to exile them in the first ability." An ability of that object still on
    /// the stack after it left finds them; the new object is not that object
    /// (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)) and does not. With
    /// no card in the game exiled this way, one AND.
    #[inline]
    pub(crate) fn forget_links_to(&mut self, slot: L::Slot) {
        if self.memos.kinds.0 & Kinds::LINKS != 0 {
            self.forget_links_to_slot(slot);
        }
    }

    /// The rare path of [`State::forget_links_to`], out of line.
    #[cold]
    #[inline(never)]
    fn forget_links_to_slot(&mut self, slot: L::Slot) {
        let done = self.memos.end(|e| {
            e.memo != Memo::ExiledWith(slot) && e.memo != Memo::PutOntoBattlefieldWith(slot)
        });
        self.settle_ended(done);
    }

    /// The player under whose control the phased-out permanent in `slot` phased out, if it is not
    /// its base controller ([`Memo::PhasedOutUnder`]). With no permanent in the game that
    /// remembers one, one AND.
    #[inline]
    pub(crate) fn phased_out_under(&self, slot: L::Slot) -> Option<Seat> {
        self.memos.of(Kinds::PHASED_OUT_UNDER).iter().find_map(|e| {
            let Memo::PhasedOutUnder(player) = e.memo else {
                return None;
            };
            (e.slot == slot).then_some(player)
        })
    }

    /// Ends the [`Memo::PhasedOutUnder`] of the permanents in `slots` as they phase in.
    pub(crate) fn forget_phased_out_under(&mut self, slots: Bitboard<L::Slot>) {
        if self.memos.kinds.0 & Kinds::PHASED_OUT_UNDER != 0 {
            let done = self
                .memos
                .end(|e| !(matches!(e.memo, Memo::PhasedOutUnder(_)) && slots.contains(e.slot)));
            self.settle_ended(done);
        }
    }

    /// Everything the object in `slot` remembers: for [`State::signature`], tools, tests
    /// and a view. The rules ask by kind. With nothing stored about the object, one bit
    /// test.
    #[inline]
    pub fn memos(&self, slot: L::Slot) -> MemosOf<'_, L> {
        MemosOf {
            entries: if self.keyed.contains(slot) {
                self.memos.entries.as_slice()
            } else {
                &[]
            },
            slot,
        }
    }
}

impl<'a, L: Layout> Observation<'a, L> {
    /// Everything the object in `slot` remembers. A memo is as public as its object: how a
    /// spell was cast is announced
    /// (see [CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b)), and the permanent
    /// it became is on the battlefield for every player to see.
    pub fn memos(&self, slot: L::Slot) -> MemosOf<'a, L> {
        self.shown().memos(slot)
    }
}

#[cfg(test)]
mod tests;
