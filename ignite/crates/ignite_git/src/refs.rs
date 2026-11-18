//! Reading and fetching refs: a remote branch's tip, a tree's HEAD, commits between two points.

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::{Error, Result};

use crate::cmd;

/// Fetches `branch` from `remote` and returns its newest commit.
///
/// The fetch updates the remote-tracking ref `refs/remotes/<remote>/<branch>`, so the commit's
/// objects are present for a later rebase or checkout. A branch the remote does not have is
/// [`Error::NotFound`]; any other failed fetch is [`Error::Unreachable`].
pub fn remote_tip(repo: &Path, remote: &str, branch: &str) -> Result<Commit> {
    let tracking = format!("refs/remotes/{remote}/{branch}");
    let refspec = format!("+refs/heads/{branch}:{tracking}");
    let mut fetch = cmd::git(repo, ["fetch", "--quiet", "--no-tags", remote, &refspec]);
    let out = cmd::output(&mut fetch)?;
    if !out.status.success() {
        let message = cmd::failure(&fetch, &out);
        if String::from_utf8_lossy(&out.stderr).contains("couldn't find remote ref") {
            return Err(Error::NotFound(message));
        }
        return Err(Error::Unreachable(message));
    }
    cmd::commit(&cmd::run(
        repo,
        ["rev-parse", "--verify", &format!("{tracking}^{{commit}}")],
    )?)
}

/// The commit a tree has checked out.
pub fn head(tree: &Path) -> Result<Commit> {
    cmd::commit(&cmd::run(tree, ["rev-parse", "--verify", "HEAD^{commit}"])?)
}

/// The branch a tree has checked out, or `None` for a detached `HEAD`.
pub fn branch(tree: &Path) -> Result<Option<String>> {
    let mut command = cmd::git(tree, ["symbolic-ref", "--quiet", "--short", "HEAD"]);
    let out = cmd::output(&mut command)?;
    if out.status.success() {
        Ok(Some(String::from_utf8_lossy(&out.stdout).trim().to_owned()))
    } else {
        Ok(None)
    }
}

/// The commits after `from` up to and including `to`, oldest first.
pub fn commits_between(repo: &Path, from: &Commit, to: &Commit) -> Result<Vec<Commit>> {
    let range = format!("{from}..{to}");
    cmd::run(repo, ["rev-list", "--reverse", &range])?
        .lines()
        .map(cmd::commit)
        .collect()
}
