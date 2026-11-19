//! The traits that keep Ignite's parts apart.
//!
//! Each part that needs the clock, a command runner, the builds, git or the hub takes one of
//! these traits, so it can be tested against the in-memory fakes in [`crate::fakes`] before the
//! real implementation exists.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::ids::{Commit, Oid, RecordId, Seq, SessionId, TicketId, TreeId};
use crate::protocol::Class;
use crate::protocol::Usage;
use crate::protocol::hub::{
    AuditReport, CachedResult, Claim, CompileUnit, ExplorationFind, FlakyReport, IncompleteReport,
    JusticiarReport, Landed, LandedCommit, ListingKey, MachineFree, MetricsImport, NewResult,
    OpenRegression, Pause, Record, RegressionFiled, RegressionReport, ReportFiled, Reservation,
    ResultKey, SessionTitle, StoredListing, TicketBrief, TipClaim, TipClaimReply, TipVerified,
    TipWaiting, TipWaitingFiled,
};
use crate::protocol::tickets::{
    BatchReply, BatchRequest, FeedBatch, ImportReply, ImportRequest, LeaseOp, LeaseReply, Ticket,
    TicketDetail, TicketQuery, WholeTicket,
};
use crate::time::Timestamp;
use crate::tipround::{TipCosts, TipCostsRequest, TipRound, TipRoundFiled};

/// Why an operation behind one of these traits failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The hub or the git remote cannot be reached.
    Unreachable(String),
    /// A rebase conflict, a lost claim or a failed precondition.
    Conflict(String),
    /// The named thing does not exist.
    NotFound(String),
    /// The request was malformed or the configuration is wrong.
    Invalid(String),
    /// A local file or process failed.
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unreachable(m) => write!(f, "unreachable: {m}"),
            Error::Conflict(m) => write!(f, "conflict: {m}"),
            Error::NotFound(m) => write!(f, "not found: {m}"),
            Error::Invalid(m) => write!(f, "invalid: {m}"),
            Error::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Io(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

/// One command for a supervisor to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    /// The program and its arguments.
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Variables added to the daemon's environment.
    pub env: Vec<(String, String)>,
    pub class: Class,
    /// Cores wanted; the grant may be smaller.
    pub cores: u32,
    /// Shared resources held while it runs.
    pub resources: Vec<String>,
    /// The command rereads the file `IGNITE_CORES_FILE` names while it runs and starts more
    /// threads as its grant grows (`grows` on a family or product).
    pub grows: bool,
    pub tree: Option<TreeId>,
    pub ticket: Option<TicketId>,
    /// Seconds the command may run before its supervisor ends it with everything it started.
    /// Time spent waiting for cores does not count. `None` lets it run until it exits.
    pub hang_limit_s: Option<u64>,
}

/// How a supervised command ended.
#[derive(Clone, Debug, PartialEq)]
pub struct RunResult {
    /// The exit code; `None` if a signal ended it.
    pub exit: Option<i32>,
    /// The file that holds its whole output.
    pub output: PathBuf,
    pub usage: Usage,
    /// It ran past its hang limit and its supervisor ended it.
    pub hung: bool,
}

pub trait Launcher: Send + Sync {
    /// Runs one command under a supervisor and waits for it.
    fn run(&self, spec: &CommandSpec) -> RunResult;

    /// [`Launcher::run`], except that the command is ended once `cancelled` returns true, as
    /// when the client that asked for it has gone away. A launcher that cannot end a command
    /// runs it to the end.
    fn run_until(&self, spec: &CommandSpec, cancelled: &(dyn Fn() -> bool + Sync)) -> RunResult {
        let _ = cancelled;
        self.run(spec)
    }
}

/// How a request for a build product was answered.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildResult {
    /// The tree already held the product in this state; nothing ran.
    Current,
    /// A build ran (or was joined) and succeeded. `units` are the compiler processes it ran,
    /// as the compiler wrapper recorded them.
    Built {
        usage: Usage,
        units: Vec<CompileUnit>,
    },
    /// The build failed; its output is in the file named.
    Failed { output: PathBuf, usage: Usage },
}

pub trait Builds: Send + Sync {
    /// Makes `product` current in `tree`, joining a build already running.
    fn ensure(&self, tree: &TreeId, product: &str, packages: &[String]) -> BuildResult;

