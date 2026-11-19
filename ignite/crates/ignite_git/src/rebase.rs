//! Rebasing a tree's branch onto the tip, aborting cleanly on a conflict.
//!
//! `git rebase` first checks out the tip and then applies the branch's commits again, so it
//! writes every file that differs between the branch and the tip, the branch's own edits
//! among them, even when the file ends with the content it had. Cargo decides whether a crate
//! is current from its files' modification times, so on 2026-11-19 the landing of a one-card
//! comment change that rebased over commits touching only another Cargo workspace rebuilt
//! 74 crates downstream of the card's crate, which `ignite check` had built minutes before.
//! [`onto`] therefore gives each file that git wrote back with its old content the
//! modification time it had before the rebase.

use std::collections::HashSet;
use std::fs::{self, File};
use std::path::Path;
use std::time::SystemTime;

use ignite_core::ids::Commit;
use ignite_core::traits::{Error, Rebase, Result};

use crate::{changed, cmd, refs};

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

    let before = refs::head(tree)?;
    let times = times_of(tree, &changed::between(tree, &before, tip)?);
    let mut rebase = cmd::git(tree, ["rebase", "--no-autostash", tip.as_str()]);
    let out = cmd::output(&mut rebase)?;
    if out.status.success() {
        restore_times(tree, &before, &times)?;
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
    restore_times(tree, &before, &times)?;
    let mut files: Vec<String> = unmerged?
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    files.sort();
    files.dedup();
    Ok(Rebase::Conflict { files })
}

/// The modification time of each of `paths` that is a regular file in `tree`.
fn times_of(tree: &Path, paths: &[String]) -> Vec<(String, SystemTime)> {
    paths
        .iter()
        .filter_map(|path| {
            let meta = fs::symlink_metadata(tree.join(path)).ok()?;
            meta.is_file()
                .then(|| meta.modified().ok().map(|time| (path.clone(), time)))?
        })
        .collect()
}

/// Gives each file of `times` whose content is the same at the tree's head as at `before` the
/// modification time it had before the rebase. A file that differs was changed by the commits
/// that landed, and keeps the time git gave it, so the crates that read it are rebuilt.
fn restore_times(tree: &Path, before: &Commit, times: &[(String, SystemTime)]) -> Result<()> {
    let after = refs::head(tree)?;
    let differing: HashSet<String> = changed::between(tree, before, &after)?
        .into_iter()
        .collect();
    for (path, time) in times {
        if differing.contains(path) {
            continue;
        }
        let full = tree.join(path);
        let Ok(meta) = fs::symlink_metadata(&full) else {
            continue;
        };
        if meta.is_file() && meta.modified().ok() != Some(*time) {
            File::options()
                .write(true)
                .open(&full)
                .and_then(|file| file.set_modified(*time))
                .map_err(|error| Error::Io(format!("{}: {error}", full.display())))?;
        }
    }
    Ok(())
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
