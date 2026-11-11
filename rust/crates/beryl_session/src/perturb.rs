//! Perturbations of what a save must not depend on (architecture §14), for tests: the seed's
//! derivation, the order a decision lists its candidates in, and the slot numbers cards get.
//! A save replays the same under each, so it depends on what the rules of Magic define, not
//! on engine internals. The frozen corpus is held to that (`tests/corpus.rs`).
//!
//! A perturbation holds on one thread, while [`with`] runs. It reaches only replay and the
//! setup of a game: nothing in search.

use std::cell::Cell;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{Choice, GameTable, Profile, State};

/// What a replay on this thread perturbs. The default perturbs nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Perturbation {
    /// Derives the game's random streams from its seed another way: every shuffle, the deal
    /// into slots and each coin flip fall otherwise.
    pub seed: bool,
    /// The order each decision lists its candidates in, as replay matches answers.
    pub order: Order,
    /// Deals each deck into its slots last first ([`beryl_engine::State::reverse_deck_slots`]).
    pub slots: bool,
}

/// An order a decision's candidates are listed in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Order {
    /// As the engine lists them.
    #[default]
    Listed,
    /// Last first.
    Reversed,
    /// Shuffled, by a fixed seed.
    Shuffled,
}

impl Perturbation {
    /// Each perturbation on its own: its name, the first save format that replays the same
    /// under it, and the perturbation. Older formats leave ties to the candidate's index: a
    /// version 2 save replays from its seed, and one before version 6 names too little of an
    /// object to tell apart every two that differ (a token copy and its card, a summoning
    /// sick creature and one that is not).
    pub const EACH: [(&str, u8, Perturbation); 4] = [
        (
            "another seed derivation",
            3,
            Perturbation {
                seed: true,
                order: Order::Listed,
                slots: false,
            },
        ),
        (
            "candidates reversed",
            6,
            Perturbation {
                seed: false,
                order: Order::Reversed,
                slots: false,
            },
        ),
        (
            "candidates shuffled",
            6,
            Perturbation {
                seed: false,
                order: Order::Shuffled,
                slots: false,
            },
        ),
        (
            "other slot numbers",
            6,
            Perturbation {
                seed: false,
                order: Order::Listed,
                slots: true,
            },
        ),
    ];
}

thread_local! {
    static CURRENT: Cell<Perturbation> = const {
        Cell::new(Perturbation { seed: false, order: Order::Listed, slots: false })
    };
}

/// Runs `f` with `p` perturbing this thread's replays and game setups.
pub fn with<T>(p: Perturbation, f: impl FnOnce() -> T) -> T {
    /// Puts back the perturbation before, though `f` panics.
    struct Restore(Perturbation);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.set(self.0);
        }
    }
    let _restore = Restore(CURRENT.replace(p));
    f()
}

/// This thread's perturbation.
pub(crate) fn current() -> Perturbation {
    CURRENT.get()
}

/// The seed the game's random streams start from, for a game recorded with `seed`.
pub(crate) fn seed(seed: u64) -> u64 {
    if current().seed {
        SplitMix64::new(seed ^ 0x0dd5_eed5).next_u64()
    } else {
        seed
    }
}

/// A new game between the decks of `t`, recorded with `seed` ([`State::new`]).
pub(crate) fn new_game(t: &GameTable, seed: u64, profile: Profile) -> State {
    let mut state = State::new(t, self::seed(seed), profile);
    if current().slots {
        state.board().reverse_deck_slots();
    }
    state
}

/// The candidates of a decision of `n`, in the order this thread lists them.
pub(crate) fn candidates(n: u16) -> Vec<Choice> {
    let mut listed: Vec<Choice> = (0..n).map(Choice).collect();
    match current().order {
        Order::Listed => {}
        Order::Reversed => listed.reverse(),
        Order::Shuffled => SplitMix64::new(0x5a1f_f1ed).shuffle(&mut listed),
    }
    listed
}
