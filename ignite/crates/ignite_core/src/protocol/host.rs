//! The host protocol: the calls a verb process makes to its machine's daemon.
//!
//! A long verb, such as `land`, runs in a process of its own (a verb process, started as
//! `ignite internal verb <handle>`) that outlives restarts of the daemon. It asks the daemon for
//! the few things that need one authority on the machine: starting a supervised command (which
//! includes waiting for cores), making a build product current, leasing a landing tree, and
//! keeping the handle's step and stop state where `ignite wait`, `status` and `stop` see them.
//!
//! # Transport
//!
//! Each call is one connection to the daemon's socket (a named pipe on Windows), the same one
//! clients use, carrying one [`client::Request`] line and one [`client::Reply`] line:
//!
//! - The request's `verb` is the call's name, such as `host-run`, and `args` holds exactly one
//!   string: the call's body as JSON. `cwd` is the verb process's working directory, `session`
//!   is `null` and `wait_s` is 0. See [`encode_request`] and [`decode_request`].
//! - A call that succeeded replies with exit 0, empty `text`, and the reply body as `json`.
//! - A call that failed replies with a non-zero exit, the message as `text`, and
//!   `{"error": {"kind": ..., "message": ...}}` as `json` ([`HostError`]). A daemon that does
//!   not know the call answers as it does for any unknown verb, with no `json`; the caller reads
//!   that as [`Error::Invalid`]. See [`encode_reply`] and [`decode_reply`].
//!
//! # Repeating a call
//!
//! Every call may be sent again, with the same body, after its connection was cut, as when the
//! daemon restarts into a new build, and the answer is the same as if the first attempt had
//! been answered. `host-run` and `host-build` carry a [`RequestId`], the handle and a counter the
//! verb process keeps, by which the daemon finds a command or build it already started for the
//! call. They also carry `first_asked_at`, the moment the verb process first sent the call, so a
//! request still waiting for cores keeps its place in the queue across a restart. A call that
//! waits (`host-run`, `host-build`) answers `waiting` after at most [`MAX_BLOCK_S`] seconds, so
//! a long wait is a series of calls and no call is cut off for long.
//!
//! After any failed call the verb process sends `host-hello` first, then its current step again
//! (`host-step`), then the call that failed. It does the same, without a failed call, when the
//! `daemon_started` of a reply differs from the one before.
//!
//! # Versions
//!
//! Version 1 is frozen: golden files under `ignite_core/tests/host_v1/` pin the JSON of one
//! request and one reply of every call, and of the envelope. An old verb process keeps running
//! under a newer daemon, and a newer verb process may meet a daemon that was rolled back, so:
//!
//! - A change adds an optional field or a new call (such as `host-run2`, introduced in a new
//!   [`PROTOCOL_VERSION`]). It never renames, retypes or removes a field or a call, and never
//!   gives an existing field a new meaning.
//! - An added field is an `Option` with `#[serde(default, skip_serializing_if =
//!   "Option::is_none")]`, so that a body without it still parses and a body that leaves it
//!   unset serialises exactly as version 1 did.
//! - Unknown fields are ignored on both sides: no type here denies them.
//! - The daemon serves every version it ever served. `host-hello` reports the daemon's newest
//!   version, and a verb process uses only calls whose [`HostCall::SINCE`] is at most both its
//!   own and the daemon's version.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::ids::{Commit, Handle, IdError, SessionId, TicketId, TreeId};
use crate::protocol::client::{Exit, Reply, Request};
use crate::protocol::hub::CompileUnit;
use crate::protocol::{Class, Usage};
use crate::time::Timestamp;
use crate::traits::{BuildFor, BuildResult, CommandSpec, Error, Reserve, Result};

/// The newest version of the host protocol this build speaks.
pub const PROTOCOL_VERSION: u32 = 4;

/// The longest a call that waits holds its connection before it answers `waiting`.
pub const MAX_BLOCK_S: u32 = 30;

