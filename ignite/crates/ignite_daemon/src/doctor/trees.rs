//! `doctor`'s findings about the build trees: whether the declared trees exist, which
//! directories under the trees folder are not Ignite's, and whether the disk has room for the
//! trees' budget.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use ignite_core::config::Machine;

use super::{DoctorInput, Finding, TreeInfo, configured_tree_names};

pub(super) fn findings(input: &DoctorInput, machine: &Machine, findings: &mut Vec<Finding>) {
    let dir = input.repo.join(&input.config.repository.trees);
    let restart = "restart the daemon, which creates the trees it is missing";
    if !input.host.path_exists(&dir) {
        findings.push(Finding::problem(
            "trees",
            format!("the tree directory {} is missing.", dir.display()),
            restart,
        ));
        return;
    }
    let declared = machine.ticket_trees + machine.landing_trees;
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
            let configured = configured_tree_names(machine.ticket_trees, machine.landing_trees);
            // A directory under the trees folder that ignite.toml does not declare is not one
            // of the machine's trees: it is not counted, but it is named, since Ignite neither
            // uses nor cleans it.
            let foreign: BTreeSet<String> = trees
                .iter()
                .map(|t| t.name.clone())
                .chain(input.trees.directories())
                .filter(|name| !configured.contains(name))
                .collect();
            if !foreign.is_empty() {
                let names: Vec<&str> = foreign.iter().map(String::as_str).collect();
                findings.push(Finding::warning(
                    "trees",
                    format!(
                        "{} holds directories that are not trees in ignite.toml: {}.",
                        dir.display(),
                        names.join(", ")
                    ),
                    "remove them, or move them out of the tree directory if they are wanted \
                     (`git worktree remove` for a worktree)",
                ));
            }
            let trees: Vec<TreeInfo> = trees
                .into_iter()
                .filter(|t| configured.contains(&t.name))
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
            if present < usize::try_from(declared).unwrap_or(usize::MAX) && missing == 0 {
                findings.push(Finding::problem(
                    "trees",
                    format!("{present} of the {declared} trees in ignite.toml exist."),
                    restart,
                ));
            } else if missing == 0 {
                findings.push(Finding::ok("trees", format!("{present} trees present.")));
            }
        }
    }

    let probe = std::iter::successors(Some(dir.as_path()), |p| p.parent())
        .find(|p| input.host.path_exists(p))
        .unwrap_or(&dir);
    findings.push(disk_finding(
        input.host.disk_free_gb(probe),
        input.host.trees_used_gb(&dir),
        machine.trees_disk_gb,
    ));
}

/// The `disk` finding. The trees may use `budget` GB in all, and what they use already counts
/// toward it, so the disk has room when what is free plus what the trees use reaches the budget.
/// Comparing the free space alone with the budget called a disk short that had room to spare.
fn disk_finding(free: Option<u64>, used: Option<u64>, budget: Option<u32>) -> Finding {
    let Some(free) = free else {
        return Finding::problem(
            "disk",
            "cannot read the free disk space.",
            "run `df -k` in the tree directory and check that the disk is mounted",
        );
    };
    let Some(budget) = budget.map(u64::from) else {
        return Finding::ok("disk", format!("{free} GB free; no budget is set."));
    };
    match used {
        Some(used) if free + used < budget => Finding::problem(
            "disk",
            format!(
                "{free} GB free and the trees use {used} GB, so they can grow to {} GB, less \
                 than the {budget} GB they may use.",
                free + used
            ),
            "free disk space, or lower trees_disk_gb for this machine in ignite.toml",
        ),
        Some(used) => Finding::ok(
            "disk",
            format!(
                "{free} GB free and the trees use {used} GB, room for the {budget} GB they may use."
            ),
        ),
        None if free < budget => Finding::warning(
            "disk",
            format!(
                "{free} GB free, less than the {budget} GB the trees may use; what the trees use \
                 already is not measured yet."
            ),
            "run `ignite doctor` again in a few minutes, once the trees have been measured",
        ),
        None => Finding::ok(
            "disk",
            format!("{free} GB free; the trees may use {budget} GB."),
        ),
    }
}

/// How long a measurement of the trees' disk use is used before it is made again.
const USED_FRESH: Duration = Duration::from_hours(1);

