//! `doctor`'s findings about the build trees: whether the declared trees exist, which
//! directories under the trees folder are not Ignite's, and the free disk space.

use std::collections::BTreeSet;

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
    findings.push(
        match (input.host.disk_free_gb(probe), machine.trees_disk_gb) {
            (Some(free), Some(budget)) if free < u64::from(budget) => Finding::problem(
                "disk",
                format!("{free} GB free, less than the {budget} GB the trees may use."),
                "free disk space, or lower trees_disk_gb for this machine in ignite.toml",
            ),
            (Some(free), Some(budget)) => Finding::ok(
                "disk",
                format!("{free} GB free; the trees may use {budget} GB."),
            ),
            (Some(free), None) => Finding::ok("disk", format!("{free} GB free; no budget is set.")),
            (None, _) => Finding::problem(
                "disk",
                "cannot read the free disk space.",
                "run `df -k` in the tree directory and check that the disk is mounted",
            ),
        },
    );
}
