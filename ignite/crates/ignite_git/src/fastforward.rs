//! Keeping the main checkout's local integration branches at the remote's, by fast-forward
//! only. Sessions start in the main checkout and read its instruction files, so those should be
//! current, but nobody works there: a checkout with local changes, or a branch that has
//! commits the remote lacks, is left alone. Nothing here rebases or resets.

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::refname;
use ignite_core::traits::{Error, Result};

use crate::{cmd, rebase, refs};

/// What happened to one branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Branch {
    /// The local branch already matched the remote.
    Current,
    Moved {
        from: Commit,
        to: Commit,
    },
    /// The local branch has commits the remote does not, so it cannot fast-forward.
    Diverged,
    /// The checkout has no local branch of that name.
    Missing,
    /// The remote has no branch of that name. A branch that a tip round creates, such as
    /// `repository.green`, is absent until the first round pushes it.
    NotOnRemote,
}

/// What [`main_checkout`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FastForward {
    /// The checkout has edits to tracked files, or a rebase or merge in progress; nothing moved.
    /// Untracked files do not count.
    Busy,
    /// Each branch asked for, with what happened to it.
    Branches(Vec<(String, Branch)>),
}

/// A phase of the main checkout's fast-forward, reported before it starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step<'a> {
    CheckingCheckout,
    ReadingHead,
    Fetching(&'a str),
    Comparing(&'a str),
    Merging(&'a str),
    UpdatingRef(&'a str),
}

/// Fetches each of `branches` from `remote` and fast-forwards the local branch kept at it
/// ([`refname::local_branch`]: `staging` for both `staging` and `refs/ignite/staging`) in the
/// checkout at `repo`. The branch checked out moves with `git merge --ff-only`, which also
/// updates its files; any other moves with a compare-and-swap of its ref.
pub fn main_checkout(repo: &Path, remote: &str, branches: &[String]) -> Result<FastForward> {
    main_checkout_with_progress(repo, remote, branches, |_| {})
}

/// Like [`main_checkout`], with a callback before each phase that runs Git.
pub fn main_checkout_with_progress(
    repo: &Path,
    remote: &str,
    branches: &[String],
    mut progress: impl FnMut(Step<'_>),
) -> Result<FastForward> {
    progress(Step::CheckingCheckout);
    if busy(repo)? {
        return Ok(FastForward::Busy);
    }
    progress(Step::ReadingHead);
    let current = cmd::run(repo, ["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .map(|name| name.trim().to_owned());
    let mut done = Vec::new();
    for branch in branches {
        progress(Step::Fetching(branch));
        let remote_tip = match refs::remote_tip(repo, remote, branch) {
            Ok(tip) => tip,
            Err(Error::NotFound(_)) => {
                done.push((branch.clone(), Branch::NotOnRemote));
                continue;
            }
            Err(error) => return Err(error),
        };
        let local_branch = refname::local_branch(branch);
        let local_ref = format!("refs/heads/{local_branch}");
        progress(Step::Comparing(&local_ref));
        let Ok(local) = cmd::run(repo, ["rev-parse", "--verify", "--quiet", &local_ref]) else {
            done.push((branch.clone(), Branch::Missing));
            continue;
        };
        let local = cmd::commit(&local)?;
        sync_upstream(repo, remote, branch, &local_branch)?;
        let outcome = if local == remote_tip {
            Branch::Current
        } else if !is_ancestor(repo, &local, &remote_tip)? {
            Branch::Diverged
        } else {
            if current.as_deref() == Some(local_branch.as_str()) {
                progress(Step::Merging(&local_ref));
                cmd::run(repo, ["merge", "--ff-only", "--quiet", remote_tip.as_str()])?;
            } else {
                progress(Step::UpdatingRef(&local_ref));
                cmd::run(
                    repo,
                    [
                        "update-ref",
                        &local_ref,
                        remote_tip.as_str(),
                        local.as_str(),
                    ],
                )?;
            }
            Branch::Moved {
                from: local,
                to: remote_tip,
            }
        };
        done.push((branch.clone(), outcome));
    }
    Ok(FastForward::Branches(done))
}

/// Points `local_branch`'s upstream at `remote` and `refs/heads/<branch>` when `branch` is a
/// remote branch and the local branch is either untracked or tracks a different ref on the same
/// remote (such as a stale `refs/ignite/staging` upstream after switching back to a branch).
/// A branch tracking another remote, or a custom ref outside `refs/heads/`, is left alone.
fn sync_upstream(repo: &Path, remote: &str, branch: &str, local_branch: &str) -> Result<()> {
    if !refname::is_branch(branch) {
        return Ok(());
    }
    let expected_merge = refname::remote_ref(branch);
    let remote_key = format!("branch.{local_branch}.remote");
    let merge_key = format!("branch.{local_branch}.merge");
    let current_remote = refs::config_get(repo, &remote_key)?;
    if let Some(other) = &current_remote
        && other != remote
    {
        return Ok(());
    }
    let current_merge = refs::config_get(repo, &merge_key)?;
    if current_remote.as_deref() != Some(remote) {
        cmd::run(repo, ["config", &remote_key, remote])?;
    }
    if current_merge.as_deref() != Some(expected_merge.as_str()) {
        cmd::run(repo, ["config", &merge_key, &expected_merge])?;
    }
    Ok(())
}

fn busy(repo: &Path) -> Result<bool> {
    let status = cmd::run(
        repo,
        [
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=no",
        ],
    )?;
    if !status.trim().is_empty() || rebase::in_progress(repo)? {
        return Ok(true);
    }
    let merge_head = cmd::run(
        repo,
        [
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "MERGE_HEAD",
        ],
    )?;
    Ok(Path::new(merge_head.trim()).exists())
}

fn is_ancestor(repo: &Path, ancestor: &Commit, of: &Commit) -> Result<bool> {
    let mut command = cmd::git(
        repo,
        [
            "merge-base",
            "--is-ancestor",
            ancestor.as_str(),
            of.as_str(),
        ],
    );
    let out = cmd::output(&mut command)?;
    match out.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(Error::Io(cmd::failure(&command, &out))),
    }
}
