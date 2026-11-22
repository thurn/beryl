//! `doctor`'s findings about the build trees: which trees exist, which
//! directories under the trees folder are not Ignite's, and where build output uses disk.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::capacity::{BYTES_PER_GB, BYTES_PER_GIB, DiskLimitSource};
use ignite_core::ids::Commit;
use ignite_core::traits::Result;
use ignite_os::system::DiskSpace;

use crate::trees::TreeConfig;

use super::{DoctorInput, Finding, Host, StatusSection};

/// One build tree, as the daemon knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeInfo {
    pub name: String,
    pub path: PathBuf,
    pub head: Option<Commit>,
}

/// The build trees that exist.
pub trait TreeSource: Send + Sync {
    fn trees(&self) -> Result<Vec<TreeInfo>>;

    /// The names of the directories directly under the trees folder, hidden ones left out,
    /// whether or not git knows them as trees.
    fn directories(&self) -> Vec<String> {
        Vec::new()
    }

    fn stray_builds(&self) -> Result<Vec<PathBuf>> {
        Ok(Vec::new())
    }

    fn shared_volume(&self) -> bool {
        false
    }
}

/// The trees git has registered under the repository's tree directory. A tree whose directory
/// was deleted is still registered until `git worktree prune`, which is how `doctor` finds it.
pub struct GitTrees {
    pub repo: PathBuf,
    pub dir: PathBuf,
}

impl TreeSource for GitTrees {
    fn trees(&self) -> Result<Vec<TreeInfo>> {
        Ok(ignite_git::worktree::list(&self.repo)?
            .into_iter()
            .filter(|tree| tree.path.starts_with(&self.dir))
            .map(|tree| TreeInfo {
                name: tree
                    .path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                path: tree.path,
                head: tree.head,
            })
            .collect())
    }

    fn stray_builds(&self) -> Result<Vec<PathBuf>> {
        let path_on_disk =
            |path: &Path| ignite_os::paths::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        let main = path_on_disk(&self.repo);
        let root = path_on_disk(&self.dir);
        let worktrees: BTreeSet<_> = ignite_git::worktree::list(&self.repo)?
            .iter()
            .map(|tree| path_on_disk(&tree.path))
            .collect();
        let mut builds = Vec::new();
        for tree in &worktrees {
            if tree != &main && !tree.starts_with(&root) {
                cargo_targets(tree, &root, &worktrees, &mut builds);
            }
        }
        builds.sort();
        Ok(builds)
    }

    #[cfg(target_os = "macos")]
    fn shared_volume(&self) -> bool {
        crate::trees::volumes::mac_volume_is_shared(&self.dir)
    }

    fn directories(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| !name.starts_with('.'))
            .collect()
    }
}

/// Check only registered git worktrees. Symlinks, nested worktrees, and the managed root
/// are not traversed. Once a Cargo target is found there is no need to walk its outputs.
fn cargo_targets(
    tree: &Path,
    managed_root: &Path,
    worktrees: &BTreeSet<PathBuf>,
    builds: &mut Vec<PathBuf>,
) {
    let mut pending = vec![tree.to_owned()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|kind| kind.is_dir())
                || entry.file_name() == ".git"
                || path == managed_root
                || worktrees.contains(&path)
            {
                continue;
            }
            if entry.file_name() == "target" && is_cargo_target(&path) {
                builds.push(path);
            } else {
                pending.push(path);
            }
        }
    }
}

fn is_cargo_target(path: &Path) -> bool {
    path.join(".rustc_info.json").is_file()
        || fs::read_dir(path).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && (entry.path().join(".fingerprint").is_dir()
                        || fs::read_dir(entry.path()).is_ok_and(|profiles| {
                            profiles
                                .flatten()
                                .any(|profile| profile.path().join(".fingerprint").is_dir())
                        }))
            })
        })
}

/// Status reads the current root when requested, including after a fleet edit.
pub struct LiveGitTrees(pub crate::trees::Trees);

impl TreeSource for LiveGitTrees {
    fn trees(&self) -> Result<Vec<TreeInfo>> {
        self.source().trees()
    }

    fn directories(&self) -> Vec<String> {
        self.source().directories()
    }
}

impl LiveGitTrees {
    fn source(&self) -> GitTrees {
        GitTrees {
            repo: self.0.config().repo.clone(),
            dir: self.0.root(),
        }
    }
}

/// The names Ignite uses under the build tree directory.
fn managed_tree(name: &str, landing_trees: u32) -> bool {
    crate::trees::ticket_tree_number(name).is_some()
        || (1..=landing_trees).any(|n| name == format!("landing-{n}"))
}

/// The trees on disk, with missing paths and foreign directories reported separately.
pub struct TreesSection {
    pub source: Arc<dyn TreeSource>,
    pub landing_trees: u32,
    pub host: Arc<dyn Host + Send + Sync>,
}