/// The variable the daemon adds to a command's environment, and so to its run's `spec.json`,
/// naming the [`RequestId`] of the `host-run` call that started it.
pub const REQUEST_VARIABLE: &str = "IGNITE_REQUEST";

/// One call of the host protocol: its request body, with the call's name and reply type.
pub trait HostCall: Serialize + DeserializeOwned {
    /// The verb the request line names, such as `host-run`.
    const VERB: &'static str;
    /// The protocol version that introduced the call.
    const SINCE: u32;
    type Reply: Serialize + DeserializeOwned;
    /// The verb process making the call.
    fn handle(&self) -> &Handle;
}

/// Every call, with the version that introduced it. A daemon registers a verb for each.
pub const CALLS: [(&str, u32); 15] = [
    (Hello::VERB, Hello::SINCE),
    (Run::VERB, Run::SINCE),
    (Build::VERB, Build::SINCE),
    (ExpressBuild::VERB, ExpressBuild::SINCE),
    (LeaseLanding::VERB, LeaseLanding::SINCE),
    (ReleaseLanding::VERB, ReleaseLanding::SINCE),
    (StepUpdate::VERB, StepUpdate::SINCE),
    (RefuseStop::VERB, RefuseStop::SINCE),
    (AllowStop::VERB, AllowStop::SINCE),
    (Poll::VERB, Poll::SINCE),
    (Finished::VERB, Finished::SINCE),
    (BorrowCores::VERB, BorrowCores::SINCE),
    (ReturnCores::VERB, ReturnCores::SINCE),
    (Selected::VERB, Selected::SINCE),
    (ReadDispatchPressure::VERB, ReadDispatchPressure::SINCE),
];

/// Whether a verb process speaking `own` may make the call introduced in `since` to a daemon
/// that reported `daemon` in its `host-hello` reply.
pub fn usable(since: u32, own: u32, daemon: u32) -> bool {
    since <= own.min(daemon)
}

/// Names one `host-run` or `host-build` call of one verb process: the handle and a counter the
/// process increments for each new call, written `<handle>#<counter>`, such as `L-6#17`. A
/// repeated call carries the same ID.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RequestId {
    handle: Handle,
    counter: u64,
}

impl RequestId {
    pub fn new(handle: Handle, counter: u64) -> RequestId {
        RequestId { handle, counter }
    }

    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    pub fn counter(&self) -> u64 {
        self.counter
    }

    /// Whether `text`, such as a run's `IGNITE_REQUEST`, names a call of `handle`.
    pub fn of_handle(text: &str, handle: &Handle) -> bool {
        text.parse::<RequestId>()
            .is_ok_and(|id| id.handle == *handle)
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.handle, self.counter)
    }
}

impl FromStr for RequestId {
    type Err = IdError;
    fn from_str(value: &str) -> std::result::Result<Self, IdError> {
        let invalid = |reason| IdError {
            kind: "request ID",
            value: value.to_owned(),
            reason,
        };
        let (handle, counter) = value
            .rsplit_once('#')
            .ok_or_else(|| invalid("must be <handle>#<counter>"))?;
        let counter = counter
            .parse()
            .map_err(|_| invalid("the counter is not a number"))?;
        let handle = Handle::new(handle).map_err(|_| invalid("the handle is not valid"))?;
        Ok(RequestId { handle, counter })
    }
}

impl TryFrom<String> for RequestId {
    type Error = IdError;
    fn try_from(value: String) -> std::result::Result<Self, IdError> {
        value.parse()
    }
}

impl From<RequestId> for String {
    fn from(id: RequestId) -> String {
        id.to_string()
    }
}

/// Hands out the [`RequestId`]s of one verb process, starting at 1.
#[derive(Debug)]
pub struct RequestIds {
    handle: Handle,
    next: AtomicU64,
}

impl RequestIds {
    pub fn new(handle: Handle) -> RequestIds {
        RequestIds {
            handle,
            next: AtomicU64::new(1),
        }
    }

