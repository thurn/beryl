//! Changed paths in worktrees, between commits, and per landing.
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

/// Paths among `files` whose changes between `from` and `to` touch only test-only code in a
/// Rust source file (`#[cfg(test)]` items, comments, or whitespace). Added or deleted files
/// are not reported as test-only.
pub fn test_only_files(
    repo: &Path,
    from: &Commit,
    to: &Commit,
    files: &[String],
) -> Result<Vec<String>> {
    let mut result = Vec::new();
    for file in files {
        if !Path::new(file)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
        {
            continue;
        }
        let Some(before) = blob(repo, from, file)? else {
            continue;
        };
        let Some(after) = blob(repo, to, file)? else {
            continue;
        };
        if ignite_core::rust_source::is_test_only_change(&before, &after) {
            result.push(file.clone());
        }
    }
    Ok(result)
}

/// The text of `path` at `commit`, or `None` when the file does not exist at that commit.
fn blob(repo: &Path, commit: &Commit, path: &str) -> Result<Option<String>> {
    let spec = format!("{commit}:{path}");
    let mut command = cmd::git(repo, ["show", &spec]);
    let out = cmd::output(&mut command)?;
    if out.status.success() {
        return Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()));
    }
    // `git show <commit>:<path>` exits 128 when the path does not exist in the commit's tree.
    if out.status.code() == Some(128) {
        return Ok(None);
    }
    Err(ignite_core::traits::Error::Io(cmd::failure(&command, &out)))
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

/// A worktree's changed paths since it diverged from the integration branch.
#[derive(Default)]
pub struct WorktreeChanges {
    pub paths: Vec<String>,
    /// Paths removed from the merge base. A rename also removes its old path.
    pub deleted: Vec<String>,
}

/// Compares the worktree, including uncommitted changes, with its merge base with e.g.
/// `origin/staging`.
pub fn worktree(root: &Path, integration: &str) -> Result<WorktreeChanges> {
    let base = cmd::run(root, ["merge-base", "HEAD", integration])?;
    let diff = cmd::run(
        root,
        [
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            base.trim(),
            "--",
        ],
    )?;
    let untracked = cmd::run(root, ["ls-files", "-z", "--others", "--exclude-standard"])?;
    let mut paths: BTreeSet<_> = untracked
        .split_terminator('\0')
        .map(str::to_owned)
        .collect();
    let mut deleted = Vec::new();
    let mut entries = diff.split_terminator('\0');
    while let (Some(status), Some(path)) = (entries.next(), entries.next()) {
        // A staged deletion followed by recreating the file leaves the path present.
        if status == "D" && !paths.contains(path) {
            deleted.push(path.to_owned());
        }
        paths.insert(path.to_owned());
    }
    deleted.sort();
    Ok(WorktreeChanges {
        paths: paths.into_iter().collect(),
        deleted,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_deletions_are_relative_to_the_merge_base() {
        let root = ignite_core::test_files::temp_dir().join("worktree-changes");
        std::fs::create_dir(&root).unwrap();
        let git = |args: &[&str]| cmd::run(&root, args).unwrap();
        git(&["init", "--quiet", "-b", "integration"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "user.email", "test@example.com"]);
        for path in ["old fixture.txt", "removed.txt", "restored.txt"] {
            std::fs::write(root.join(path), path).unwrap();
        }
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "base"]);
        git(&["branch", "topic"]);
        std::fs::write(root.join("upstream.txt"), "upstream").unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "advance integration"]);

        git(&["checkout", "--quiet", "topic"]);
        git(&["mv", "old fixture.txt", "new fixture.txt"]);
        git(&["commit", "--quiet", "-m", "rename fixture"]);
        std::fs::remove_file(root.join("removed.txt")).unwrap();
        git(&["rm", "restored.txt"]);
        std::fs::write(root.join("restored.txt"), "different").unwrap();

        let changed = worktree(&root, "integration").unwrap();
        assert_eq!(changed.deleted, ["old fixture.txt", "removed.txt"]);
        assert_eq!(
            changed.paths,
            [
                "new fixture.txt",
                "old fixture.txt",
                "removed.txt",
                "restored.txt"
            ]
        );
    }
}
