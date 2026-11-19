use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

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

/// A repository in a fresh temporary directory, removed when dropped.
struct Repo(PathBuf);

impl Repo {
    fn new(name: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!("ignite-holds-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "--quiet", "-b", "main"]);
        let repo = Repo(dir);
        repo.commit("src/a.rs", "one", "base");
        repo
    }

    /// Writes `text` to `file` and commits it on the branch checked out.
    fn commit(&self, file: &str, text: &str, subject: &str) -> Commit {
        let path = self.0.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        git(&self.0, &["add", "-A"]);
        git(&self.0, &["commit", "--quiet", "-m", subject]);
        self.head()
    }

    fn head(&self) -> Commit {
        Commit::new(git(&self.0, &["rev-parse", "HEAD"])).unwrap()
    }

    fn switch(&self, branch: &str, at: &Commit) {
        git(&self.0, &["switch", "--quiet", "-C", branch, at.as_str()]);
    }

    /// Copies `commit` onto the branch checked out, as a rebase does.
    fn pick(&self, commit: &Commit) -> Commit {
        git(&self.0, &["cherry-pick", "--quiet", commit.as_str()]);
        self.head()
    }

    fn holds(&self, history: &Commit, version: &Commit) -> bool {
        holds_changes(&self.0, history, version, "src").unwrap()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_commit_in_the_history_is_held_and_a_later_one_is_not() {
    let repo = Repo::new("ancestor");
    let base = repo.head();
    let fix = repo.commit("src/a.rs", "two", "fix");
    assert!(repo.holds(&fix, &base));
    assert!(!repo.holds(&base, &fix));
}

#[test]
fn a_rebased_copy_holds_the_commit_it_copies() {
    // The fix was pushed to the release branch by hand from a branch off `base`; meanwhile an
    // unrelated commit landed, and the landing rebased the fix onto it.
    let repo = Repo::new("copy");
    let base = repo.head();
    repo.switch("hand", &base);
    let fix = repo.commit("src/a.rs", "two", "fix");
    repo.switch("main", &base);
    repo.commit("docs/readme.md", "words", "docs");
    let copy = repo.pick(&fix);
    assert!(repo.holds(&copy, &fix));
}

#[test]
fn a_copy_rebased_over_other_changes_to_the_directory_still_holds_the_commit() {
    // Another change to `src` landed before the copy, so no commit has the fix's tree.
    let repo = Repo::new("cherry");
    let base = repo.head();
    repo.switch("hand", &base);
    let fix = repo.commit("src/a.rs", "two", "fix");
    repo.switch("main", &base);
    repo.commit("src/b.rs", "other", "other fix");
    let copy = repo.pick(&fix);
    assert!(repo.holds(&copy, &fix));
}

#[test]
fn a_branch_without_the_change_does_not_hold_it() {
    // A daemon installed by hand from a commit the release never got stays on it.
    let repo = Repo::new("lacking");
    let base = repo.head();
    repo.switch("hand", &base);
    let ahead = repo.commit("src/a.rs", "newer", "unreleased");
    repo.switch("main", &base);
    let release = repo.commit("src/b.rs", "other", "released");
    assert!(!repo.holds(&release, &ahead));
}

#[test]
fn a_commit_the_repository_lacks_is_not_held() {
    let repo = Repo::new("unknown");
    let unknown = Commit::new("1234567890abcdef1234567890abcdef12345678").unwrap();
    assert!(!repo.holds(&repo.head(), &unknown));
}
