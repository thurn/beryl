//! Builds that a previous daemon started and that still run after a restart.
//!
//! A build command keeps running under its supervisor when the daemon restarts. Every build
//! command's environment carries `IGNITE_BUILD_KEY`, a JSON [`BuildKey`] naming the tree,
//! product, packages and input hash it builds and the output directories it writes, so the run's
//! `spec.json` says which build it is. [`BuildManager::rejoin`] turns each such run that nobody
//! has collected back into a running build: its output directories are busy, a request for the
//! same key joins it instead of starting Cargo, and when its supervisor exits a success is
//! stored as a build this daemon started would be. A failure is not remembered, so the next
//! request builds again: the restart may have ended the build from outside. This covers builds
//! that a background loop started, which have no handle and are never adopted (see
//! `crate::adopt`).

use std::path::PathBuf;
use std::sync::Arc;

use ignite_core::ids::{InputHash, TreeId};
use serde::{Deserialize, Serialize};

use super::flight::FlightGuard;
use super::repair;
use super::{BuildManager, Flight, Plan, Request, UNITS_ENV, lock};
use crate::supervise::{Run, SpecFile};

/// The environment variable that names the build a build command makes, as a JSON
/// [`BuildKey`].
pub const BUILD_KEY_ENV: &str = "IGNITE_BUILD_KEY";

/// Which build a build command makes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct BuildKey {
    pub tree: String,
    pub product: String,
    /// Sorted; empty for the whole workspace.
    pub packages: Vec<String>,
    pub hash: String,
    /// The directories in the tree the build writes.
    pub outputs: Vec<String>,
}

impl BuildKey {
    pub(super) fn of(plan: &Plan) -> Self {
        Self {
            tree: plan.request.tree.to_string(),
            product: plan.request.product.clone(),
            packages: plan.request.packages.clone(),
            hash: plan.hash.to_string(),
            outputs: plan.product.outputs.clone(),
        }
    }

    /// The variable that carries the key in the command's environment.
    pub(super) fn variable(&self) -> (String, String) {
        let json = serde_json::to_string(self).unwrap_or_default();
        (BUILD_KEY_ENV.to_owned(), json)
    }

    fn request(&self) -> Option<(Request, InputHash)> {
        let request = Request {
            tree: TreeId::new(self.tree.as_str()).ok()?,
            product: self.product.clone(),
            packages: self.packages.clone(),
        };
        Some((request, InputHash::new(self.hash.as_str()).ok()?))
    }
}

/// The build key and compile units file of a run, if a build manager started it.
fn build_of(run: &Run) -> Option<(SpecFile, BuildKey, Option<PathBuf>)> {
    let spec: SpecFile =
        serde_json::from_slice(&std::fs::read(run.dir().join("spec.json")).ok()?).ok()?;
    let value = |name: &str| {
        spec.env
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let key: BuildKey = serde_json::from_str(&value(BUILD_KEY_ENV)?).ok()?;
    let units = value(UNITS_ENV).map(PathBuf::from);
    Some((spec, key, units))
}

impl BuildManager {
    /// Makes each build run in `runs` (the unfinished runs a previous daemon left) a running
    /// build again, and returns how many it made. Runs inside `reset_trees` are skipped: the
    /// restarted daemon ends them, since their tree is reset under them.
    pub fn rejoin(self: &Arc<Self>, runs: Vec<Run>, reset_trees: &[PathBuf]) -> usize {
        let mut rejoined = 0;
        for run in runs {
            let Some((spec, key, units)) = build_of(&run) else {
                continue;
            };
            if reset_trees.iter().any(|tree| spec.cwd.starts_with(tree)) {
                continue;
            }
            let Some((request, hash)) = key.request() else {
                continue;
            };
            let owner = format!(
                "supervised run {}",
                run.dir().file_name().unwrap_or_default().to_string_lossy()
            );
            let flight = Arc::new(Flight::new(key.outputs, Some(owner)));
            {
                let mut state = lock(&self.state);
                let flight_key = (request.clone(), hash.clone());
                if state.flights.contains_key(&flight_key) {
                    continue;
                }
                state.flights.insert(flight_key, Arc::clone(&flight));
            }
            let manager = Arc::clone(self);
            let units = units.unwrap_or_default();
            let thread = format!("ignite-build-{}", request.product);
            let held = (request.clone(), hash.clone(), Arc::clone(&flight));
            let spawned = std::thread::Builder::new().name(thread).spawn(move || {
                // The guard frees the directories and wakes the joiners however this ends.
                let tree = request.tree.clone();
                let guard = FlightGuard::new(&manager, request, hash, flight);
                let ended = run.wait();
                if let Some(trees) = manager
                    .tree_control
                    .get()
                    .and_then(crate::trees::TreeBuildControl::trees)
                {
                    trees.measure_build(
                        &tree,
                        (ended.exit == Some(0)).then(|| ended.output.clone()),
                    );
                }
                let ended_partway = repair::ended_partway(&ended);
                if ended_partway && let Some(path) = manager.trees.path(&tree) {
                    repair::mark_ended(&spec, &path);
                }
                // A rejoined build's failure is never remembered. The restart that left it
                // running may also have ended it: the restarted daemon frees landing trees whose
                // owner will not run again by sending a signal to every process inside them,
                // which leaves no `ended` mark in the run, and a wrapper can turn that signal
                // into an ordinary exit code. Remembering it would answer every later request
                // for the same inputs with a failure that says nothing about them; a genuine
                // failure costs only one more build.
                let (result, _) = BuildManager::outcome(ended, &units, ended_partway);
                guard.finish(&result, false);
            });
            match spawned {
                Ok(_) => rejoined += 1,
                Err(error) => {
                    eprintln!("ignite: cannot wait for a build that still runs: {error}");
                    // Dropping an unfinished guard frees the directories and ends the flight, so
                    // no request waits for a build nobody watches.
                    let (request, hash, flight) = held;
                    drop(FlightGuard::new(self, request, hash, flight));
                }
            }
        }
        rejoined
    }
}
