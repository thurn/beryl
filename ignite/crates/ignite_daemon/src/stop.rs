//! `ignite stop <handle> [--force]`: ends a running command of the asking session's own.
//!
//! The command's work is asked to stop through its activity (see [`crate::steps`]). Its
//! supervised commands are ended the way those of a caller that left are, a command waiting for
//! cores withdraws its request, and nothing new starts. The verb then unwinds as from a failure,
//! which gives back its cores, resources, landing tree and lease, and its handle ends `stopped`
//! with exit code 3. A landing whose push has begun refuses to stop, with the reason, because a
//! push cut short may or may not have reached the remote.
//!
//! `ignite close` and `ignite release` stop the running commands of the tree they free in the
//! same way (see [`crate::trees`]).
//!
//! A verb that runs in a process of its own (see [`crate::host`]) has an activity in the same
//! registry, fed by its calls, so it is stopped, and refuses to stop, in the same way. Its
//! commands are not on a daemon thread, so the daemon ends them itself once the stop is asked:
//! every unfinished run whose request ID names the handle. The process sees the stop on its next
//! `host-poll`, and the handle ends `stopped` when it has written its reply. A verb process that
//! agreed to stop and is still running after [`STOP_WAIT`] never looks at its stop where it is,
//! as in a loop on the CPU, and is ended with its process group as `--force` ends one
//! ([`EndVerbProcess`]). Its stop was agreed, so its push has not begun, and ending it leaves
//! nothing half done.
//!
//! `--force` is for an emergency, such as a tip round started by a daemon of an older release
//! that hangs and holds `master` back. It stops a command of any session's, or of none, such as
//! an internal verb a loop of the daemon started, and only the operator (a request that names no
//! session, from a terminal) or the holder of an in-progress incident or justiciar ticket may
//! give it. It asks the command to stop as above, refused or not, and a verb process that
//! has not finished within [`FORCE_WAIT`] is ended with its whole process group, `SIGTERM` and
//! then `SIGKILL` ([`EndVerbProcess`]). The daemon then finds the process dead, ends its
//! leftover commands and its handle, and frees its landing trees only once the process has
//! exited (see [`crate::trees::owner_alive`]).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use ignite_core::ids::Handle;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::emergency::{EMERGENCY_LABELS, has_emergency_label};
use ignite_core::protocol::tickets::{Status, TicketQuery};
use ignite_store::local::handles as handle_store;

use crate::handles::{Handles, Waited};
use crate::server::{Context, Verb, still_running_at};
use crate::steps::Steps;

/// How long `ignite stop` waits for the command to end. Its processes get `SIGTERM`, then
/// `SIGKILL` ten seconds later, and a request waiting for cores is withdrawn within a second, so
/// this is only reached by a verb that does not look at its stop between commands. A verb
/// process still running then is ended, and the stop waits as long again for the daemon to see
/// it finish. A daemon restarted into a new build waits as long for a landing it asked to stop
/// (see `server/outdated.rs`).
pub const STOP_WAIT: Duration = Duration::from_secs(60);

/// How long `ignite stop --force` waits for a verb process to stop when asked before it ends
/// the process.
pub const FORCE_WAIT: Duration = Duration::from_secs(10);

/// How long a forcibly ended verb process has after `SIGTERM` before `SIGKILL`.
const FORCE_GRACE: Duration = Duration::from_secs(5);

/// Ends a verb process that did not stop when asked, for `ignite stop --force`.
pub trait EndVerbProcess: Send + Sync {
    /// Ends the verb process that runs `handle` and every process in its process group, and
    /// waits until it has exited. `Ok` says what was ended, or `None` when no verb process runs
    /// the handle; `Err` says why it could not be ended.
    fn end(&self, handle: &Handle) -> Result<Option<String>, String>;
}

pub struct StopVerb {
    handles: Arc<Handles>,
    steps: Arc<Steps>,
    wait: Duration,
    force_wait: Duration,
    processes: Option<Arc<dyn EndVerbProcess>>,
}

impl StopVerb {
    pub fn new(handles: Arc<Handles>, steps: Arc<Steps>) -> StopVerb {
        StopVerb {
            handles,
            steps,
            wait: STOP_WAIT,
            force_wait: FORCE_WAIT,
            processes: None,
        }
    }

    /// The verb with shorter waits for the command to end, for tests.
    #[must_use]
    pub fn with_wait(mut self, wait: Duration) -> StopVerb {
        self.wait = wait;
        self.force_wait = self.force_wait.min(wait);
        self
    }

    /// The verb ending, with `processes`, the verb processes that `--force` finds still running.
    #[must_use]
    pub fn ending_processes_with(mut self, processes: Arc<dyn EndVerbProcess>) -> StopVerb {
        self.processes = Some(processes);
        self
    }

    /// Whether the asker may give `--force` (see [`emergency_or_operator`]).
    fn may_force(ctx: &Context) -> Result<(), String> {
        emergency_or_operator(ctx)
    }
}

