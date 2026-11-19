//! Whether one commit's history already holds another commit's changes to a directory, even
//! when those changes reached it as different commits, as a rebase makes them.
//!
//! A release branch moved by hand to a ticket's commit holds that commit, and the landing of the
//! same ticket then pushes a rebased copy of it to the integration branch. Neither commit is in
//! the other's history, yet the rebased copy holds every change the first one made. Telling that
//! apart from a branch that really lacks the changes is what lets a daemon running the first
//! commit accept a release built from the copy, and lets the landing move the release branch
//! over the first commit.

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

use crate::{cmd, refs};

/// The most commits of `history` compared by tree; a release branch that diverged further than
/// this from the version is treated as not holding it.
const MOST_COMPARED: usize = 200;

/// Whether `history` holds every change under `path` that `version` holds. It does when:
///
/// - `version` is in `history`'s history;
/// - a commit of `history` that `version` lacks, or the newest commit both share, has the same
///   tree at `path` as `version`, as the rebased copy of a commit does when nothing else
///   changed `path` meanwhile;
/// - every commit of `version` that `history` lacks and that changes `path` has an equivalent
///   in `history` (the same patch, as `git cherry` compares them), as the rebased copy of a
///   commit does when other changes to `path` landed before it.
///
/// A `version` this repository does not have is not held.
pub fn holds_changes(repo: &Path, history: &Commit, version: &Commit, path: &str) -> Result<bool> {
    if !known(repo, version)? {
        return Ok(false);
    }
    if refs::is_ancestor(repo, version, history)? {
        return Ok(true);
    }
    let wanted = tree_at(repo, version, path)?;
    let range = [history.as_str().to_owned(), format!("^{version}")];
    let mut candidates = commits_changing(repo, &range, path, Some(MOST_COMPARED))?;
    candidates.extend(merge_base(repo, history, version)?);
    for candidate in &candidates {
        if tree_at(repo, candidate, path)? == wanted {
            return Ok(true);
        }
    }
    let range = [version.as_str().to_owned(), format!("^{history}")];
    let missing = commits_changing(repo, &range, path, None)?;
    if missing.is_empty() {
        return Ok(true);
    }
    // `git cherry <upstream> <head>` prints each commit of head that upstream lacks, marked `-`
    // when upstream holds an equivalent change and `+` when it does not.
    let cherry = cmd::run(repo, ["cherry", history.as_str(), version.as_str()])?;
    let equivalent: Vec<&str> = cherry
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(str::trim)
        .collect();
    Ok(missing
        .iter()
        .all(|commit| equivalent.contains(&commit.as_str())))
}

/// Whether the repository has `commit`.
fn known(repo: &Path, commit: &Commit) -> Result<bool> {
    let spec = format!("{commit}^{{commit}}");
    let mut command = cmd::git(repo, ["cat-file", "-e", &spec]);
    Ok(cmd::output(&mut command)?.status.success())
}

/// The object ID of `path` in `commit`, or `None` when the commit has no such path.
fn tree_at(repo: &Path, commit: &Commit, path: &str) -> Result<Option<String>> {
    let spec = format!("{commit}:{}", path.trim_end_matches('/'));
    let mut command = cmd::git(repo, ["rev-parse", "--verify", "--quiet", &spec]);
    let out = cmd::output(&mut command)?;
    if !out.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&out.stdout).trim().to_owned()))
}

/// The commits of `range` (`git rev-list` arguments) that change `path`, newest first.
fn commits_changing(
    repo: &Path,
    range: &[String],
    path: &str,
    most: Option<usize>,
) -> Result<Vec<Commit>> {
    let mut command = cmd::git(repo, ["rev-list"]);
    if let Some(most) = most {
        command.arg(format!("--max-count={most}"));
    }
    command.args(range).arg("--").arg(path);
    cmd::checked(&mut command)?
        .lines()
        .map(cmd::commit)
        .collect()
}

/// The newest commit both histories share, if they share one.
fn merge_base(repo: &Path, a: &Commit, b: &Commit) -> Result<Option<Commit>> {
    let mut command = cmd::git(repo, ["merge-base", a.as_str(), b.as_str()]);
    let out = cmd::output(&mut command)?;
    if !out.status.success() {
        return Ok(None);
    }
    cmd::commit(String::from_utf8_lossy(&out.stdout).trim()).map(Some)
}

#[cfg(test)]
#[path = "holds_tests.rs"]
mod tests;
