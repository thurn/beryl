//! Rebasing a tree's branch onto the tip, aborting cleanly on a conflict.

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::{Error, Rebase, Result};

use crate::{cmd, refs};

/// Rebases the branch checked out in `tree` onto `tip`.
///
/// On a conflict the rebase is aborted, so the tree is back on its branch as it was, with no
/// rebase in progress, and the conflicting files are returned. Any other failure, such as
/// uncommitted edits that stop git from starting, is an error and leaves the tree as git left
/// it.
pub fn onto(tree: &Path, tip: &Commit) -> Result<Rebase> {
    let base = cmd::run(tree, ["merge-base", "HEAD", tip.as_str()])?;
    let landed_since = cmd::run(
        tree,
        ["rev-list", "--count", &format!("{}..{tip}", base.trim())],
    )?
    .trim()
    .parse::<u32>()
    .map_err(|error| Error::Io(format!("git rev-list --count: {error}")))?;

    let mut rebase = cmd::git(tree, ["rebase", "--no-autostash", tip.as_str()]);
    let out = cmd::output(&mut rebase)?;
    if out.status.success() {
        return Ok(Rebase::Clean {
            onto: tip.clone(),
            head: refs::head(tree)?,
            landed_since,
        });
    }
    if !in_progress(tree)? {
        return Err(Error::Io(cmd::failure(&rebase, &out)));
    }
    let unmerged = cmd::run(tree, ["diff", "--name-only", "--diff-filter=U", "-z"]);
    // Abort before reporting any error from the listing, so the tree is never left mid-rebase.
    cmd::run(tree, ["rebase", "--abort"])?;
    let mut files: Vec<String> = unmerged?
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    files.sort();
    files.dedup();
    Ok(Rebase::Conflict { files })
}

/// Whether a rebase has stopped partway in `tree`.
pub(crate) fn in_progress(tree: &Path) -> Result<bool> {
    for name in ["rebase-merge", "rebase-apply"] {
        let path = cmd::run(
            tree,
            ["rev-parse", "--path-format=absolute", "--git-path", name],
        )?;
        if Path::new(path.trim()).exists() {
            return Ok(true);
        }
    }
    Ok(false)
}
