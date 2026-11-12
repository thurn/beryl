//! The engine's per-thread working memory, and the checks debug builds of `next` and
//! `apply` make with it (architecture §17.4).

#[cfg(debug_assertions)]
use crate::decision::{Outcome, StallReason};
use crate::event::Sink;
use crate::layout::{Layout, Small};
use crate::profile::Profile;
#[cfg(debug_assertions)]
use crate::reference::check_references;
use crate::state::State;
#[cfg(debug_assertions)]
use crate::table::GameTable;

/// Per-thread working memory, never copied with the state: the event sink, the counts, and
/// the spare state transactional replay runs bodies on (§7.3), allocated once so the hot
/// path never allocates.
#[derive(Debug)]
pub struct Scratch<L: Layout = Small> {
    pub counts: Counts,
    pub events: Sink<L>,
    spare: Box<State<L>>,
    /// Whether debug builds check each state `next` returns and `apply` is handed
    /// ([`assert_sound`]); [`Scratch::unchecked`] turns it off.
    #[cfg(debug_assertions)]
    checked: bool,
    /// How many states [`assert_sound`] has checked with this scratch.
    #[cfg(debug_assertions)]
    checks: u32,
}

impl<L: Layout> Default for Scratch<L> {
    fn default() -> Self {
        Self {
            counts: Counts::default(),
            events: Sink::default(),
            spare: Box::new(State::empty(Profile::EXACT)),
            #[cfg(debug_assertions)]
            checked: true,
            #[cfg(debug_assertions)]
            checks: 0,
        }
    }
}

impl<L: Layout> Scratch<L> {
    /// Working memory whose sink drops events, as search wants.
    pub fn new() -> Self {
        Self::default()
    }

    /// Working memory whose sink records every executed event, for the UI and replays.
    pub fn recording() -> Self {
        Self {
            events: Sink::recording(),
            ..Self::default()
        }
    }

    /// Working memory as [`Scratch::new`] gives it, for the games an agent plays in its head:
    /// debug builds skip the checks of `next` and `apply` on it ([`assert_sound`]). Search
    /// plays thousands of steps for each step of the game it decides in, and the checks
    /// would make its debug tests several times slower; the game itself is still checked at
    /// each of its own steps, and the fuzz tests check random games throughout.
    pub fn unchecked() -> Self {
        Self {
            #[cfg(debug_assertions)]
            checked: false,
            ..Self::default()
        }
    }

    /// The spare state, a copy of `s`, and the sink, for a run of a body on a copy.
    pub(crate) fn copy_to_spare(&mut self, s: &State<L>) -> (&mut State<L>, &mut Sink<L>) {
        (*self.spare).clone_from(s);
        self.counts.copied();
        (&mut self.spare, &mut self.events)
    }

    /// Moves the work the sink counted into the counts.
    pub(crate) fn settle_counts(&mut self) {
        debug_assert!(self.events.is_idle(), "a cause's scope left open");
        let work = std::mem::take(&mut self.events.work);
        self.counts.events += work.events;
        self.counts.layer_recomputes += work.layer_recomputes;
        self.counts.solver_runs += work.solver_runs;
    }
}

/// Deterministic counts of the engine's logical operations, the stable performance signal CI
/// gates on with tolerance bands (architecture §17.4), summed over every game the scratch
/// serves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Decisions `next` returned.
    pub decisions: u64,
    /// Decisions `next` settled itself: one candidate, or a priority pass away from a stop.
    pub silent: u64,
    /// State-based action checks.
    pub state_based_checks: u64,
    /// Whole-state copies: the engine's, for transactional replay (§7.3), and a searcher's,
    /// which it counts itself with [`Counts::copied`].
    pub state_copies: u64,
    /// Recomputes of the derived characteristics (§9).
    pub layer_recomputes: u64,
    /// Runs of the mana payment solver (§10).
    pub solver_runs: u64,
    /// Events executed (§8), whether or not the sink keeps them.
    pub events: u64,
}

impl Counts {
    /// Counts a copy of a whole state that a caller of the engine made.
    pub fn copied(&mut self) {
        self.state_copies += 1;
    }
}

/// How many checked states [`assert_sound`] sees for each one it also checks against the
/// reference implementations, which cost about thirty times what a step does in a debug
/// build: at every step they would make the debug tests six times slower. The fuzz tests
/// and the scenario runner check the references at every decision themselves.
#[cfg(debug_assertions)]
const REFERENCES_EVERY: u32 = 32;

/// Panics unless `s` keeps the state's invariants (§17.4): debug builds check every state
/// `next` returns and every state `apply` is handed, unless the scratch is
/// [unchecked](Scratch::unchecked), so any test that plays the engine is a fuzz test too.
/// Every [`REFERENCES_EVERY`]th of them is also checked against the reference
/// implementations of the engine's fast paths. A violation is an engine bug, never a rules
/// outcome. Release and search builds check nothing here, so the count gate's count is
/// untouched.
#[cfg(debug_assertions)]
#[track_caller]
pub(crate) fn assert_sound<L: Layout>(s: &State<L>, t: &GameTable, x: &mut Scratch<L>) {
    // A game that ran out of capacity stopped partway through an event, and is replayed
    // from before it in a larger layout, never played on.
    if !x.checked || s.outcome() == Some(Outcome::Stalled(StallReason::Capacity)) {
        return;
    }
    if let Err(v) = s.check_invariants(t) {
        panic!("an engine invariant broke: {v:?}");
    }
    x.checks = x.checks.wrapping_add(1);
    if x.checks.is_multiple_of(REFERENCES_EVERY)
        && let Err(v) = check_references(s, t)
    {
        panic!("the engine disagrees with a reference: {v:?}");
    }
}