/// The last measurement of a trees folder: when it was made, and the gigabytes it found.
static USED: Mutex<Option<(PathBuf, SystemTime, u64)>> = Mutex::new(None);
/// A measurement is running.
static MEASURING: AtomicBool = AtomicBool::new(false);

/// The gigabytes under `dir` from the last measurement, which may be up to [`USED_FRESH`] old.
/// When there is none, or it is older, a measurement starts on a thread of its own (`du` over
/// hundreds of gigabytes takes minutes), and this answers with what it has: an older figure,
/// or `None`.
pub(super) fn used_gb(dir: &Path) -> Option<u64> {
    let last = USED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .filter(|(measured, _, _)| measured == dir)
        .or_else(|| saved(dir));
    let fresh = last
        .as_ref()
        .is_some_and(|(_, at, _)| at.elapsed().is_ok_and(|age| age < USED_FRESH));
    if !fresh && !MEASURING.swap(true, Ordering::SeqCst) {
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            if let Some(gb) = measure_gb(&dir) {
                let at = SystemTime::now();
                save(&dir, at, gb);
                *USED.lock().unwrap_or_else(PoisonError::into_inner) = Some((dir, at, gb));
            }
            MEASURING.store(false, Ordering::SeqCst);
        });
    }
    last.map(|(_, _, gb)| gb)
}

/// Where the last measurement of `dir` is kept, so that it outlives the daemon: the daemon
/// restarts far more often than a measurement (`du` over hundreds of gigabytes) finishes, and
/// a measurement kept only in memory left `doctor` saying "not measured yet" for good.
fn saved_path(dir: &Path) -> PathBuf {
    let name: String = dir
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    std::env::temp_dir().join(format!("ignite-trees-used{name}"))
}

/// The measurement of `dir` a previous daemon saved: seconds since the epoch and gigabytes.
fn saved(dir: &Path) -> Option<(PathBuf, SystemTime, u64)> {
    let text = std::fs::read_to_string(saved_path(dir)).ok()?;
    let (at, gb) = text.trim().split_once(' ')?;
    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(at.parse().ok()?);
    Some((dir.to_path_buf(), at, gb.parse().ok()?))
}

fn save(dir: &Path, at: SystemTime, gb: u64) {
    let secs = at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // A measurement that cannot be saved is only measured again by the next daemon.
    let _ = std::fs::write(saved_path(dir), format!("{secs} {gb}\n"));
}

/// The gigabytes `du` counts under `dir`.
fn measure_gb(dir: &Path) -> Option<u64> {
    let output = Command::new("du").arg("-sk").arg(dir).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let kilobytes: u64 = text.split_whitespace().next()?.parse().ok()?;
    Some(kilobytes / (1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The daemon restarts far more often than `du` finishes over the trees, so a measurement
    /// kept only in memory never reached `doctor`. A saved measurement is read back.
    #[test]
    fn a_measurement_is_read_back_after_a_restart() {
        let dir = std::env::temp_dir().join(format!("ignite-used-test-{}", std::process::id()));
        let at = SystemTime::now();
        save(&dir, at, 287);
        let (_, read_at, gb) = saved(&dir).unwrap();
        assert_eq!(gb, 287);
        assert!(at.duration_since(read_at).unwrap() < Duration::from_secs(1));
        let _ = std::fs::remove_file(saved_path(&dir));
    }

    #[test]
    fn the_disk_has_room_when_free_space_and_the_trees_own_use_reach_the_budget() {
        // jarvis: 277 GB free, the trees use 200 GB, and they may use 400 GB.
        let roomy = disk_finding(Some(277), Some(200), Some(400));
        assert!(roomy.ok && !roomy.warning, "{roomy:?}");
        assert_eq!(
            roomy.text,
            "277 GB free and the trees use 200 GB, room for the 400 GB they may use."
        );
        let short = disk_finding(Some(150), Some(200), Some(400));
        assert!(!short.ok, "{short:?}");
        assert!(
            short
                .text
                .starts_with("150 GB free and the trees use 200 GB, so they can grow to 350 GB"),
            "{short:?}"
        );
        let unmeasured = disk_finding(Some(277), None, Some(400));
        assert!(unmeasured.ok && unmeasured.warning, "{unmeasured:?}");
        assert!(disk_finding(Some(500), None, Some(400)).fix.is_none());
    }
}
