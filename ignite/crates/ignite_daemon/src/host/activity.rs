//! The activity of each verb process (see [`crate::steps`]): its step and waits, its stop
//! state, kept in the local database as well, and the end of its commands once a stop is asked.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Weak};
use std::time::Instant;

use ignite_core::ids::{Handle, SessionId, TreeId};
use ignite_core::protocol::client::Reply;
use ignite_core::protocol::host::{
    Hello, HelloReply, PROTOCOL_VERSION, RefuseStop, RefuseStopReply, StepUpdate, StopState,
};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};
use ignite_store::local::{handle_stops, handles as handle_store, rows};

use super::{Host, Remote};
use crate::handles::lock;
use crate::server::Context;
use crate::steps::{Activity, Kind, Work};

/// When this daemon started, as the replies that carry it say.
fn daemon_started() -> String {
    crate::update::started_at().to_string()
}

impl Host {
    /// The activity of `handle`'s verb process, created from its handle's row when this daemon
    /// has none yet. `None` when the handle is unknown or no longer `running`, as when `ignite
    /// stop` ended it while no daemon ran or the process wrote its reply.
    pub(super) fn remote(
        self: &Arc<Self>,
        _ctx: &Context,
        handle: &Handle,
    ) -> Option<Arc<Activity>> {
        if self.is_handed_back(handle) {
            return None;
        }
        let mut remotes = lock(&self.remotes);
        if let Some(remote) = remotes.get(handle)
            && !remote.activity.is_finished()
        {
            return Some(remote.activity.clone());
        }
        let row = self.row(handle).filter(|row| row.state == "running")?;
        let tree = row.tree.as_deref().and_then(|t| TreeId::new(t).ok());
        let caller = crate::handles::running_request(&lock(&self.db), handle)
            .ok()
            .flatten()
            .and_then(|(_, request)| request.pid);
        let work = Work {
            caller_processes: crate::trees::caller_processes(caller),
            class: None,
            ticket: None,
            session: row.session.as_deref().and_then(|s| SessionId::new(s).ok()),
            tree_path: tree
                .as_ref()
                .and_then(|tree| crate::trees::root::recorded_path(&lock(&self.db), tree)),
            tree,
        };
        // The verb started under the daemon before this one: its age counts from its handle's
        // start, not from the moment this daemon first heard from it.
        let started = Timestamp::parse(&row.started_at).ok();
        let activity = self
            .steps
            .begin_adopted(Kind::Verb(row.verb), work, started);
        activity.attach_handle(handle);
        match self.stop_row(handle) {
            Some(stop) if stop.asked => {
                let _ = activity.request_stop();
            }
            Some(rows::HandleStop {
                refused: Some(reason),
                ..
            }) => {
                activity.refuse_stop(&reason);
            }
            _ => {}
        }
        remotes.insert(
            handle.clone(),
            Remote {
                activity: activity.clone(),
                hello: None,
            },
        );
        drop(remotes);
        self.watch(handle, activity.clone());
        Some(activity)
    }

    pub(super) fn hello(self: &Arc<Self>, ctx: &Context, call: Hello) -> HelloReply {
        let Some(activity) = self.remote(ctx, &call.handle) else {
            return HelloReply {
                protocol: PROTOCOL_VERSION,
                running: false,
                stop: true,
                daemon_started: Some(daemon_started()),
            };
        };
        if let Some(remote) = lock(&self.remotes).get_mut(&call.handle) {
            remote.hello = Some(call);
        }
        HelloReply {
            protocol: PROTOCOL_VERSION,
            running: true,
            stop: activity.stop_requested(),
            daemon_started: Some(daemon_started()),
        }
    }

    pub(super) fn step(self: &Arc<Self>, ctx: &Context, call: &StepUpdate) -> StopState {
        let Some(activity) = self.remote(ctx, &call.handle) else {
            return StopState {
                stop: true,
                daemon_started: Some(daemon_started()),
            };
        };
        activity.report(
            call.step.as_deref(),
            &call.waiting,
            call.class,
            call.ticket.as_ref(),
        );
        StopState {
            stop: activity.stop_requested(),
            daemon_started: Some(daemon_started()),
        }
    }

    pub(super) fn poll(self: &Arc<Self>, ctx: &Context, handle: &Handle) -> StopState {
        let stop = self
            .remote(ctx, handle)
            .is_none_or(|activity| activity.stop_requested());
        StopState {
            stop,
            daemon_started: Some(daemon_started()),
        }
    }

    /// Refuses later stops, unless one was asked already. The refusal is kept in the local
    /// database before the answer, so a restarted daemon refuses as well.
    pub(super) fn refuse_stop(
        self: &Arc<Self>,
        ctx: &Context,
        call: &RefuseStop,
    ) -> Result<RefuseStopReply> {
        let activity = self.running(ctx, &call.handle)?;
        let refused = activity.refuse_stop(&call.reason);
        if refused {
            self.save_stop(&call.handle, false, Some(&call.reason))?;
        }
        Ok(RefuseStopReply { refused })
    }

    pub(super) fn allow_stop(self: &Arc<Self>, ctx: &Context, handle: &Handle) -> Result<()> {
        let activity = self.running(ctx, handle)?;
        activity.allow_stop();
        if !activity.stop_requested() {
            self.save_stop(handle, false, None)?;
        }
        Ok(())
    }

