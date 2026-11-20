//! `host-run`: one command per request ID, however often the call is repeated and however many
//! times the daemon restarts in between.
//!
//! A new request ID gets a thread of its own that waits for the grant and starts the command
//! ([`RunStarter::begin`]); the call waits for that thread at most
//! [`Timing::block`](super::Timing::block) and answers `waiting` if it has not started yet. The
//! request stays queued meanwhile, and the next call with the same ID waits for the same thread.
//! A request whose verb process stops asking for [`Timing::abandon`](super::Timing::abandon) is
//! withdrawn, so a dead process holds no place in the queue; its next call, if it comes, queues
//! it again at the moment it first asked.
//!
//! A request ID this daemon has not seen may belong to a command an earlier daemon started:
//! [`RunStarter::find`] looks for the run directory whose `spec.json` names it before anything
//! is started.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use ignite_core::ids::Handle;
use ignite_core::protocol::Class;
use ignite_core::protocol::host::{REQUEST_VARIABLE, RequestId, Run, RunReply};
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Error, Result};

use super::{Host, Pending, RunStarter};
use crate::adopt::{HANDLE_VARIABLE, TREE_COMMIT_VARIABLE};
use crate::handles::lock;
use crate::sched::{GrantRequest, Scheduler};
use crate::server::Context;
use crate::steps::Activity;
use crate::supervise::{Run as RunDir, SupervisedLauncher};
use crate::trees::head;

impl Host {
    pub(super) fn run(self: &Arc<Self>, ctx: &Context, call: &Run) -> Result<RunReply> {
        if call.request_id.handle() != &call.handle {
            return Err(Error::Invalid(format!(
                "request {} is not one of {}",
                call.request_id, call.handle
            )));
        }
        let pending = match self.pending_run(ctx, call) {
            Ok(pending) => pending,
            Err(answer) => return Ok(answer),
        };
        Ok(pending
            .wait(self.timing.block)
            .unwrap_or_else(|| RunReply::Waiting {
                what: self
                    .starter
                    .waiting_for(&CommandSpec::from(call.spec.clone())),
            }))
    }

    /// The thread working on `call`'s request ID: the one already started, or a new one. The
    /// answer itself when no thread is needed: the run an earlier daemon started, or a refusal.
    fn pending_run(
        self: &Arc<Self>,
        ctx: &Context,
        call: &Run,
    ) -> std::result::Result<Arc<Pending<RunReply>>, RunReply> {
        let mut runs = lock(&self.runs);
        runs.retain(|_, pending| !pending.stale());
        if let Some(pending) = runs.get(&call.request_id) {
            return Ok(pending.clone());
        }
        if let Some(run) = self.starter.find(&call.request_id) {
            return Err(RunReply::Started { run });
        }
        let Some(activity) = self.remote(ctx, &call.handle) else {
            return Err(RunReply::Refused {
                reason: format!("{} is not running", call.handle),
            });
        };
        if activity.stop_requested() {
            return Err(RunReply::Refused {
                reason: stopped(&call.handle),
            });
        }
        let pending = Pending::new(Instant::now());
        runs.insert(call.request_id.clone(), pending.clone());
        self.start(call, activity, pending.clone());
        Ok(pending)
    }

    /// Starts the thread that waits for the call's grant and starts its command.
    fn start(
        self: &Arc<Self>,
        call: &Run,
        activity: Arc<Activity>,
        pending: Arc<Pending<RunReply>>,
    ) {
        let mut spec = CommandSpec::from(call.spec.clone());
        let id = call.request_id.clone();
        tag(&mut spec, &id);
        let since = call.first_asked_at;
        let host = Arc::clone(self);
        let thread = format!("ignite-host-run-{id}");
        let spawned = std::thread::Builder::new().name(thread).spawn(move || {
            let abandon = host.timing.abandon;
            let cancelled = || {
                activity.stop_requested() || activity.is_finished() || pending.abandoned(abandon)
            };
            let reply = match host.starter.begin(&spec, since, &cancelled) {
                Ok(Some(run)) => RunReply::Started { run },
                Ok(None) if activity.stop_requested() || activity.is_finished() => {
                    RunReply::Refused {
                        reason: stopped(id.handle()),
                    }
                }
                Ok(None) => {
                    // Nobody asked for it for a long while; a later call queues it again.
                    lock(&host.runs).remove(&id);
                    return;
                }
                Err(error) => RunReply::Refused {
                    reason: format!("its supervisor could not be started: {error}"),
                },
            };
            pending.settle(reply);
        });
        if let Err(error) = spawned {
            lock(&self.runs).remove(&call.request_id);
            eprintln!(
                "ignite: cannot start a thread for {}: {error}",
                call.request_id
            );
        }
    }
}

