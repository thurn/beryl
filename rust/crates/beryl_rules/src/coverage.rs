//! Which abilities and casting options a game exercised, for the bar a supported card meets
//! (architecture §17.3): "every one of its abilities is exercised by at least one scenario".
//!
//! An ability is exercised when the engine does what it says: a spell, activated or triggered
//! ability's body runs, a mana ability makes mana, a static ability's continuous effect
//! applies, its rule changes what a player may do, or its replacement modifies an event. A
//! casting option is exercised when a spell is cast with it.
//!
//! Each instruction of effect data carried out is recorded too, for the bar a library entry
//! meets: "Library effects carry hand-written scenarios".
//!
//! Builds with debug assertions keep the record: the tests, the scripts and fixtures the checks
//! run, and `just fuzz` and `just exhaust`, which turn debug assertions on in an optimized build.
//! Elsewhere, as in the app, the search binaries and the benchmarks, [`hit`] is empty and costs
//! nothing, and [`take`] returns nothing. A thread records only between [`start`] and [`take`],
//! so other code in a debug build, such as the allocation counter's tests, records nothing.
//!
//! This was a Cargo feature that `beryl_cards`' and `beryl_search`' dev-dependencies turned on.
//! A feature changes the hash Cargo stores a crate's build under, so every build that did not
//! include those dev-dependencies, such as `cargo build -p beryl_cli` for the fast check's
//! script and browser checks, compiled the rules, the engine, every card crate and the CLI a
//! second time: 68 crates and 45 s after a fresh worktree's test build. Keying the record on
//! `debug_assertions` makes it the same in every debug build.

/// Notes that the ability, casting option or instruction at `what`, a `'static` part of a
/// card definition, did what it says. Identity is the address: card definitions are statics.
#[inline]
pub(crate) fn hit<T>(what: &'static T) {
    #[cfg(debug_assertions)]
    record::hit(std::ptr::from_ref(what).addr());
    #[cfg(not(debug_assertions))]
    let _ = what;
}

#[cfg(debug_assertions)]
pub use record::{start, take};

/// Starts recording on this thread; does nothing in a build without debug assertions.
#[cfg(not(debug_assertions))]
pub fn start() {}

/// What was recorded on this thread since [`start`]: nothing in a build without debug
/// assertions.
#[cfg(not(debug_assertions))]
#[must_use]
pub fn take() -> std::collections::BTreeSet<usize> {
    std::collections::BTreeSet::new()
}

/// Runs a payment forecast without counting its hypothetical events as coverage.
pub(crate) fn without_recording<T>(run: impl FnOnce() -> T) -> T {
    #[cfg(debug_assertions)]
    let _pause = record::Pause::new();
    run()
}

#[cfg(debug_assertions)]
mod record {
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    // Write-only state the engine never reads: what a game did cannot depend on it, which
    // the determinism test's ban on thread-locals is for (architecture §14).
    thread_local! {
        static HITS: RefCell<Option<BTreeSet<usize>>> = const { RefCell::new(None) };
    }

    pub(super) struct Pause(Option<BTreeSet<usize>>);

    impl Pause {
        pub(super) fn new() -> Self {
            Self(HITS.take())
        }
    }

    impl Drop for Pause {
        fn drop(&mut self) {
            HITS.set(self.0.take());
        }
    }

    pub(super) fn hit(address: usize) {
        HITS.with_borrow_mut(|hits| {
            if let Some(hits) = hits {
                hits.insert(address);
            }
        });
    }

    /// Starts recording on this thread, forgetting anything recorded before.
    pub fn start() {
        HITS.set(Some(BTreeSet::new()));
    }

    /// Stops recording on this thread, and returns what was recorded since [`start`]: the
    /// addresses of the abilities and casting options exercised. Compare them with
    /// [`super::address`] of a card definition's parts.
    pub fn take() -> BTreeSet<usize> {
        HITS.take().unwrap_or_default()
    }
}

/// The address [`take`] records a part of a card definition by.
pub fn address<T>(what: &'static T) -> usize {
    std::ptr::from_ref(what).addr()
}
