//! The hub's routes beyond tickets, under `/<project>/api`.
//!
//! - Machines: `POST /machines/register`, `POST /machines/heartbeat`.
//! - Hand-offs: `POST /next`, `POST /claims`, `POST /landed`, `POST /release`.
//! - The result cache: `POST /results/lookup`, `POST /results/store`, `POST /flaky`,
//!   `GET /checks/stats?family=`.
//! - Tips: `POST /tips/claim`, `POST /tips/verified`, `POST /regressions`.
//! - Records: `POST /records`, `POST /metrics/import`.

use serde::{Deserialize, Serialize};

use super::tickets::{LandedWork, Ticket};
use super::{Class, Outcome};
use crate::config::Os;
use crate::ids::{CheckName, Commit, InputHash, MachineId, RecordId, SessionId, TicketId, TreeId};
use crate::time::Timestamp;

/// `POST /machines/register`, sent once by `ignite install`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineRegister {
    /// The ID the daemon created and stored for this machine.
    pub machine: MachineId,
    /// The hostname, which names the machine's table in `ignite.toml`.
    pub name: String,
    pub os: Os,
    pub cores: u32,
    pub memory_mb: u64,
    /// The check families the machine can run; empty means every family.
    pub families: Vec<String>,
    /// The Ignite commit the daemon runs.
    pub version: String,
}

/// The reply to a registration: the write token the machine stores.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineRegistered {
    pub machine: MachineId,
    pub token: String,
}

/// Whether a session is a worker or a dispatcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerRole {
    Worker,
    Dispatcher,
}

/// One pool member, as its machine reports it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerState {
    pub session: SessionId,
    /// `claude` or `codex`.
    pub app: String,
    pub role: WorkerRole,
    /// `idle`, `reserved`, `working`, `stalled` or `down`.
    pub state: String,
    pub ticket: Option<TicketId>,
    /// The files the worker's last ticket touched.
    #[serde(default)]
    pub files: Vec<String>,
    /// The `ignite` command the session is running or last ran: the verb and its first
    /// argument, such as `wait L-5521`.
    #[serde(default)]
    pub command: Option<String>,
    /// When that command started, or when it ended if it is no longer running.
    #[serde(default)]
    pub command_since: Option<Timestamp>,
}

/// `POST /machines/heartbeat`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub machine: MachineId,
    pub version: String,
    pub workers: Vec<WorkerState>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatReply {
    /// The Ignite commit the hub runs; a daemon on another version updates before asking for
    /// work.
    pub hub_version: String,
}

/// An idle pool member that could take a ticket.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdleWorker {
    pub session: SessionId,
    pub app: String,
    /// The model tiers the worker's app can run, such as `opus` or `sonnet`.
    pub models: Vec<String>,
}

/// `POST /next`: what a machine has free.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MachineFree {
    pub machine: MachineId,
    pub os: Os,
    pub idle_workers: Vec<IdleWorker>,
    pub free_ticket_trees: u32,
    /// Cores the machine can admit beside the tickets in progress there.
    pub free_cores: f64,
    /// The families the machine can run; empty means every family.
    pub families: Vec<String>,
}

/// A ticket reserved for one machine and session until `expires_at`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reservation {
    pub ticket: TicketId,
    pub machine: MachineId,
    pub session: SessionId,
    pub expires_at: Timestamp,
}

/// `POST /claims`: `ignite start` turning a reservation (or nothing) into a claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub ticket: TicketId,
    pub machine: MachineId,
    pub session: SessionId,
    pub tree: TreeId,
}

/// What a dependency left behind, printed by `ignite start`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyNote {
    pub ticket: TicketId,
    pub title: String,
    pub landed: Option<LandedWork>,
    pub close_reason: Option<String>,
}

/// The reply to a claim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TicketBrief {
    pub ticket: Ticket,
    pub needs: Vec<DependencyNote>,
}

/// `POST /landed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Landed {
    pub ticket: TicketId,
    pub machine: MachineId,
    pub session: SessionId,
    pub work: LandedWork,
    /// `ignite land --fresh`: the session's context is already heavy, so the next ticket of
    /// its chain goes to a cleared session instead of continuing in this one.
    #[serde(default)]
    pub fresh: bool,
}

/// `POST /release`: a session gives a ticket back without landing it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub ticket: TicketId,
    pub machine: MachineId,
    pub session: SessionId,
    pub reason: String,
}

/// A result cache key. `platform` is `None` for a check with `platforms = "any"`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResultKey {
    pub check: CheckName,
    pub hash: InputHash,
    pub platform: Option<Os>,
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
    pub checks: Vec<CheckStats>,
}

/// `POST /tips/claim`: one batch of one family's checks at one tip.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipClaim {
    pub commit: Commit,
    pub family: String,
    pub batch: u32,
    pub machine: MachineId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipClaimReply {
    pub granted: bool,
    pub expires_at: Option<Timestamp>,
}

/// A tip's verification state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TipState {
    Seen,
    Verifying,
    Verified,
    Failed,
}

/// `POST /tips/verified`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipVerified {
    pub commit: Commit,
    pub machine: MachineId,
    pub state: TipState,
}

/// `POST /regressions`: a check that passed at `last_good` fails from `first_bad`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegressionReport {
    pub check: CheckName,
    pub first_bad: Commit,
    pub last_good: Commit,
    /// The ticket whose landing introduced `first_bad`, when known.
    pub caused_by: Option<TicketId>,
    /// The last lines of the failure's output.
    pub output: String,
    /// When the culprit search ended with a range instead of one landing: the ticket of every
    /// landing in it.
    #[serde(default)]
    pub suspects: Vec<TicketId>,
    /// The operating system the check failed on, for a check that is not portable.
    #[serde(default)]
    pub platform: Option<Os>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegressionFiled {
    pub ticket: TicketId,
}

