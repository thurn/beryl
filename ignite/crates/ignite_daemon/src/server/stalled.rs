//! Ending a stuck verb process that a restarted daemon took over from an older build.
//!
//! A restarted daemon goes on watching every verb process that outlived the daemon before it
//! (see `server/process.rs`), for as long as the process lives. On 2026-11-26 the Windows
//! machine's tip loop waited more than two hours on a `tip-round` verb process started by the
//! previous build: it reported no step, wrote nothing to its `verb.log` and used 23 s of CPU in
//! those two hours, and no tip round ran on the machine until someone ended it by hand. The
//! daemon then ended the handle `incomplete` and started a new round within seconds.
//!
//! So a restarted daemon that adopts the live verb process of a verb it cannot run again
//! safely (one that `Registry::resumable` does not return, such as `tip-round`), started under
//! another build than its own, also watches it for progress. Progress is any of: a new step or
//! a change in what it waits for, through the host protocol's `host-step`; more text in
//! `verbs/<handle>/verb.log`; or the daemon doing something for it ([`Host::busy`]: one of its
//! commands runs, or the daemon is still working out a `host-run` or `host-build` it keeps
//! asking about). The `host-poll` calls a process sends about once a second do not count: they
//! come from a thread of their own, which goes on while the verb itself is stuck. A process that
//! makes no progress for [`STALL_LIMIT`] is ended with its process group, as `ignite stop` ends
//! one, and its handle then ends `incomplete` like that of any verb process that died, so the
//! loop that waits on it starts afresh.
//!
//! A verb the daemon can run again, such as a landing, is left alone: `server/outdated.rs`
//! decides whether it runs again under the new build, and a stuck one that is not run again is
//! the job of `ignite stop`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ignite_core::ids::Handle;
use ignite_core::time::Timestamp;

use super::Shared;
use crate::host::Host;
use crate::stop::EndVerbProcess;

/// How long an adopted verb process may go without progress before it is ended. A tip round
/// between commands only lists checks and reads results, which takes seconds to minutes, so
/// twenty minutes without a step, output or command is a stuck process.
pub const STALL_LIMIT: Duration = Duration::from_mins(20);

/// The version an executable outside the executable store reports.
const UNKNOWN_VERSION: &str = "unknown";

/// `version`, unless it is missing or says nothing about the code.
fn known(version: Option<&str>) -> Option<&str> {
    version.filter(|v| *v != UNKNOWN_VERSION)
}

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
    /// The watch for a verb process the restarted daemon adopted, or `None` when it needs none:
    /// the verb can be run again (`resumable`), the process runs the daemon's own build
    /// (`own` and the row's `executable` name the same commit), or nothing can end it.
    pub(super) fn for_adopted(
        shared: &Shared,
        handle: &Handle,
        resumable: bool,
        own: Option<&str>,
        limit: Duration,
        ender: Option<Arc<dyn EndVerbProcess>>,
    ) -> Option<Stall> {
        if resumable {
            return None;
        }
        let executable = shared.row(handle).and_then(|row| row.executable);
        if let (Some(own), Some(old)) = (known(own), known(executable.as_deref()))
            && own == old
        {
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

    /// Looks at the verb process once, as its watch does on each round, and ends it when it has
    /// made no progress for the limit.
    pub(super) fn look(&mut self, shared: &Shared, handle: &Handle) {
        if self.ended {
            return;
        }
        let now = shared.ctx.clock.now();
        let progress = self.progress(shared, handle);
        if progress != self.seen || self.host.busy(handle) {
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
                "ignite: ended {what}: `{verb}` ({handle}), adopted from an older build, made no \
                 progress for {idle:.0} s"
            ),
            Ok(None) => {}
            Err(reason) => eprintln!(
                "ignite: `{verb}` ({handle}), adopted from an older build, made no progress for \
                 {idle:.0} s and could not be ended: {reason}"
            ),
        }
    }
}
