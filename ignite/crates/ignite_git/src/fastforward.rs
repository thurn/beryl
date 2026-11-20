//! Keeping the main checkout's local integration branches at the remote's, by fast-forward
//! only. Sessions start in the main checkout and read its instruction files, so those should be
//! current, but nobody works there: a checkout with local changes, or a branch that has
//! commits the remote lacks, is left alone. Nothing here rebases or resets.

use std::path::Path;

use ignite_core::ids::Commit;
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

/// Fetches each of `branches` from `remote` and fast-forwards the local branch of that name in
/// the checkout at `repo`. The branch checked out moves with `git merge --ff-only`, which also
/// updates its files; any other moves with a compare-and-swap of its ref.
pub fn main_checkout(repo: &Path, remote: &str, branches: &[String]) -> Result<FastForward> {
    if busy(repo)? {
        return Ok(FastForward::Busy);
    }
    let current = cmd::run(repo, ["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .map(|name| name.trim().to_owned());
    let mut done = Vec::new();
    for branch in branches {
        let remote_tip = match refs::remote_tip(repo, remote, branch) {
            Ok(tip) => tip,
            Err(Error::NotFound(_)) => {
                done.push((branch.clone(), Branch::NotOnRemote));
                continue;
            }
            Err(error) => return Err(error),
        };
        let local_ref = format!("refs/heads/{branch}");
        let Ok(local) = cmd::run(repo, ["rev-parse", "--verify", "--quiet", &local_ref]) else {
            done.push((branch.clone(), Branch::Missing));
            continue;
        };
        let local = cmd::commit(&local)?;
        let outcome = if local == remote_tip {
            Branch::Current
        } else if !is_ancestor(repo, &local, &remote_tip)? {
            Branch::Diverged
        } else {
            if current.as_deref() == Some(branch.as_str()) {
                cmd::run(repo, ["merge", "--ff-only", "--quiet", remote_tip.as_str()])?;
            } else {
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