impl StatusSection for TreesSection {
    fn title(&self) -> &'static str {
        "trees"
    }

    fn lines(&self) -> Vec<String> {
        match self.source.trees() {
            Err(error) => vec![format!("cannot list trees: {error}")],
            Ok(trees) => {
                let known: Vec<&TreeInfo> = trees
                    .iter()
                    .filter(|t| managed_tree(&t.name, self.landing_trees))
                    .collect();
                // Directories git does not know as trees are named too.
                let extra: BTreeSet<String> = trees
                    .iter()
                    .map(|t| t.name.clone())
                    .chain(self.source.directories())
                    .filter(|name| !managed_tree(name, self.landing_trees))
                    .collect();
                let missing: Vec<&TreeInfo> = known
                    .iter()
                    .copied()
                    .filter(|t| !self.host.path_exists(&t.path))
                    .collect();
                let mut lines = vec![present_count(known.len() - missing.len())];
                lines.extend(
                    missing
                        .iter()
                        .map(|t| format!("{}: directory is missing", t.name)),
                );
                if !extra.is_empty() {
                    let names: Vec<&str> = extra.iter().map(String::as_str).collect();
                    lines.push(format!("outside Ignite's tree names: {}", names.join(", ")));
                }
                lines
            }
        }
    }
}

fn present_count(present: usize) -> String {
    let trees = if present == 1 { "tree" } else { "trees" };
    format!("{present} {trees} present")
}

pub(super) fn findings(input: &DoctorInput, landing_trees: u32, findings: &mut Vec<Finding>) {
    let default = input.repo.join(&input.config.repository.trees);
    let dir = input
        .fleet
        .as_ref()
        .map_or_else(|| default.clone(), |notice| notice.trees.root(&default));
    volume_findings(input.trees, &dir, findings);
    let restart = "restart the daemon, which creates the trees it is missing";
    if !input.host.path_exists(&dir) {
        findings.push(Finding::problem(
            "trees",
            format!("the tree directory {} is missing.", dir.display()),
            restart,
        ));
        return;
    }
    match input.trees.trees() {
        Err(error) => findings.push(Finding::problem(
            "trees",
            format!("cannot list the trees: {error}."),
            format!(
                "run `git worktree list` in {} and fix what it reports",
                input.repo.display()
            ),
        )),
        Ok(trees) => {
            // Other directories are reported separately from Ignite's registered trees.
            let foreign: BTreeSet<String> = trees
                .iter()
                .map(|t| t.name.clone())
                .chain(input.trees.directories())
                .filter(|name| !managed_tree(name, landing_trees))
                .collect();
            if !foreign.is_empty() {
                let names: Vec<&str> = foreign.iter().map(String::as_str).collect();
                findings.push(Finding::warning(
                    "trees",
                    format!(
                        "{} holds directories that are outside Ignite's tree names: {}.",
                        dir.display(),
                        names.join(", ")
                    ),
                    "remove them, or move them out of the tree directory if they are wanted \
                     (`git worktree remove` for a worktree)",
                ));
            }
            let trees: Vec<TreeInfo> = trees
                .into_iter()
                .filter(|t| managed_tree(&t.name, landing_trees))
                .collect();
            let mut missing = 0;
            for tree in &trees {
                if !input.host.path_exists(&tree.path) {
                    missing += 1;
                    findings.push(Finding::problem(
                        &tree.name,
                        format!("the tree directory {} is missing.", tree.path.display()),
                        format!(
                            "run `git worktree prune` in {}, then {restart}",
                            input.repo.display()
                        ),
                    ));
                }
            }
            let present = trees.len() - missing;
            if missing == 0 {
                findings.push(Finding::ok("trees", format!("{}.", present_count(present))));
            }
        }
    }

    let probe = std::iter::successors(Some(dir.as_path()), |p| p.parent())
        .find(|p| input.host.path_exists(p))
        .unwrap_or(&dir);
    let disk = input.host.disk_space(probe);
    let cap = input.fleet.as_ref().and_then(|notice| notice.disk_cap_gb);
    findings.push(disk_finding(
        disk,
        input.used_tree_bytes,
        input.leased_tree_bytes,
        cap,
    ));
}

fn volume_findings(trees: &dyn TreeSource, dir: &Path, findings: &mut Vec<Finding>) {
    if trees.shared_volume() {
        findings.push(Finding::warning(
            "trees",
            format!(
                "the volume holding {} also holds other files.",
                dir.display()
            ),
            "run `ignite trees volume --size-gb <n>` to create a dedicated APFS volume",
        ));
    }
    match trees.stray_builds() {
        Ok(builds) => {
            for path in builds {
                findings.push(Finding::warning(
                    "disk",
                    format!("Cargo build outside the managed trees: {}.", path.display()),
                    "remove the build directory when the worktree no longer needs it",
                ));
            }
        }
        Err(error) => findings.push(Finding::warning(
            "disk",
            format!("cannot look for Cargo builds outside the managed trees: {error}."),
            "check `git worktree list`",
        )),
    }
}

