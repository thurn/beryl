//! The commit a directory's checkout has at `HEAD`, read from the repository's files without
//! starting a `git` process, since it is read for every command a verb with a handle starts.
//!
//! It understands what Ignite's trees use: a `.git` directory or a `.git` file that points at a
//! worktree's directory, whose `commondir` names the shared repository; a `HEAD` that is a
//! commit or names a branch; and branches stored as loose files or in `packed-refs`. Anything
//! else, such as the reftable format, reads as `None`.

use std::fs;
use std::path::{Path, PathBuf};

use ignite_core::ids::Commit;

/// How many symbolic refs are followed before giving up, as `git` does.
const MAX_DEPTH: usize = 5;

/// The commit at `HEAD` of the checkout that holds `dir`, which may be any directory inside it;
/// `None` outside every checkout or when the files cannot be read.
pub fn commit_at(dir: &Path) -> Option<Commit> {
    let git_dir = git_dir(dir)?;
    let common = match fs::read_to_string(git_dir.join("commondir")) {
        Ok(text) => git_dir.join(text.trim()),
        Err(_) => git_dir.clone(),
    };
    let mut value = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    for _ in 0..MAX_DEPTH {
        let Some(name) = value.trim().strip_prefix("ref:").map(str::trim) else {
            return Commit::new(value.trim()).ok();
        };
        value = read_ref(&git_dir, &common, name)?;
    }
    None
}

/// The repository directory of the checkout holding `dir`: `.git` itself, or the directory a
/// `.git` file names.
fn git_dir(dir: &Path) -> Option<PathBuf> {
    for ancestor in dir.ancestors() {
        let dot_git = ancestor.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        if let Ok(text) = fs::read_to_string(&dot_git) {
            let named = text.trim().strip_prefix("gitdir:")?.trim();
            return Some(ancestor.join(named));
        }
    }
    None
}

/// What the ref `name` holds: a loose file in the worktree's own directory or the shared one,
/// or its line in the shared `packed-refs`.
fn read_ref(git_dir: &Path, common: &Path, name: &str) -> Option<String> {
    for dir in [common, git_dir] {
        if let Ok(text) = fs::read_to_string(dir.join(name)) {
            return Some(text);
        }
    }
    let packed = fs::read_to_string(common.join("packed-refs")).ok()?;
    packed
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with('^'))
        .find_map(|line| {
            let (commit, packed_name) = line.split_once(' ')?;
            (packed_name.trim() == name).then(|| commit.to_owned())
        })
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn commit(repo: &Path, n: u32) -> Commit {
        fs::write(repo.join("a.txt"), format!("{n}\n")).unwrap();
        git(repo, &["add", "a.txt"]);
        git(
            repo,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                &format!("commit {n}"),
            ],
        );
        Commit::new(git(repo, &["rev-parse", "HEAD"])).unwrap()
    }

    /// Every form of `HEAD` a tree has reads as the commit `git rev-parse HEAD` names.
    #[test]
    fn head_is_read_from_the_files_as_git_reads_it() {
        let root = std::env::temp_dir().join(format!("ignite-head-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let repo = root.join("repo");
        fs::create_dir_all(repo.join("sub/dir")).unwrap();
        git(&repo, &["init", "--quiet", "-b", "main"]);
        let first = commit(&repo, 1);
        // A branch in a loose file, read from a subdirectory.
        assert_eq!(commit_at(&repo.join("sub/dir")), Some(first.clone()));

        // A worktree on a branch of its own, through its `.git` file and `commondir`.
        let tree = root.join("tree");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "ignite-tree/landing-1",
                tree.to_str().unwrap(),
            ],
        );
        let second = commit(&tree, 2);
        assert_eq!(commit_at(&tree), Some(second.clone()));
        assert_eq!(commit_at(&repo), Some(first.clone()));

        // Branches moved into `packed-refs`.
        git(&repo, &["pack-refs", "--all"]);
        assert_eq!(commit_at(&tree), Some(second));
        assert_eq!(commit_at(&repo), Some(first.clone()));

        // A detached `HEAD`.
        git(&tree, &["checkout", "--quiet", "--detach", first.as_str()]);
        assert_eq!(commit_at(&tree), Some(first));

        assert_eq!(commit_at(&root), None);
        fs::remove_dir_all(&root).unwrap();
    }
}
