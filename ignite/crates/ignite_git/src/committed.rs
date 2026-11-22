//! The tree ID of what a working tree holds, committed or not, and a commit that holds it.

use std::path::{Path, PathBuf};

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

use crate::{cmd, treehash};

/// The name and email on commits that Ignite writes itself.
pub(crate) const IGNITE_NAME: &str = "ignite";
pub(crate) const IGNITE_EMAIL: &str = "ignite@localhost";

/// Reads regular files with an extension from one directory of a commit. The working copy
/// may contain edits or be updating to another commit.
pub fn text_files(
    repo: &Path,
    commit: &Commit,
    dir: &Path,
    extension: &str,
) -> Result<Vec<(PathBuf, String)>> {
    let dir_name = dir.to_string_lossy().replace('\\', "/");
    let tree = if dir_name.is_empty() || dir_name == "." {
        commit.to_string()
    } else {
        format!("{commit}:{dir_name}")
    };
    let entries = cmd::run(repo, ["ls-tree", "-z", &tree])?;
    let mut files = Vec::new();
    for entry in entries.split_terminator('\0') {
        let Some((object, filename)) = entry.split_once('\t') else {
            continue;
        };
        let path = dir.join(filename);
        let Some(object) = object
            .strip_prefix("100644 blob ")
            .or_else(|| object.strip_prefix("100755 blob "))
        else {
            continue;
        };
        if path.extension().is_some_and(|value| value == extension) {
            files.push((path, cmd::run(repo, ["cat-file", "blob", object])?));
        }
    }
    Ok(files)
}

/// What a working tree holds: the ID of the tree a commit of all of it would have, and a commit
/// with that tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// The tree ID. Two commits with the same content have the same tree ID, whatever their
    /// parents, so a later commit of exactly these files has this tree ID.
    pub tree_id: String,
    /// `HEAD` when the working copy matches it; otherwise a commit of the working copy whose
    /// parent is `HEAD` and that no branch holds, so that git can compare it with later commits.
    pub commit: Commit,
}

/// The [`Snapshot`] of `tree`. Modified, staged and untracked files count; files git ignores do
/// not, as they would not be in a commit. Neither the real index nor any branch is touched.
pub fn snapshot(tree: &Path) -> Result<Snapshot> {
    let head = cmd::commit(&cmd::run(tree, ["rev-parse", "--verify", "HEAD"])?)?;
    let head_tree = cmd::run(tree, ["rev-parse", "--verify", "HEAD^{tree}"])?;
    let head_tree = head_tree.trim();
    let tree_id = treehash::write_working_tree(tree)?.as_str().to_owned();
    if tree_id == head_tree {
        return Ok(Snapshot {
            tree_id,
            commit: head,
        });
    }
    // A fixed author and date make the same content on the same `HEAD` the same commit.
    let mut commit = cmd::git(
        tree,
        [
            "commit-tree",
            tree_id.as_str(),
            "-p",
            head.as_str(),
            "-m",
            "ignite: uncommitted edits",
        ],
    );
    for (name, value) in [
        ("GIT_AUTHOR_NAME", IGNITE_NAME),
        ("GIT_AUTHOR_EMAIL", IGNITE_EMAIL),
        ("GIT_AUTHOR_DATE", "1970-01-01T00:00:00Z"),
        ("GIT_COMMITTER_NAME", IGNITE_NAME),
        ("GIT_COMMITTER_EMAIL", IGNITE_EMAIL),
        ("GIT_COMMITTER_DATE", "1970-01-01T00:00:00Z"),
    ] {
        commit.env(name, value);
    }
    let commit = cmd::commit(&cmd::checked(&mut commit)?)?;
    Ok(Snapshot { tree_id, commit })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn text_files_are_read_from_the_commit_and_only_at_the_requested_depth() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-committed-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("checks/nested")).unwrap();
        git(&dir, &["init", "--quiet", "-b", "main"]);
        fs::write(dir.join("checks/family.toml"), "reviewed").unwrap();
        fs::write(dir.join("checks/nested/other.toml"), "nested").unwrap();
        fs::write(dir.join("checks/notes.txt"), "notes").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "--quiet", "-m", "manifests"]);
        let commit = git(&dir, &["rev-parse", "HEAD"]).parse().unwrap();
        fs::write(dir.join("checks/family.toml"), "local edit").unwrap();

        assert_eq!(
            text_files(&dir, &commit, Path::new("checks"), "toml").unwrap(),
            [(PathBuf::from("checks/family.toml"), "reviewed".to_owned())]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dirty_working_copy_has_the_tree_id_its_commit_will_have() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-snapshot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "--quiet", "-b", "main"]);
        fs::write(dir.join(".gitignore"), "target/\n").unwrap();
        fs::write(dir.join("a.txt"), "one").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "--quiet", "-m", "one"]);
        let clean = snapshot(&dir).unwrap();
        assert_eq!(clean.commit.as_str(), git(&dir, &["rev-parse", "HEAD"]));
        // An empty commit has a new commit ID but the same tree.
        git(&dir, &["commit", "--quiet", "--allow-empty", "-m", "again"]);
        assert_eq!(snapshot(&dir).unwrap().tree_id, clean.tree_id);

        fs::write(dir.join("a.txt"), "two").unwrap();
        fs::write(dir.join("new.txt"), "x").unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::write(dir.join("target/ignored.txt"), "x").unwrap();
        let status = git(&dir, &["status", "--porcelain"]);
        let index = fs::read(dir.join(".git/index")).unwrap();
        let dirty = snapshot(&dir).unwrap();
        assert_ne!(dirty.tree_id, clean.tree_id);
        assert_eq!(
            snapshot(&dir).unwrap(),
            dirty,
            "the same content, the same commit"
        );
        assert_eq!(
            git(&dir, &["rev-parse", &format!("{}^", dirty.commit)]),
            git(&dir, &["rev-parse", "HEAD"])
        );
        // The real index, the status and the branch are untouched.
        assert_eq!(fs::read(dir.join(".git/index")).unwrap(), index);
        assert_eq!(git(&dir, &["status", "--porcelain"]), status);

        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "--quiet", "-m", "two"]);
        assert_eq!(git(&dir, &["rev-parse", "HEAD^{tree}"]), dirty.tree_id);
        let _ = fs::remove_dir_all(&dir);
    }
}
