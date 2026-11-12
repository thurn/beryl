//! The Memos store: what an object remembers of how it was cast or entered (plan K4; the
//! crate's contract, "Stores").
//!
//! A memo is written once, with [`State::remember`], and read by the object's own abilities
//! through a query by kind: [`State::paid`], [`State::x_of`]. It ends with its object,
//! inside the zone move
//! ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): "An object that moves
//! from one zone to another becomes a new object with no memory of, or relation to, its
//! previous existence."), so an entry names a slot and is always about the object now in
//! it. A kind with nothing live costs its reader one AND.
//!
//! Built so far: what a permanent remembers of the spell it was, the costs paid for it and
//! the value of X (by-w8gv.7.6); and what exiled a card, a memo of the exiled card naming
//! the slot of what exiled it (by-w8gv.7.7). Such a memo ends with the card as every memo
//! does, and one of the two also as a new object arrives in the slot it names
//! ([`Memo::ExiledWith`]).

use beryl_card::{CastingKind, CastingKinds};

use crate::bitboard::Bitboard;
use crate::fit::{Misfit, room};
use crate::fixed::Full;
use crate::ids::SlotId;
use crate::invariants::{At, Violation, check};
use crate::layout::{Layout, Queue, Small};
use crate::observation::Observation;
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
    /// ([CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d): "If a spell's
    /// controller declares the intention to pay any of that spell's kicker costs, that
    /// spell has been 'kicked.'"). Never the empty set: a permanent whose spell paid none
    /// remembers nothing.
    Paid(CastingKinds),
    /// The value of X the permanent's spell was cast with, for its enters abilities
    /// ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)): "the value of X
    /// for that ability is the same as the value of X for that spell". Never 0, which is
    /// what a permanent with no such memo answers.
    X(u8),
    /// The card in exile was exiled by an ability of the object in this slot, whose linked
    /// ability refers to the cards "exiled with" it
    /// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "If an object has
    /// an activated or triggered ability printed on it that instructs a player to exile one
    /// or more cards and an ability printed on it that refers either to 'the exiled cards'
    /// or to cards 'exiled with [this object],' these abilities are linked."
    ///
    /// It names the slot and not one incarnation of it: the card stays exiled with that
    /// object after the object has left its zone, for an ability of it still on the stack,
    /// until a new object arrives in the slot on the battlefield or the slot of a token is
    /// freed ([`State::forget_exiled_with`]).
    ExiledWith(L::Slot),
    /// The card in exile was exiled until the permanent in this slot leaves the
    /// battlefield, which returns it
    /// ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)): "Some one-shot
    /// effects cause an object to change zones 'until' a specified event occurs."
    ExiledUntil(L::Slot),
}

impl<L: Layout> Memo<L> {
    /// The bits of its kinds in a store's [`Kinds`].
    #[inline]
    fn kinds(self) -> u16 {
        match self {
            Memo::Paid(paid) => Kinds::paid(paid),
            Memo::X(_) => Kinds::X,
            Memo::ExiledWith(_) => Kinds::EXILED_WITH,
            Memo::ExiledUntil(_) => Kinds::EXILED_UNTIL,
        }
    }

    /// The zone of an object that remembers it: a permanent remembers its spell, a card in
    /// exile what exiled it.
    #[inline]
    fn zone(self) -> Zone {
        match self {
            Memo::Paid(_) | Memo::X(_) => Zone::Battlefield,
            Memo::ExiledWith(_) | Memo::ExiledUntil(_) => Zone::Exile,
        }
    }

    /// Whether the layers read it: a static ability may apply only while a cost was paid,
    /// as one with impending does
    /// ([CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a)).
    #[inline]
    fn layer_input(self) -> bool {
        matches!(self, Memo::Paid(_))
    }

    /// Whether it says something, and of an exiler that is where such a memo needs it:
    /// what the store's invariant holds each memo to, given the zone of the object in a
    /// slot.
    fn holds(self, zone: impl Fn(L::Slot) -> Zone) -> bool {
        match self {
            Memo::X(x) => x > 0,
            Memo::Paid(paid) => !paid.is_empty(),
            // What exiled it left a card or a token in its slot.
            Memo::ExiledWith(by) => zone(by) != Zone::Nowhere,
            // An object exiled "until" its exiler leaves returns as it leaves (CR 610.3).
            Memo::ExiledUntil(by) => zone(by) == Zone::Battlefield,
        }
    }

