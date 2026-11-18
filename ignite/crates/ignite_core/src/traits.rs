//! The traits that keep Ignite's parts apart.
//!
//! Each part that needs the clock, a command runner, the builds, git or the hub takes one of
//! these traits, so it can be tested against the in-memory fakes in [`crate::fakes`] before the
//! real implementation exists.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::ids::{Commit, Oid, Seq, TicketId, TreeId};
use crate::protocol::Class;
use crate::protocol::Usage;
use crate::protocol::hub::{
    CachedResult, Claim, Landed, MachineFree, NewResult, Record, Reservation, ResultKey,
    TicketBrief,
};
use crate::protocol::tickets::{FeedBatch, LeaseOp, LeaseReply};
use crate::time::Timestamp;

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
    pub tree: Option<TreeId>,
    pub ticket: Option<TicketId>,
}

/// How a supervised command ended.
#[derive(Clone, Debug, PartialEq)]
pub struct RunResult {
    /// The exit code; `None` if a signal ended it.
    pub exit: Option<i32>,
    /// The file that holds its whole output.
    pub output: PathBuf,
    pub usage: Usage,
}

pub trait Launcher: Send + Sync {
    /// Runs one command under a supervisor and waits for it.
    fn run(&self, spec: &CommandSpec) -> RunResult;
}

/// How a request for a build product was answered.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildResult {
    /// The tree already held the product in this state; nothing ran.
    Current,
    /// A build ran (or was joined) and succeeded.
    Built { usage: Usage },
    /// The build failed; its output is in the file named.
    Failed { output: PathBuf, usage: Usage },
}

pub trait Builds: Send + Sync {
    /// Makes `product` current in `tree`, joining a build already running.
    fn ensure(&self, tree: &TreeId, product: &str, packages: &[String]) -> BuildResult;
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
}
