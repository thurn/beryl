//! The parts of a verb's [`Context`](crate::server::Context) that need one authority on the
//! machine, as a verb process has them: commands, builds and landing trees, each asked of the
//! daemon through the host protocol.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use ignite_core::ids::{Commit, TicketId, TreeId};
use ignite_core::protocol::Class;
use ignite_core::protocol::Usage;
use ignite_core::protocol::host::{
    BorrowCores, Build, BuildReply, Command, LeaseLanding, LeaseLandingReply, ReleaseLanding,
    RequestIds, ReturnCores, Run, RunReply, Selected,
};
use ignite_core::traits::{
    BuildFor, BuildResult, Builds, Clock, CommandSpec, Launcher, Result, RunResult,
};

use super::link::Link;
use crate::land::env::{GateTree, LandTrees, MachineTrees, TicketTree};
use crate::land::express::{CoreLender, Loan};
use crate::steps;
use crate::supervise::Run as RunDir;

/// How often a command's wait looks whether it should be ended.
const CANCEL_POLL: Duration = Duration::from_millis(250);

/// What the remote parts share: the link, the request IDs of `host-run` and `host-build`, the
/// clock that dates each request's first ask, and the folder for notes on calls that failed.
pub struct Remote {
    pub link: Arc<Link>,
    pub ids: Arc<RequestIds>,
    pub clock: Arc<dyn Clock>,
    /// `verbs/<handle>` in the Ignite home.
    pub notes: PathBuf,
}

impl Remote {
    /// Writes `text` into a note named after `name` and returns its path, for a result whose
    /// output must name a file.
    fn note(&self, name: &str, text: &str) -> PathBuf {
        let path = self.notes.join(format!("{name}.log"));
        if let Err(error) = std::fs::write(&path, text) {
            eprintln!("ignite: cannot write {}: {error}", path.display());
        }
        path
    }
}

/// The waits a call shows in the verb's step while the daemon answers `waiting`: one at a
/// time, replaced when what it waits for changes.
#[derive(Default)]
struct ShownWait {
    shown: Option<(String, steps::Waiting)>,
}

impl ShownWait {
    fn show(&mut self, what: String) {
        if self.shown.as_ref().is_none_or(|(old, _)| *old != what) {
            self.shown = None;
            let guard = steps::waiting(&what);
            self.shown = Some((what, guard));
        }
    }
}

/// [`Launcher`] that starts each command through the daemon (`host-run`) and then waits for it
/// here: a shared lock on the run's `lock` and its `result.json`, as for an adopted run.
pub struct RemoteLauncher(pub Arc<Remote>);

impl RemoteLauncher {
    fn failed(&self, request: &str, why: &str) -> RunResult {
        RunResult {
            exit: None,
            output: self.0.note(&format!("run-{request}"), why),
            usage: Usage::default(),
            hung: false,
        }
    }
}

impl Launcher for RemoteLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        self.run_until(spec, &|| false)
    }

    fn run_until(&self, spec: &CommandSpec, cancelled: &(dyn Fn() -> bool + Sync)) -> RunResult {
        let remote = &self.0;
        let call = Run {
            handle: remote.link.handle().clone(),
            request_id: remote.ids.next(),
            first_asked_at: remote.clock.now(),
            spec: Command::from(spec),
        };
        let counter = call.request_id.counter().to_string();
        let mut wait = ShownWait::default();
        let run = loop {
            if steps::stop_requested() || cancelled() {
                return self.failed(&counter, "the command was not started: it was cancelled");
            }
            match remote.link.run(&call) {
                Ok(RunReply::Started { run }) => break run,
                Ok(RunReply::Waiting { what }) => wait.show(what),
                Ok(RunReply::Refused { reason }) => {
                    return self.failed(&counter, &format!("the daemon refused it: {reason}"));
                }
                Err(error) => {
                    return self.failed(&counter, &format!("cannot reach the daemon: {error}"));
                }
            }
        };
        drop(wait);
        wait_for(&RunDir::at(run), &|| steps::stop_requested() || cancelled())
    }
}

/// Waits for the command in `run`, ending it once `stop` turns true. The daemon also ends the
/// commands of a verb that was asked to stop.
fn wait_for(run: &RunDir, stop: &dyn Fn() -> bool) -> RunResult {
    let (done, result) = mpsc::channel();
    let waited = run.clone();
    std::thread::spawn(move || {
        let _ = done.send(waited.wait());
    });
    let mut ended = false;
    loop {
        match result.recv_timeout(CANCEL_POLL) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !ended && stop() {
                    let _ = run.end();
                    ended = true;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return RunResult {
                    exit: None,
                    output: run.dir().join("output.log"),
                    usage: Usage::default(),
                    hung: false,
                };
            }
        }
    }
}

/// Whether a tree has built a product before (see [`Builds::has_built`]).
pub type HasBuilt = Arc<dyn Fn(&TreeId, &str) -> bool + Send + Sync>;

/// [`Builds`] that makes products current through the daemon's build manager (`host-build`).
pub struct RemoteBuilds {
    pub remote: Arc<Remote>,
    /// Whether a tree has built a product before (see [`Builds::has_built`]), answered from
    /// what the daemon records and what is on disk.
    pub has_built: HasBuilt,
}

impl Builds for RemoteBuilds {
    fn ensure(&self, tree: &TreeId, product: &str, packages: &[String]) -> BuildResult {
        let who = BuildFor {
            class: steps::class().unwrap_or(Class::Agent),
            ticket: steps::ticket(),
            session: steps::session(),
        };
        self.ensure_for(tree, product, packages, &who)
    }

