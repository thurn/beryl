//! `host-borrow-cores` and `host-return-cores`: scheduler cores lent to a verb process for a
//! command it runs itself. An express run in a verb process has its own reserved cores, and
//! borrows whatever else is free when it starts so that it finishes sooner. The grant is the
//! scheduler's, so a higher class can reclaim it like any other, and the host releases whatever
//! a handle still holds when the handle ends, so a verb process that dies cannot keep cores.

use ignite_core::ids::Handle;
use ignite_core::protocol::host::{BorrowCores, BorrowCoresReply};

use super::Host;
use crate::handles::lock;

impl Host {
    pub(super) fn borrow_cores(&self, call: &BorrowCores) -> BorrowCoresReply {
        match self.starter.borrow_cores(call.cores) {
            Some((grant, cores)) => {
                lock(&self.borrowed)
                    .entry(call.handle.clone())
                    .or_default()
                    .push(grant);
                BorrowCoresReply {
                    grant: Some(grant),
                    cores,
                }
            }
            None => BorrowCoresReply {
                grant: None,
                cores: 0,
            },
        }
    }

    /// Releases `grant` if `handle` still holds it; a grant already returned is ignored.
    pub(super) fn return_cores(&self, handle: &Handle, grant: u64) {
        let held = {
            let mut borrowed = lock(&self.borrowed);
            let grants = borrowed.entry(handle.clone()).or_default();
            let before = grants.len();
            grants.retain(|held| *held != grant);
            let held = grants.len() != before;
            if grants.is_empty() {
                borrowed.remove(handle);
            }
            held
        };
        if held {
            self.starter.return_cores(grant);
        }
    }

    /// Releases every grant `handle` still holds, as when its verb process ended.
    pub(super) fn return_all_cores(&self, handle: &Handle) {
        let grants = lock(&self.borrowed).remove(handle).unwrap_or_default();
        for grant in grants {
            self.starter.return_cores(grant);
        }
    }
}
