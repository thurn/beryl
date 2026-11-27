//! Ending a stuck verb process whose verb cannot be run again safely.
//!
//! A daemon watches every verb process it starts or adopts across a restart (see
//! `server/process.rs`), for as long as the process lives. On 2026-11-26 the Windows machine's
//! tip loop waited more than two hours on a `tip-round` verb process (`T-2223`) that hung under
//! its own daemon for 65 minutes and then outlived a daemon upgrade for another 55 minutes. Later
//! the same day, jarvis's tip loop waited 67 minutes on a `tip-round` verb process (`T-15098`)
//! that hung under its own daemon for 48 minutes before an unrelated daemon upgrade adopted it
//! and waited another 20 minutes to end it. On 2026-11-27, jarvis's tip loop waited over 75
//! minutes on a `tip-round` verb process (`T-16028`) spinning at 100% CPU inside `git grep`
//! while four daemon restarts each reset the in-memory stall clock.
//!
//! So the daemon watches every live verb process of a verb it cannot run again safely (one that
//! `Registry::resumable` does not return, such as `tip-round` or `idle-job`), whether this daemon
//! started it or adopted it across a restart under the same or an older build. Progress is any
//! of: a new step or a change in what it waits for, through the host protocol's `host-step`; more
//! text in `verbs/<handle>/verb.log`; or the daemon doing something for it ([`Host::busy`]: one
//! of its commands runs, or the daemon is still working out a `host-run` or `host-build` it keeps
//! asking about). Neither `host-poll` calls nor raw process-group CPU usage count as progress: a
//! CPU-bound hang without a step change, log growth or daemon-managed command must still be ended
//! at [`STALL_LIMIT`]. The last observed progress and its timestamp are saved in
//! `verbs/<handle>/stall.json`, so daemon restarts and an adopted process re-reporting its
//! unchanged step do not reset the stall clock. A process that makes no progress for
//! [`STALL_LIMIT`] is ended with its process group, as `ignite stop` ends one, and its handle
//! then ends `incomplete` like that of any verb process that died, so the loop that waits on it
//! starts afresh.
//!
//! A verb the daemon can run again, such as a landing, is left alone: `server/outdated.rs`
//! decides whether it runs again under a new build, and a stuck one that is not run again is
//! the job of `ignite stop`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ignite_core::ids::Handle;
use ignite_core::time::Timestamp;
use serde::{Deserialize, Serialize};

use super::Shared;
use crate::host::Host;
use crate::stop::EndVerbProcess;

/// How long a non-resumable verb process may go without progress before it is ended. A tip round
/// between commands only lists checks and reads results, which takes seconds to minutes, so
/// twenty minutes without a step, output or command is a stuck process.
pub const STALL_LIMIT: Duration = Duration::from_mins(20);

/// How often `stall.json` is rewritten while [`Host::busy`] keeps advancing `since`.
const SAVE_INTERVAL_S: f64 = 30.0;

/// What the watch of a stuck verb process needs: the limit, how to end the process, and the
/// host that says whether the daemon is doing something for it.
pub(crate) struct Stall {
    limit: Duration,
    ender: Arc<dyn EndVerbProcess>,
    host: Arc<Host>,
    log: PathBuf,
    state: PathBuf,
    /// The progress last seen, and when it was first seen.
    seen: Progress,
    since: Timestamp,
    /// When `state` was last written, to throttle writes while [`Host::busy`] stays true.
    saved_at: Timestamp,
    /// Set once the process was ended, or could not be, so it is not tried again.
    ended: bool,
}

/// Everything that changes while a verb process makes progress.
struct Progress {
    step: Option<String>,
    /// When `step` was set on this daemon, or `None` when loaded from `stall.json` before an
    /// adopted verb process has re-reported its step through `host-step`.
    step_at: Option<Timestamp>,
    waits: Vec<String>,
    log_len: u64,
}

/// Persisted form of [`Stall`]'s last progress in `verbs/<handle>/stall.json`.
#[derive(Serialize, Deserialize)]
struct SavedProgress {
    since: Timestamp,
    step: Option<String>,
    waits: Vec<String>,
    log_len: u64,
}