    /// The ID for a new call. A repeat of a call reuses the ID it got the first time.
    pub fn next(&self) -> RequestId {
        RequestId::new(
            self.handle.clone(),
            self.next.fetch_add(1, Ordering::Relaxed),
        )
    }
}

/// `host-hello`: registers the verb process with the daemon. Sent first, and again before
/// repeating any call that failed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub handle: Handle,
    /// The newest protocol version the verb process speaks.
    pub protocol: u32,
    /// The commit the verb process's executable was built from, as its folder under `bin/` is
    /// named, so that `ignite status` can say which build each verb runs under.
    pub executable: String,
    pub pid: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloReply {
    /// The newest protocol version the daemon speaks.
    pub protocol: u32,
    /// The daemon's handle is still running. False when the handle is unknown or has ended, as
    /// when `ignite stop` ended it while no daemon ran: the verb process ends without a reply.
    pub running: bool,
    /// Someone asked the verb to stop.
    pub stop: bool,
    /// When the daemon that answered started (see [`StopState::daemon_started`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_started: Option<String>,
}

impl HostCall for Hello {
    const VERB: &'static str = "host-hello";
    const SINCE: u32 = 1;
    type Reply = HelloReply;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// One command for a supervisor to run, as [`CommandSpec`] on the wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Variables added to the daemon's environment, as `[name, value]` pairs. The daemon adds
    /// [`REQUEST_VARIABLE`] itself.
    pub env: Vec<(String, String)>,
    pub class: Class,
    pub cores: u32,
    pub resources: Vec<String>,
    pub grows: bool,
    pub tree: Option<TreeId>,
    pub ticket: Option<TicketId>,
    pub hang_limit_s: Option<u64>,
    /// Cores set aside for this kind of command. Left out when unset, so a verb process and a
    /// daemon of releases before or after it still understand each other.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserve: Option<Reserve>,
}

impl From<&CommandSpec> for Command {
    fn from(spec: &CommandSpec) -> Command {
        Command {
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            class: spec.class,
            cores: spec.cores,
            resources: spec.resources.clone(),
            grows: spec.grows,
            tree: spec.tree.clone(),
            ticket: spec.ticket.clone(),
            hang_limit_s: spec.hang_limit_s,
            reserve: spec.reserve,
        }
    }
}

impl From<Command> for CommandSpec {
    fn from(command: Command) -> CommandSpec {
        CommandSpec {
            args: command.args,
            cwd: command.cwd,
            env: command.env,
            class: command.class,
            cores: command.cores,
            resources: command.resources,
            grows: command.grows,
            tree: command.tree,
            ticket: command.ticket,
            hang_limit_s: command.hang_limit_s,
            reserve: command.reserve,
        }
    }
}

/// `host-run`: waits for a grant of cores, starts the command under a supervisor and names its
/// run directory. The verb process then waits for the command itself: it takes a shared lock on
/// the run's `lock` file and reads `result.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub handle: Handle,
    pub request_id: RequestId,
    /// When the verb process first sent this call; the queue for cores orders by it.
    pub first_asked_at: Timestamp,
    pub spec: Command,
}

/// The answer to `host-run`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum RunReply {
    /// No grant yet after [`MAX_BLOCK_S`] seconds; send the same call again.
    Waiting {
        /// What the request waits for, such as `8 cores`, for the verb's step.
        #[serde(rename = "for")]
        what: String,
    },
    /// The command was started, by this call or by an earlier attempt with the same request ID.
    /// It may have finished already.
    Started { run: PathBuf },
    /// The daemon will not start it, as when a stop was asked for the handle.
    Refused { reason: String },
}

impl HostCall for Run {
    const VERB: &'static str = "host-run";
    const SINCE: u32 = 1;
    type Reply = RunReply;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// Whom a build is made for, as [`BuildFor`] on the wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildWho {
    pub class: Class,
    pub ticket: Option<TicketId>,
    pub session: Option<SessionId>,
}