    /// [`Builds::ensure`] for a command: the build runs in the command's class and is charged
    /// to its ticket and session, so that a landing's builds count as the landing's. An
    /// implementation that cannot tell builds apart ignores `who`.
    fn ensure_for(
        &self,
        tree: &TreeId,
        product: &str,
        packages: &[String],
        who: &BuildFor,
    ) -> BuildResult {
        let _ = who;
        self.ensure(tree, product, packages)
    }

    /// Whether `tree` has built `product` before, so that making it current is an incremental
    /// rebuild rather than a cold build. The landing gate builds no product a tree has never
    /// built for checks chosen by score. An implementation that cannot tell says it has.
    fn has_built(&self, tree: &TreeId, product: &str) -> bool {
        let _ = (tree, product);
        true
    }

    /// Forgets what `tree` holds, after its build directories were deleted, so the next
    /// request builds instead of answering that the product is current.
    fn forget(&self, tree: &TreeId) {
        let _ = tree;
    }

    /// The products `tree` holds, for any packages and inputs, whose output directories all lie
    /// in `dirs` (paths relative to the tree's root). A lease seeding its tree prefers the donor
    /// holding the most of them: a product the donor never built is built cold in the copy.
    /// An implementation that cannot tell holds none.
    fn held_in(&self, tree: &TreeId, dirs: &[String]) -> BTreeSet<String> {
        let _ = (tree, dirs);
        BTreeSet::new()
    }

    /// Records, after `from`'s directories `dirs` were copied into `to`, that `to` holds each
    /// product [`Builds::held_in`] names for `from`, at inputs no request matches: the copy's
    /// units are there, so [`Builds::has_built`] is true and `to` can be a donor in turn, but
    /// the next request still runs the build, which finds the copied units current.
    fn copy_held(&self, from: &TreeId, to: &TreeId, dirs: &[String]) {
        let _ = (from, to, dirs);
    }
}

/// The command a build is made for: its class, ticket and session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildFor {
    pub class: Class,
    pub ticket: Option<TicketId>,
    pub session: Option<SessionId>,
}

/// How a rebase ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rebase {
    /// The branch now sits on `onto`; `landed_since` commits arrived since it was last based.
    Clean {
        onto: Commit,
        head: Commit,
        landed_since: u32,
    },
    /// The rebase stopped and was aborted; these files conflicted.
    Conflict { files: Vec<String> },
}

/// How a push ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Push {
    Pushed {
        commit: Commit,
    },
    /// The remote refused it, usually because the branch moved.
    Rejected {
        reason: String,
    },
}

pub trait Git: Send + Sync {
    /// The blob or tree ID of each path in the working tree, uncommitted edits included.
    fn object_ids(&self, tree: &Path, paths: &[String]) -> Result<Vec<(String, Oid)>>;
    /// The paths that differ between two commits.
    fn changed(&self, from: &Commit, to: &Commit) -> Result<Vec<String>>;
    fn rebase_onto(&self, tree: &Path, tip: &Commit) -> Result<Rebase>;
    fn push(&self, tree: &Path, commit: &Commit, branch: &str) -> Result<Push>;
    /// The remote branch's newest commit.
    fn tip(&self, branch: &str) -> Result<Commit>;
    /// The remote branch's newest commit as this checkout last saw it, by a fetch or by a push
    /// from it, without asking the remote. Cheap enough to call every few seconds. Fakes, which
    /// never fetch, answer [`Git::tip`].
    fn known_tip(&self, branch: &str) -> Result<Commit> {
        self.tip(branch)
    }
    /// The commit the local ref `name` (a full name, `refs/...`) points at; `None` when there is
    /// no such ref. Cheap enough to call every second. Fakes have no refs of their own.
    fn local_ref(&self, _name: &str) -> Result<Option<Commit>> {
        Ok(None)
    }
    /// Points the local ref `name` at `commit` in the repository of `tree`, which every worktree
    /// of the repository shares.
    fn set_local_ref(&self, _tree: &Path, _name: &str, _commit: &Commit) -> Result<()> {
        Ok(())
    }
    /// Whether `ancestor` is `descendant` or one of its ancestors.
    fn is_ancestor(&self, ancestor: &Commit, descendant: &Commit) -> Result<bool>;
    /// Whether `history` holds every change under `path` that `version` holds: `version` is in
    /// its history, or reached it as other commits with the same changes, as a rebased copy of
    /// a commit does.
    fn holds_changes(&self, history: &Commit, version: &Commit, path: &str) -> Result<bool>;
    /// Pushes `commit` to the remote `branch` in place of `expected`, whether or not `commit`
    /// contains it; [`Push::Rejected`] if the branch is no longer at `expected`.
    fn replace(
        &self,
        tree: &Path,
        commit: &Commit,
        branch: &str,
        expected: &Commit,
    ) -> Result<Push>;
}

