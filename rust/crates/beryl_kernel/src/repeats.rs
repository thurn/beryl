//! Finding a loop of mandatory actions ([CR 104.4b]): "If a game that's not using the
//! limited range of influence option (including a two-player game) somehow enters a "loop" of
//! mandatory actions, repeating a sequence of events with no way to stop, the game is a draw.
//! Loops that contain an optional action don't result in a draw."
//!
//! The state counts the resolutions in a row in which no player made a choice ([`Repeats`]):
//! a choice is an answer picked from more than one candidate, other than passing priority. Once
//! more than [`Repeats::QUIET`] resolutions have passed so, each one that ends compares a
//! [canonical hash](State::canonical_hash) of the state with a stored one, and stores its own
//! each time the count reaches a power of two (Brent's method): so one stored hash finds a loop
//! of any length, within about twice its length of resolutions. A match is a position that
//! repeats with no choice between, which can only repeat again, and the game is a draw.
//!
//! A loop with an optional action in it, as a "may" a player answers, resets the count at each
//! answer, so it is never a draw here and runs to its budget.
//!
//! [CR 104.4b]: https://yawgatog.com/resources/magic-rules/#R1044b
//!
//! Implements: CR 104.4b

use std::cell::{Cell, RefCell};
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::ids::SlotId;
use crate::layout::{Large, Layout, PerLayout, Queue};
use crate::state::{State, Zone};

/// The resolutions in a row with no choice, and the hash a loop is compared with: part of the
/// state, so a game stays a pure function of its choices, and left out of its `Hash`, as
/// bookkeeping about the past that only this check reads: two states equal in all else play
/// the same game but for when a repeat is found, and no pinned hash changed with the field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Repeats {
    /// Resolutions since the last choice, saturating.
    quiet: u16,
    /// The canonical hash stored at the last power of two of `quiet` past [`Repeats::QUIET`].
    stored: u64,
}

impl Repeats {
    /// The resolutions with no choice before any hash is taken: ordinary play, in which a
    /// player chooses something every few resolutions, pays one increment and one compare.
    pub const QUIET: u16 = 8;
}

struct CanonicalHash;

impl PerLayout for CanonicalHash {
    type Of<L: Layout> = fn(&State<L>) -> u64;
}

impl<L: Layout> State<L> {
    /// A player chose something other than passing priority, among more than one candidate:
    /// no loop that runs through this moment is mandatory.
    #[inline]
    pub fn note_choice(&mut self) {
        self.repeats.quiet = 0;
    }

    /// A spell or ability finished resolving. Says whether the state is one it was at an
    /// earlier resolution since the last choice: a loop of mandatory actions, which the caller
    /// ends as a draw.
    #[inline]
    pub fn note_resolution(&mut self) -> bool {
        self.repeats.quiet = self.repeats.quiet.saturating_add(1);
        self.repeats.quiet > Repeats::QUIET && self.compare_repeat()
    }

    #[cold]
    #[inline(never)]
    fn compare_repeat(&mut self) -> bool {
        let quiet = self.repeats.quiet;
        let hash = self.canonical_hash();
        // The first hash taken is stored, and only compared from the next resolution on.
        if quiet > Repeats::QUIET + 1 && hash == self.repeats.stored {
            return true;
        }
        if quiet == Repeats::QUIET + 1 || quiet.is_power_of_two() {
            self.repeats.stored = hash;
        }
        false
    }

    /// A hash of the game that is the same for two positions that differ only in names the
    /// game gives as it goes: every field `Hash` feeds but the budgets, with each object's
    /// incarnation left out and each reference to one read as how many zone changes ago it was
    /// current, and each timestamp read as its rank among the live ones (an object off the
    /// battlefield keeps a stale one, read as none). A loop that returns its cards to where
    /// they were changes their incarnations and timestamps each time around, and nothing else.
    ///
    /// The random streams are hashed, so a loop that shuffles is not a repeat. Both layouts
    /// hash the position in Large so a game can still use its stored checkpoint after widening.
    pub fn canonical_hash(&self) -> u64 {
        L::of::<CanonicalHash>(
            |state| state.widen().hash_for_repeats(),
            State::<Large>::hash_with_suspended_games,
        )(self)
    }
}

impl State<Large> {
    fn hash_for_repeats(&self) -> u64 {
        let incarnations = <Large as Layout>::Slot::all()
            .map(|slot| self.objects[slot.index()].incarnation)
            .collect();
        let mut live: Vec<u16> = self
            .battlefield
            .iter()
            .map(|&slot| self.objects[slot.index()].timestamp)
            .chain(self.effects().iter().map(|e| e.timestamp))
            .collect();
        live.sort_unstable();
        live.dedup();
        let _canonical = Canonical::begin(Names { incarnations, live });
        let mut h = DefaultHasher::new();
        self.hash_position(&mut h, false);
        h.finish()
    }

    fn hash_with_suspended_games(&self) -> u64 {
        let current = self.hash_for_repeats();
        let Some(outer) = self.suspended_game() else {
            return current;
        };
        // The suspended game numbers object incarnations and timestamps in its own state.
        let mut h = DefaultHasher::new();
        (current, outer.hash_with_suspended_games()).hash(&mut h);
        h.finish()
    }
}

/// The names [`State::canonical_hash`] reads canonically: each slot's current incarnation and
/// the live timestamps in order.
struct Names {
    incarnations: Vec<u16>,
    live: Vec<u16>,
}

