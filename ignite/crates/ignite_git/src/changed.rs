//! The paths that differ between two commits, in total and per landing.
//!
//! A landing is one ticket's commits, pushed to the integration branch together. Git does not
//! record where one landing ends and the next begins, so [`landings`] takes the last commit of
//! each landing from the caller, who has it from the hub's landing records.

use std::collections::BTreeSet;
use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

use crate::cmd;

/// The paths that differ between `from` and `to`, sorted. A renamed file appears under both
/// its old and its new path.
pub fn between(repo: &Path, from: &Commit, to: &Commit) -> Result<Vec<String>> {
    let out = cmd::run(
        repo,
        [
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            from.as_str(),
            to.as_str(),
        ],
    )?;
    let mut paths: Vec<String> = out
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    paths.sort();
    Ok(paths)
}

/// The paths one commit changed against its first parent, or added in a root commit.
pub fn in_commit(repo: &Path, commit: &Commit) -> Result<Vec<String>> {
    let out = cmd::run(
        repo,
        [
            "show",
            "--format=",
            "--diff-merges=first-parent",
            "--root",
            "--name-only",
            "--no-renames",
            "-z",
            commit.as_str(),
            "--",
        ],
    )?;
    Ok(out.split_terminator('\0').map(str::to_owned).collect())
}

/// One commit and the paths it changed against its first parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitPaths {
    pub commit: Commit,
    pub paths: Vec<String>,
}

/// One landing: its commits, oldest first, and the union of the paths they changed, sorted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landing {
    pub commits: Vec<Commit>,
    pub paths: Vec<String>,
}

/// The commits after `from` up to and including `to` on the first-parent line, oldest first,
/// each with the paths it changed. A merge commit counts as one commit whose paths are its
/// difference from its first parent, which is what it brought to the branch.
pub fn commits(repo: &Path, from: &Commit, to: &Commit) -> Result<Vec<CommitPaths>> {
    let range = format!("{from}..{to}");
    // Each commit prints as "\x01<sha>\0", followed, when it changed anything, by a newline
    // and its paths, each ending in "\0".
    let out = cmd::run(
        repo,
        [
            "log",
            "--reverse",
            "--first-parent",
            "--diff-merges=first-parent",
            "--no-renames",
            "--name-only",
            "-z",
            "--format=%x01%H",
            &range,
        ],
    )?;
    let mut result: Vec<CommitPaths> = Vec::new();
    for token in out.split('\0') {
        let token = token.strip_prefix('\n').unwrap_or(token);
        if let Some(sha) = token.strip_prefix('\u{1}') {
            result.push(CommitPaths {
                commit: cmd::commit(sha)?,
                paths: Vec::new(),
            });
        } else if !token.is_empty()
            && let Some(last) = result.last_mut()
        {
            last.paths.push(token.to_owned());
        }
    }
    Ok(result)
}

/// The landings after `from` up to and including `to`, oldest first.
///
/// `ends` names the last commit of each landing, in any order; IDs outside the range are
/// ignored. A landing runs from the commit after the previous end up to its own end. Commits
/// after the last end in the range form one more landing, so no commit is ever left out.
pub fn landings(repo: &Path, from: &Commit, to: &Commit, ends: &[Commit]) -> Result<Vec<Landing>> {
    Ok(group(commits(repo, from, to)?, ends))
}

/// Groups commits, oldest first as [`commits`] lists them, into landings by the last commit of
/// each, by the same rule as [`landings`].
pub fn group(commits: Vec<CommitPaths>, ends: &[Commit]) -> Vec<Landing> {
    let mut result = Vec::new();
    let mut commits_so_far = Vec::new();
    let mut paths = BTreeSet::new();
    for entry in commits {
        let is_end = ends.iter().any(|end| same_commit(end, &entry.commit));
        paths.extend(entry.paths);
        commits_so_far.push(entry.commit);
        if is_end {
            result.push(Landing {
                commits: std::mem::take(&mut commits_so_far),
                paths: std::mem::take(&mut paths).into_iter().collect(),
            });
        }
    }
    if !commits_so_far.is_empty() {
        result.push(Landing {
            commits: commits_so_far,
            paths: paths.into_iter().collect(),
        });
    }
    result
}

/// Whether two commit IDs name the same commit, allowing either to be abbreviated.
fn same_commit(a: &Commit, b: &Commit) -> bool {
    a.as_str().starts_with(b.as_str()) || b.as_str().starts_with(a.as_str())
}

/// The paths in the checkout at `root` whose files differ from its `HEAD`: tracked files with
/// changes staged or not, deleted files, and untracked files git does not ignore. Sorted.
pub fn uncommitted(root: &Path) -> Result<Vec<String>> {
    let modified = cmd::run(root, ["diff", "--name-only", "-z", "--no-renames", "HEAD"])?;
    let untracked = cmd::run(root, ["ls-files", "-z", "--others", "--exclude-standard"])?;
    let paths: BTreeSet<String> = modified
        .split('\0')
        .chain(untracked.split('\0'))
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    Ok(paths.into_iter().collect())
}
