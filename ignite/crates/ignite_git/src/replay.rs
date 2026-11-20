//! Rebasing and reverting commits in git's object database.
//!
//! [`crate::rebase::onto`] runs `git rebase` in the ticket tree, which rewrites the tree's files,
//! so the build that `ignite check` made there no longer matches and the landing compiles
//! everything that landed since the ticket started: on 2026-11-19 that was about 25 CPU-minutes
//! per landing on one machine. A landing that already has a passing gate for its files needs
//! only the rebased commit, so [`replay`] makes it with [`crate::objects`] and leaves every
//! working tree, index and ref as it was. [`revert`] does the same for the commit that undoes a
//! landing which broke the build.

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::{Error, Rebase, Result, RevertOutcome};

use crate::cmd;
use crate::committed::{IGNITE_EMAIL, IGNITE_NAME};
use crate::objects::{self, Merged};

/// Rebases the commits of `head` that `tip` does not hold onto `tip`, keeping each commit's
/// author, author date and message, and dropping a commit whose change `tip` already has.
///
/// Like `git rebase`, merge commits are left out and the commits of the branches they merged
/// are replayed one by one. The committer is whoever git's configuration names, as for
/// `git rebase`.
pub fn replay(repo: &Path, head: &Commit, tip: &Commit) -> Result<Rebase> {
    objects::require_merge_tree(repo)?;
    let base = objects::merge_base(repo, head, tip)?;
    let landed_since = count(repo, &base, tip)?;
    let range = format!("{tip}..{head}");
    let commits = cmd::run(
        repo,
        [
            "rev-list",
            "--reverse",
            "--topo-order",
            "--no-merges",
            &range,
        ],
    )?;

    let mut parent = tip.clone();
    let mut parent_tree = objects::tree_of(repo, tip)?;
    for line in commits.lines() {
        let commit = cmd::commit(line)?;
        let original_parent = objects::parent_of(repo, &commit)?;
        let tree = match objects::merge(repo, &original_parent, &parent, &commit)? {
            Merged::Clean(tree) => tree,
            Merged::Conflict(files) => return Ok(Rebase::Conflict { files }),
        };
        if tree == parent_tree {
            continue;
        }
        let info = objects::info(repo, &commit)?;
        parent = objects::commit_tree(
            repo,
            &tree,
            &parent,
            &info.message,
            &[
                ("GIT_AUTHOR_NAME", info.author_name.as_str()),
                ("GIT_AUTHOR_EMAIL", info.author_email.as_str()),
                ("GIT_AUTHOR_DATE", info.author_date.as_str()),
            ],
        )?;
        parent_tree = tree;
    }
    Ok(Rebase::Clean {
        onto: tip.clone(),
        head: parent,
        landed_since,
    })
}

/// One commit whose parent is `tip` and which undoes `commits` (oldest first), written by
/// Ignite's identity with `message`.
///
/// Each commit C, newest first, is undone by merging C's parent into the result so far with C
/// as the base. A later commit on `tip` that changed the same lines makes that merge conflict,
/// and the conflicting files are returned.
pub fn revert(
    repo: &Path,
    tip: &Commit,
    commits: &[Commit],
    message: &str,
) -> Result<RevertOutcome> {
    objects::require_merge_tree(repo)?;
    let identity = [
        ("GIT_AUTHOR_NAME", IGNITE_NAME),
        ("GIT_AUTHOR_EMAIL", IGNITE_EMAIL),
        ("GIT_COMMITTER_NAME", IGNITE_NAME),
        ("GIT_COMMITTER_EMAIL", IGNITE_EMAIL),
    ];
    // `merge-tree` merges commits, so each step but the last is kept as a commit no ref holds.
    let mut current = tip.clone();
    let mut tree = objects::tree_of(repo, tip)?;
    for commit in commits.iter().rev() {
        let parent = objects::parent_of(repo, commit)?;
        tree = match objects::merge(repo, commit, &current, &parent)? {
            Merged::Clean(tree) => tree,
            Merged::Conflict(files) => return Ok(RevertOutcome::Conflict { files }),
        };
        current = objects::commit_tree(repo, &tree, &current, "ignite: revert step", &identity)?;
    }
    let message = if message.ends_with('\n') {
        message.to_owned()
    } else {
        format!("{message}\n")
    };
    let commit = objects::commit_tree(repo, &tree, tip, &message, &identity)?;
    Ok(RevertOutcome::Clean { commit })
}

/// How many commits `to` holds that `from` does not.
fn count(repo: &Path, from: &Commit, to: &Commit) -> Result<u32> {
    cmd::run(repo, ["rev-list", "--count", &format!("{from}..{to}")])?
        .trim()
        .parse::<u32>()
        .map_err(|error| Error::Io(format!("git rev-list --count: {error}")))
}
