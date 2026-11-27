//! Which players pass priority without being asked while the same triggered ability resolves
//! again and again, under [`Profile::passes_identical_triggers`]: the two [`Flags`] that say
//! it, their mutators, and the hash that leaves them out while unused.
//!
//! Implements: none. Notes which players a profile lets pass on a repeated trigger.
//!
//! A restriction of the decision surface (architecture §15): a pass is always legal (see
//! [CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d): "If a player has priority
//! and chooses not to take any actions, that player passes."), so a profile may make one for a
//! player.
//!
//! [`Profile::passes_identical_triggers`]: crate::profile::Profile::passes_identical_triggers

use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::state::{Flags, State};

/// Every field, the two that say who passes on identical triggers only once one is set, so
/// that a game in which no player passes that way hashes as it did before they existed
/// (`beryl_bench` pins the hashes of its corpus).
impl std::hash::Hash for Flags {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let Flags {
            derived_dirty,
            abilities_lost,
            mana_changed,
            face_down,
            yielded,
            repeats,
        } = self;
        (derived_dirty, abilities_lost, mana_changed, face_down).hash(h);
        if yielded.contains(&true) || *repeats {
            (yielded, repeats).hash(h);
        }
    }
}

impl<L: Layout> State<L> {
    /// Whether `player` passes this priority without being asked: they passed with a triggered
    /// ability on top of the stack ([`State::note_yield`]), nothing was put on the stack since,
    /// and the entry on top now is the same trigger again ([`State::note_resolving`]).
    #[inline]
    pub fn yields(&self, player: Seat) -> bool {
        self.flags.repeats && self.flags.yielded[player.index()] && !self.stack.is_empty()
    }

    /// Whether some player passed with a triggered ability on top of the stack and nothing
    /// was put on the stack since: only then does a resolution need to say whether the next
    /// entry is the same trigger again.
    #[inline]
    pub fn any_yielded(&self) -> bool {
        self.flags.yielded.contains(&true)
    }

    /// Notes that `player` passed priority with a triggered ability on top of the stack, under
    /// a profile that lets them pass on the same trigger again without being asked.
    /// Bookkeeping about who is asked, which no player sees, so it needs no event.
    #[inline]
    pub fn note_yield(&mut self, player: Seat) {
        self.flags.yielded[player.index()] = true;
    }

    /// Notes, as the top of the stack begins to resolve, whether the entry beneath it is the
    /// same trigger again (`again`). If it is not, no player passes on it unasked.
    #[inline]
    pub fn note_resolving(&mut self, again: bool) {
        self.flags.repeats = again;
        if !again {
            self.forget_yields();
        }
    }

    /// Every player is asked again: something was put on the stack, or the next entry to
    /// resolve is another.
    #[inline]
    pub(crate) fn forget_yields(&mut self) {
        self.flags.yielded = Default::default();
        self.flags.repeats = false;
    }
}