/// Whether the asker may take work away from other sessions: the operator, whose request names
/// no session, or a session that holds an in-progress incident or justiciar ticket.
pub(crate) fn emergency_or_operator(ctx: &Context) -> Result<(), String> {
    let Some(session) = &ctx.session else {
        return Ok(());
    };
    let query = TicketQuery {
        status: Some(Status::InProgress),
        assignee: Some(session.to_string()),
        label_any: EMERGENCY_LABELS.map(str::to_owned).to_vec(),
        ..TicketQuery::default()
    };
    match ctx.hub.tickets(&query) {
        Ok(held)
            if held.iter().any(|ticket| {
                ticket.session.as_ref() == Some(session) && has_emergency_label(&ticket.labels)
            }) =>
        {
            Ok(())
        }
        Ok(_) => Err(format!(
            "session {session} holds no in-progress incident or justiciar ticket"
        )),
        Err(error) => Err(format!(
            "the hub could not check the tickets held by {session}: {error}"
        )),
    }
}

impl Verb for StopVerb {
    fn name(&self) -> &'static str {
        "stop"
    }

    /// It waits at most [`STOP_WAIT`] by itself.
    fn keeps_its_own_wait(&self) -> bool {
        true
    }

    /// The session is checked against the command's own instead.
    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn blocks_update(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let force = req.args.iter().any(|arg| arg == "--force");
        let names: Vec<&String> = req.args.iter().filter(|arg| *arg != "--force").collect();
        let [name] = names.as_slice() else {
            return Reply::text(Exit::Usage, "usage    ignite stop <handle> [--force]");
        };
        let Ok(handle) = Handle::new(name.as_str()) else {
            return Reply::text(Exit::Usage, format!("unknown handle: {name}"));
        };
        let Some(activity) = self.steps.by_handle(&handle) else {
            return match self.handles.wait(&handle, 0, None) {
                Waited::Unknown => Reply::text(Exit::Usage, format!("unknown handle: {name}")),
                _ => Reply::text(
                    Exit::Success,
                    format!(
                        "finished {handle} is not running; nothing to stop\nnext     ignite wait \
                         {handle} prints its reply"
                    ),
                ),
            };
        };
        let owner = activity.work().session;
        let own = owner.is_some() && owner == ctx.session;
        if !own && !force {
            let holder = owner.map_or("no session".to_owned(), |s| format!("session {s}"));
            return Reply::text(
                Exit::Conflict,
                format!(
                    "conflict {handle} belongs to {holder}; a session stops only its own \
                     commands\nnext     ignite tell that session to stop it; in an emergency \
                     an incident commander, a justiciar or the operator can run `ignite stop {handle} --force`"
                ),
            );
        }
        if force && let Err(reason) = Self::may_force(ctx) {
            return Reply::text(
                Exit::Conflict,
                format!(
                    "refused  `ignite stop --force` is for incident commanders, justiciars and the operator: \
                     {reason}\nnext     ignite justiciar asks for one"
                ),
            );
        }
        if force {
            return self.force(&handle, &activity);
        }
        self.stop_activity(&handle, &activity)
    }
}

impl StopVerb {
    /// Stops work after the caller has established ownership. The activity may still refuse
    /// the stop while it is publishing a change.
    pub(crate) fn stop_activity(
        &self,
        handle: &Handle,
        activity: &crate::steps::Activity,
    ) -> Reply {
        if let Err(reason) = activity.request_stop() {
            return Reply::text(
                Exit::Conflict,
                format!(
                    "refused  {handle} cannot be stopped: {reason}\nnext     ignite wait {handle}"
                ),
            );
        }
        let mut ended = None;
        if !activity.wait_finished(self.wait) {
            ended = self.end_unstopped(handle);
            if ended.is_none() || !activity.wait_finished(self.wait) {
                let view = activity.view();
                let mut reply = still_running_at(&view.name, handle, Some(&view));
                let what = if ended.is_some() {
                    "its verb process was ended after it did not stop, and the daemon has not \
                     seen it finish"
                } else {
                    "its processes were told to end, and it has not finished"
                };
                reply.text = format!(
                    "stopping {handle}: {what} within {} s\n{}",
                    self.wait.as_secs(),
                    reply.text
                );
                return reply;
            }
        }
        let verb = match activity.kind() {
            crate::steps::Kind::Verb(verb) | crate::steps::Kind::Loop(verb) => verb.clone(),
        };
        let ended = ended.map_or_else(String::new, |ended| {
            format!(
                "\nended    {ended}: it had not stopped within {} s of being asked",
                self.wait.as_secs()
            )
        });
        let text = format!(
            "stopped  `ignite {verb}` ({handle}); its processes were ended and its cores, trees \
             and lease given back{ended}"
        );
        Reply {
            exit: Exit::Success,
            text,
            json: Some(serde_json::json!({ "handle": handle, "verb": verb, "stopped": true })),
            handle: None,
        }
    }