    /// The verb process wrote its reply into the handle's row: wakes whoever waits on the
    /// handle and ends its activity. A repeated call does it again, harmlessly.
    pub(super) fn finished(self: &Arc<Self>, handle: &Handle) -> Result<()> {
        match self.row(handle) {
            Some(row) if row.state == "running" => Err(Error::Conflict(format!(
                "{handle} is still running in the local database; write its reply first"
            ))),
            Some(row) => {
                self.settle(handle, &row);
                Ok(())
            }
            None => Err(Error::NotFound(format!("unknown handle {handle}"))),
        }
    }

    /// The activity of a running handle, or the error a call that changes it answers otherwise.
    pub(super) fn running(
        self: &Arc<Self>,
        ctx: &Context,
        handle: &Handle,
    ) -> Result<Arc<Activity>> {
        self.remote(ctx, handle)
            .ok_or_else(|| Error::Conflict(format!("{handle} is not running")))
    }

    /// Hands the reply in a finished handle's row to whoever waits on it, and ends its activity.
    fn settle(&self, handle: &Handle, row: &rows::Handle) {
        self.return_all_cores(handle);
        // Held while the reply is handed on, so that once `hand_back` has returned no reply of
        // the ended process reaches the handle's waiters.
        let handed_back = lock(&self.handed_back);
        if handed_back.contains(handle) {
            return;
        }
        let reply = row
            .reply
            .as_deref()
            .and_then(|text| serde_json::from_str::<Reply>(text).ok());
        if let (Some(handles), Some(reply)) = (self.handles.get(), reply) {
            match row.state.as_str() {
                "stopped" => handles.finish_stopped(handle, reply),
                "incomplete" => handles.finish_incomplete(handle, reply),
                _ => handles.finish(handle, reply),
            }
        }
        if let Some(remote) = lock(&self.remotes).remove(handle) {
            remote.activity.finish();
        }
    }

    /// Starts the thread that, until the verb process's activity finishes, ends its commands
    /// once a stop is asked (with the stop kept in the database), and notices a handle that
    /// ended without `host-finished`.
    fn watch(self: &Arc<Self>, handle: &Handle, activity: Arc<Activity>) {
        let host: Weak<Host> = Arc::downgrade(self);
        let timing = self.timing;
        let name = format!("ignite-host-{handle}");
        let watched = handle.clone();
        let spawned = std::thread::Builder::new().name(name).spawn(move || {
            let handle = watched;
            // Each run ended, with when and whether it was killed since.
            let mut ended: HashMap<PathBuf, (Instant, bool)> = HashMap::new();
            let mut saved = false;
            loop {
                let Some(host) = host.upgrade() else {
                    return;
                };
                // The daemon runs the verb itself now, or a new verb process of the handle runs
                // it, and its commands are the new run's.
                if host.is_handed_back(&handle) || !host.is_current(&handle, &activity) {
                    return;
                }
                // Before looking whether the verb finished: a verb process that ends at once
                // when asked to stop, or dies meanwhile, leaves its commands running otherwise.
                if activity.stop_requested() {
                    if !saved {
                        saved = host.save_stop(&handle, true, None).is_ok();
                    }
                    for run in host.starter.runs_of(&handle) {
                        match ended.get_mut(&run) {
                            None => {
                                host.starter.end(&run);
                                ended.insert(run, (Instant::now(), false));
                            }
                            Some((at, killed)) if !*killed && at.elapsed() >= timing.grace => {
                                host.starter.kill(&run);
                                *killed = true;
                            }
                            Some(_) => {}
                        }
                    }
                }
                if activity.is_finished() {
                    return;
                }
                match host.row(&handle) {
                    Some(row) if row.state == "running" => {}
                    Some(row) => {
                        host.settle(&handle, &row);
                        return;
                    }
                    None => {
                        host.return_all_cores(&handle);
                        if let Some(remote) = lock(&host.remotes).remove(&handle) {
                            remote.activity.finish();
                        }
                        return;
                    }
                }
                drop(host);
                if activity.stop_requested() {
                    activity.wait_finished(timing.watch);
                } else {
                    activity.wait_stop_requested(timing.watch);
                }
            }
        });
        if let Err(error) = spawned {
            eprintln!("ignite: cannot watch the verb process of {handle}: {error}");
        }
    }

    /// Whether `activity` is still the one the host keeps for `handle`'s verb process. Every
    /// place that drops it from the host also stops watching it, so one that is not is an
    /// earlier process's.
    fn is_current(&self, handle: &Handle, activity: &Arc<Activity>) -> bool {
        lock(&self.remotes)
            .get(handle)
            .is_some_and(|remote| Arc::ptr_eq(&remote.activity, activity))
    }

    fn row(&self, handle: &Handle) -> Option<rows::Handle> {
        handle_store::get(&lock(&self.db), handle.as_str())
            .ok()
            .flatten()
    }

    fn stop_row(&self, handle: &Handle) -> Option<rows::HandleStop> {
        handle_stops::get(&lock(&self.db), handle.as_str())
            .ok()
            .flatten()
    }

    fn save_stop(&self, handle: &Handle, asked: bool, refused: Option<&str>) -> Result<()> {
        let row = rows::HandleStop {
            handle: handle.to_string(),
            asked,
            refused: refused.map(str::to_owned),
            at: self.clock.now().to_string(),
        };
        handle_stops::set(&lock(&self.db), &row).map_err(|e| Error::Io(e.to_string()))
    }
}