pub trait Hub: Send + Sync {
    fn lookup(&self, keys: &[ResultKey]) -> Result<Vec<Option<CachedResult>>>;
    fn store(&self, results: &[NewResult]) -> Result<()>;
    fn next(&self, free: &MachineFree) -> Result<Option<Reservation>>;
    fn claim(&self, claim: &Claim) -> Result<TicketBrief>;
    fn landed(&self, landed: &Landed) -> Result<()>;
    fn lease(&self, name: &str, op: LeaseOp) -> Result<LeaseReply>;
    fn feed(&self, since: Seq, wait_s: u32) -> Result<FeedBatch>;
    fn records(&self, batch: &[Record]) -> Result<()>;
    /// Sends records as [`Hub::records`] does and returns the IDs the hub gave them, in order,
    /// so a report can name the record of a run. A hub that does not number records returns
    /// none.
    fn numbered_records(&self, batch: &[Record]) -> Result<Vec<RecordId>> {
        self.records(batch).map(|()| Vec::new())
    }
    /// Sends one machine's new efficiency rows (`POST /metrics/import`).
    fn metrics(&self, import: &MetricsImport) -> Result<()>;
    /// Reports a check that failed, then passed at once on the same inputs (`POST /flaky`).
    fn flaky(&self, report: &FlakyReport) -> Result<()>;
    /// Reports the title a desktop session shows (`POST /session-titles`). A hub that keeps no
    /// titles accepts and drops it.
    fn session_title(&self, _title: &SessionTitle) -> Result<()> {
        Ok(())
    }
    /// Claims one batch of one family's checks at a tip, so two machines never run the same
    /// batch (`POST /tips/claim`).
    fn tip_claim(&self, claim: &TipClaim) -> Result<TipClaimReply>;
    /// Sets a tip's verification state (`POST /tips/verified`).
    fn tip_verified(&self, verified: &TipVerified) -> Result<()>;
    /// Reports a regression; the hub files or reopens its ticket (`POST /regressions`).
    fn regression(&self, report: &RegressionReport) -> Result<RegressionFiled>;
    /// Every check with an open regression and the open ticket that holds it, including checks
    /// the hub grouped onto another check's ticket (`GET /regressions/open`).
    fn open_regressions(&self) -> Result<Vec<OpenRegression>> {
        Err(not_supported("list open regressions"))
    }
    /// Reports a cache audit whose rerun disagreed with the cached result (`POST /audits`).
    fn audit(&self, report: &AuditReport) -> Result<ReportFiled>;
    /// Reports a failed exploration check (`POST /finds`).
    fn find(&self, find: &ExplorationFind) -> Result<ReportFiled>;

    /// Which ticket landed each of `commits`, leaving out commits no landing recorded
    /// (`POST /landings/lookup`).
    fn landed_commits(&self, commits: &[Commit]) -> Result<Vec<LandedCommit>> {
        let _ = commits;
        Err(not_supported("look up landings"))
    }

    /// The stored listing of each key, in order, or `None` where the hub has none
    /// (`POST /listings/lookup`).
    fn listings(&self, keys: &[ListingKey]) -> Result<Vec<Option<String>>> {
        let _ = keys;
        Err(not_supported("look up listings"))
    }

    /// Stores a family's listing for every machine to reuse (`POST /listings/store`).
    fn store_listing(&self, listing: &StoredListing) -> Result<()> {
        let _ = listing;
        Err(not_supported("store a listing"))
    }

