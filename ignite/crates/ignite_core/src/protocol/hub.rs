//! The hub's routes beyond tickets, under `/<project>/api`.
//!
//! - Machines: `POST /machines/register`, `POST /machines/heartbeat`.
//! - Hand-offs: `POST /next`, `POST /claims`, `POST /landed`, `POST /release`.
//! - The result cache: `POST /results/lookup`, `POST /results/store`, `POST /flaky`,
//!   `GET /checks/stats?family=`.
//! - Tips: `POST /tips/claim`, `POST /tips/verified`, `POST /tips/waiting`, `POST /regressions`,
//!   `POST /landings/lookup`.
//! - Records: `POST /records`, `POST /metrics/import`.
//! - Listings: `POST /listings/lookup`, `POST /listings/store`.

use serde::{Deserialize, Serialize};

use super::tickets::{LandedWork, Ticket};
use super::{Class, Outcome};
use crate::config::Os;
use crate::ids::{CheckName, Commit, InputHash, MachineId, RecordId, SessionId, TicketId, TreeId};
use crate::time::Timestamp;

/// The largest request body the hub reads from a machine with a token, as the ticket service
/// allowed. The hub answers a larger one with 413 before reading it, so a client checks a body
/// against this before sending it.
pub const MAX_BODY_BYTES: u64 = 100 * 1024 * 1024;

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
    /// Whether that command is still running.
    #[serde(default)]
    pub command_running: bool,
}

/// `POST /session-titles`: the title a Claude desktop session on `machine` shows, which the
/// dashboard uses to name the session instead of its id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTitle {
    pub machine: MachineId,
    pub session: SessionId,
    pub title: String,
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
    /// The emergency in force, which the daemon enforces until a reply leaves it out (see
    /// [`super::emergency`]). A hub that predates it sends none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emergency: Option<super::emergency::Emergency>,
    /// Hand-offs are paused on every machine (`ignite pool pause --all`), so not even the
    /// tickets that hold `master` back are handed off. `ignite status` says so.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paused_everywhere: bool,
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
    /// The cores the machine's scheduler has free at this moment. Dispatch tests a ticket's
    /// expected cores against it only when `admit_cores` is missing.
    pub free_cores: f64,
    /// The cores the machine admits tickets against: all the cores its scheduler hands out. The
    /// hub tests a ticket's expected cores against this less the expected cores of the tickets
    /// in progress on the machine or reserved for it ([`crate::dispatch::room`]), not against
    /// `free_cores`, because a landing that fills the scheduler for a minute says nothing about
    /// room for a ticket whose worker will spend its first minutes reading and editing. A
    /// daemon that predates it sends none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admit_cores: Option<f64>,
    /// The families the machine can run; empty means every family.
    pub families: Vec<String>,
    /// The machine's registered name, which a `needs:<name>` label asks for. The hub fills it
    /// from the machine's registration.
    #[serde(default)]
    pub name: Option<String>,
    /// Offer only justiciar tickets. A daemon asks this way for its last idle worker while its
    /// pool has more than one, so a pool whose other workers all hold stuck tickets still has
    /// a worker for the justiciar ticket that would free them.
    #[serde(default)]
    pub justiciar_only: bool,
    /// Offer only a ticket that holds `master` back (a justiciar or regression ticket), and
    /// none while the machine already holds one. A daemon asks this way while its own pool is
    /// paused (`ignite pool pause`): a pause stops new feature work, but `master` must not stay
    /// stuck behind it.
    #[serde(default)]
    pub master_only: bool,
    /// Answer `{"nothing": "<reason>"}` instead of `null` when no ticket fits, so the daemon can
    /// show in `ignite status` why its idle workers are idle ([`NextReply`]). A daemon that
    /// predates it sends none and gets `null`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub explain: bool,
}

/// The hub's answer to `POST /next`: a reservation, or nothing with the reason when the request
/// set `explain` and the hub knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextReply {
    Reserved(Reservation),
    Nothing(Option<String>),
}

/// `POST /next`'s body on the wire: a reservation, `{"nothing": "<reason>"}`, or `null`.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum NextWire {
    Reserved(Reservation),
    Nothing { nothing: String },
}

