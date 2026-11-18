//! What the check runner reads besides the traits in [`crate::server::Context`]: facts about a
//! build tree ([`TreeSource`]) and each check's record at the hub ([`CheckHistory`]), with their
//! real implementations.

use std::path::{Path, PathBuf};
use std::process::Command;

use ignite_core::ids::{Commit, TreeId};
use ignite_core::inputs::CrateGraph;
use ignite_core::protocol::hub::CheckStatsReply;
use ignite_core::traits::{Error, Result};

use crate::build::{DirectoryTrees, TreeFacts};

/// What the check runner needs to know about build trees.
pub trait TreeSource: Send + Sync {
    /// The tree whose working directory holds `cwd`, if any does.
    fn tree_of(&self, cwd: &Path) -> Option<TreeId>;
    /// The tree's working directory.
    fn path(&self, tree: &TreeId) -> Option<PathBuf>;
    /// Every file git tracks in the tree, relative to it with `/` separators.
    fn tracked(&self, path: &Path) -> Result<Vec<String>>;
    /// The workspace packages of every Cargo workspace in the tree.
    fn crate_graph(&self, path: &Path, tracked: &[String]) -> Result<CrateGraph>;
    /// The commit the tree's branch is at.
    fn head(&self, path: &Path) -> Result<Commit>;
    /// The paths the tree changes since its branch left `base` (such as `origin/staging`),
    /// uncommitted edits and new files included.
    fn changed(&self, path: &Path, base: &str) -> Result<Vec<String>>;
}

impl TreeSource for DirectoryTrees {
    fn tree_of(&self, cwd: &Path) -> Option<TreeId> {
        let rest = cwd.strip_prefix(&self.root).ok()?;
        let first = rest.components().next()?.as_os_str().to_str()?;
        TreeId::new(first).ok()
    }

    fn path(&self, tree: &TreeId) -> Option<PathBuf> {
        TreeFacts::path(self, tree)
    }

    fn tracked(&self, path: &Path) -> Result<Vec<String>> {
        TreeFacts::tracked(self, path)
    }

    fn crate_graph(&self, path: &Path, tracked: &[String]) -> Result<CrateGraph> {
        TreeFacts::crate_graph(self, path, tracked)
    }

    fn head(&self, path: &Path) -> Result<Commit> {
        ignite_git::refs::head(path)
    }

    fn changed(&self, path: &Path, base: &str) -> Result<Vec<String>> {
        let fork = git(path, &["merge-base", "HEAD", base])?;
        let mut paths = lines(&git(path, &["diff", "--name-only", fork.trim()])?);
        paths.extend(lines(&git(
            path,
            &["ls-files", "--others", "--exclude-standard"],
        )?));
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    if !output.status.success() {
        return Err(Error::Io(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Each check's runs, failures and costs, as the hub keeps them.
pub trait CheckHistory: Send + Sync {
    /// `GET /checks/stats?family=`.
    fn stats(&self, family: &str) -> Result<CheckStatsReply>;
}

/// The hub's record of checks, read over HTTP.
#[derive(Clone, Debug)]
pub struct HubHistory {
    /// `<url>/<project>/api`.
    pub base: String,
    pub token: String,
}

impl CheckHistory for HubHistory {
    fn stats(&self, family: &str) -> Result<CheckStatsReply> {
        let family: String = family
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-_.".contains(&b) {
                    char::from(b).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        let url = format!("{}/checks/stats?family={family}", self.base);
        let mut response = ureq::get(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| Error::Unreachable(e.to_string()))?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::Unreachable(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| Error::Invalid(e.to_string()))
    }
}
