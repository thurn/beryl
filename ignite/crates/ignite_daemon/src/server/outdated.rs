//! Running a landing again under the daemon's new build when it was started under an older one.
//!
//! A verb process runs the executable that started it, so a landing whose verb process outlives a
//! restart of the daemon into a new release goes on with the old release's code. When that code
//! is what the release fixed, the landing keeps the bug: on 2026-11-18 six landings went on
//! waiting for the express lane under a broken daemon for minutes after the fix had reached every
//! machine, until each session stopped its landing and landed again by hand.
//!
//! So a restarted daemon that finds a `land` verb process alive whose row names another Ignite
//! commit than its own asks that landing to stop, exactly as `ignite stop` would. A landing whose
//! push has begun refuses to stop (the refusal is kept in the local database, so the restarted
//! daemon knows it), and is left to finish under the old build: a push cut short may or may not
//! have reached the remote. Asking for the stop first also means a landing about to begin its push
//! no longer can, because its refusal is then turned down.
//!
//! A landing that stops unwinds as from a failure, giving back its cores, lease and landing tree.
//! Its `stopped` reply is not handed to whoever waits on the handle ([`Handles::hold_stopped`]):
//! once the verb process has ended, the daemon makes the handle's row `running` again with the
//! same request, so the same note and session, and runs the landing again on a thread of its own
//! under the same handle (see `server/resume.rs`). The session's `ignite wait` on the handle goes
//! on waiting and gets the new run's reply; a wait that ends first says, in a `restart` line, that
//! the landing was restarted under the new build.
//!
//! Only landings are restarted. A `check` or `test` started under the old build only reports on a
//! commit, its commands would be ended and started again, and the landing gate runs the checks
//! again under the new build anyway; so it finishes under the build that started it.
//!
//! Its tests are in `outdated_tests.rs`, which uses the verb processes of `process_tests.rs`.
//!
//! [`Handles::hold_stopped`]: crate::handles::Handles::hold_stopped

use std::sync::Arc;

use ignite_core::ids::Handle;
use ignite_core::protocol::client::Request;

use super::process::Shared;
use super::{Server, Verb};
use crate::host::Host;

/// The verbs whose verb process a restarted daemon stops and runs again under its own build.
pub const RESTARTED_UNDER_NEW_BUILD: &[&str] = &["land"];

/// The version an executable outside the executable store reports, which says nothing about
/// what code it runs.
const UNKNOWN_VERSION: &str = "unknown";

impl Server {
    /// For a verb process the restarted daemon found alive: when it runs a landing under another
    /// build than the daemon's own and agrees to stop, asks it to stop and returns true; a thread
    /// then runs the landing again under the same handle once the process has ended. False
    /// otherwise, as when its push has begun: the caller goes on watching the process.
    pub(super) fn restart_if_outdated(
        &self,
        handle: &Handle,
        verb: &str,
        request: Option<&Request>,
    ) -> bool {
        if !RESTARTED_UNDER_NEW_BUILD.contains(&verb) {
            return false;
        }
        let Some(processes) = &self.processes else {
            return false;
        };
        let Some(own) = processes
            .version
            .as_deref()
            .filter(|v| *v != UNKNOWN_VERSION)
        else {
            return false;
        };
        let shared = self.shared();
        let Some(old) = shared
            .row(handle)
            .and_then(|row| row.executable)
            .filter(|old| old != own && old != UNKNOWN_VERSION)
        else {
            return false;
        };
        let (Some(request), Some(rerun)) = (request.cloned(), self.registry.resumable(verb)) else {
            return false;
        };
        let Some(activity) = self.steps.by_handle(handle) else {
            return false;
        };
        let note = format!(
            "{handle} was restarted under ignite {}: it had not begun its push under ignite {}",
            short(own),
            short(&old)
        );
        self.handles.hold_stopped(handle, &note);
        if let Err(reason) = activity.request_stop() {
            self.handles.release_hold(handle, false);
            eprintln!(
                "ignite: `ignite {verb}` ({handle}) runs under ignite {old} and is left to \
                 finish: {reason}"
            );
            return false;
        }
        eprintln!(
            "ignite: stopping `ignite {verb}` ({handle}), which runs under ignite {old}, to run \
             it again under ignite {own}"
        );
        let host = processes.host.clone();
        let (waited, name) = (handle.clone(), verb.to_owned());
        let spawned = std::thread::Builder::new()
            .name(format!("ignite-restart-{handle}"))
            .spawn(move || run_again_once_ended(&shared, &host, &waited, &name, rerun, request));
        if let Err(error) = spawned {
            // The stop was asked, so the process ends `stopped`, and that reply must reach its
            // waiters after all.
            eprintln!("ignite: cannot wait to run {handle} again: {error}");
            self.handles.release_hold(handle, false);
        }
        true
    }
}

/// Waits until the verb process of `handle` has ended, then runs `verb` again under the same
/// handle with `request` if it ended `stopped`, as asked, or died without a reply. A landing that
/// finished on its own first has its reply handed on.
fn run_again_once_ended(
    shared: &Shared,
    host: &Arc<Host>,
    handle: &Handle,
    verb: &str,
    rerun: Arc<dyn Verb>,
    request: Request,
) {
    while crate::verbproc::alive(&shared.home, handle) {
        std::thread::sleep(shared.watch);
        if shared.server.upgrade().is_none() {
            return;
        }
    }
    let state = shared.row(handle).map(|row| row.state);
    if !matches!(state.as_deref(), Some("stopped" | "running")) {
        shared.handles.release_hold(handle, false);
        shared.deliver_if_finished(handle);
        return;
    }
    host.end_runs_of(handle);
    host.hand_back(handle);
    shared.handles.reopen(handle, &request);
    shared.handles.release_hold(handle, true);
    super::resume::run_again(shared, handle, verb, Ok((rerun, request)));
}

/// The first seven characters of a commit, as `ignite status` shows it.
fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}
