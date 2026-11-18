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

/// Every file git tracks in the checkout at `root`, relative to it with `/` separators.
pub fn files(root: &Path) -> Result<Vec<String>> {
    let text = git(root, &["ls-files", "-z"])?;
    Ok(text
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect())
}