impl From<&BuildFor> for BuildWho {
    fn from(who: &BuildFor) -> BuildWho {
        BuildWho {
            class: who.class,
            ticket: who.ticket.clone(),
            session: who.session.clone(),
        }
    }
}

impl From<BuildWho> for BuildFor {
    fn from(who: BuildWho) -> BuildFor {
        BuildFor {
            class: who.class,
            ticket: who.ticket,
            session: who.session,
        }
    }
}

/// `host-build`: makes a product current in a tree (`Builds::ensure_for` in the daemon),
/// joining a build already running for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    pub handle: Handle,
    pub request_id: RequestId,
    pub first_asked_at: Timestamp,
    pub tree: TreeId,
    pub product: String,
    pub packages: Vec<String>,
    #[serde(rename = "for")]
    pub who: BuildWho,
}

/// The answer to `host-build`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum BuildReply {
    /// The build still runs (or waits for cores) after [`MAX_BLOCK_S`] seconds; send the same
    /// call again.
    Waiting {
        #[serde(rename = "for")]
        what: String,
    },
    /// The tree already held the product in this state; nothing ran.
    Current,
    /// A build ran, or was joined, and succeeded.
    Built {
        usage: Usage,
        units: Vec<CompileUnit>,
    },
    /// The build failed; its output is in the file named.
    Failed { output: PathBuf, usage: Usage },
}

impl BuildReply {
    /// The finished answer, or `None` while the build is `waiting`.
    pub fn into_result(self) -> Option<BuildResult> {
        match self {
            BuildReply::Waiting { .. } => None,
            BuildReply::Current => Some(BuildResult::Current),
            BuildReply::Built { usage, units } => Some(BuildResult::Built { usage, units }),
            BuildReply::Failed { output, usage } => Some(BuildResult::Failed { output, usage }),
        }
    }
}

impl From<BuildResult> for BuildReply {
    fn from(result: BuildResult) -> BuildReply {
        match result {
            BuildResult::Current => BuildReply::Current,
            BuildResult::Built { usage, units } => BuildReply::Built { usage, units },
            BuildResult::Failed { output, usage } => BuildReply::Failed { output, usage },
        }
    }
}

impl HostCall for Build {
    const VERB: &'static str = "host-build";
    const SINCE: u32 = 1;
    type Reply = BuildReply;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// The express runner owns its reserved cores and command. The daemon owns its build slot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpressBuild {
    pub handle: Handle,
    pub request_id: RequestId,
    pub tree: PathBuf,
    pub output: PathBuf,
    pub stage: ExpressBuildStage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressBuildStage {
    Prepare,
    Passed,
    Failed,
}

impl HostCall for ExpressBuild {
    const VERB: &'static str = "host-express-build";
    const SINCE: u32 = 4;
    type Reply = BuildReply;

    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// `host-lease-landing`: leases a free landing tree checked out at `commit` to the handle. A
/// lease the handle already holds under the same holder at the same commit is returned as is,
/// so a repeated call leases nothing new.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseLanding {
    pub handle: Handle,
    /// Who holds the lease, such as `<machine>:<ticket>` for a landing.
    pub holder: String,
    pub commit: Commit,
    /// Lease only while this many other landing trees stay free (1 for idle work, else 0).
    pub keep_free: u32,
}

/// The answer to `host-lease-landing`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum LeaseLandingReply {
    Leased {
        tree: TreeId,
        path: PathBuf,
        branch: String,
        /// The commit the tree is checked out at.
        at: Commit,
    },
    /// Every landing tree is busy (or too few would stay free); ask again later.
    Busy,
}