thread_local! {
    /// Whether a canonical hash is under way on this thread: the one read an ordinary hash
    /// pays.
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static NAMES: RefCell<Option<Names>> = const { RefCell::new(None) };
}

/// A canonical hash under way, ended as it drops, even by a panic.
struct Canonical;

impl Canonical {
    fn begin(names: Names) -> Canonical {
        NAMES.set(Some(names));
        ACTIVE.set(true);
        Canonical
    }
}

impl Drop for Canonical {
    fn drop(&mut self) {
        ACTIVE.set(false);
        NAMES.set(None);
    }
}

/// What a `Hash` feeds for a reference to incarnation `incarnation` of the object in the slot
/// at `slot`: the incarnation itself, or in a canonical hash how many zone changes ago it was
/// current.
#[inline]
pub(crate) fn reference(slot: usize, incarnation: u16) -> u16 {
    if !ACTIVE.get() {
        return incarnation;
    }
    NAMES.with_borrow(|n| {
        n.as_ref().map_or(incarnation, |n| {
            n.incarnations[slot].wrapping_sub(incarnation)
        })
    })
}

/// What a `Hash` feeds for an object's own incarnation: none in a canonical hash.
#[inline]
pub(crate) fn incarnation(incarnation: u16) -> u16 {
    if ACTIVE.get() { 0 } else { incarnation }
}

/// What a `Hash` feeds for a timestamp, `live` if the layers compare it: in a canonical hash
/// its rank among the live ones, and none for a stale one.
#[inline]
pub(crate) fn timestamp(timestamp: u16, live: bool) -> u16 {
    if !ACTIVE.get() {
        return timestamp;
    }
    NAMES.with_borrow(|n| match n {
        Some(n) if live => {
            u16::try_from(n.live.partition_point(|&t| t < timestamp)).unwrap_or(u16::MAX)
        }
        _ => 0,
    })
}

/// What a `Hash` feeds for the next timestamp to give: in a canonical hash, how many are live.
#[inline]
pub(crate) fn next_timestamp(next: u16) -> u16 {
    if !ACTIVE.get() {
        return next;
    }
    NAMES.with_borrow(|n| {
        n.as_ref()
            .map_or(next, |n| u16::try_from(n.live.len()).unwrap_or(u16::MAX))
    })
}

/// Whether an object in `zone` has a live timestamp.
#[inline]
pub(crate) fn timestamp_live(zone: Zone) -> bool {
    zone == Zone::Battlefield
}

#[cfg(test)]
mod tests {
    use std::hash::{DefaultHasher, Hash, Hasher};

    use crate::ids::{CardId, Seat, SlotId};
    use crate::layout::Small;
    use crate::profile::Profile;
    use crate::repeats::Repeats;
    use crate::state::{Continuous, Duration, LayerChange, State};
    use crate::{CardIx, Slot};

    fn hash(s: &State<Small>) -> u64 {
        let mut h = DefaultHasher::new();
        s.hash(&mut h);
        h.finish()
    }

    /// A state with a continuous effect on the object in slot 0, after that object changed
    /// zones `moved` times and `given` timestamps were given, as a loop that moves the object
    /// around and back leaves them.
    fn referring(moved: u16, given: u16) -> State<Small> {
        let mut s: State<Small> = State::empty(Profile::EXACT);
        s.objects[0].incarnation += moved;
        s.zones.next_timestamp += given;
        let on = s.object_ref(Slot::from_index(0));
        let effect = Continuous::Layer(on, LayerChange::ModifyPt(1, 1));
        s.start_effect(CardIx::from_index(0), effect, Duration::Indefinite)
            .unwrap();
        s
    }

    /// Two positions that differ only in incarnations and timestamps have one canonical hash,
    /// though their hashes differ; a stale reference is not a current one.
    #[test]
    fn the_canonical_hash_ignores_names_the_game_gives() {
        let s = referring(0, 0);
        let r = referring(3, 7);
        assert_ne!(hash(&s), hash(&r));
        assert_eq!(s.canonical_hash(), r.canonical_hash());
        let mut stale = r;
        stale.objects[0].incarnation += 1;
        assert_ne!(s.canonical_hash(), stale.canonical_hash());
    }

    #[test]
    fn widening_preserves_a_mandatory_loop_checkpoint() {
        let mut small = referring(0, 0);
        for _ in 0..=Repeats::QUIET {
            assert!(!small.note_resolution());
        }

        let mut large = small.widen();
        assert!(small.note_resolution());
        assert!(large.note_resolution());
    }

    /// A position that repeats is a loop once more than `QUIET` resolutions passed with no
    /// choice; a choice in between starts the count again.
    #[test]
    fn a_repeat_with_no_choice_between_is_found() {
        let mut s = referring(0, 0);
        for _ in 0..=Repeats::QUIET {
            assert!(!s.note_resolution());
        }
        let mut other = referring(3, 7);
        other.repeats = s.repeats;
        assert!(other.note_resolution(), "the same position, renamed");
        s.note_choice();
        for _ in 0..=Repeats::QUIET {
            assert!(!s.note_resolution());
        }
        let mut changed = s;
        changed.players[Seat::new(1).index()].life -= 1;
        assert!(!changed.note_resolution(), "a different position");
    }
}
