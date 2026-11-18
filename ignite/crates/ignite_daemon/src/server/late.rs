//! Verbs registered after the server exists, and a restart's first step.
//!
//! Most verbs are registered before the [`Server`] is built. A verb whose own parts need the
//! server first, such as `internal-restart`, which restarts the daemon through the self-update
//! that holds the server, is added afterwards with [`Server::register_late`].

use std::sync::{Arc, PoisonError};

use super::{Server, Verb};

impl Server {
    /// Adds a verb after the server was built. Registering a name twice, here or among the
    /// verbs the server was built with, is a programming error.
    pub fn register_late(&self, verb: Box<dyn Verb>) {
        let name = verb.name().to_owned();
        assert!(
            self.registry.get(&name).is_none(),
            "the verb {name:?} is registered twice"
        );
        self.late
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .register(verb);
    }

    pub(super) fn late_verb(&self, name: &str) -> Option<Arc<dyn Verb>> {
        self.late
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
    }

    /// Answers every new request from now on with [`super::restarting_reply`], as
    /// [`Server::drain`] does, without waiting for anything: a restart asked for by a client
    /// sets this before it replies, so the client's next request cannot reach this daemon.
    pub fn refuse_new_requests(&self) {
        self.draining
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
