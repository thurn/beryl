//! Which abilities and casting options a game exercised, for the bar a supported card meets
//! (architecture §17.3): "every one of its abilities is exercised by at least one scenario".
//!
//! An ability is exercised when the engine does what it says: a spell, activated or triggered
//! ability's body runs, a mana ability makes mana, a static ability's continuous effect
//! applies, its rule changes what a player may do, or its replacement modifies an event. A
//! casting option is exercised when a spell is cast with it.
//!
//! The record is the `coverage` feature's, which only tests enable; without it [`hit`] is
//! empty and costs nothing. A thread records only between [`start`] and [`take`], so other
//! tests built with the feature, such as the allocation counter's, record nothing.

/// Notes that the ability or casting option at `what`, a `'static` part of a card
/// definition, did what it says. Identity is the address: card definitions are statics.
#[inline]
pub(crate) fn hit<T>(what: &'static T) {
    #[cfg(feature = "coverage")]
    record::hit(std::ptr::from_ref(what).addr());
    #[cfg(not(feature = "coverage"))]
    let _ = what;
}

#[cfg(feature = "coverage")]
pub use record::{start, take};

#[cfg(feature = "coverage")]
mod record {
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    // Write-only state the engine never reads: what a game did cannot depend on it, which
    // the determinism test's ban on thread-locals is for (architecture §14).
    thread_local! {
        static HITS: RefCell<Option<BTreeSet<usize>>> = const { RefCell::new(None) };
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
