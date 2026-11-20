//! Committing what a tool changed in a tree's tracked files, such as a formatter at landing.

use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

use crate::cmd;

/// Commits every change to a tracked file in `tree` with `message` on its branch, and returns
/// the new commit; `None` when no tracked file changed. Untracked files are left out: a tool
/// that rewrites files never adds new ones that belong in the commit. Hooks do not run, because
/// the commit only records what a tool the repository configures has already done.
pub fn tracked_changes(tree: &Path, message: &str) -> Result<Option<Commit>> {
    cmd::run(tree, ["add", "--update", "--", "."])?;
    let mut staged = cmd::git(tree, ["diff", "--cached", "--quiet"]);
    if cmd::output(&mut staged)?.status.success() {
        return Ok(None);
    }
    cmd::run(tree, ["commit", "--quiet", "--no-verify", "-m", message])?;
    cmd::commit(&cmd::run(tree, ["rev-parse", "--verify", "HEAD^{commit}"])?).map(Some)
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
    fn only_changed_tracked_files_are_committed() {
        let repo = ignite_core::test_files::temp_dir()
            .join(format!("ignite-git-commit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&repo);
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "--quiet", "-b", "main"]);
        git(&repo, &["config", "user.name", "t"]);
        git(&repo, &["config", "user.email", "t@example.com"]);
        fs::write(repo.join("a.txt"), "one").unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "--quiet", "-m", "one"]);

        assert_eq!(tracked_changes(&repo, "style: format").unwrap(), None);

        fs::write(repo.join("a.txt"), "two").unwrap();
        fs::write(repo.join("new.txt"), "untracked").unwrap();
        let commit = tracked_changes(&repo, "style: format").unwrap().unwrap();
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), commit.as_str());
        assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), "style: format");
        assert_eq!(git(&repo, &["show", "--name-only", "--format="]), "a.txt");
        assert_eq!(git(&repo, &["status", "--porcelain"]), "?? new.txt");
        let _ = fs::remove_dir_all(&repo);
    }
}
