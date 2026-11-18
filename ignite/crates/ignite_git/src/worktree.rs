//! Build trees as git worktrees: adding one at a commit, resetting it to a clean checkout
//! that keeps its build directories, listing and removing them.

use std::path::{Path, PathBuf};

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

use crate::{cmd, rebase};

/// A worktree as `git worktree list` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// `None` for a repository with no commit yet.
    pub head: Option<Commit>,
    /// The short branch name; `None` when the tree's HEAD is detached.
    pub branch: Option<String>,
}

/// Adds a worktree at `path` on a new or reset `branch` at `at`.
///
/// Registrations of worktrees whose directories were deleted are pruned first, so a tree
/// removed by hand can be added again at the same path.
pub fn add(repo: &Path, path: &Path, branch: &str, at: &Commit) -> Result<()> {
    cmd::run(repo, ["worktree", "prune"])?;
    let mut command = cmd::git(repo, ["worktree", "add", "--quiet", "-B", branch]);
    command.arg(path).arg(at.as_str());
    cmd::checked(&mut command)?;
    Ok(())
}

/// Puts an existing tree back to a clean checkout of `branch` at `at`.
///
/// Any rebase or merge in progress is aborted, edits to tracked files are discarded, and every
/// untracked file is deleted, ignored ones included, except under the directories in `keep`.
/// Those are the build directories, named relative to the tree's root (`out/build`), whose
/// contents survive so the next build in the tree is incremental.
pub fn reset(tree: &Path, branch: &str, at: &Commit, keep: &[String]) -> Result<()> {
    if rebase::in_progress(tree)? {
        cmd::run(tree, ["rebase", "--abort"])?;
    }
    let merge_head = cmd::run(
        tree,
        [
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "MERGE_HEAD",
        ],
    )?;
    if Path::new(merge_head.trim()).exists() {
        cmd::run(tree, ["merge", "--abort"])?;
    }
    cmd::run(
        tree,
        ["checkout", "--quiet", "--force", "-B", branch, at.as_str()],
    )?;
    // `-x` stops git from sparing ignored files; patterns given with `-e` still apply, so only
    // the kept directories survive.
    let mut clean = cmd::git(tree, ["clean", "--quiet", "-ffdx"]);
    for dir in keep {
        clean.arg("-e").arg(format!("/{}/", dir.trim_matches('/')));
    }
    cmd::checked(&mut clean)?;
    Ok(())
}

/// Every worktree of the repository, the main checkout first.
pub fn list(repo: &Path) -> Result<Vec<Worktree>> {
    let out = cmd::run(repo, ["worktree", "list", "--porcelain", "-z"])?;
    let mut trees = Vec::new();
    let mut current: Option<Worktree> = None;
    for field in out.split('\0') {
        if let Some(path) = field.strip_prefix("worktree ") {
            trees.extend(current.take());
            current = Some(Worktree {
                path: PathBuf::from(path),
                head: None,
                branch: None,
            });
        } else if let Some(tree) = current.as_mut() {
            if let Some(sha) = field.strip_prefix("HEAD ") {
                // A repository with no commit yet reports an all-zero HEAD.
                tree.head =
                    Some(cmd::commit(sha)?).filter(|c| c.as_str().bytes().any(|b| b != b'0'));
            } else if let Some(name) = field.strip_prefix("branch ") {
                tree.branch = Some(name.strip_prefix("refs/heads/").unwrap_or(name).to_owned());
            }
        }
    }
    trees.extend(current);
    Ok(trees)
}

/// Removes the worktree at `path`, discarding its edits and untracked files.
pub fn remove(repo: &Path, path: &Path) -> Result<()> {
    let mut command = cmd::git(repo, ["worktree", "remove", "--force"]);
    command.arg(path);
    cmd::checked(&mut command)?;
    Ok(())
}
