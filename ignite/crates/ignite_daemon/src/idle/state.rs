//! The idle work's state file: what exploration has played, the cache hits sampled for audits,
//! the rebuild counts, the jobs that keep failing and the work a stop interrupted.
//!
//! The file is JSON, rewritten whole through a temporary file and a rename, so a stop leaves
//! either the old state or the new one and never half of each. Each job writes what it is about
//! to do before it acts, which is what lets a job that starts again after a stop give the result
//! it would have given without one.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use ignite_core::ids::{CheckName, Commit, TreeId};
use ignite_core::protocol::hub::{AuditReport, CachedResult, ExplorationFind};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};

use super::RebuildCount;
use super::backoff::Failing;

/// Everything the idle work remembers between runs and restarts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct State {
    /// The exploration checks already played, by family, as [`NewCheck::identity`].
    pub played: BTreeMap<String, BTreeSet<String>>,
    /// Checks a family's `next` command printed that have not finished running, by family.
    /// They are run before `next` is asked for more, because `next` never prints them again.
    pub batches: BTreeMap<String, Vec<NewCheck>>,
    /// Instructions each exploration family spent, by family and then by day (`YYYY-MM-DD`).
    pub spent: BTreeMap<String, BTreeMap<String, f64>>,
    /// The cache hits sampled for audits, by the day they were used.
    pub hits: BTreeMap<String, DayHits>,
    /// Reports the hub has not taken yet, oldest first.
    pub unsent: Vec<Report>,
    /// A rebuild edit that is applied and not yet undone.
    pub journal: Option<Journal>,
    /// Every stored rebuild count, oldest first.
    pub counts: Vec<RebuildCount>,
    /// The verified commit each rebuild edit was last measured at, or tried at if the
    /// measurement failed.
    pub rebuild_tried: BTreeMap<String, Commit>,
    /// Idle jobs whose last run failed, by [`super::IdleJob::key`].
    pub failing: BTreeMap<String, Failing>,
    /// Recovery reports the hub has not accepted, keyed by job and failure streak.
    pub recovered: BTreeSet<(String, Timestamp)>,
}

/// A check an exploration family's `next` command printed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct NewCheck {
    pub check: CheckName,
    /// What the check's inputs hash to, if the command said.
    #[serde(default)]
    pub digest: Option<String>,
    /// Path patterns the check reads beyond its family's own.
    #[serde(default)]
    pub inputs: Vec<String>,
}

impl NewCheck {
    /// The check with its inputs: two lines with the same identity are the same check played
    /// on the same inputs.
    pub fn identity(&self) -> String {
        if let Some(digest) = &self.digest {
            return format!("{} {digest}", self.check);
        }
        let mut inputs = self.inputs.clone();
        inputs.sort();
        format!("{} {}", self.check, inputs.join(" "))
    }
}

/// One day's sampled cache hits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct DayHits {
    /// The hits under the sampling threshold.
    pub sampled: Vec<Hit>,
    /// The hit with the lowest sample value so far today, which `ignite audit --now` reruns
    /// when no hit is under the threshold yet.
    pub lowest: Option<(u64, Hit)>,
}

/// A cache hit and the commit of the tree it answered a check in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Hit {
    pub cached: CachedResult,
    pub commit: Commit,
    pub audited: bool,
}

/// A report for the hub.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Report {
    Audit(AuditReport),
    Find(ExplorationFind),
}

/// A rebuild edit in progress: the file it changed and the contents to write back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Journal {
    pub name: String,
    pub tree: TreeId,
    pub file: PathBuf,
    pub original: String,
}

impl State {
    /// The state saved at `path`, or an empty state if there is none.
    pub fn load(path: &Path) -> Result<State> {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| Error::Invalid(format!("{}: {e}", path.display()))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
            Err(error) => Err(error.into()),
        }
    }

    /// Writes the state to `path` whole, through a temporary file beside it.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string(self).map_err(|e| Error::Io(e.to_string()))?;
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(".new");
        let temporary = PathBuf::from(temporary);
        fs::write(&temporary, text)?;
        fs::rename(&temporary, path)?;
        Ok(())
    }
}
