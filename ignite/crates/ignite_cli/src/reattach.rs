//! Keeping a command's reply across a restart of the daemon.
//!
//! The daemon restarts into a new build as soon as one is ready (see "Processes" in
//! `docs/ignite.md`). Its connection to a waiting client then drops. The command itself goes on:
//! a long verb runs in a verb process of its own, which outlives the daemon, and any other verb
//! is run again under the same handle by the restarted daemon (`ignite_daemon::server`). So the
//! client does what `ignite wait` does:
//!
//! - The daemon names the handle of a verb that does work before the verb starts. A client cut
//!   off after that waits on the handle, trying to reach the daemon for up to [`RETRY_FOR`],
//!   printing the verb's steps as they start, as the first request did, and prints the reply as
//!   if nothing had happened.
//! - A daemon about to restart answers a new request with a reply that says so, before it
//!   starts anything ([`ignite_daemon::server::is_restarting`]); the client sends the request
//!   again as for a daemon that refuses the connection.
//! - A verb that was cut off before it named a handle is sent again only if it is one that only
//!   reads or waits ([`REPEATABLE`]). Any other is answered with a reply that says its outcome
//!   is unknown and how to find it.
//! - A daemon that refuses the connection before the request is written is being started, so
//!   the request is sent again for up to [`RETRY_FOR`], counted from the last time a daemon
//!   accepted a request of the command and held it until it went away (a command that outlives
//!   one restart can meet the next long after). So is one whose socket does not exist
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
pub const REPEATABLE: &[&str] = &["doctor", "status", "stop", "wait"];

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
/// reach a daemon that did not answer.
pub fn send(
    daemon: &mut dyn Daemon,
    req: &Request,
    mut progress: Option<&mut dyn FnMut(&str)>,
) -> Result<Reply, Cut> {
    let cut = match exchange(daemon, req, reborrow(&mut progress)) {
        Ok(reply) => return Ok(reply),
        Err(cut) => cut,
    };
    if !(cut.sent || starting(daemon, &cut)) {
        return Err(cut);
    }
    let mut since = daemon.elapsed();
    let mut pause = FIRST_PAUSE;
    let mut cut = cut;
    loop {
        if let Some(handle) = cut.handle.take() {
            return Ok(reattach(
                daemon,
                req,
                &handle,
                since,
                reborrow(&mut progress),
            ));
        }
        if cut.sent && !REPEATABLE.contains(&req.verb.as_str()) {
            return Ok(unknown_outcome(req, &cut));
        }
        if daemon.elapsed().saturating_sub(since) >= RETRY_FOR {
            return Err(cut);
        }
        daemon.pause(pause);
        pause = (pause * 2).min(MAX_PAUSE);
        cut = match exchange(daemon, req, reborrow(&mut progress)) {
            Ok(reply) => return Ok(reply),
            Err(cut) => cut,
        };
        if cut.sent {
            // A daemon accepted the request and held it until it restarted again: it was
            // reachable up to this moment, so the next [`RETRY_FOR`] starts here.
            since = daemon.elapsed();
            pause = FIRST_PAUSE;
        }
    }
}

/// One exchange, with a daemon that answers that it is about to restart taken as one that
/// refused the connection: it started nothing.
fn exchange(
    daemon: &mut dyn Daemon,
    req: &Request,
    progress: Option<&mut dyn FnMut(&str)>,
) -> Result<Reply, Cut> {
    match daemon.exchange(req, progress) {
        Ok(reply) if ignite_daemon::server::is_restarting(&reply) => Err(Cut {
            sent: false,
            handle: None,
            error: std::io::Error::new(ErrorKind::ConnectionRefused, reply.text),
        }),
        other => other,
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

/// Waits on `handle` for `req`'s reply, as `ignite wait` does, passing the verb's steps to
/// `progress`, trying to reach the daemon for up to [`RETRY_FOR`] after `since`, which moves to
/// the moment each wait that the daemon accepted was cut. The wait lasts what is left of the
/// request's own.
fn reattach(
    daemon: &mut dyn Daemon,
    req: &Request,
    handle: &Handle,
    mut since: Duration,
    mut progress: Option<&mut dyn FnMut(&str)>,
) -> Reply {
    let mut pause = FIRST_PAUSE;
    loop {
        let left = u64::from(req.wait_s).saturating_sub(daemon.elapsed().as_secs());
        let wait = Request {
            verb: "wait".to_owned(),
            args: vec![handle.to_string()],
            wait_s: u32::try_from(left).unwrap_or(u32::MAX).max(1),
            ..req.clone()
        };
        let cut = match exchange(daemon, &wait, reborrow(&mut progress)) {
            Ok(reply) => return reply,
            Err(cut) => cut,
        };
        if cut.sent {
            // The daemon accepted this wait and held it until it went away, so it was
            // reachable up to this moment, however long the wait lasted.
            since = daemon.elapsed();
            pause = FIRST_PAUSE;
        }
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