    /// The memo in layout `M`: a slot it names at the same number.
    fn relayout<M: Layout>(self) -> Memo<M> {
        match self {
            Memo::Paid(paid) => Memo::Paid(paid),
            Memo::X(x) => Memo::X(x),
            Memo::ExiledWith(by) => Memo::ExiledWith(slot(by)),
            Memo::ExiledUntil(by) => Memo::ExiledUntil(slot(by)),
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
/// cost the vocabulary's list has, and one for each way a card remembers what exiled it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Kinds(u16);

impl Kinds {
    const X: u16 = 1 << 0;
    const EXILED_WITH: u16 = 1 << 9;
    const EXILED_UNTIL: u16 = 1 << 10;

    /// The bits of the kinds of cost in `paid`.
    #[inline]
    const fn paid(paid: CastingKinds) -> u16 {
        (paid.bits() as u16) << 1
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
    fn of(&self, kinds: u16) -> &[Remembered<L>] {
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
    fn remembering(&self, kinds: u16, memo: Memo<L>) -> Bitboard<L::Slot> {
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
    /// with" it (CR 607.2a).
    #[inline]
    pub fn exiled_with(&self) -> Option<L::Slot> {
        self.iter().find_map(|memo| match memo {
            Memo::ExiledWith(by) => Some(by),
            _ => None,
        })
    }

    /// The permanent this card is exiled until the leaving of (CR 610.3).
    #[inline]
    pub fn exiled_until(&self) -> Option<L::Slot> {
        self.iter().find_map(|memo| match memo {
            Memo::ExiledUntil(by) => Some(by),
            _ => None,
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
    /// Has the object in `slot` remember `memo`: a permanent, of the spell it was, as it
    /// enters; a card in exile, what exiled it. It remembers until it leaves its zone
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), and one thing of
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
    /// kind paid ([CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d)). With
    /// no permanent in the game that remembers one of the kind, one AND.
    #[inline]
    pub fn paid(&self, slot: L::Slot, kind: CastingKind) -> bool {
        let asked = CastingKinds::NONE.with(kind);
        self.memos
            .of(Kinds::paid(asked))
            .iter()
            .any(|e| e.slot == slot && matches!(e.memo, Memo::Paid(paid) if paid.contains(kind)))
    }

    /// The value of X the permanent in `slot` was cast with
    /// ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)), 0 if none. With
    /// no permanent in the game that remembers one, one AND.
    #[inline]
    pub fn x_of(&self, slot: L::Slot) -> u8 {
        self.memos
            .of(Kinds::X)
            .iter()
            .find_map(|e| match e.memo {
                Memo::X(x) if e.slot == slot => Some(x),
                _ => None,
            })
            .unwrap_or(0)
    }

    /// The cards exiled with the object in `source` by an ability linked to another of its
    /// abilities ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second
    /// ability refers only to cards in the exile zone that were put there as a result of an
    /// instruction to exile them in the first ability." Kept while that object has left its
    /// zone, until a new object arrives in the slot (CR 400.7). With no card in the game
    /// exiled this way, one AND.
    #[inline]
    pub fn exiled_with(&self, source: L::Slot) -> Bitboard<L::Slot> {
        self.memos
            .remembering(Kinds::EXILED_WITH, Memo::ExiledWith(source))
    }

    /// The cards exiled until the permanent in `source` leaves the battlefield
    /// ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)). With no card in
    /// the game exiled this way, one AND.
    #[inline]
    pub fn exiled_until(&self, source: L::Slot) -> Bitboard<L::Slot> {
        self.memos
            .remembering(Kinds::EXILED_UNTIL, Memo::ExiledUntil(source))
    }

    /// Unlinks the cards exiled with an earlier object in `slot`, as a new object arrives
    /// there or a token's slot is freed: the zone move calls it. The cards stay "exiled
    /// with" the object that exiled them
    /// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second
    /// ability refers only to cards in the exile zone that were put there as a result of an
    /// instruction to exile them in the first ability." An ability of that object still on
    /// the stack after it left finds them; the new object is not that object
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)) and does not. With
    /// no card in the game exiled this way, one AND.
    #[inline]
    pub(crate) fn forget_exiled_with(&mut self, slot: L::Slot) {
        if self.memos.kinds.0 & Kinds::EXILED_WITH != 0 {
            self.forget_exiled_with_slot(slot);
        }
    }

    /// The rare path of [`State::forget_exiled_with`], out of line.
    #[cold]
    #[inline(never)]
    fn forget_exiled_with_slot(&mut self, slot: L::Slot) {
        let done = self.memos.end(|e| e.memo != Memo::ExiledWith(slot));
        self.settle_ended(done);
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
    /// ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b)), and the permanent
    /// it became is on the battlefield for every player to see.
    pub fn memos(&self, slot: L::Slot) -> MemosOf<'a, L> {
        self.shown().memos(slot)
    }
}

#[cfg(test)]
mod tests;