impl Stall {
    /// The watch for a started or adopted verb process, or `None` when it needs none: the verb
    /// can be run again (`resumable`) or nothing can end it.
    pub(super) fn for_verb(
        shared: &Shared,
        handle: &Handle,
        resumable: bool,
        limit: Duration,
        ender: Option<Arc<dyn EndVerbProcess>>,
    ) -> Option<Stall> {
        if resumable {
            return None;
        }
        let folder = crate::verbproc::folder(&shared.home, handle);
        let log = folder.join("verb.log");
        let state = folder.join("stall.json");
        let now = shared.ctx.clock.now();
        let (seen, since, should_save) = match load_state(&state) {
            Some(saved) => (
                Progress {
                    step: saved.step,
                    step_at: None,
                    waits: saved.waits,
                    log_len: saved.log_len,
                },
                saved.since,
                false,
            ),
            None => (current_progress(shared, handle, &log), now, true),
        };
        let stall = Stall {
            limit,
            ender: ender?,
            host: shared.host.clone()?,
            log,
            state,
            seen,
            since,
            saved_at: since,
            ended: false,
        };
        if should_save {
            stall.save();
        }
        Some(stall)
    }

    fn save(&self) {
        let saved = SavedProgress {
            since: self.since,
            step: self.seen.step.clone(),
            waits: self.seen.waits.clone(),
            log_len: self.seen.log_len,
        };
        let Ok(bytes) = serde_json::to_vec(&saved) else {
            return;
        };
        let tmp = self.state.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &self.state);
        }
    }

    /// Updates `self.seen` from `current` and returns whether real progress occurred.
    ///
    /// When `self.seen` was loaded from `stall.json` (`step_at.is_none()`), an empty `current`
    /// before `Link::note_daemon` reconnects, or a re-report of the exact same step and waits,
    /// does not count as progress.
    fn advance(&mut self, current: Progress) -> bool {
        let log_grew = current.log_len > self.seen.log_len;
        if self.seen.step_at.is_none() {
            if current.step.is_none() && current.waits.is_empty() {
                if log_grew {
                    self.seen.log_len = current.log_len;
                    return true;
                }
                return false;
            }
            if current.step == self.seen.step && current.waits == self.seen.waits {
                self.seen.step_at = current.step_at;
                if log_grew {
                    self.seen.log_len = current.log_len;
                    return true;
                }
                return false;
            }
            self.seen = current;
            return true;
        }
        let changed = current.step != self.seen.step
            || current.step_at != self.seen.step_at
            || current.waits != self.seen.waits
            || current.log_len != self.seen.log_len;
        if changed {
            self.seen = current;
        }
        changed
    }

    /// Looks at the verb process once, as its watch does on each round, and ends it when it has
    /// made no progress for the limit.
    pub(super) fn look(&mut self, shared: &Shared, handle: &Handle) {
        if self.ended {
            return;
        }
        let now = shared.ctx.clock.now();
        let current = current_progress(shared, handle, &self.log);
        if self.host.busy(handle) {
            let changed = self.advance(current);
            self.since = now;
            if changed || now.seconds_since(self.saved_at) >= SAVE_INTERVAL_S {
                self.saved_at = now;
                self.save();
            }
            return;
        }
        if self.advance(current) {
            self.since = now;
            self.saved_at = now;
            self.save();
            return;
        }
        let idle = now.seconds_since(self.since);
        if idle < self.limit.as_secs_f64() {
            return;
        }
        self.ended = true;
        let verb = shared.row(handle).map_or_else(String::new, |row| row.verb);
        match self.ender.end(handle) {
            Ok(Some(what)) => eprintln!(
                "ignite: ended {what}: `{verb}` ({handle}) made no progress for {idle:.0} s"
            ),
            Ok(None) => {}
            Err(reason) => eprintln!(
                "ignite: `{verb}` ({handle}) made no progress for {idle:.0} s and could not be \
                 ended: {reason}"
            ),
        }
    }
}

fn current_progress(shared: &Shared, handle: &Handle, log: &Path) -> Progress {
    let activity = shared.steps.by_handle(handle);
    let (step, step_at) = match activity.as_ref().and_then(|a| a.timing().1) {
        Some((text, at)) => (Some(text), Some(at)),
        None => (None, None),
    };
    Progress {
        step,
        step_at,
        waits: activity.map(|a| a.waits()).unwrap_or_default(),
        log_len: std::fs::metadata(log).map_or(0, |m| m.len()),
    }
}

fn load_state(path: &Path) -> Option<SavedProgress> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}
