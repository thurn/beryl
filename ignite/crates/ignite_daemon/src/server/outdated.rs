//! Running a landing again under the daemon's new build when it was started under an older one.
//!
//! A verb process runs the executable that started it, so a landing whose verb process outlives a
//! restart of the daemon into a new release goes on with the old release's code. When that code
//! is what the release fixed, the landing keeps the bug: on 2026-11-18 six landings went on
//! waiting for the express lane under a broken daemon for minutes after the fix had reached every
//! machine, until each session stopped its landing and landed again by hand.
//!
//! So a restarted daemon that finds a `land` verb process alive whose row names another Ignite
//! commit than its own, and whose release changed the landing code between the two (the `land`
//! module, see [`crate::update::scope::affects_landing`]), asks that landing to stop, exactly as
//! `ignite stop` would. A release that changed only other daemon code leaves the landing running
//! under its build, as a `check` or `test` is, so the daemon need not wait for the landings that
//! run before it restarts. On 2026-11-19 every restart instead waited up to its 600 s cap
//! while landings ran, because each landing would have been run again, and `ignite wait
//! deploy-<commit>` took 470 to 563 s during bursts of landings. A landing whose
//! push has begun refuses to stop (the refusal is kept in the local database, so the restarted
//! daemon knows it), and is left to finish under the old build: a push cut short may or may not
//! have reached the remote. Asking for the stop first also means a landing about to begin its push
//! no longer can, because its refusal is then turned down.
//!
//! A landing that agreed to stop and is still running [`Processes::stop_wait`] later (the
//! [`STOP_WAIT`] of `ignite stop`) never looks at its stop where it is, and its verb process is
//! ended with its process group, as `ignite stop` ends one. On 2026-11-19 two landings spent
//! over an hour in a loop on the CPU while planning their gates, and each restart of the daemon
//! asked them to stop again without effect, until someone ended the verb process by hand.
//!
//! A landing that stops unwinds as from a failure, giving back its cores, lease and landing tree.
//! Its `stopped` reply is not handed to whoever waits on the handle ([`Handles::hold_stopped`]):
//! once the verb process has ended, the daemon makes the handle's row `running` again with the
//! same request, so the same note and session, and runs the landing again in a new verb process
//! under the same handle (see `server/resume.rs`). The session's `ignite wait` on the handle goes
//! on waiting and gets the new run's reply; a wait that ends first says, in a `restart` line, that
//! the landing was restarted under the new build.
//!
//! Only landings and tip rounds are stopped for an older build. A `tip-round` whose release
//! changed the tip code (the `tip` module, see [`crate::update::scope::affects_tip`]) is asked to
//! stop and ended after [`Processes::stop_wait`] if it does not, so a fix to a stuck tip round
//! takes effect on the restart instead of waiting on the old build; the tip loop then starts a
//! fresh round under the new build. A `check` or `test` started under the old build only reports
//! on a commit, its commands would be ended and started again, and the landing gate runs the
//! checks again under the new build anyway; so it finishes under the build that started it.
//!
//! Its tests are in `outdated_tests.rs`, which uses the verb processes of `process_tests.rs`.
//!
//! [`Handles::hold_stopped`]: crate::handles::Handles::hold_stopped
//! [`Processes::stop_wait`]: super::process::Processes::stop_wait
//! [`STOP_WAIT`]: crate::stop::STOP_WAIT

use std::sync::Arc;
use std::time::{Duration, Instant};

use ignite_core::ids::{Commit, Handle};
use ignite_core::protocol::client::Request;
use ignite_core::traits::Git;

use super::process::Shared;
use super::{Server, Verb};
use crate::host::Host;
use crate::stop::EndVerbProcess;
use crate::update::scope::{Source, landing_changes, tip_changes};

/// The verbs whose verb process a restarted daemon stops and runs again under its own build.
pub const RESTARTED_UNDER_NEW_BUILD: &[&str] = &["land"];

/// Non-resumable verbs whose verb process a restarted daemon stops when the release changed
/// their code, so their loop starts a fresh run under the new build.
const STOPPED_UNDER_NEW_BUILD: &[&str] = &["tip-round"];

/// The version an executable outside the executable store reports, which says nothing about
/// what code it runs.
const UNKNOWN_VERSION: &str = "unknown";