fn leased_disk_finding(
    budget: Option<(u64, DiskLimitSource)>,
    leased: Option<u64>,
) -> Option<Finding> {
    let ((budget, source), leased) = budget.zip(leased)?;
    let gb = |bytes: u64| bytes as f64 / BYTES_PER_GB as f64;
    (leased > budget).then(|| {
        Finding::warning(
            "disk",
            format!(
                "leased ticket trees hold {:.1} GB, over the {:.1} GB tree budget (set by the {source}).",
                gb(leased), gb(budget)
            ),
            "wait for leases to end so the disk loop can reclaim their builds",
        )
    })
}

fn disk_finding(
    disk: Option<DiskSpace>,
    used: Option<u64>,
    leased: Option<u64>,
    cap_gb: Option<u32>,
) -> Finding {
    let Some(disk) = disk else {
        return Finding::problem(
            "disk",
            "cannot read the volume size and available space.",
            "check that the volume holding the tree directory is mounted",
        );
    };
    let gb = |bytes: u64| bytes as f64 / BYTES_PER_GB as f64;
    let free = disk.available / BYTES_PER_GB;
    let budget = TreeConfig::disk_budget(Some(disk), used, cap_gb);
    let budget_text = match (used, budget) {
        (Some(_), Some((bytes, source))) => {
            format!("{:.1} GB tree budget (set by the {source})", gb(bytes))
        }
        _ => match cap_gb {
            Some(cap) => format!("tree budget pending a measurement ({cap} GB cap)"),
            None => "tree budget pending a measurement".to_owned(),
        },
    };
    let leased_warning = leased_disk_finding(budget, leased);
    let floor_bytes = crate::idle::FLOOR_GB * BYTES_PER_GIB;
    if disk.available < floor_bytes {
        let leased_text =
            leased_warning.map_or_else(String::new, |finding| format!(" {}", finding.text));
        return Finding::problem(
            "disk",
            format!(
                "{free} GB free, under the {:.1} GB free-space floor; builds may wait for \
                 space. {budget_text}.{leased_text}",
                gb(floor_bytes)
            ),
            "free disk space; the disk loop clears free trees' builds first, then the \
             incremental caches and builds of leased trees that run nothing",
        );
    }
    if let Some(warning) = leased_warning {
        return warning;
    }
    match (used, budget) {
        (Some(used), Some((bytes, _))) => {
            let usage = format!("{free} GB free, {:.1} GB in the trees", gb(used));
            if used > bytes {
                Finding::warning(
                    "disk",
                    format!("{usage}, over the {budget_text}."),
                    "the disk loop clears free trees and idle leased trees; if the trees stay over \
                     the budget, look for a tree whose build directories keep growing",
                )
            } else {
                Finding::ok("disk", format!("{usage}. {budget_text}."))
            }
        }
        _ => Finding::warning(
            "disk",
            format!("{free} GB free, {budget_text}."),
            "run `ignite doctor` again once the build directories have been measured",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(total_gb: u64, free_gb: u64) -> DiskSpace {
        DiskSpace {
            total: total_gb * BYTES_PER_GB,
            available: free_gb * BYTES_PER_GB,
        }
    }

    #[test]
    fn doctor_warns_when_leased_trees_alone_exceed_the_budget() {
        let used = Some(400 * BYTES_PER_GB);
        let disk = Some(disk(1000, 500));
        for leased in [None, Some(250 * BYTES_PER_GB)] {
            assert!(
                !disk_finding(disk, used, leased, Some(250))
                    .text
                    .contains("leased ticket trees")
            );
        }
        let finding = disk_finding(disk, used, Some(300 * BYTES_PER_GB), Some(250));
        assert!(finding.warning);
        assert!(finding.text.contains(
            "leased ticket trees hold 300.0 GB, over the 250.0 GB tree budget (set by the cap)"
        ));
    }

    #[test]
    fn doctor_names_the_term_that_sets_the_budget() {
        let used = Some(200 * BYTES_PER_GB);
        let capped = disk_finding(Some(disk(1000, 277)), used, None, Some(300));
        assert!(capped.ok && !capped.warning, "{capped:?}");
        assert!(
            capped
                .text
                .contains("300.0 GB tree budget (set by the cap)")
        );

        let derived = disk_finding(Some(disk(1000, 150)), used, None, Some(400));
        assert!(derived.ok && !derived.warning, "{derived:?}");
        assert!(
            derived
                .text
                .contains("250.0 GB tree budget (set by the disk)")
        );

        let full = disk_finding(Some(disk(1858, 4)), used, None, None);
        assert!(!full.ok, "{full:?}");
        assert!(full.text.starts_with("4 GB free, under the"));
        assert!(full.text.contains("18.2 GB tree budget (set by the disk)"));

        let unmeasured = disk_finding(Some(disk(1000, 500)), None, None, Some(400));
        assert!(unmeasured.ok && unmeasured.warning, "{unmeasured:?}");
        assert!(
            unmeasured
                .text
                .contains("tree budget pending a measurement (400 GB cap)")
        );

        let over = disk_finding(
            Some(disk(1000, 400)),
            Some(600 * BYTES_PER_GB),
            None,
            Some(250),
        );
        assert!(over.ok && over.warning, "{over:?}");
        assert!(
            over.text
                .contains("over the 250.0 GB tree budget (set by the cap)")
        );
    }
}