impl HostCall for LeaseLanding {
    const VERB: &'static str = "host-lease-landing";
    const SINCE: u32 = 1;
    type Reply = LeaseLandingReply;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// `host-release-landing`: frees a landing tree the handle leased. Releasing a tree that is
/// already free under the handle answers as the first release did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseLanding {
    pub handle: Handle,
    pub tree: TreeId,
    /// The commit the tree's builds now match; `None` when unknown.
    pub built: Option<Commit>,
}

/// The reply to a call that answers nothing but success.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ack {}

impl HostCall for ReleaseLanding {
    const VERB: &'static str = "host-release-landing";
    const SINCE: u32 = 1;
    type Reply = Ack;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// `host-borrow-cores`: claims the express reserve for a verb process without waiting.
/// The daemon grants up to `cores` extra, and keeps the claim until returned or the handle ends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BorrowCores {
    pub handle: Handle,
    /// The most cores wanted.
    pub cores: u32,
}

/// The answer to `host-borrow-cores`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BorrowCoresReply {
    /// The claim to return with `host-return-cores`, even if it gave zero extra cores.
    pub grant: Option<u64>,
    /// The cores granted; zero when `grant` is `None`.
    pub cores: u32,
}

impl HostCall for BorrowCores {
    const VERB: &'static str = "host-borrow-cores";
    const SINCE: u32 = 2;
    type Reply = BorrowCoresReply;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// `host-return-cores`: gives back a grant from `host-borrow-cores`. A grant already returned
/// is ignored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnCores {
    pub handle: Handle,
    pub grant: u64,
}

impl HostCall for ReturnCores {
    const VERB: &'static str = "host-return-cores";
    const SINCE: u32 = 2;
    type Reply = Ack;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// `host-selected`: asks which packages the build of `product` selects for `packages`
/// ([`Builds::selected`](crate::traits::Builds::selected)). The daemon compares the features of
/// the subset with the whole workspace's, which only its build manager can do, and the verb
/// process uses the answer to list and run tests with the same packages the build compiled. It
/// never waits for a build, and repeating it gives the same answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selected {
    pub handle: Handle,
    pub tree: TreeId,
    pub product: String,
    /// The packages the caller wants.
    pub packages: Vec<String>,
}

/// The answer to `host-selected`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedReply {
    /// The packages selected: `packages` themselves, or none when the build is of the whole
    /// workspace.
    pub packages: Vec<String>,
}

impl HostCall for Selected {
    const VERB: &'static str = "host-selected";
    const SINCE: u32 = 2;
    type Reply = SelectedReply;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// `host-step`: what the verb is doing now, as a whole, so that one call after a restart puts
/// it all back. `ignite wait`, `status` and the client's progress lines read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepUpdate {
    pub handle: Handle,
    /// The current step, such as `rebasing onto staging 1a2b3c4`.
    pub step: Option<String>,
    /// What the verb waits for, outermost first; empty when it waits for nothing.
    pub waiting: Vec<String>,
    pub class: Option<Class>,
    pub ticket: Option<TicketId>,
}

/// Whether someone has asked the verb to stop.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopState {
    pub stop: bool,
    /// The instant the daemon that answered started. A verb process that sees it change knows
    /// the daemon restarted and sends `host-hello` and its current step at once, instead of
    /// waiting for a call to fail: a verb that waits on a lock or runs one long command makes no
    /// call that a restart would cut off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_started: Option<String>,
}