impl Server {
    /// For a verb process the restarted daemon found alive: when it runs a landing under another
    /// build than the daemon's own and agrees to stop, asks it to stop and returns true; once the
    /// process has ended, a new verb process runs the landing again under the same handle. When
    /// it runs an outdated `tip-round` whose tip code changed, asks it to stop (and ends it after
    /// `stop_wait` if it does not) and returns false so the caller still watches it until the tip
    /// loop starts a fresh round. False otherwise, as when its push has begun.
    pub(super) fn restart_if_outdated(
        &self,
        handle: &Handle,
        verb: &str,
        request: Option<&Request>,
    ) -> bool {
        let is_restarted = RESTARTED_UNDER_NEW_BUILD.contains(&verb);
        let is_stopped = STOPPED_UNDER_NEW_BUILD.contains(&verb);
        if !is_restarted && !is_stopped {
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
        if is_stopped {
            self.stop_outdated_tip_round(processes, &shared, handle, verb, &old, own);
            return false;
        }
        if !landing_code_changed(processes.git.as_deref(), &old, own) {
            eprintln!(
                "ignite: `ignite {verb}` ({handle}) goes on under ignite {}: ignite {} changed \
                 none of the landing code",
                short(&old),
                short(own)
            );
            return false;
        }
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
        let ending = Ending {
            ender: processes.ender.clone(),
            wait: processes.stop_wait,
        };
        let (waited, name) = (handle.clone(), verb.to_owned());
        let spawned = std::thread::Builder::new()
            .name(format!("ignite-restart-{handle}"))
            .spawn(move || {
                run_again_once_ended(&shared, &host, &waited, &name, &ending, rerun, request);
            });
        if let Err(error) = spawned {
            // The stop was asked, so the process ends `stopped`, and that reply must reach its
            // waiters after all.
            eprintln!("ignite: cannot wait to run {handle} again: {error}");
            self.handles.release_hold(handle, false);
        }
        true
    }

    /// Asks an adopted `tip-round` running under `old` to stop when `own` changed tip code, and
    /// ends its process group after `stop_wait` if it does not stop on its own.
    fn stop_outdated_tip_round(
        &self,
        processes: &super::process::Processes,
        shared: &Shared,
        handle: &Handle,
        verb: &str,
        old: &str,
        own: &str,
    ) {
        if !tip_code_changed(processes.git.as_deref(), old, own) {
            eprintln!(
                "ignite: `ignite {verb}` ({handle}) goes on under ignite {}: ignite {} changed \
                 none of the tip code",
                short(old),
                short(own)
            );
            return;
        }
        let Some(activity) = self.steps.by_handle(handle) else {
            return;
        };
        if let Err(reason) = activity.request_stop() {
            eprintln!(
                "ignite: `ignite {verb}` ({handle}) runs under ignite {old} and is left to \
                 finish: {reason}"
            );
            return;
        }
        eprintln!(
            "ignite: stopping `ignite {verb}` ({handle}), which runs under ignite {old}, because \
             ignite {own} changed the tip code"
        );
        let ending = Ending {
            ender: processes.ender.clone(),
            wait: processes.stop_wait,
        };
        let (shared, waited, name) = (shared.clone(), handle.clone(), verb.to_owned());
        let _ = std::thread::Builder::new()
            .name(format!("ignite-stop-outdated-{handle}"))
            .spawn(move || {
                wait_and_end_if_stuck(&shared, &waited, &name, &ending);
            });
    }
}

/// How a verb process asked to stop is ended when it does not.
struct Ending {
    ender: Option<Arc<dyn EndVerbProcess>>,
    /// How long it has to end by itself.
    wait: Duration,
}

/// Waits until `handle`'s verb process ends, ending it once it has run on for `ending.wait`
/// after the stop was asked. Returns false if the server was dropped while waiting.
fn wait_and_end_if_stuck(shared: &Shared, handle: &Handle, verb: &str, ending: &Ending) -> bool {
    let asked = Instant::now();
    let mut ender = ending.ender.as_ref();
    while crate::verbproc::alive(&shared.home, handle) {
        if asked.elapsed() >= ending.wait
            && let Some(end) = ender.take()
        {
            match end.end(handle) {
                Ok(Some(what)) => eprintln!(
                    "ignite: ended {what}: `ignite {verb}` ({handle}) had not stopped within \
                     {} s of being asked",
                    ending.wait.as_secs()
                ),
                Ok(None) => {}
                Err(reason) => eprintln!(
                    "ignite: `ignite {verb}` ({handle}) did not stop and could not be ended: \
                     {reason}"
                ),
            }
        }
        std::thread::sleep(shared.watch);
        if shared.server.upgrade().is_none() {
            return false;
        }
    }
    true
}

/// Waits until the verb process of `handle` has ended, ending it once it has run on for
/// `ending.wait` after the stop was asked, then runs `verb` again under the same handle with
/// `request` if it ended `stopped`, as asked, or died without a reply. A landing that finished
/// on its own first has its reply handed on.
fn run_again_once_ended(
    shared: &Shared,
    host: &Arc<Host>,
    handle: &Handle,
    verb: &str,
    ending: &Ending,
    rerun: Arc<dyn Verb>,
    request: Request,
) {
    if !wait_and_end_if_stuck(shared, handle, verb, ending) {
        return;
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
    super::resume::run_again(shared, handle, verb, Ok((rerun, request)), true);
}

/// Whether a landing started under the build `old` would run other landing code under `own`
/// (see [`crate::update::scope::affects_landing`]). True whenever that cannot be told: with no
/// `git`, when either version is not a commit, when `own` does not follow `old`, or when Git
/// fails, since running the landing again is what keeps a fix from missing it.
fn landing_code_changed(git: Option<&dyn Git>, old: &str, own: &str) -> bool {
    let (Some(git), Ok(old), Ok(own)) = (git, Commit::new(old), Commit::new(own)) else {
        return true;
    };
    if !git.is_ancestor(&old, &own).unwrap_or(false) {
        return true;
    }
    // The landing code is the `land` module, which never lies in the check manifests' directory,
    // so this needs no directory to leave out.
    landing_changes(git, Source::ignite(""), &old, &own).map_or(true, |changes| !changes.is_empty())
}

/// Whether a tip round started under the build `old` would run other tip code under `own`
/// (see [`crate::update::scope::affects_tip`]). True whenever that cannot be told.
fn tip_code_changed(git: Option<&dyn Git>, old: &str, own: &str) -> bool {
    let (Some(git), Ok(old), Ok(own)) = (git, Commit::new(old), Commit::new(own)) else {
        return true;
    };
    if !git.is_ancestor(&old, &own).unwrap_or(false) {
        return true;
    }
    tip_changes(git, Source::ignite(""), &old, &own).map_or(true, |changes| !changes.is_empty())
}

/// The first seven characters of a commit, as `ignite status` shows it.
fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}