impl Serialize for NextReply {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Reserved(r) => r.serialize(serializer),
            Self::Nothing(None) => serializer.serialize_none(),
            Self::Nothing(Some(reason)) => NextWire::Nothing {
                nothing: reason.clone(),
            }
            .serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for NextReply {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<NextWire>::deserialize(deserializer)? {
            Some(NextWire::Reserved(r)) => Self::Reserved(r),
            Some(NextWire::Nothing { nothing }) => Self::Nothing(Some(nothing)),
            None => Self::Nothing(None),
        })
    }
}

/// A ticket reserved for one machine and session until `expires_at`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reservation {
    pub ticket: TicketId,
    pub machine: MachineId,
    pub session: SessionId,
    pub expires_at: Timestamp,
    /// The one-time code that the hand-off's `ignite start --reservation <code>` must send to
    /// claim the ticket, so that a hand-off arriving after the ticket went elsewhere claims
    /// nothing.
    pub code: String,
}

/// `POST /claims`: `ignite start` turning a reservation (or nothing) into a claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub ticket: TicketId,
    pub machine: MachineId,
    pub session: SessionId,
    pub tree: TreeId,
    /// The reservation's code. A reserved ticket is claimed only with it, by any session of the
    /// machine it was reserved for; a claim with a code but no live reservation is refused.
    #[serde(default)]
    pub code: Option<String>,
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
    /// The branch on the remote where an earlier release saved the ticket's unfinished work.
    #[serde(default)]
    pub saved_branch: Option<String>,
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
    /// The note: why the ticket was given back and the exact state it was left in.
    pub reason: String,
    /// What the ticket needs that this session lacks, such as `macos`, a machine's name or
    /// `person`; the hub labels the ticket `needs:<needs>`.
    #[serde(default)]
    pub needs: Option<String>,
    /// The branch on the remote where the release saved the tree's work.
    #[serde(default)]
    pub branch: Option<String>,
    /// Why the ticket was given back, which decides whether the hub counts the release.
    #[serde(default)]
    pub cause: ReleaseCause,
}

/// `POST /close`: `ignite close` closes a ticket that needs no landing and frees its worker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Close {
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

// `ResultCost` lives in a file of its own, with its tests, to keep this one short.
#[path = "hub_cost.rs"]
mod cost;
pub use cost::ResultCost;

// Why a ticket was released lives in a file of its own, with its tests.
#[path = "hub_releases.rs"]
mod releases;
pub use releases::ReleaseCause;

// Verdicts and reverted landings live in a file of their own, with their tests.
#[path = "hub_verdicts.rs"]
mod verdicts;
pub use verdicts::{RevertFiled, Reverted, Verdict, VerdictKey, verdict_settings};

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

/// `POST /tips/waiting`: a tip has waited an hour for checks that only a machine running
/// `platform` can run, or waits on families no such machine runs at all. The hub files one P1
/// ticket naming each such machine, or one naming the families.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TipWaiting {
    pub commit: Commit,
    /// The machine that is waiting.
    pub machine: MachineId,
    pub platform: Os,
    /// The checks the tip waits for.
    pub checks: Vec<CheckName>,
    /// How long the tips since the last verified commit have waited on `platform`.
    pub waited_s: f64,
    /// The families of those checks that no `platform` machine declared in `ignite.toml` lists
    /// in its `families`, so the tip can never verify. Sent at once rather than after an hour.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unrunnable: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipWaitingFiled {
    pub tickets: Vec<TicketId>,
}

/// `POST /landings/lookup`: which ticket landed each of these commits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandingsRequest {
    pub commits: Vec<Commit>,
}

/// One commit and the ticket whose landing recorded it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandedCommit {
    pub commit: Commit,
    pub ticket: TicketId,
}

/// The requested commits that a ticket's landing recorded; a commit no landing recorded, such
/// as one pushed by hand, is left out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandingsReply {
    pub commits: Vec<LandedCommit>,
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
    /// How the culprit search ended; `None` from a daemon built before it said.
    #[serde(default)]
    pub search: Option<crate::tipround::CulpritSearch>,
    /// Who found the failure. A landing that finds it means tip verification missed it, and
    /// the hub records why.
    #[serde(default)]
    pub found_by: FoundBy,
}

/// Who found a regression.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FoundBy {
    /// A tip verification round, at the tip it verified.
    #[default]
    TipVerification,
    /// A landing's gate, at the `staging` tip the landing was rebased onto.
    Landing,
}

