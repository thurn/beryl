//! Saving a tree's work before it is reset: whatever is not committed becomes one commit on top
//! of the tree's `HEAD`, and that commit is pushed to a branch of its own on the remote.
//!
//! The commit is made through a temporary index with `git commit-tree`, so the tree's branch,
//! index and files are never touched. A failed push therefore leaves the tree exactly as it was.
//! A tree in the middle of a rebase is saved as it is: its `HEAD` is the rebase's progress, and
//! files with conflicts are saved with their conflict markers.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ignite_core::ids::Commit;
use ignite_core::traits::{Error, Result};

use crate::cmd;

/// Gives each temporary index in this process a distinct name.
static NEXT_INDEX: AtomicU64 = AtomicU64::new(0);

/// What [`save`] pushed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    /// The commit now at the remote branch.
    pub commit: Commit,
    pub branch: String,
    /// The paths that were not committed and went into the commit made at save, as
    /// `git status` names them. Empty when everything was committed and `commit` is `HEAD`.
    pub uncommitted: Vec<String>,
}

/// Saves the tree's work to `branch` on `remote`, replacing whatever that branch held.
///
/// When the tree holds edits that are not committed, they become one commit with `message` on
/// top of `HEAD`. The push is forced: the branch belongs to one ticket, and a rebase in an
/// earlier session may have rewritten it. A push the remote refuses is [`Error::Conflict`]; a
/// remote that cannot be reached is [`Error::Unreachable`].
pub fn save(tree: &Path, remote: &str, branch: &str, message: &str) -> Result<Saved> {
    let head = cmd::commit(&cmd::run(tree, ["rev-parse", "--verify", "HEAD^{commit}"])?)?;
    let uncommitted = uncommitted(tree)?;
    let commit = if uncommitted.is_empty() {
        head
    } else {
        commit_working_tree(tree, &head, message)?
    };
    let target = format!("refs/heads/{branch}");
    let refspec = format!("+{commit}:{target}");
    let mut push = cmd::git(tree, ["push", "--porcelain", remote, &refspec]);
    let out = cmd::output(&mut push)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().find(|line| {
        line.split('\t')
            .nth(1)
            .and_then(|refs| refs.split_once(':'))
            .is_some_and(|(_, to)| to == target)
    });
    match line {
        Some(line) if line.starts_with('!') => Err(Error::Conflict(format!(
            "the remote refused {branch}: {}",
            line.split('\t').nth(2).unwrap_or(line)
        ))),
        Some(_) if out.status.success() => Ok(Saved {
            commit,
            branch: branch.to_owned(),
            uncommitted,
        }),
        _ => Err(Error::Unreachable(cmd::failure(&push, &out))),
    }
}

/// The paths `git status` lists, untracked files included and ignored ones left out.
pub fn uncommitted(tree: &Path) -> Result<Vec<String>> {
    // `--no-optional-locks` stops status from refreshing and rewriting the real index.
    let status = cmd::run(
        tree,
        [
            "--no-optional-locks",
            "status",
            "--porcelain",
            "-z",
            "--no-renames",
            "--untracked-files=all",
        ],
    )?;
    Ok(status
        .split('\0')
        .filter(|entry| entry.len() > 3)
        .map(|entry| entry[3..].to_owned())
        .collect())
}

/// Commits every file of the working tree on top of `head` without moving any ref.
fn commit_working_tree(tree: &Path, head: &Commit, message: &str) -> Result<Commit> {
    let real = cmd::run(
        tree,
        ["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )?;
    let real = PathBuf::from(real.trim());
    let temporary = TemporaryIndex(PathBuf::from(format!(
        "{}.ignite-save-{}-{}",
        real.display(),
        std::process::id(),
        NEXT_INDEX.fetch_add(1, Ordering::Relaxed)
    )));
    if real.exists() {
        fs::copy(&real, &temporary.0)?;
    }
    // `add --all` also settles paths with conflicts, staging each file as it stands.
    let mut add = cmd::git(tree, ["add", "--all", "--", "."]);
    add.env("GIT_INDEX_FILE", &temporary.0);
    cmd::checked(&mut add)?;
    let mut write = cmd::git(tree, ["write-tree"]);
    write.env("GIT_INDEX_FILE", &temporary.0);
    let root = cmd::oid(&cmd::checked(&mut write)?)?;
    drop(temporary);
    cmd::commit(&cmd::run(
        tree,
        [
            "commit-tree",
            root.as_str(),
            "-p",
            head.as_str(),
            "-m",
            message,
        ],
    )?)
}

/// A temporary index file, deleted when dropped.
struct TemporaryIndex(PathBuf);

impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