    /// Reports a check that stayed incomplete, or a family that cannot be listed; the hub files
    /// one ticket per signature (`POST /incomplete`).
    fn incomplete(&self, report: &IncompleteReport) -> Result<ReportFiled> {
        let _ = report;
        Err(not_supported("report an incomplete check"))
    }

    /// Reports a tip that has waited an hour on another platform's checks; the hub files a
    /// ticket naming each machine that runs it (`POST /tips/waiting`).
    fn tip_waiting(&self, waiting: &TipWaiting) -> Result<TipWaitingFiled> {
        let _ = waiting;
        Err(not_supported("report a waiting tip"))
    }

    /// The check costs and claim length that a round at `request.commit` cuts one family's
    /// batches by, frozen by the hub the first time any machine asks, so every machine cuts the
    /// same batches (`POST /tips/costs`).
    fn tip_costs(&self, request: &TipCostsRequest) -> Result<TipCosts> {
        let _ = request;
        Err(not_supported("give tip costs"))
    }

    /// Records one finished tip verification round. When the round asks for an improvement
    /// ticket, the hub files one or reuses the open one (`POST /tips/rounds`).
    fn tip_round(&self, round: &TipRound) -> Result<TipRoundFiled> {
        let _ = round;
        Err(not_supported("record tip rounds"))
    }

    /// The newest tip verification round any machine recorded (`GET /tips/rounds/last`).
    fn last_tip_round(&self) -> Result<Option<TipRound>> {
        Err(not_supported("give the last tip round"))
    }

    /// Each class's rate of check commands for today, which converts the two budgets from
    /// seconds to instructions (`GET /rates`).
    fn rates(&self) -> Result<crate::budget::ClassRates> {
        Err(not_supported("give the class rates"))
    }

    /// Reports an emergency; the hub files or comments on a justiciar ticket
    /// (`POST /justiciar`).
    fn justiciar(&self, report: &JusticiarReport) -> Result<ReportFiled> {
        let _ = report;
        Err(not_supported("report an emergency"))
    }

    /// One ticket with its comments (`GET /tickets/<id>`).
    fn ticket(&self, id: &TicketId) -> Result<TicketDetail> {
        Err(not_supported(&format!("read the ticket {id}")))
    }

    /// The tickets matching `query` (`GET /tickets`).
    fn tickets(&self, query: &TicketQuery) -> Result<Vec<Ticket>> {
        let _ = query;
        Err(not_supported("list tickets"))
    }

    /// Whole tickets with their edges, comments, history and aliases (`GET /export`): when
    /// `ids` is empty, the next page in ID order after `after` (empty at the end); otherwise
    /// each of `ids` that exists, at most 200.
    fn export(&self, after: Option<&TicketId>, ids: &[TicketId]) -> Result<Vec<WholeTicket>> {
        let _ = (after, ids);
        Err(not_supported("export tickets"))
    }

    /// Writes up to 200 whole tickets (`POST /import`).
    fn import(&self, request: &ImportRequest) -> Result<ImportReply> {
        let _ = request;
        Err(not_supported("import tickets"))
    }

    /// Applies every operation or none (`POST /batch`).
    fn batch(&self, request: &BatchRequest) -> Result<BatchReply> {
        let _ = request;
        Err(not_supported("write a batch"))
    }

    /// Pauses or resumes hand-offs to one machine or to all (`POST /pause`).
    fn pause(&self, pause: &Pause) -> Result<()> {
        let _ = pause;
        Err(not_supported("pause hand-offs"))
    }

    /// Declares, renews or adds to the emergency pause (`POST /emergency`); the reply is the
    /// emergency after the change.
    fn emergency(
        &self,
        change: &crate::protocol::emergency::EmergencyChange,
    ) -> Result<Option<crate::protocol::emergency::Emergency>> {
        let _ = change;
        Err(not_supported("change the emergency"))
    }

    /// The Ignite commit the hub and each registered machine runs (`GET /machines/versions`).
    fn fleet_versions(&self) -> Result<crate::protocol::fleet::FleetVersions> {
        Err(not_supported("give the fleet's versions"))
    }
}

/// The error of a [`Hub`] method that an implementation does not provide yet.
fn not_supported(what: &str) -> Error {
    Error::Invalid(format!("this hub client cannot {what} yet"))
}