impl HostCall for StepUpdate {
    const VERB: &'static str = "host-step";
    const SINCE: u32 = 1;
    type Reply = StopState;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// `host-refuse-stop`: refuses every later request to stop the verb, with this reason, as for
/// a landing whose push begins.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefuseStop {
    pub handle: Handle,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefuseStopReply {
    /// False when a stop was already asked: the verb gives up instead of starting what may not
    /// be interrupted. A repeated call gives the same answer.
    pub refused: bool,
}

impl HostCall for RefuseStop {
    const VERB: &'static str = "host-refuse-stop";
    const SINCE: u32 = 1;
    type Reply = RefuseStopReply;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// `host-allow-stop`: lifts the refusal, as when the remote rejected a landing's push.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowStop {
    pub handle: Handle,
}

impl HostCall for AllowStop {
    const VERB: &'static str = "host-allow-stop";
    const SINCE: u32 = 1;
    type Reply = Ack;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// `host-poll`: asks whether someone has asked the verb to stop. The verb process polls about
/// once a second.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Poll {
    pub handle: Handle,
}

impl HostCall for Poll {
    const VERB: &'static str = "host-poll";
    const SINCE: u32 = 1;
    type Reply = StopState;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// `host-finished`: the verb process wrote its reply into the handle's row in `daemon.db` and
/// is about to exit; the daemon wakes whoever waits on the handle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finished {
    pub handle: Handle,
}

impl HostCall for Finished {
    const VERB: &'static str = "host-finished";
    const SINCE: u32 = 1;
    type Reply = Ack;
    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// A landing takes a fresh admission snapshot immediately before reporting to the hub,
/// including when an earlier attempt was interrupted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadDispatchPressure {
    pub handle: Handle,
}

impl HostCall for ReadDispatchPressure {
    const VERB: &'static str = "host-dispatch-pressure";
    const SINCE: u32 = 3;
    type Reply = Option<crate::dispatch_pressure::DispatchPressure>;

    fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// How a call failed, as the reply's `json` carries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostError {
    pub kind: ErrorKind,
    pub message: String,
}

/// The kinds of [`Error`], on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Unreachable,
    Conflict,
    NotFound,
    Invalid,
    Io,
}

impl From<&Error> for HostError {
    fn from(error: &Error) -> HostError {
        let (kind, message) = match error {
            Error::Unreachable(m) => (ErrorKind::Unreachable, m),
            Error::Conflict(m) => (ErrorKind::Conflict, m),
            Error::NotFound(m) => (ErrorKind::NotFound, m),
            Error::Invalid(m) => (ErrorKind::Invalid, m),
            Error::Io(m) => (ErrorKind::Io, m),
        };
        HostError {
            kind,
            message: message.clone(),
        }
    }
}

