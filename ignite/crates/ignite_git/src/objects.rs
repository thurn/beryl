//! Merging and committing in git's object database, without a working tree or index.
//!
//! `git merge-tree --write-tree` merges two commits against a given base and writes the result
//! as a tree object, and `git commit-tree` wraps a tree in a commit. Neither reads or writes a
//! working tree, the index or any ref, so [`crate::replay`] can rebase and revert commits while
//! the tree they came from keeps its files, and its build, exactly as they were.

use std::io::Write as _;
use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;

use ignite_core::ids::Commit;
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};

use crate::cmd;

/// The oldest git whose `merge-tree` has `--write-tree` (2.38) and `--merge-base` (2.40).
const MINIMUM: (u32, u32) = (2, 40);

/// How a three-way merge in the object database ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Merged {
    /// The ID of the merged tree.
    Clean(String),
    /// These paths conflicted, sorted.
    Conflict(Vec<String>),
}

/// The parts of a commit that a copy of it keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommitInfo {
    pub author_name: String,
    pub author_email: String,
    /// In git's internal form, `<seconds> <offset>`, which `GIT_AUTHOR_DATE` accepts.
    pub author_date: String,
    /// The message exactly as stored, trailing newline included.
    pub message: String,
}

/// Fails unless the `git` on `PATH` is new enough for [`merge`]. The answer is computed once per
/// process.
pub(crate) fn require_merge_tree(repo: &Path) -> Result<()> {
    static FOUND: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let text = cmd::run(repo, ["version"]).map_err(|error| error.to_string())?;
            check_version(text.trim())
        })
        .clone()
        .map_err(Error::Invalid)
}

/// `Ok` when `version`, the output of `git version`, names git 2.40 or later.
fn check_version(version: &str) -> std::result::Result<(), String> {
    let number = version.strip_prefix("git version ").unwrap_or(version);
    let mut parts = number
        .split(|c: char| !c.is_ascii_digit())
        .map(|part| part.parse::<u32>().ok());
    let found = match (parts.next().flatten(), parts.next().flatten()) {
        (Some(major), Some(minor)) => (major, minor),
        _ => return Err(format!("cannot read the git version from {version:?}")),
    };
    if found >= MINIMUM {
        return Ok(());
    }
    Err(format!(
        "rebasing without a working tree needs `git merge-tree --write-tree --merge-base`, \
         which git {}.{} added; found {version}",
        MINIMUM.0, MINIMUM.1
    ))
}

/// Merges `theirs` into `ours` as if both descended from `base`, writing the result as a tree
/// object. Exit status 1 with a tree line followed by paths is a conflict; anything else that
/// is not 0 is an error.
pub(crate) fn merge(repo: &Path, base: &Commit, ours: &Commit, theirs: &Commit) -> Result<Merged> {
    let mut command = cmd::git(
        repo,
        [
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            "-z",
            &format!("--merge-base={base}"),
            ours.as_str(),
            theirs.as_str(),
        ],
    );
    let out = cmd::output(&mut command)?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    match out.status.code() {
        Some(0) => match fields.next() {
            Some(tree) => Ok(Merged::Clean(tree.trim().to_owned())),
            None => Err(Error::Io(cmd::failure(&command, &out))),
        },
        Some(1) if fields.next().is_some() => {
            let mut files: Vec<String> = fields.map(str::to_owned).collect();
            files.sort();
            files.dedup();
            Ok(Merged::Conflict(files))
        }
        _ => Err(Error::Io(cmd::failure(&command, &out))),
    }
}

/// The tree ID of `commit`.
pub(crate) fn tree_of(repo: &Path, commit: &Commit) -> Result<String> {
    Ok(cmd::run(
        repo,
        ["rev-parse", "--verify", &format!("{commit}^{{tree}}")],
    )?
    .trim()
    .to_owned())
}

/// The first parent of `commit`; an error for a commit with none.
pub(crate) fn parent_of(repo: &Path, commit: &Commit) -> Result<Commit> {
    let mut command = cmd::git(repo, ["rev-parse", "--verify", &format!("{commit}^1")]);
    let out = cmd::output(&mut command)?;
    if !out.status.success() {
        return Err(Error::Invalid(format!(
            "commit {commit} has no parent to compare it with"
        )));
    }
    cmd::commit(&String::from_utf8_lossy(&out.stdout))
}

/// The author and message of `commit`, read from its raw object.
pub(crate) fn info(repo: &Path, commit: &Commit) -> Result<CommitInfo> {
    let raw = cmd::run(repo, ["cat-file", "commit", commit.as_str()])?;
    let (headers, message) = raw.split_once("\n\n").unwrap_or((raw.as_str(), ""));
    let author = headers
        .lines()
        .find_map(|line| line.strip_prefix("author "))
        .ok_or_else(|| Error::Invalid(format!("commit {commit} has no author")))?;
    let bad = || {
        Error::Invalid(format!(
            "commit {commit} has an unreadable author: {author}"
        ))
    };
    let (name, rest) = author.split_once(" <").ok_or_else(bad)?;
    let (email, date) = rest.split_once("> ").ok_or_else(bad)?;
    Ok(CommitInfo {
        author_name: name.to_owned(),
        author_email: email.to_owned(),
        author_date: date.to_owned(),
        message: message.to_owned(),
    })
}

/// Writes a commit of `tree` with `parent`, the message fed on standard input so that it is
/// stored byte for byte, and `env` set (author or committer identity). No ref moves.
pub(crate) fn commit_tree(
    repo: &Path,
    tree: &str,
    parent: &Commit,
    message: &str,
    env: &[(&str, &str)],
) -> Result<Commit> {
    let mut command = cmd::git(
        repo,
        ["commit-tree", tree, "-p", parent.as_str(), "-F", "-"],
    );
    command
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| Error::Io(format!("cannot run git: {error}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(message.as_bytes())
            .map_err(|error| Error::Io(format!("git commit-tree: {error}")))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|error| Error::Io(format!("git commit-tree: {error}")))?;
    if !out.status.success() {
        return Err(Error::Io(cmd::failure(&command, &out)));
    }
    cmd::commit(&String::from_utf8_lossy(&out.stdout))
}

/// The newest commit that both `a` and `b` hold; an error when they share no history.
pub(crate) fn merge_base(repo: &Path, a: &Commit, b: &Commit) -> Result<Commit> {
    cmd::commit(&cmd::run(repo, ["merge-base", a.as_str(), b.as_str()])?)
}

/// When `commit` was committed: its committer date, to the second.
pub(crate) fn committed_at(repo: &Path, commit: &Commit) -> Result<Timestamp> {
    let text = cmd::run(repo, ["show", "-s", "--format=%ct", commit.as_str()])?;
    let seconds = text
        .trim()
        .parse::<i64>()
        .map_err(|error| Error::Invalid(format!("committer date of {commit}: {error}")))?;
    Ok(Timestamp::from_millis(seconds * 1000))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_older_than_2_40_is_refused_with_the_version_found() {
        assert_eq!(check_version("git version 2.50.1 (Apple Git-155)"), Ok(()));
        assert_eq!(check_version("git version 2.40.0.windows.1"), Ok(()));
        assert_eq!(check_version("git version 3.0.0"), Ok(()));
        let error = check_version("git version 2.39.5").unwrap_err();
        assert!(error.contains("git 2.40"), "{error}");
        assert!(error.ends_with("found git version 2.39.5"), "{error}");
        assert!(check_version("something else").is_err());
    }
}
