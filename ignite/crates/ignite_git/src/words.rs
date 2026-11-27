//! Reading the lines commits changed, and searching a commit's files for words, so that a tip
//! round can tell which later commits use what a reverted landing added (see the daemon's
//! `tip::dependents`).

use std::collections::{BTreeSet, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Stdio;

use ignite_core::ids::Commit;
use ignite_core::traits::{Error, LineChanges, Result};

use crate::cmd;
use crate::objects;

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
/// A whole word is a run of ASCII letters, digits and underscores, as `git grep -w` reads one.
/// A text file is one with no NUL byte in its first [`BINARY_PROBE`] bytes, as `git grep -I`
/// decides. The parent's tree is listed once and every blob read once through
/// `git cat-file --batch`, and each blob is split into words here. That costs one pass over the
/// tree whatever the number of words. `git grep -F -w -f` tests each pattern against each line
/// separately instead, and with a landing's 834 words over an 81 MB tree one search ran for
/// more than 24 minutes while a tip round waited on it.
pub fn before(repo: &Path, commit: &Commit, words: &[String]) -> Result<BTreeSet<String>> {
    let parent = objects::parent_of(repo, commit)?;
    let mut left: HashSet<&[u8]> = words
        .iter()
        .filter(|w| !w.is_empty())
        .map(String::as_bytes)
        .collect();
    let mut found = BTreeSet::new();
    if left.is_empty() {
        return Ok(found);
    }
    let blobs = blobs_of(repo, &parent)?;
    let mut command = cmd::git(repo, ["cat-file", "--batch"]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| Error::Io(format!("cannot run git: {error}")))?;
    // `cat-file` writes each blob as it reads its name, so the names are written from another
    // thread: written from this one, both pipes could fill and each side wait on the other.
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::Io("git cat-file: no standard input".to_owned()))?;
    let writer = std::thread::spawn(move || {
        let mut names = String::new();
        for blob in blobs {
            names.push_str(&blob);
            names.push('\n');
        }
        // A write error means `cat-file` exited, which the reader reports.
        let _ = stdin.write_all(names.as_bytes());
    });
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Io("git cat-file: no standard output".to_owned()))?;
    let read = read_batch(BufReader::new(stdout), &mut left, &mut found);
    // Stopping early leaves `cat-file` blocked on a full pipe, so it is ended rather than
    // awaited.
    if left.is_empty() {
        let _ = child.kill();
    }
    let out = child
        .wait_with_output()
        .map_err(|error| Error::Io(format!("git cat-file: {error}")))?;
    let _ = writer.join();
    read?;
    if !left.is_empty() && !out.status.success() {
        return Err(Error::Io(cmd::failure(&command, &out)));
    }
    Ok(found)
}

/// How many leading bytes of a blob are looked at for a NUL to call it binary, as git does.
const BINARY_PROBE: usize = 8000;

/// The distinct blob names in `commit`'s tree, at any depth. Submodules are commits, not blobs,
/// and are left out.
fn blobs_of(repo: &Path, commit: &Commit) -> Result<Vec<String>> {
    let listing = cmd::run(repo, ["ls-tree", "-r", "-z", commit.as_str()])?;
    let mut blobs = BTreeSet::new();
    for entry in listing.split('\0') {
        // Each entry is `<mode> SP <type> SP <object> TAB <path>`.
        let Some((meta, _path)) = entry.split_once('\t') else {
            continue;
        };
        let mut fields = meta.split(' ');
        if let (Some(_mode), Some("blob"), Some(object)) =
            (fields.next(), fields.next(), fields.next())
        {
            blobs.insert(object.to_owned());
        }
    }
    Ok(blobs.into_iter().collect())
}

/// Reads `git cat-file --batch` output, moving each word of `left` that some text blob holds
/// into `found`, and stops once `left` is empty.
fn read_batch(
    mut reader: impl BufRead,
    left: &mut HashSet<&[u8]>,
    found: &mut BTreeSet<String>,
) -> Result<()> {
    let mut header = String::new();
    let mut content = Vec::new();
    while !left.is_empty() {
        header.clear();
        let read = reader
            .read_line(&mut header)
            .map_err(|error| Error::Io(format!("git cat-file: {error}")))?;
        if read == 0 {
            return Ok(());
        }
        // Each object is `<object> SP <type> SP <size> LF <content> LF`.
        let size = header
            .trim_end()
            .rsplit(' ')
            .next()
            .and_then(|size| size.parse::<usize>().ok())
            .ok_or_else(|| Error::Io(format!("git cat-file: unexpected header {header:?}")))?;
        content.clear();
        content.resize(size + 1, 0);
        reader
            .read_exact(&mut content)
            .map_err(|error| Error::Io(format!("git cat-file: {error}")))?;
        content.pop();
        let probe = &content[..content.len().min(BINARY_PROBE)];
        if probe.contains(&0) {
            continue;
        }
        for word in content.split(|b| !(b.is_ascii_alphanumeric() || *b == b'_')) {
            if left.remove(word) {
                found.insert(String::from_utf8_lossy(word).into_owned());
            }
        }
    }
    Ok(())
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