impl From<HostError> for Error {
    fn from(error: HostError) -> Error {
        let message = error.message;
        match error.kind {
            ErrorKind::Unreachable => Error::Unreachable(message),
            ErrorKind::Conflict => Error::Conflict(message),
            ErrorKind::NotFound => Error::NotFound(message),
            ErrorKind::Invalid => Error::Invalid(message),
            ErrorKind::Io => Error::Io(message),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ErrorBody {
    error: HostError,
}

/// The request line for `call`, sent from the verb process's working directory `cwd`.
pub fn encode_request<C: HostCall>(call: &C, cwd: PathBuf) -> Request {
    Request {
        verb: C::VERB.to_owned(),
        args: vec![serde_json::to_string(call).expect("host calls serialise")],
        cwd,
        session: None,
        wait_s: 0,
        pid: None,
    }
}

/// The daemon's side: the call a request line carries.
pub fn decode_request<C: HostCall>(request: &Request) -> Result<C> {
    if request.verb != C::VERB {
        return Err(Error::Invalid(format!(
            "{} is not {}",
            request.verb,
            C::VERB
        )));
    }
    let [body] = request.args.as_slice() else {
        return Err(Error::Invalid(format!(
            "{} takes one JSON argument",
            C::VERB
        )));
    };
    serde_json::from_str(body).map_err(|e| Error::Invalid(format!("{}: {e}", C::VERB)))
}

/// The daemon's side: the reply line for the answer to a call.
pub fn encode_reply<R: Serialize>(answer: &Result<R>) -> Reply {
    match answer {
        Ok(reply) => Reply {
            exit: Exit::Success,
            text: String::new(),
            json: Some(serde_json::to_value(reply).expect("host replies serialise")),
            handle: None,
        },
        Err(error) => {
            let exit = match error {
                Error::Unreachable(_) => Exit::Unreachable,
                Error::Conflict(_) => Exit::Conflict,
                Error::NotFound(_) | Error::Invalid(_) | Error::Io(_) => Exit::Usage,
            };
            Reply {
                exit,
                text: error.to_string(),
                json: Some(
                    serde_json::to_value(ErrorBody {
                        error: error.into(),
                    })
                    .expect("host errors serialise"),
                ),
                handle: None,
            }
        }
    }
}

/// The verb process's side: the answer a reply line carries for a call of type `C`.
pub fn decode_reply<C: HostCall>(reply: Reply) -> Result<C::Reply> {
    if reply.exit != Exit::Success {
        if let Some(body) = reply
            .json
            .and_then(|json| serde_json::from_value::<ErrorBody>(json).ok())
        {
            return Err(body.error.into());
        }
        return Err(Error::Invalid(format!("{}: {}", C::VERB, reply.text)));
    }
    let json = reply
        .json
        .ok_or_else(|| Error::Invalid(format!("{} replied without a body", C::VERB)))?;
    serde_json::from_value(json).map_err(|e| Error::Invalid(format!("{} reply: {e}", C::VERB)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle() -> Handle {
        Handle::new("L-6").unwrap()
    }

    #[test]
    fn request_ids_round_trip_and_tell_handles_apart() {
        let ids = RequestIds::new(handle());
        assert_eq!(ids.next().to_string(), "L-6#1");
        let second = ids.next();
        assert_eq!(second.to_string().parse::<RequestId>().unwrap(), second);
        assert!(RequestId::of_handle("L-6#2", &handle()));
        assert!(!RequestId::of_handle("L-60#2", &handle()));
        assert!(!RequestId::of_handle("L-6", &handle()));
        assert!("L-6#x".parse::<RequestId>().is_err());
    }

    #[test]
    fn a_call_and_its_answer_cross_the_envelope() {
        let poll = Poll { handle: handle() };
        let request = encode_request(&poll, PathBuf::from("/trees/tree-07"));
        assert_eq!(request.verb, "host-poll");
        assert_eq!(decode_request::<Poll>(&request).unwrap(), poll);
        assert!(decode_request::<Finished>(&request).is_err());
        let reply = encode_reply(&Ok(StopState {
            stop: true,
            daemon_started: None,
        }));
        assert_eq!(
            decode_reply::<Poll>(reply).unwrap(),
            StopState {
                stop: true,
                daemon_started: None
            }
        );
    }

    #[test]
    fn errors_keep_their_kind_and_an_unknown_call_is_invalid() {
        let reply = encode_reply::<Ack>(&Err(Error::Conflict("tree-02 is busy".to_owned())));
        assert_eq!(reply.exit, Exit::Conflict);
        assert_eq!(
            decode_reply::<Finished>(reply),
            Err(Error::Conflict("tree-02 is busy".to_owned()))
        );
        let unknown = Reply::text(Exit::Usage, "failed   unknown verb host-run2");
        assert!(matches!(
            decode_reply::<Run>(unknown),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn calls_are_usable_only_when_both_sides_know_them() {
        assert!(CALLS.iter().all(|(_, since)| *since <= PROTOCOL_VERSION));
        assert!(usable(1, 1, 1));
        assert!(usable(BorrowCores::SINCE, 2, 2));
        assert!(!usable(BorrowCores::SINCE, 2, 1));
        assert!(!usable(2, 2, 1));
        assert!(!usable(2, 1, 2));
    }

    #[test]
    fn build_answers_convert_to_and_from_build_results() {
        let failed = BuildResult::Failed {
            output: PathBuf::from("/runs/b/output.log"),
            usage: Usage::default(),
        };
        assert_eq!(BuildReply::from(failed.clone()).into_result(), Some(failed));
        let waiting = BuildReply::Waiting {
            what: "4 cores".to_owned(),
        };
        assert_eq!(waiting.into_result(), None);
    }
}