fn stopped(handle: &Handle) -> String {
    format!("a stop was asked for {handle}")
}

/// Adds to the command's environment the request ID, by which a repeated call and `ignite stop`
/// find its run, and the handle and the commit its checkout is at, by which a verb run again
/// after its process died can take the command over (see `crate::adopt`).
fn tag(spec: &mut CommandSpec, id: &RequestId) {
    spec.env.retain(|(name, _)| {
        ![REQUEST_VARIABLE, HANDLE_VARIABLE, TREE_COMMIT_VARIABLE].contains(&name.as_str())
    });
    spec.env.push((REQUEST_VARIABLE.to_owned(), id.to_string()));
    spec.env
        .push((HANDLE_VARIABLE.to_owned(), id.handle().to_string()));
    if let Some(at) = head::commit_at(&spec.cwd) {
        spec.env
            .push((TREE_COMMIT_VARIABLE.to_owned(), at.to_string()));
    }
}

/// [`RunStarter`] over the daemon's supervised launcher.
pub struct SupervisedStarter {
    launcher: Arc<SupervisedLauncher>,
    scheduler: Option<Arc<Scheduler>>,
    /// The run directory of each request ID, read from the runs folder at the first lookup and
    /// kept up to date with the runs this daemon starts.
    index: Mutex<Option<HashMap<String, PathBuf>>>,
}

impl SupervisedStarter {
    pub fn new(launcher: Arc<SupervisedLauncher>, scheduler: Option<Arc<Scheduler>>) -> Self {
        SupervisedStarter {
            launcher,
            scheduler,
            index: Mutex::new(None),
        }
    }
}

impl RunStarter for SupervisedStarter {
    fn begin(
        &self,
        spec: &CommandSpec,
        since: Timestamp,
        cancelled: &dyn Fn() -> bool,
    ) -> std::io::Result<Option<PathBuf>> {
        let started = self.launcher.begin(spec, since, cancelled)?;
        if let (Some(run), Some(id)) = (&started, request_variable(spec))
            && let Some(index) = lock(&self.index).as_mut()
        {
            index.insert(id.to_owned(), run.clone());
        }
        Ok(started)
    }

    fn find(&self, request: &RequestId) -> Option<PathBuf> {
        let mut index = lock(&self.index);
        let index = index.get_or_insert_with(|| self.launcher.requests().into_iter().collect());
        index.get(&request.to_string()).cloned()
    }

    fn runs_of(&self, handle: &Handle) -> Vec<PathBuf> {
        self.launcher
            .unfinished_of(handle)
            .into_iter()
            .map(|run| run.dir().to_path_buf())
            .collect()
    }

    fn end(&self, run: &Path) {
        let _ = RunDir::at(run.to_path_buf()).end();
    }

    fn kill(&self, run: &Path) {
        let _ = RunDir::at(run.to_path_buf()).kill();
    }

    fn waiting_for(&self, spec: &CommandSpec) -> String {
        let request = GrantRequest {
            class: spec.class,
            cores: spec.cores,
            resources: spec.resources.clone(),
            memory_mb: None,
            shrinks: false,
            grows: spec.grows,
            cores_file: None,
            reserve: spec.reserve,
        };
        match &self.scheduler {
            Some(scheduler) => scheduler.describe(&request),
            None => format!("{} cores", spec.cores.max(1)),
        }
    }

    fn borrow_cores(&self, cores: u32) -> Option<(u64, u32)> {
        let grant = self.scheduler.as_ref()?.try_acquire(&GrantRequest {
            class: Class::Land,
            cores,
            resources: Vec::new(),
            memory_mb: None,
            shrinks: false,
            grows: false,
            cores_file: None,
            reserve: None,
        })?;
        Some((grant.id, grant.cores))
    }

    fn return_cores(&self, grant: u64) {
        if let Some(scheduler) = &self.scheduler {
            scheduler.release_id(grant);
        }
    }
}

fn request_variable(spec: &CommandSpec) -> Option<&str> {
    spec.env
        .iter()
        .find(|(name, _)| name == REQUEST_VARIABLE)
        .map(|(_, value)| value.as_str())
}
