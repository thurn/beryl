//! A ticket's branch can be checked out in only one worktree at a time, so a lease first frees
//! it from any other tree that still has it.
//!
//! A tree normally lets go of its ticket's branch when it is reset to its park branch at a
//! release. When that reset fails (a verb that would not stop, or a lock left by a command the
//! release killed), the release still goes through at the hub, and the tree keeps the branch
//! until a later retry resets it. Meanwhile the hub gives the ticket to the next worker, whose
//! `ignite start` failed because git refused to check the branch out a second time: a worker
//! had to detach the old tree by hand before it could start the ticket. The hub lets one
//! session hold a ticket, so another of these trees on the branch is always such a leftover.

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::Result;
use ignite_git::worktree;

use super::Trees;

impl Trees {
    /// Puts the tree at `path` on `branch` at `at`, as a clean checkout that keeps its build
    /// directories, after freeing the branch from any other tree.
    pub(super) fn check_out_branch(&self, path: &Path, branch: &str, at: &Commit) -> Result<()> {
        self.free_branch(path, branch)?;
        worktree::reset(path, branch, at, &self.inner.config.keep.get())
    }

    /// Detaches every other tree under the trees' directory that has `branch` checked out, so
    /// that the tree at `path` can check it out. Worktrees outside that directory, such as the
    /// main checkout, are left alone, and git then refuses the checkout as before.
    fn free_branch(&self, path: &Path, branch: &str) -> Result<()> {
        let config = &self.inner.config;
        for other in worktree::list(&config.repo)? {
            let holds = other.branch.as_deref() == Some(branch)
                && other.path != path
                && other.path.starts_with(&config.dir);
            if let (true, Some(head)) = (holds, &other.head) {
                worktree::detach(&other.path, head)?;
                eprintln!(
                    "ignite: detached {} from {branch}, which a release left checked out there",
                    other.path.display()
                );
            }
        }
        Ok(())
    }
}
