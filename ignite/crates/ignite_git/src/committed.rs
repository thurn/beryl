//! The tree hash of a working tree that has nothing uncommitted.

use std::path::Path;

use ignite_core::traits::Result;

use crate::cmd;

/// The ID of the tree that `HEAD` commits, when the working copy matches it exactly (no
/// modified, staged or untracked file that git does not ignore); `None` for a dirty working
/// copy. Two commits with the same content have the same tree ID, whatever their parents.
pub fn clean_tree_id(tree: &Path) -> Result<Option<String>> {
    // `--no-optional-locks` stops status from refreshing and rewriting the real index.
    let status = cmd::run(
        tree,
        [
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=all",
        ],
    )?;
    if !status.trim().is_empty() {
        return Ok(None);
    }
    cmd::run(tree, ["rev-parse", "--verify", "HEAD^{tree}"]).map(|id| Some(id.trim().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
    }

    #[test]
    fn a_dirty_working_copy_has_no_clean_tree_id() {
        let dir = std::env::temp_dir().join(format!("ignite-clean-tree-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "--quiet", "-b", "main"]);
        fs::write(dir.join("a.txt"), "one").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "--quiet", "-m", "one"]);
        let first = clean_tree_id(&dir).unwrap().expect("clean");
        // An empty commit has a new commit ID but the same tree.
        git(&dir, &["commit", "--quiet", "--allow-empty", "-m", "again"]);
        assert_eq!(clean_tree_id(&dir).unwrap(), Some(first));
        fs::write(dir.join("a.txt"), "two").unwrap();
        assert_eq!(clean_tree_id(&dir).unwrap(), None);
        fs::write(dir.join("a.txt"), "one").unwrap();
        fs::write(dir.join("new.txt"), "x").unwrap();
        assert_eq!(clean_tree_id(&dir).unwrap(), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