    /// Ends the verb process of `handle`, which agreed to stop and has not, and says what was
    /// ended. `None` when no verb process runs the handle, as for a verb on a thread of the
    /// daemon, which cannot be ended, or when it could not be ended.
    fn end_unstopped(&self, handle: &Handle) -> Option<String> {
        match self.processes.as_ref()?.end(handle) {
            Ok(ended) => ended,
            Err(reason) => {
                eprintln!(
                    "ignite: the verb process of {handle} did not stop and could not be ended: {reason}"
                );
                None
            }
        }
    }

    /// `--force`, once the asker may give it: asks `activity` to stop, and ends its verb
    /// process if it refuses or has not finished within the force wait. A refusing verb that
    /// runs on a thread of the daemon cannot be ended.
    fn force(&self, handle: &Handle, activity: &Arc<crate::steps::Activity>) -> Reply {
        let verb = match activity.kind() {
            crate::steps::Kind::Verb(verb) | crate::steps::Kind::Loop(verb) => verb.clone(),
        };
        let mut lines = Vec::new();
        let refused = activity.request_stop().err();
        if refused.is_some() || !activity.wait_finished(self.force_wait) {
            match self.processes.as_ref().map(|p| p.end(handle)) {
                Some(Ok(Some(ended))) => lines.push(format!("ended    {ended}")),
                Some(Ok(None)) | None => {
                    if let Some(reason) = refused {
                        return Reply::text(
                            Exit::Conflict,
                            format!(
                                "refused  {handle} cannot be stopped: {reason}, and no verb \
                                 process of its can be ended\nnext     ignite wait {handle}"
                            ),
                        );
                    }
                }
                Some(Err(reason)) => {
                    return Reply::text(
                        Exit::Conflict,
                        format!(
                            "stopping {handle}: it was told to stop, and its verb process could \
                             not be ended: {reason}\nnext     ignite wait {handle}"
                        ),
                    );
                }
            }
            if let Some(reason) = &refused {
                lines.push(format!(
                    "warning  it had refused to stop ({reason}); check what it left half done"
                ));
            }
            if !activity.wait_finished(self.wait) {
                let view = activity.view();
                let mut reply = still_running_at(&view.name, handle, Some(&view));
                reply.text = format!(
                    "stopping {handle}: it was stopped by force, and the daemon has not seen it \
                     finish within {} s\n{}",
                    self.wait.as_secs(),
                    reply.text
                );
                return reply;
            }
        }
        lines.insert(
            0,
            format!(
                "stopped  `ignite {verb}` ({handle}) by force; its commands were ended and its \
                 trees are freed once nothing of it runs"
            ),
        );
        Reply {
            exit: Exit::Success,
            text: lines.join("\n"),
            json: Some(serde_json::json!({
                "handle": handle, "verb": verb, "stopped": true, "forced": true
            })),
            handle: None,
        }
    }
}

/// Ends verb processes by the process ID their handle's row records (see [`crate::verbproc`]).
pub struct NativeEndVerbProcess {
    /// The Ignite home, which holds each verb process's lock.
    pub home: PathBuf,
    /// The daemon's local database.
    pub db: Arc<Mutex<Connection>>,
}

impl EndVerbProcess for NativeEndVerbProcess {
    fn end(&self, handle: &Handle) -> Result<Option<String>, String> {
        use ignite_os::occupants::Occupants as _;
        if !crate::verbproc::alive(&self.home, handle) {
            return Ok(None);
        }
        let row = handle_store::get(&crate::handles::lock(&self.db), handle.as_str())
            .map_err(|error| format!("its row cannot be read: {error}"))?;
        let Some(pid) = row
            .and_then(|row| row.pid)
            .and_then(|pid| u32::try_from(pid).ok())
        else {
            return Err(format!(
                "its verb process holds {} but records no process ID",
                crate::verbproc::folder(&self.home, handle)
                    .join("lock")
                    .display()
            ));
        };
        // The verb process leads a process group of its own (see `server/process.rs`), which
        // holds whatever it started directly, such as `git`.
        let mut pids = ignite_os::Native
            .group_members(u64::from(pid))
            .unwrap_or_default();
        if !pids.contains(&pid) {
            pids.push(pid);
        }
        let killed = ignite_os::Native
            .end(&pids, FORCE_GRACE)
            .map_err(|error| format!("process {pid} could not be signalled: {error}"))?;
        let since = Instant::now();
        while crate::verbproc::alive(&self.home, handle) {
            if since.elapsed() > FORCE_GRACE {
                return Err(format!(
                    "process {pid} still holds its lock after `SIGKILL`"
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let how = if killed.is_empty() {
            "`SIGTERM`".to_owned()
        } else {
            format!("`SIGKILL` for {} that ignored `SIGTERM`", killed.len())
        };
        Ok(Some(format!(
            "its verb process {pid} and its group ({} processes), with {how}",
            pids.len()
        )))
    }
}

#[cfg(test)]
#[path = "stop_tests.rs"]
mod tests;
