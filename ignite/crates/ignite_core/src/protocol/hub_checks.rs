//! Check results, statistics and state on the hub's wire.

use serde::{Deserialize, Serialize};

use super::{Outcome, ResultCost};
use crate::config::Os;
use crate::ids::{CheckName, Commit, InputHash, MachineId, RecordId, TicketId};
use crate::time::Timestamp;

/// A result cache key. `platform` is `None` for a check with `platforms = "any"`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResultKey {
    pub check: CheckName,
    pub hash: InputHash,
    pub platform: Option<Os>,
}

/// The checks a failed tip still fails and the commits it verified.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipFailure {
    pub checks: Vec<ResultKey>,
    /// Commits in this tip that were outside the verified branch when the round began.
    pub unverified_commits: Vec<Commit>,
}

/// Failures observed while running required checks at one tip candidate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipCheckFailures {
    pub commit: Commit,
    pub checks: Vec<ResultKey>,
}

/// A cached outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedResult {
    pub key: ResultKey,
    pub outcome: Outcome,
    pub machine: MachineId,
    pub record: RecordId,
    pub at: Timestamp,
    /// Where the saved output is, for a failure.
    pub output: Option<String>,
}

/// `POST /results/lookup`; the reply holds one entry per key, in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupRequest {
    pub keys: Vec<ResultKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupReply {
    pub results: Vec<Option<CachedResult>>,
}

/// One result to store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewResult {
    pub key: ResultKey,
    pub outcome: Outcome,
    pub machine: MachineId,
    pub record: RecordId,
    pub output: Option<String>,
    /// What the check cost on this run. The hub adds it to the check's cost history, so scores
    /// follow live runs and not only the imported history.
    #[serde(default)]
    pub cost: ResultCost,
    /// The areas of the change the check ran on (see `score`). The hub counts a stored pass or
    /// failure as one run, and a failure as one failure, in each of them, so a check's failure
    /// rate per area follows live runs. Empty for a run at the tip, which is no one change.
    #[serde(default)]
    pub areas: Vec<String>,
}

/// `POST /results/store`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreRequest {
    pub results: Vec<NewResult>,
    /// The areas of the change the checks ran on (see `score`). Each stored pass or failure is
    /// counted as one run, and a failure as one failure, in each of these areas.
    #[serde(default)]
    pub areas: Vec<String>,
}

/// `POST /flaky`: a check failed, then passed at once on the same inputs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlakyReport {
    pub check: CheckName,
    pub hash: InputHash,
    pub commit: Commit,
    pub machine: MachineId,
    /// The check's family is in the always set, so the check is never switched off.
    #[serde(default)]
    pub always: bool,
    /// Names the command run in which the check failed, the same for every check that failed
    /// in that run; empty when unknown. The hub uses it to tell one fault of the machine, which
    /// makes many checks flake at once, from flaky checks.
    #[serde(default)]
    pub command: String,
    /// The last lines of the failed run's output.
    #[serde(default)]
    pub output: String,
}

/// A saved flake returned by `GET /checks/flaky/evidence` for its filing ticket.
/// Older imports may have a tree hash instead of an input hash, or lack a hash entirely.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlakyEvidence {
    pub check: String,
    pub hash: String,
    pub commit: String,
    pub machine: String,
    pub at: Timestamp,
    pub always: bool,
    pub command: String,
    pub output: String,
}

/// What the files in a check's fix changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixOutcome {
    Product,
    TestOnly,
    None,
}

impl FixOutcome {
    /// An empty file list means no change. A single product path makes the fix a product fix.
    pub fn classify(files: &[String], test_paths: &[String]) -> Self {
        if files.is_empty() {
            Self::None
        } else if files.iter().all(|file| {
            test_paths
                .iter()
                .any(|pattern| crate::inputs::path_matches(pattern, file))
        }) {
            Self::TestOnly
        } else {
            Self::Product
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Product => "product",
            Self::TestOnly => "test-only",
            Self::None => "none",
        }
    }
}

/// The kind of ticket that recorded a check's fix.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixSource {
    Regression,
    Flaky,
}

impl FixSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Regression => "regression",
            Self::Flaky => "flaky",
        }
    }
}

/// One check's outcome when the ticket that held it closed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckFix {
    pub check: CheckName,
    pub ticket: TicketId,
    pub at: Timestamp,
    pub outcome: FixOutcome,
    pub source: FixSource,
    /// The first bad commit when the ticket closed, for a regression.
    pub first_bad: Option<Commit>,
    /// The last commit of the fix that landed, if one was recorded.
    pub fixing_commit: Option<Commit>,
}

/// Whether a check is run and cached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckState {
    On,
    Off,
    /// Runs, but its results are not reused, after an audit disagreed with the cache.
    Uncached,
}

/// Runs and failures of a check after changes to one area.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaStats {
    pub area: String,
    pub runs: u32,
    pub failures: u32,
}

/// One check's record, in `GET /checks/stats?family=`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckStats {
    pub check: CheckName,
    pub family: String,
    pub state: CheckState,
    pub areas: Vec<AreaStats>,
    /// The median of the last 20 costs.
    pub median_cost: Option<f64>,
    /// `instructions` (billions) or `cpu_s`.
    pub unit: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckStatsReply {
    pub family: String,
    /// The family's failure rate over all its runs.
    pub family_rate: f64,
    /// Run counts across all areas, even when the response includes only selected areas.
    /// Absent on hubs that report only the original, unclassified failure rates.
    #[serde(default)]
    pub runs: Option<u64>,
    pub checks: Vec<CheckStats>,
    /// What starting one of the family's commands costs, in billions of instructions and in CPU
    /// seconds, estimated from its recent runs (`score::startup`); `None` while unknown. The
    /// checks' costs leave it out, so the gate charges it once for each family it runs.
    #[serde(default)]
    pub startup_billions: Option<f64>,
    #[serde(default)]
    pub startup_cpu_s: Option<f64>,
}

/// `POST /checks/state`: switches a check on or off by hand (`ignite flaky on|off`). The hub
/// records who did it, and comments on the check's flaky ticket if it has one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckStateChange {
    pub check: CheckName,
    /// `on` or `off`; `uncached` is refused, because only an audit sets it.
    pub state: CheckState,
    /// Why, in the words of whoever switched it.
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckStateChanged {
    pub check: CheckName,
    pub previous: CheckState,
    pub state: CheckState,
    /// The flaky ticket the change was recorded on, if the check has one.
    pub ticket: Option<String>,
}

/// One switch of a check's state by hand.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandSwitch {
    pub state: CheckState,
    pub actor: String,
    pub machine: String,
    pub at: Timestamp,
    pub note: String,
}

/// One check in `GET /checks/flaky`: a check that is switched off, has flaked or was switched
/// by hand.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlakyCheck {
    pub check: CheckName,
    pub family: String,
    pub state: CheckState,
    /// Flaky records of the check, not counting those from a command in which many checks
    /// flaked at once (a fault of the machine).
    pub flaky_records: u32,
    pub last_flaky: Option<Timestamp>,
    /// The newest ticket filed for the check's flaky records.
    pub ticket: Option<String>,
    /// The last time someone switched it by hand.
    pub switched: Option<HandSwitch>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlakyChecksReply {
    pub checks: Vec<FlakyCheck>,
}