/// The record one command leaves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub machine: MachineId,
    pub ticket: Option<TicketId>,
    pub session: Option<SessionId>,
    pub tree: Option<TreeId>,
    /// The verb, such as `test`, or `build`, `tip` or `idle` for the daemon's own work.
    pub verb: String,
    pub class: Class,
    pub family: Option<String>,
    pub queued_s: f64,
    pub wall_s: f64,
    pub cpu_s: f64,
    pub instructions: Option<f64>,
    pub peak_mb: u64,
    pub outcome: Option<Outcome>,
    pub commit: Option<Commit>,
    /// The Ignite commit that produced the record.
    pub ignite: String,
    pub at: Timestamp,
    /// Per-crate compile costs measured by the compiler wrapper.
    #[serde(default)]
    pub compile_units: Vec<CompileUnit>,
}

/// One compiler process's cost.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompileUnit {
    #[serde(rename = "crate")]
    pub crate_name: String,
    /// `build`, `test`, `check` or another Cargo mode.
    pub mode: String,
    pub instructions: Option<f64>,
    pub cpu_s: f64,
    pub rebuilt: bool,
}

/// `POST /records`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordBatch {
    pub records: Vec<Record>,
}

/// `POST /metrics/import`: one machine's new efficiency rows, as the dashboard's ingest wrote
/// them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricsImport {
    pub machine: MachineId,
    pub source: String,
    pub rows: Vec<serde_json::Value>,
}

/// `POST /claims` from `ignite start "<summary>"`, for a session handed a task and no ticket:
/// the hub files a ticket titled with the summary and labelled `adhoc`, and claims it, in one
/// request. The reply is a [`TicketBrief`], as for a [`Claim`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdhocClaim {
    pub summary: String,
    pub machine: MachineId,
    pub session: SessionId,
    pub tree: TreeId,
}

/// The reply to `POST /landed`: the next ticket of the landed ticket's `lane:` chain, already
/// reserved for the same session, which takes it without being cleared.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandedReply {
    pub next: Option<Reservation>,
}

/// `POST /incomplete`: a check that stayed `incomplete` after its last attempt on a machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncompleteReport {
    pub check: CheckName,
    pub machine: MachineId,
    /// The machine's hostname, which the ticket names and its signature leaves out.
    pub host: String,
    /// The operating system, for a check that is not portable.
    #[serde(default)]
    pub platform: Option<Os>,
    pub attempts: u32,
    pub reason: IncompleteReason,
    pub commit: Commit,
    /// The last lines of the command's output.
    pub output: String,
}

/// Why a run left a check without a verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IncompleteReason {
    /// The command ended without reporting the check.
    Unreported,
    /// The command said it cannot run on this machine.
    CannotRun,
    /// The command ran past its hang limit and was ended.
    Hung,
}

/// `POST /audits`: a rerun of a cache hit that disagreed with the cached result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditReport {
    pub key: ResultKey,
    /// The cached result and the record of the run that produced it.
    pub cached: Outcome,
    pub cached_record: RecordId,
    pub cached_machine: MachineId,
    /// The rerun's result, record and machine.
    pub rerun: Outcome,
    pub rerun_record: RecordId,
    pub rerun_machine: MachineId,
    pub commit: Commit,
    /// The last lines of the rerun's output.
    pub output: String,
}

/// `POST /finds`: an exploration check that failed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplorationFind {
    pub check: CheckName,
    pub machine: MachineId,
    /// The machine's hostname, which the signature leaves out.
    pub host: String,
    pub commit: Commit,
    /// The command that plays the check again.
    pub replay: String,
    /// The last lines of the failure's output.
    pub output: String,
}

/// The reply to `/incomplete`, `/audits` and `/finds`: the ticket that holds the report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportFiled {
    pub ticket: TicketId,
}

/// `POST /pause`: stops (or restarts) hand-offs to one machine, or to every machine when
/// `machine` is `None`. While a pause holds, `/next` answers nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pause {
    pub machine: Option<MachineId>,
    pub paused: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_matches_the_documented_line() {
        let record: Record = serde_json::from_str(
            r#"{"machine": "m-7f3a", "ticket": "Tq44s.9.48", "session": "local_c8470108",
                "tree": "tree-07", "verb": "test", "class": "agent", "family": "b3d-test",
                "queued_s": 4.1, "wall_s": 58.2, "cpu_s": 301.0, "instructions": 1.21e12,
                "peak_mb": 2210, "outcome": "fail", "commit": "8c1f2ab", "ignite": "d97ed0e",
                "at": "2026-11-17T14:12:00.000Z"}"#,
        )
        .unwrap();
        assert_eq!(record.class, Class::Agent);
        assert_eq!(record.outcome, Some(Outcome::Fail));
        assert!(record.compile_units.is_empty());
    }

    #[test]
    fn portable_result_keys_have_no_platform() {
        let key = ResultKey {
            check: "rust:a::b".parse().unwrap(),
            hash: "0".repeat(64).parse().unwrap(),
            platform: None,
        };
        let value = serde_json::to_value(&key).unwrap();
        assert_eq!(value["platform"], serde_json::Value::Null);
        assert_eq!(serde_json::from_value::<ResultKey>(value).unwrap(), key);
    }
}
