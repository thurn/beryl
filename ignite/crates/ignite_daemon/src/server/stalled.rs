//! Ending a stuck verb process whose verb cannot be run again safely.
//!
//! A daemon watches every verb process it starts or adopts across a restart (see
//! `server/process.rs`), for as long as the process lives. On 2026-11-26 the Windows machine's
//! tip loop waited more than two hours on a `tip-round` verb process (`T-2223`) that hung under
//! its own daemon for 65 minutes and then outlived a daemon upgrade for another 55 minutes. Later
//! the same day, jarvis's tip loop waited 67 minutes on a `tip-round` verb process (`T-15098`)
//! that hung under its own daemon for 48 minutes before an unrelated daemon upgrade adopted it
//! and waited another 20 minutes to end it.
//!
//! So the daemon watches every live verb process of a verb it cannot run again safely (one that
//! `Registry::resumable` does not return, such as `tip-round` or `idle-job`), whether this daemon
//! started it or adopted it across a restart under the same or an older build. Progress is any
//! of: a new step or a change in what it waits for, through the host protocol's `host-step`; more
//! text in `verbs/<handle>/verb.log`; or the daemon doing something for it ([`Host::busy`]: one
//! of its commands runs, or the daemon is still working out a `host-run` or `host-build` it keeps
//! asking about). The `host-poll` calls a process sends about once a second do not count: they
//! come from a thread of their own, which goes on while the verb itself is stuck. A process that
//! makes no progress for [`STALL_LIMIT`] is ended with its process group, as `ignite stop` ends
//! one, and its handle then ends `incomplete` like that of any verb process that died, so the
//! loop that waits on it starts afresh.
//!
//! A verb the daemon can run again, such as a landing, is left alone: `server/outdated.rs`
//! decides whether it runs again under a new build, and a stuck one that is not run again is
//! the job of `ignite stop`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ignite_core::ids::Handle;
use ignite_core::time::Timestamp;
use ignite_os::usage::UsageReader as _;

use super::Shared;
use crate::host::Host;
use crate::stop::EndVerbProcess;

/// How long a non-resumable verb process may go without progress before it is ended. A tip round
/// between commands only lists checks and reads results, which takes seconds to minutes, so
/// twenty minutes without a step, output or command is a stuck process.
pub const STALL_LIMIT: Duration = Duration::from_mins(20);

/// Minimum wall-clock window between CPU samples for [`Stall::cpu_busy`], and minimum fraction
/// of one core over that window to count as active work rather than a 1-second `host-poll` loop.
const CPU_WINDOW_S: f64 = 5.0;
const CPU_ACTIVE_RATE: f64 = 0.02;

/// What the watch of a stuck verb process needs: the limit, how to end the process, and the
/// host that says whether the daemon is doing something for it.
pub(crate) struct Stall {
    limit: Duration,
    ender: Arc<dyn EndVerbProcess>,
    host: Arc<Host>,
    log: PathBuf,
    /// The progress last seen, and when it was first seen.
    seen: Progress,
    since: Timestamp,
    /// Last sampled process-group CPU seconds and wall-clock time.
    cpu: Option<(f64, Instant)>,
    /// Set once the process was ended, or could not be, so it is not tried again.
    ended: bool,
}

/// Everything that changes while a verb process makes progress.
#[derive(PartialEq)]
struct Progress {
    step: Option<(String, Timestamp)>,
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
        let stall = Stall {
            limit,
            ender: ender?,
            host: shared.host.clone()?,
            log: crate::verbproc::folder(&shared.home, handle).join("verb.log"),
            seen: Progress {
                step: None,
                waits: Vec::new(),
                log_len: 0,
            },
            since: shared.ctx.clock.now(),
            cpu: None,
            ended: false,
        };
        Some(Stall {
            seen: stall.progress(shared, handle),
            ..stall
        })
    }

    fn progress(&self, shared: &Shared, handle: &Handle) -> Progress {
        let activity = shared.steps.by_handle(handle);
        Progress {
            step: activity.as_ref().and_then(|a| a.timing().1),
            waits: activity.map(|a| a.waits()).unwrap_or_default(),
            log_len: std::fs::metadata(&self.log).map_or(0, |m| m.len()),
        }
    }

    fn cpu_busy(&mut self, shared: &Shared, handle: &Handle) -> bool {
        let Some(pid) = shared
            .row(handle)
            .and_then(|row| row.pid)
            .and_then(|pid| u32::try_from(pid).ok())
        else {
            return false;
        };
        let Ok(usage) = ignite_os::Native.sample(pid) else {
            return false;
        };
        let now = Instant::now();
        let Some((prev_cpu, prev_at)) = self.cpu else {
            self.cpu = Some((usage.cpu_s, now));
            return false;
        };
        let elapsed = now.duration_since(prev_at).as_secs_f64();
        if elapsed < CPU_WINDOW_S {
            return false;
        }
        self.cpu = Some((usage.cpu_s, now));
        (usage.cpu_s - prev_cpu) / elapsed >= CPU_ACTIVE_RATE
    }

    /// Looks at the verb process once, as its watch does on each round, and ends it when it has
    /// made no progress for the limit.
    pub(super) fn look(&mut self, shared: &Shared, handle: &Handle) {
        if self.ended {
            return;
        }
        let now = shared.ctx.clock.now();
        let progress = self.progress(shared, handle);
        if progress != self.seen || self.host.busy(handle) || self.cpu_busy(shared, handle) {
            self.seen = progress;
            self.since = now;
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
