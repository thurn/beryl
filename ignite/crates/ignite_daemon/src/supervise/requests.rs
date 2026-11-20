//! Finding runs by the host protocol's request IDs.
//!
//! A command started for a verb process (see `crate::host`) has the [`RequestId`] of the
//! `host-run` call that started it in its environment, as [`REQUEST_VARIABLE`], and so in its
//! run's `spec.json`. A call repeated after the daemon restarted finds the run by that ID
//! instead of starting the command again, and `ignite stop` finds every run of a handle by the
//! ID's handle.

use std::fs;
use std::path::{Path, PathBuf};

use ignite_core::ids::Handle;
use ignite_core::protocol::host::{REQUEST_VARIABLE, RequestId};

use super::{COLLECTED, Run, SPEC, SpecFile, SupervisedLauncher};

/// The request ID a run's spec names, if a verb process's `host-run` started it.
pub fn request_of(spec: &SpecFile) -> Option<&str> {
    spec.env
        .iter()
        .find(|(name, _)| name == REQUEST_VARIABLE)
        .map(|(_, value)| value.as_str())
}

/// Every run directory under `runs` that a `host-run` call started, finished or not, with the
/// request ID it names. One read of each `spec.json`.
pub fn requests_in(runs: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(runs) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter_map(|dir| {
            let spec: SpecFile = serde_json::from_slice(&fs::read(dir.join(SPEC)).ok()?).ok()?;
            Some((request_of(&spec)?.to_owned(), dir))
        })
        .collect()
}

impl SupervisedLauncher {
    /// The folder that holds the run directories.
    pub fn runs_folder(&self) -> &Path {
        &self.runs
    }

    /// Every run a `host-run` call started, with its request ID (see [`requests_in`]).
    pub fn requests(&self) -> Vec<(String, PathBuf)> {
        requests_in(&self.runs)
    }

    /// The runs of `handle`'s verb process whose results nobody has collected: those still
    /// running, and those that finished since the daemon last looked.
    pub fn unfinished_of(&self, handle: &Handle) -> Vec<Run> {
        unfinished_of_in(&self.runs, handle)
    }
}

/// [`SupervisedLauncher::unfinished_of`] for the run directories under `runs`.
pub fn unfinished_of_in(runs: &Path, handle: &Handle) -> Vec<Run> {
    let mut found: Vec<Run> = requests_in(runs)
        .into_iter()
        .filter(|(request, dir)| {
            RequestId::of_handle(request, handle) && !dir.join(COLLECTED).exists()
        })
        .map(|(_, dir)| Run { dir })
        .collect();
    found.sort_by(|a, b| a.dir.cmp(&b.dir));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(env: Vec<(String, String)>) -> SpecFile {
        SpecFile {
            args: vec!["true".to_owned()],
            cwd: PathBuf::from("/trees/tree-01"),
            env,
            queued_s: 0.0,
            jobserver: None,
            build_jobs: None,
            hang_limit_s: None,
            grant: None,
        }
    }

    #[test]
    fn runs_are_found_by_request_and_by_handle_until_collected() {
        let runs = ignite_core::test_files::temp_dir().join(format!("ignite-requests-{}", std::process::id()));
        let _ = fs::remove_dir_all(&runs);
        let write = |name: &str, env: Vec<(String, String)>| {
            let dir = runs.join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(SPEC), serde_json::to_vec(&spec(env)).unwrap()).unwrap();
            dir
        };
        let variable = |value: &str| vec![(REQUEST_VARIABLE.to_owned(), value.to_owned())];
        let first = write("1", variable("L-6#1"));
        let second = write("2", variable("L-6#2"));
        write("3", variable("L-60#1"));
        write("4", Vec::new());
        let mut found = requests_in(&runs);
        found.sort();
        let ids: Vec<&str> = found.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["L-6#1", "L-6#2", "L-60#1"]);

        let handle = Handle::new("L-6").unwrap();
        let open: Vec<PathBuf> = unfinished_of_in(&runs, &handle)
            .into_iter()
            .map(|run| run.dir)
            .collect();
        assert_eq!(open, [first.clone(), second]);
        fs::write(first.join(COLLECTED), "").unwrap();
        assert_eq!(unfinished_of_in(&runs, &handle).len(), 1);
        let _ = fs::remove_dir_all(&runs);
    }
}
