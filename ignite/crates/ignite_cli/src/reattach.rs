//! Keeping a command's reply across a restart of the daemon.
//!
//! The daemon restarts into a new build as soon as one is ready, whatever runs (see "Processes"
//! in `docs/ignite.md`). Its connection to a waiting client then drops. The command itself goes
//! on: the restarted daemon runs it again under the same handle, taking over its running
//! commands (`ignite_daemon::server`). So the client does what `ignite wait` does:
//!
//! - The daemon names the handle of a verb that does work before the verb starts. A client cut
//!   off after that waits on the handle, trying to reach the daemon for up to [`RETRY_FOR`],
//!   and prints the reply as if nothing had happened.
//! - A verb that was cut off before it named a handle is sent again only if it is one that only
//!   reads or waits ([`REPEATABLE`]). Any other is answered with a reply that says its outcome
//!   is unknown and how to find it.
//! - A daemon that refuses the connection before the request is written is being started, so
//!   the request is sent again for up to [`RETRY_FOR`]. So is one whose socket does not exist
//!   while a process holds the machine's daemon lock or a daemon start was logged in the last
//!   minute ([`Daemon::starting`]): on Windows the old daemon's pipe goes away before the new
//!   daemon creates its own. Otherwise a missing socket means no daemon is installed or
//!   running, which is answered at once.

use std::io::ErrorKind;
use std::time::{Duration, Instant};

use ignite_core::ids::Handle;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_daemon::server::Cut;

/// How long a client keeps trying to reach a daemon that went away during its request.
pub const RETRY_FOR: Duration = Duration::from_secs(300);

/// The first pause between two tries; it doubles up to [`MAX_PAUSE`].
const FIRST_PAUSE: Duration = Duration::from_millis(250);
const MAX_PAUSE: Duration = Duration::from_secs(2);

/// The verbs that only read or wait, which are safe to send twice. The daemon gives none of
/// them a handle.
pub const REPEATABLE: &[&str] = &["dispatch", "doctor", "status", "stop", "wait"];

/// The machine's daemon, as the client reaches it.
pub trait Daemon {
    /// Sends `req` and reads the reply, passing each step to `progress`.
    fn exchange(
        &mut self,
        req: &Request,
        progress: Option<&mut dyn FnMut(&str)>,
    ) -> Result<Reply, Cut>;
    /// Waits before the next try.
    fn pause(&mut self, pause: Duration);
    /// How long ago the command started.
    fn elapsed(&self) -> Duration;
    /// Whether a daemon is starting although its socket does not exist yet.
    fn starting(&self) -> bool;
}

/// Sends `req`, keeping its reply across a restart of the daemon. `Err` holds the failure to
/// reach a daemon that is not running at all (or, without `retry_unsent`, one that refused the
/// connection), for the caller to report or fall back from.
pub fn send(
    daemon: &mut dyn Daemon,
    req: &Request,
    mut progress: Option<&mut dyn FnMut(&str)>,
    retry_unsent: bool,
) -> Result<Reply, Cut> {
    let cut = match daemon.exchange(req, reborrow(&mut progress)) {
        Ok(reply) => return Ok(reply),
        Err(cut) => cut,
    };
    if !(cut.sent || retry_unsent && starting(daemon, &cut)) {
        return Err(cut);
    }
    let since = daemon.elapsed();
    let mut pause = FIRST_PAUSE;
    let mut cut = cut;
    loop {
        if let Some(handle) = cut.handle.take() {
            return Ok(reattach(daemon, req, &handle, since));
        }
        if cut.sent && !REPEATABLE.contains(&req.verb.as_str()) {
            return Ok(unknown_outcome(req, &cut));
        }
        if daemon.elapsed().saturating_sub(since) >= RETRY_FOR {
            return Err(cut);
        }
        daemon.pause(pause);
        pause = (pause * 2).min(MAX_PAUSE);
        cut = match daemon.exchange(req, reborrow(&mut progress)) {
            Ok(reply) => return Ok(reply),
            Err(cut) => cut,
        };
    }
}

/// `progress` again, for one more exchange.
fn reborrow<'a>(progress: &'a mut Option<&mut dyn FnMut(&str)>) -> Option<&'a mut dyn FnMut(&str)> {
    match progress {
        Some(progress) => Some(&mut **progress),
        None => None,
    }
}

/// Whether the daemon could not be reached because it is starting: its socket is there and
/// nothing listens on it yet, or the socket is missing while a daemon is starting.
fn starting(daemon: &dyn Daemon, cut: &Cut) -> bool {
    match cut.error.kind() {
        ErrorKind::ConnectionRefused => true,
        ErrorKind::NotFound => daemon.starting(),
        _ => false,
    }
}

/// Waits on `handle` for `req`'s reply, as `ignite wait` does, trying to reach the daemon for
/// up to [`RETRY_FOR`] after `since`. The wait lasts what is left of the request's own.
fn reattach(daemon: &mut dyn Daemon, req: &Request, handle: &Handle, since: Duration) -> Reply {
    let mut pause = FIRST_PAUSE;
    loop {
        let left = u64::from(req.wait_s).saturating_sub(daemon.elapsed().as_secs());
        let wait = Request {
            verb: "wait".to_owned(),
            args: vec![handle.to_string()],
            wait_s: u32::try_from(left).unwrap_or(u32::MAX).max(1),
            ..req.clone()
        };
        let cut = match daemon.exchange(&wait, None) {
            Ok(reply) => return reply,
            Err(cut) => cut,
        };
        if daemon.elapsed().saturating_sub(since) >= RETRY_FOR {
            return Reply::text(
                Exit::Usage,
                format!(
                    "failed   the daemon has not answered for {} minutes since `ignite {}` ({handle}) \
                     was cut off: {}\nnext     ignite wait {handle} once it answers; ignite doctor",
                    RETRY_FOR.as_secs() / 60,
                    req.verb,
                    cut.error
                ),
            );
        }
        daemon.pause(pause);
        pause = (pause * 2).min(MAX_PAUSE);
    }
}

/// The reply for a verb that does work and was cut off before the daemon named its handle.
fn unknown_outcome(req: &Request, cut: &Cut) -> Reply {
    Reply::text(
        Exit::Conflict,
        format!(
            "unknown  the connection to the daemon dropped ({}) before it named a handle for \
             `ignite {}`, so whether it ran is unknown\nnext     ignite status lists the running \
             commands; run it again if it is not there and did not finish",
            cut.error, req.verb
        ),
    )
}

/// The daemon behind the machine's socket.
pub struct SocketDaemon {
    pub socket: String,
    pub started: Instant,
}

impl Daemon for SocketDaemon {
    fn exchange(
        &mut self,
        req: &Request,
        progress: Option<&mut dyn FnMut(&str)>,
    ) -> Result<Reply, Cut> {
        ignite_daemon::server::exchange(&self.socket, req, progress)
    }

    fn pause(&mut self, pause: Duration) {
        std::thread::sleep(pause);
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn starting(&self) -> bool {
        use ignite_core::traits::Clock;
        ignite_daemon::machine_lock::daemon_starting(
            &ignite_daemon::update::Home::from_env(),
            ignite_core::time::SystemClock.now(),
        )
    }
}

#[cfg(test)]
#[path = "reattach_tests.rs"]
mod tests;
