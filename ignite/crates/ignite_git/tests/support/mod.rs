//! Temporary repositories for the git layer's tests.
//!
//! Each test file compiles this module on its own and uses only part of it.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ignite_core::ids::Commit;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A directory under the system temp directory, deleted when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        let path = ignite_core::test_files::temp_dir().join(format!(
            "ignite-git-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        // The temp directory can sit behind a symlink (/var on macOS); git reports real paths.
        Scratch(ignite_os::paths::canonicalize(&path).unwrap())
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Runs git in `dir` with a fixed identity and no user configuration, returning stdout.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

pub fn write(dir: &Path, relative: &str, text: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

pub fn commit_all(dir: &Path, message: &str) -> Commit {
    git(dir, &["add", "--all"]);
    git(dir, &["commit", "--quiet", "--allow-empty", "-m", message]);
    head(dir)
}

pub fn head(dir: &Path) -> Commit {
    Commit::new(git(dir, &["rev-parse", "HEAD"]).trim()).unwrap()
}

/// A repository whose identity is in its own configuration, so the library's git calls (which
/// read the user's configuration) can commit during a rebase.
pub fn init(dir: &Path) {
    git(dir, &["init", "--quiet", "-b", "main"]);
    configure(dir);
}

pub fn configure(dir: &Path) {
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
    git(dir, &["config", "core.hooksPath", "/dev/null"]);
}

/// A bare remote with one commit on `main`, and a clone of it.
pub fn remote_and_clone(scratch: &Scratch) -> (PathBuf, PathBuf) {
    let remote = scratch.path("remote.git");
    let seed = scratch.path("seed");
    let clone = scratch.path("clone");
    fs::create_dir_all(&remote).unwrap();
    fs::create_dir_all(&seed).unwrap();
    git(&remote, &["init", "--quiet", "--bare", "-b", "main"]);
    init(&seed);
    write(&seed, "a.txt", "one\n");
    commit_all(&seed, "first");
    git(
        &seed,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&seed, &["push", "--quiet", "origin", "main"]);
    git(
        &scratch.0,
        &["clone", "--quiet", remote.to_str().unwrap(), "clone"],
    );
    configure(&clone);
    (remote, clone)
}