/// A check whose regression is open, with the open ticket that holds it. One ticket can hold
/// several checks, because the hub groups failing checks onto few tickets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRegression {
    pub check: CheckName,
    pub ticket: TicketId,
}

/// `GET /regressions/open`: every check with an open regression and its ticket.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRegressions {
    pub regressions: Vec<OpenRegression>,
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

/// `POST /incomplete`: one cause that left checks `incomplete` on a machine, such as a build
/// that failed or a command that ended without reporting them. The hub files one ticket per
/// cause, never one per check.
///
/// `check` is the first check the cause holds back, or a name for the cause itself such as
/// `<family>:build`. A report from an older daemon names one check and leaves `checks`,
/// `count` and `product` empty.
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
    /// The first [`INCOMPLETE_LISTED`] of the checks this cause holds back, for the ticket.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<CheckName>,
    /// How many checks this cause holds back, when that is more than `check` alone.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub count: u32,
    /// The build product whose build failed or was refused, for [`IncompleteReason::BuildFailed`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    /// Whether the tip being verified waits on some check this cause holds back, so that
    /// `master` cannot move until the cause is fixed. The hub files such a ticket released. An
    /// older daemon leaves it out, which reads as `false`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub holds_tip: bool,
}

/// How many checks an [`IncompleteReport`] names; `count` gives the whole number.
pub const INCOMPLETE_LISTED: usize = 20;

#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Why a run left a check without a verdict. New values are appended, so that an older hub's
/// reading of the ones it knows is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IncompleteReason {
    /// The command ended without reporting the check.
    Unreported,
    /// The command said it cannot run on this machine.
    CannotRun,
    /// The command ran past its hang limit and was ended.
    Hung,
    /// A build the checks need failed or was refused, so their command never ran.
    BuildFailed,
    /// The checks' manifest entry is invalid: their inputs cannot be resolved.
    InvalidEntry,
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

/// `POST /commands/rejected`: a command line the `ignite` executable refused to parse, sent so
/// the hub can tell which verbs and flags agents look for and do not find. The machine is the
/// one whose token sent it; the hub adds the ticket the session holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedCommand {
    /// The subcommands the parser recognized, then the word it rejected, such as
    /// `ticket dep list` or `ticket children --open`.
    pub verb: String,
    /// Every argument after the executable's name.
    pub args: Vec<String>,
    /// What the parser objected to: `unknown subcommand` or `unknown argument`.
    pub kind: String,
    /// The parser's suggestion, when it had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionId>,
}

/// `POST /justiciar`: an emergency that stops work and that no ordinary ticket will fix, such
/// as a crash of Ignite itself, a daemon loop that keeps failing, or a session that asked for
/// one. The hub files a P0 ticket labelled `justiciar`, which is offered before any other
/// ticket. A report whose subject matches an open justiciar ticket adds a comment instead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JusticiarReport {
    /// What the emergency is about, such as `crash:ignite-tip` or `loop:sender`. Reports with
    /// the same subject (and machine, when `on_machine` is set) share one ticket.
    pub subject: String,
    /// The ticket's title.
    pub title: String,
    /// The ticket's body, or the comment on the open ticket: what was seen and where.
    pub details: String,
    /// Names this sighting, such as a crash record's time, so a report sent twice adds nothing.
    pub evidence: String,
    pub machine: MachineId,
    /// The reporting machine's hostname.
    pub host: String,
    /// Whether only that machine can work the ticket, such as for a crash of its daemon. The
    /// ticket is then labelled `needs:<host>`.
    #[serde(default)]
    pub on_machine: bool,
}

/// A family's stored listing is found by the family's name and its listing hash, the hash of
/// everything the family's `list` command can read.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ListingKey {
    pub family: String,
    pub hash: InputHash,
}

/// `POST /listings/lookup`; the reply holds one entry per key, in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListingLookup {
    pub keys: Vec<ListingKey>,
}

/// The stored listing of each key asked for, or `None` where the hub has none. A listing is
/// text the daemon wrote; the hub keeps it without reading it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListingReply {
    pub listings: Vec<Option<String>>,
}

/// `POST /listings/store`: a listing one machine made, for every machine to reuse while the
/// family's listing hash holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredListing {
    pub key: ListingKey,
    pub listing: String,
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
