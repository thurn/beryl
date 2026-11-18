//! `host-lease-landing` and `host-release-landing`: landing trees leased to a verb process's
//! handle, which the tree records as its owner (see [`crate::trees`]), so a restarted daemon
//! keeps the tree while the handle runs. A repeated lease returns the tree the handle already
//! holds, untouched when it is at the commit asked for; a repeated release leaves alone a tree
//! the handle no longer holds.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use ignite_core::ids::{Commit, Handle, TreeId};
use ignite_core::protocol::host::{LeaseLanding, LeaseLandingReply, ReleaseLanding};
use ignite_core::traits::{Error, Result};
use ignite_store::local::trees as store;

use super::{Host, LandingTrees};
use crate::handles::lock;
use crate::server::Context;
use crate::trees::{TreeLease, Trees};

impl Host {
    pub(super) fn lease_landing(
        self: &Arc<Self>,
        ctx: &Context,
        call: &LeaseLanding,
    ) -> Result<LeaseLandingReply> {
        self.running(ctx, &call.handle)?;
        let keep_free = usize::try_from(call.keep_free).unwrap_or(usize::MAX);
        Ok(
            match self
                .landing
                .lease(&call.holder, &call.handle, &call.commit, keep_free)?
            {
                Some(lease) => LeaseLandingReply::Leased {
                    tree: lease.tree,
                    path: lease.path,
                    branch: lease.branch,
                    at: lease.at,
                },
                None => LeaseLandingReply::Busy,
            },
        )
    }

    pub(super) fn release_landing(&self, call: &ReleaseLanding) -> Result<()> {
        self.landing
            .release(&call.handle, &call.tree, call.built.as_ref())
    }
}

/// [`LandingTrees`] over the machine's trees.
pub struct TreesLanding {
    pub trees: Trees,
    /// The local database, to read which handle owns a landing tree.
    pub db: Arc<Mutex<Connection>>,
}

impl LandingTrees for TreesLanding {
    fn lease(
        &self,
        holder: &str,
        owner: &Handle,
        at: &Commit,
        keep_free: usize,
    ) -> Result<Option<TreeLease>> {
        self.trees
            .lease_landing_keeping(holder, Some(owner), at, keep_free)
    }

    fn release(&self, owner: &Handle, tree: &TreeId, built: Option<&Commit>) -> Result<()> {
        let row = store::get(&lock(&self.db), tree.as_str())
            .map_err(|e| Error::Io(format!("local store: {e}")))?;
        let held = row.is_some_and(|row| {
            row.state == store::State::Landing.as_str()
                && row.owner.as_deref() == Some(owner.as_str())
        });
        if held {
            self.trees.release_landing(tree, built)?;
        }
        Ok(())
    }
}
