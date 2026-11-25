//! Reading the lines commits changed, and searching a commit's files for words, so that a tip
//! round can tell which later commits use what a reverted landing added (see the daemon's
//! `tip::dependents`).

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use ignite_core::ids::Commit;
use ignite_core::traits::{Error, LineChanges, Result};

use crate::cmd;
use crate::objects;

/// The most searches [`before`] runs. Each search finds at least one more word on each file's
/// first matching line, so a handful settles what a landing's few hundred words need.
const MAX_SEARCHES: usize = 16;

/// The lines `commits` (oldest first, consecutive on the first-parent line) added and removed
/// together, from the first commit's parent to the last commit. Binary files show no lines.
pub fn line_changes(repo: &Path, commits: &[Commit]) -> Result<LineChanges> {
    let (Some(first), Some(last)) = (commits.first(), commits.last()) else {
        return Ok(LineChanges::default());
    };
    let parent = objects::parent_of(repo, first)?;
    let diff = cmd::run(
        repo,
        [
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--unified=0",
            parent.as_str(),
            last.as_str(),
        ],
    )?;
    Ok(parse_diff(&diff))
}

/// The added and removed lines of a unified diff. A line counts only inside a hunk, so the
/// `+++ b/path` and `--- a/path` headers of each file are never taken for content.
fn parse_diff(diff: &str) -> LineChanges {
    let mut changes = LineChanges::default();
    let mut in_hunk = false;
    for line in diff.lines() {
        if line.starts_with("diff ") {
            in_hunk = false;
            continue;
        }
        if line.starts_with("@@") {
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        if let Some(added) = line.strip_prefix('+') {
            changes.added.push(added.to_owned());
        } else if let Some(removed) = line.strip_prefix('-') {
            changes.removed.push(removed.to_owned());
        }
    }
    changes
}

/// Which of `words` occur as whole words in some text file of `commit`'s first parent.
///
/// `git grep -o` prints every match, and a common word matches in nearly every file, so each
/// search stops at the first matching line of each file (`--max-count=1`), and `-h` leaves out
/// the file names. A word found is not searched for again; the search repeats until one finds
/// no new word. A word still unfound after [`MAX_SEARCHES`] searches is counted as absent.
pub fn before(repo: &Path, commit: &Commit, words: &[String]) -> Result<BTreeSet<String>> {
    let parent = objects::parent_of(repo, commit)?;
    let mut found = BTreeSet::new();
    let mut left: Vec<&String> = words.iter().filter(|w| !w.is_empty()).collect();
    for _ in 0..MAX_SEARCHES {
        if left.is_empty() {
            break;
        }
        let matched = search(repo, &parent, &left)?;
        let new: BTreeSet<String> = matched
            .into_iter()
            .filter(|word| left.contains(&word))
            .collect();
        if new.is_empty() {
            break;
        }
        left.retain(|word| !new.contains(*word));
        found.extend(new);
    }
    Ok(found)
}

/// One `git grep` for `words` as fixed whole words in `at`, printing the words matched on the
/// first matching line of each text file.
fn search(repo: &Path, at: &Commit, words: &[&String]) -> Result<BTreeSet<String>> {
    let mut command = cmd::git(
        repo,
        [
            "grep",
            "-I",
            "--no-color",
            "-h",
            "--only-matching",
            "--word-regexp",
            "--fixed-strings",
            "--max-count=1",
            "-f",
            "-",
            at.as_str(),
        ],
    );
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| Error::Io(format!("cannot run git: {error}")))?;
    let mut patterns = String::new();
    for word in words {
        patterns.push_str(word);
        patterns.push('\n');
    }
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(patterns.as_bytes())
            .map_err(|error| Error::Io(format!("git grep: {error}")))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|error| Error::Io(format!("git grep: {error}")))?;
    // `git grep` exits with 1 when nothing matched.
    match out.status.code() {
        Some(0) => Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect()),
        Some(1) => Ok(BTreeSet::new()),
        _ => Err(Error::Io(cmd::failure(&command, &out))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lines_inside_hunks_count() {
        let diff = "diff --git a/x.rs b/x.rs\n\
                    index 1..2 100644\n\
                    --- a/x.rs\n\
                    +++ b/x.rs\n\
                    @@ -1 +1,2 @@\n\
                    -old\n\
                    +new\n\
                    ++plus\n\
                    diff --git a/y.bin b/y.bin\n\
                    Binary files a/y.bin and b/y.bin differ\n";
        assert_eq!(
            parse_diff(diff),
            LineChanges {
                added: vec!["new".to_owned(), "+plus".to_owned()],
                removed: vec!["old".to_owned()],
            }
        );
    }
}