    fn ensure_for(
        &self,
        tree: &TreeId,
        product: &str,
        packages: &[String],
        who: &BuildFor,
    ) -> BuildResult {
        let remote = &self.remote;
        let call = Build {
            handle: remote.link.handle().clone(),
            request_id: remote.ids.next(),
            first_asked_at: remote.clock.now(),
            tree: tree.clone(),
            product: product.to_owned(),
            packages: packages.to_vec(),
            who: who.into(),
        };
        let mut wait = ShownWait::default();
        loop {
            match remote.link.build(&call) {
                Ok(BuildReply::Waiting { what }) => wait.show(what),
                Ok(reply) => {
                    return reply
                        .into_result()
                        .expect("only a waiting build has no result");
                }
                Err(error) => {
                    let name = format!("build-{}", call.request_id.counter());
                    let why = format!("cannot build {product} in {tree}: {error}");
                    return BuildResult::Failed {
                        output: remote.note(&name, &why),
                        usage: Usage::default(),
                    };
                }
            }
        }
    }

    /// Asks the daemon (`host-selected`), whose build manager compares the subset's features with
    /// the whole workspace's, so that a listing or a run selects what the daemon's build built.
    /// When the daemon cannot be reached or does not know the call, the packages asked for are
    /// selected, as for any [`Builds`] that cannot tell.
    fn selected(&self, tree: &TreeId, product: &str, packages: &[String]) -> Vec<String> {
        let call = Selected {
            handle: self.remote.link.handle().clone(),
            tree: tree.clone(),
            product: product.to_owned(),
            packages: packages.to_vec(),
        };
        match self.remote.link.selected(&call) {
            Ok(reply) => reply.packages,
            Err(_) => packages.to_vec(),
        }
    }

    fn has_built(&self, tree: &TreeId, product: &str) -> bool {
        (self.has_built)(tree, product)
    }
}

/// [`LandTrees`] for a verb process: landing trees are leased and freed through the daemon
/// (`host-lease-landing`, `host-release-landing`), owned by the verb's handle; everything else
/// reads and acts on the machine's trees as the daemon would.
pub struct RemoteLandTrees {
    pub remote: Arc<Remote>,
    pub local: MachineTrees,
}

impl LandTrees for RemoteLandTrees {
    fn ticket_tree(&self, tree: &TreeId) -> Result<Option<TicketTree>> {
        self.local.ticket_tree(tree)
    }

    fn branch(&self, path: &Path) -> Result<Option<String>> {
        self.local.branch(path)
    }

    fn uncommitted(&self, path: &Path) -> Result<Vec<String>> {
        self.local.uncommitted(path)
    }

    fn commits(&self, path: &Path, from: &Commit, to: &Commit) -> Result<Vec<Commit>> {
        self.local.commits(path, from, to)
    }

    fn subjects(&self, path: &Path, from: &Commit, to: &Commit) -> Result<Vec<String>> {
        self.local.subjects(path, from, to)
    }

    fn is_ancestor(&self, path: &Path, ancestor: &Commit, descendant: &Commit) -> Result<bool> {
        self.local.is_ancestor(path, ancestor, descendant)
    }

    fn lease_landing(&self, holder: &str, at: &Commit) -> Result<Option<GateTree>> {
        let call = LeaseLanding {
            handle: self.remote.link.handle().clone(),
            holder: holder.to_owned(),
            commit: at.clone(),
            keep_free: 0,
        };
        Ok(match self.remote.link.lease_landing(&call)? {
            LeaseLandingReply::Leased { tree, path, .. } => Some(GateTree { tree, path }),
            LeaseLandingReply::Busy => None,
        })
    }

    fn release_landing(&self, tree: &TreeId, built_at: Option<&Commit>) -> Result<()> {
        let call = ReleaseLanding {
            handle: self.remote.link.handle().clone(),
            tree: tree.clone(),
            built: built_at.cloned(),
        };
        self.remote.link.release_landing(&call).map(|_| ())
    }

    fn ticket_builds_less(
        &self,
        ticket: &TreeId,
        built: Option<&Commit>,
        head: &Commit,
        change: &[String],
    ) -> Result<bool> {
        self.local.ticket_builds_less(ticket, built, head, change)
    }

    fn release_ticket(&self, tree: &TreeId, park_at: &Commit, caller: Option<u32>) -> Result<()> {
        self.local.release_ticket(tree, park_at, caller)
    }

    fn delete_saved(&self, ticket: &TicketId) -> Result<()> {
        self.local.delete_saved(ticket)
    }
}

/// [`CoreLender`] over the daemon's `host-borrow-cores` and `host-return-cores`. A daemon that
/// does not know the calls, or cannot be reached, lends nothing, and the express run keeps its
/// reserved cores.
pub struct RemoteLender(pub Arc<Remote>);

impl CoreLender for RemoteLender {
    fn borrow(&self, wanted: u32) -> Option<Loan> {
        let call = BorrowCores {
            handle: self.0.link.handle().clone(),
            cores: wanted,
        };
        let reply = self.0.link.borrow_cores(&call).ok()?;
        Some(Loan {
            grant: reply.grant?,
            cores: reply.cores,
        })
    }

    fn give_back(&self, loan: &Loan) {
        let call = ReturnCores {
            handle: self.0.link.handle().clone(),
            grant: loan.grant,
        };
        let _ = self.0.link.return_cores(&call);
    }
}
