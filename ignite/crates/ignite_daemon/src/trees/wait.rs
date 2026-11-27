//! Waiting for a ticket tree: a lease that finds every ticket tree in use joins a queue, and the
//! next release leases the freed tree to the first waiter in it.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ignite_core::ids::SessionId;
use ignite_core::traits::Result;

use super::{LeaseRequest, Locked, TreeLease, Trees};

pub(super) enum SlotState {
    Waiting,
    Served(Result<TreeLease>),
    /// The waiter collected its lease.
    Taken,
    /// The waiter gave up after it was taken off the queue to be served.
    Abandoned,
}

/// One waiting lease: its request and, once served, its answer.
pub(super) struct Slot {
    request: LeaseRequest,
    reason: String,
    state: Mutex<SlotState>,
    served: Condvar,
}

impl Slot {
    pub(super) fn new(request: LeaseRequest, reason: String) -> Self {
        Slot {
            request,
            reason,
            state: Mutex::new(SlotState::Waiting),
            served: Condvar::new(),
        }
    }

    pub(super) fn request(&self) -> &LeaseRequest {
        &self.request
    }

    fn lock(&self) -> MutexGuard<'_, SlotState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hands the waiter its lease. Returns false if the waiter has given up, in which case the
    /// caller frees the tree.
    pub(super) fn serve(&self, result: Result<TreeLease>) -> bool {
        let mut state = self.lock();
        if matches!(*state, SlotState::Abandoned) {
            return false;
        }
        *state = SlotState::Served(result);
        self.served.notify_all();
        true
    }
}

/// A lease waiting for a ticket tree. Dropping it gives up the wait; a tree already leased to
/// it and not collected is freed again.
pub struct Waiter {
    slot: Arc<Slot>,
    trees: Trees,
}

impl std::fmt::Debug for Waiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Waiter")
            .field("ticket", &self.slot.request.ticket)
            .field("reason", &self.slot.reason)
            .finish_non_exhaustive()
    }
}

impl Waiter {
    pub(super) fn new(slot: Arc<Slot>, trees: Trees) -> Self {
        Waiter { slot, trees }
    }

    /// Why the lease waits: who holds each ticket tree, and how many tickets wait ahead.
    pub fn reason(&self) -> &str {
        &self.slot.reason
    }

    /// Waits up to `timeout` for a release to serve this lease. Returns `None` if none has yet;
    /// the waiter stays in the queue and can wait again.
    pub fn wait(&self, timeout: Duration) -> Option<Result<TreeLease>> {
        let state = self.slot.lock();
        let (mut state, _) = self
            .slot
            .served
            .wait_timeout_while(state, timeout, |s| matches!(s, SlotState::Waiting))
            .unwrap_or_else(PoisonError::into_inner);
        match std::mem::replace(&mut *state, SlotState::Taken) {
            SlotState::Served(result) => Some(result),
            other => {
                *state = other;
                None
            }
        }
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        if self.trees.forget(&self.slot) {
            return;
        }
        let mut state = self.slot.lock();
        match std::mem::replace(&mut *state, SlotState::Abandoned) {
            SlotState::Served(Ok(lease)) => {
                drop(state);
                self.trees.give_back(&lease.tree);
            }
            SlotState::Taken => *state = SlotState::Taken,
            SlotState::Waiting | SlotState::Served(Err(_)) | SlotState::Abandoned => {}
        }
    }
}

impl Trees {
    /// Where a lease for `request` joins the queue: at the end, except that a rushed session's
    /// lease goes ahead of every queued lease but an earlier rush's (see `crate::rush`).
    pub(super) fn queue_position(&self, locked: &Locked, request: &LeaseRequest) -> usize {
        let end = locked.waiters.len();
        if !self.rushed(locked, request) {
            return end;
        }
        locked
            .waiters
            .iter()
            .position(|waiter| !self.rushed(locked, waiter.request()))
            .unwrap_or(end)
    }

    /// Whether `request` comes from a rushed session or for a rushed ticket.
    fn rushed(&self, locked: &Locked, request: &LeaseRequest) -> bool {
        crate::rush::rushed(
            &locked.db,
            self.inner.clock.now(),
            Some(&request.ticket),
            request.session.as_ref().map(SessionId::as_str),
        )
    }
}
