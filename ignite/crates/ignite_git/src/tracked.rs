//! The files git tracks and the checkout's root, which `ignite manifest lint` checks the
//! manifests' input patterns against.

use std::path::{Path, PathBuf};
use std::process::Command;

use ignite_core::traits::{Error, Result};

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    if !output.status.success() {
        return Err(Error::Io(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout).map_err(|e| Error::Io(e.to_string()))
}

/// The root of the checkout that holds `dir`.
pub fn toplevel(dir: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(
        git(dir, &["rev-parse", "--show-toplevel"])?.trim_end(),
    ))
}

/// The git directory of the checkout that holds `dir`: `<repository>/.git/worktrees/<name>` for
/// a linked worktree. Build scripts that watch the checked-out commit name files in it.
pub fn git_dir(dir: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(
        git(dir, &["rev-parse", "--absolute-git-dir"])?.trim_end(),
    ))
}

/// Every file git tracks in the checkout at `root`, and every untracked file it does not ignore,
/// relative to it with `/` separators.
///
/// Untracked files are included because input hashes are taken over the files `git add --all`
/// would stage (see [`crate::treehash`]). An input glob resolved over the index alone would miss
/// a new file an agent has not added yet, so a check that reads it would keep its old hash.
pub fn files(root: &Path) -> Result<Vec<String>> {
    let text = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--deduplicate",
        ],
    )?;
    let mut files: Vec<String> = text
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect();
    // git lists untracked files after the index's, so sort them in.
    files.sort();
    Ok(files)
}
