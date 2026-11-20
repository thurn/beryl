//! The idle host of a verb process: landing trees are leased and freed through the daemon, so
//! that the tree is owned by the idle job's handle and a restarted daemon keeps it while the job
//! runs (see `crate::host`). Everything else is answered from the machine's trees as the daemon
//! would.

use std::path::PathBuf;
use std::sync::Arc;

use ignite_core::ids::{Commit, TreeId};
use ignite_core::protocol::host::{LeaseLanding, LeaseLandingReply, ReleaseLanding};
use ignite_core::traits::Result;

use super::{DiskReport, IdleHost};
use crate::verbproc::Remote;

/// How many landing trees idle work leaves free for landings and tip verification.
const KEEP_FREE: u32 = 1;

/// [`IdleHost`] for a verb process.
pub struct RemoteIdleHost {
    pub remote: Arc<Remote>,
    /// Answers what needs no lease: [`super::LiveHost`] in a verb process.
    pub local: Box<dyn IdleHost>,
}

impl IdleHost for RemoteIdleHost {
    fn lease(&self, commit: &Commit) -> Result<Option<(TreeId, PathBuf)>> {
        let call = LeaseLanding {
            handle: self.remote.link.handle().clone(),
            holder: "idle work".to_owned(),
            commit: commit.clone(),
            keep_free: KEEP_FREE,
        };
        Ok(match self.remote.link.lease_landing(&call)? {
            LeaseLandingReply::Leased { tree, path, .. } => Some((tree, path)),
            LeaseLandingReply::Busy => None,
        })
    }

    fn release(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()> {
        let call = ReleaseLanding {
            handle: self.remote.link.handle().clone(),
            tree: tree.clone(),
            built: built_at.cloned(),
        };
        self.remote.link.release_landing(&call).map(|_| ())
    }

    fn commits_between(&self, from: &Commit, to: &Commit) -> Result<usize> {
        self.local.commits_between(from, to)
    }

    fn hold_disk(&self, short: u64) -> Result<DiskReport> {
        self.local.hold_disk(short)
    }

    fn free_disk(&self, floor_gb: u64) -> Result<DiskReport> {
        self.local.free_disk(floor_gb)
    }

    fn disk_free_gb(&self) -> Option<u64> {
        self.local.disk_free_gb()
    }
}
