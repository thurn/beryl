//! The socket between the client and the daemon: one JSON request line from the client, then
//! lines from the daemon until the reply.
//!
//! Before the reply, the daemon may send two kinds of line, each only to a client that asked for
//! it in its request line (see [`Wants`]), so that an older client never reads one it does not
//! know:
//!
//! - `{"handle": "T-12"}`, once, before a verb that does work starts. A client whose connection
//!   drops before the reply, as when the daemon restarts into a new build, waits on that handle
//!   instead (`ignite wait T-12`).
//! - `{"progress": "rebasing onto staging 1a2b3c4"}`, for each step of the verb as it starts.
//!
//! The daemon listens before it has finished starting (see [`Starting`]): a request that arrives
//! meanwhile waits until the daemon is ready and is then answered, instead of being refused.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use ignite_core::ids::Handle;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_os::ipc::{Ipc, Listener, Stream};

use super::{CallerProbe, Server};

/// The daemon's socket: `daemon.sock` in the machine's Ignite home (`$IGNITE_HOME`, or
/// `~/.config/ignite`).
pub fn default_socket() -> PathBuf {
    crate::update::Home::from_env().root.join("daemon.sock")
}

/// Listens on `socket` and answers every client from `server`, one thread per connection,
/// until the process ends.
pub fn serve(server: &Arc<Server>, socket: &str) -> std::io::Result<()> {
    let listener = ignite_os::Native.listen(socket)?;
    serve_listener(server, listener.as_ref())
}

/// The verbs answered at once while the daemon is starting, with a reply that says so, rather
/// than waiting until it is ready: they only report, and are what a person runs to see why the
/// daemon does not answer. `alive` is the monitor's probe, which a starting daemon passes as it
/// passed `status` before.
const ANSWERED_WHILE_STARTING: &[&str] = &["status", "doctor", "alive"];

/// The server a socket answers from, which exists only once the daemon has started.
///
/// The daemon binds its socket first, right after it takes the machine's lock, and serves it
/// through a `Starting` while it reads its configuration, takes over the runs a previous daemon
/// left and builds its parts. A request that arrives meanwhile waits for [`Starting::ready`]
/// and is then answered as usual, so a client sees a slower reply rather than a refused
/// connection. If the daemon cannot start, every waiting request is answered with the reason.
pub struct Starting {
    state: Mutex<State>,
    changed: Condvar,
    since: Instant,
    activity: Arc<crate::steps::Activity>,
}

struct State {
    phase: Phase,
    /// How many requests are waiting for the start.
    waiting: usize,
}

enum Phase {
    Starting,
    Ready(Arc<Server>),
    Failed(String),
}

impl Starting {
    /// A daemon that has begun to start.
    pub fn new() -> Arc<Starting> {
        let steps = crate::steps::Steps::new(Arc::new(ignite_core::time::SystemClock));
        let activity = steps.begin(
            crate::steps::Kind::Loop(crate::main::STARTUP_ACTIVITY.to_owned()),
            crate::steps::Work::default(),
        );
        Arc::new(Starting {
            state: Mutex::new(State {
                phase: Phase::Starting,
                waiting: 0,
            }),
            changed: Condvar::new(),
            since: Instant::now(),
            activity,
        })
    }

    /// Enters the startup activity on the calling thread so `crate::steps::step` and
    /// `crate::main::startup_step` update what `ignite status` reports while starting.
    pub fn enter(&self) -> crate::steps::Entered {
        self.activity.enter()
    }

    /// A daemon that is ready already, answering from `server`.
    pub fn serving(server: Arc<Server>) -> Arc<Starting> {
        let starting = Starting::new();
        starting.ready(server);
        starting
    }

    /// The daemon has started: every waiting request and every later one is answered from
    /// `server`.
    pub fn ready(&self, server: Arc<Server>) {
        self.settle(Phase::Ready(server));
    }

    /// The daemon could not start: every waiting request and every later one is answered with
    /// `reason`.
    pub fn failed(&self, reason: impl Into<String>) {
        self.settle(Phase::Failed(reason.into()));
    }

    fn settle(&self, phase: Phase) {
        self.activity.finish();
        self.lock().phase = phase;
        self.changed.notify_all();
    }

    /// The server, or why there is none, without waiting; `None` while the daemon is starting.
    fn now(&self) -> Option<Result<Arc<Server>, String>> {
        match &self.lock().phase {
            Phase::Starting => None,
            Phase::Ready(server) => Some(Ok(server.clone())),
            Phase::Failed(reason) => Some(Err(reason.clone())),
        }
    }

    /// Waits until the daemon has started or failed to.
    fn wait(&self) -> Result<Arc<Server>, String> {
        let mut state = self.lock();
        state.waiting += 1;
        self.changed.notify_all();
        let settled = loop {
            match &state.phase {
                Phase::Starting => {}
                Phase::Ready(server) => break Ok(server.clone()),
                Phase::Failed(reason) => break Err(reason.clone()),
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        };
        state.waiting -= 1;
        settled
    }

    /// Blocks until `count` requests are waiting for the start, for a test that must know its
    /// request arrived before the daemon was ready.
    #[cfg(test)]
    pub(crate) fn until_waiting(&self, count: usize) {
        let mut state = self.lock();
        while state.waiting < count {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The reply to a verb in [`ANSWERED_WHILE_STARTING`] while the daemon is starting.
    fn starting_reply(&self) -> Reply {
        let seconds = self.since.elapsed().as_secs_f64();
        let view = self.activity.view();
        let doing = view.doing();
        Reply {
            json: Some(serde_json::json!({
                "starting": true,
                "seconds": seconds,
                "step": view.step,
                "step_age_s": view.step_age_s,
                "waiting_for": view.waiting_for,
            })),
            ..Reply::text(
                Exit::Success,
                format!(
                    "starting the daemon has been starting for {seconds:.1} s: {doing}; other \
                     commands wait until it is ready"
                ),
            )
        }
    }
}

/// How long a client that has connected has to send its request. A client that sends nothing
/// would otherwise hold its thread for as long as it stays connected.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest pause after failed accepts in a row.
const ACCEPT_PAUSE_MAX: Duration = Duration::from_secs(2);

/// [`serve`] on a listener that is already open. A failed accept (as when the process has run
/// out of file descriptors for a moment) is reported and tried again after a pause that grows
/// while they continue; it never ends the daemon.
pub fn serve_listener(server: &Arc<Server>, listener: &dyn Listener) -> std::io::Result<()> {
    serve_starting(&Starting::serving(server.clone()), listener)
}

/// [`serve_listener`] for a daemon that may still be starting: each request waits for
/// `starting` to be ready (see [`Starting`]).
pub fn serve_starting(starting: &Arc<Starting>, listener: &dyn Listener) -> std::io::Result<()> {
    let mut pause = Duration::from_millis(50);
    loop {
        let stream = match listener.accept() {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("ignite: cannot accept a connection: {error}");
                std::thread::sleep(pause);
                pause = (pause * 2).min(ACCEPT_PAUSE_MAX);
                continue;
            }
        };
        pause = Duration::from_millis(50);
        let starting = starting.clone();
        let spawned = std::thread::Builder::new()
            .name("ignite-connection".to_owned())
            .spawn(move || answer(&starting, stream));
        if let Err(error) = spawned {
            eprintln!("ignite: cannot start a thread for a connection: {error}");
        }
    }
}

/// Reads one request line from `stream` and writes the reply line, with the lines before it
/// that the client asked for. While the daemon is starting, the request waits for it (see
/// [`Starting`]).
fn answer(starting: &Starting, mut stream: Box<dyn Stream>) {
    let _ = stream.set_read_timeout(Some(REQUEST_TIMEOUT));
    let mut line = String::new();
    if BufReader::new(&mut stream).read_line(&mut line).is_err() {
        return;
    }
    let _ = stream.set_read_timeout(None);
    let caller: Option<CallerProbe> = stream.closed_probe().map(Arc::from);
    let wants = serde_json::from_str::<Wants>(&line).unwrap_or_default();
    let req = match serde_json::from_str::<Request>(&line) {
        Ok(req) => req,
        Err(error) => {
            let reply = Reply::text(Exit::Usage, format!("failed   unreadable request: {error}"));
            let _ = write_line(&mut stream, &reply);
            return;
        }
    };
    let server = match starting.now() {
        Some(settled) => settled,
        None if ANSWERED_WHILE_STARTING.contains(&req.verb.as_str()) => {
            let _ = write_line(&mut stream, &starting.starting_reply());
            return;
        }
        None => {
            let settled = starting.wait();
            // A client that went away while the daemon was starting was never told a handle,
            // so nobody waits for the work; it is not started.
            if caller.as_ref().is_some_and(|closed| closed()) {
                return;
            }
            settled
        }
    };
    let server = match server {
        Ok(server) => server,
        Err(reason) => {
            let reply = Reply::text(
                Exit::Usage,
                format!("failed   the daemon did not start: {reason}\nnext     ignite doctor"),
            );
            let _ = write_line(&mut stream, &reply);
            return;
        }
    };
    let reply = {
        // Both callbacks write to the stream, one after the other, never at once.
        let stream = std::cell::RefCell::new(&mut stream);
        let mut open = true;
        let mut pass = |text: &str| {
            open = open
                && write_line(&mut **stream.borrow_mut(), &Progress { progress: text }).is_ok();
            open
        };
        let mut named = |handle: &Handle| {
            let _ = write_line(&mut **stream.borrow_mut(), &HandleLine { handle });
        };
        server.handle_answering(
            &req,
            caller,
            wants
                .progress
                .then_some(&mut pass as &mut dyn FnMut(&str) -> bool),
            wants
                .reattach
                .then_some(&mut named as &mut dyn FnMut(&Handle)),
        )
    };
    // The client may have gone away, as when its tool call was interrupted; nothing is lost,
    // because a reply that outlives its caller is kept under a handle.
    let _ = write_line(&mut stream, &reply);
}

pub(super) fn write_line(stream: &mut dyn Write, value: &impl Serialize) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    stream.write_all(&bytes)?;
    stream.flush()
}

/// What a client asks for beside its [`Request`], read from the same JSON line. A daemon that
/// does not know a field ignores it, and a client that sends none gets one reply line.
#[derive(Default, Deserialize)]
struct Wants {
    /// Send each step of the verb as a [`Progress`] line before the reply.
    #[serde(default)]
    progress: bool,
    /// Send the handle of a verb that does work as a [`HandleLine`] before it starts.
    #[serde(default)]
    reattach: bool,
}

/// A line sent before the reply to a client that asked for progress: a step of the verb.
#[derive(Serialize)]
struct Progress<'a> {
    progress: &'a str,
}

/// A line sent before a verb starts to a client that asked to reattach: the verb's handle.
#[derive(Serialize)]
struct HandleLine<'a> {
    handle: &'a Handle,
}

/// A line before the reply, as the client reads it: an object with one field, `progress` or
/// `handle`. The reply itself has a `handle` field too, beside `exit` and `text`.
enum Before {
    Progress(String),
    Handle(Handle),
}

impl Before {
    fn read(line: &str) -> Option<Before> {
        let serde_json::Value::Object(object) = serde_json::from_str(line).ok()? else {
            return None;
        };
        if object.len() != 1 {
            return None;
        }
        match object.into_iter().next()? {
            (key, serde_json::Value::String(text)) if key == "progress" => {
                Some(Before::Progress(text))
            }
            (key, serde_json::Value::String(text)) if key == "handle" => {
                Handle::new(text).ok().map(Before::Handle)
            }
            _ => None,
        }
    }
}

/// How an exchange with the daemon ended without a reply.
#[derive(Debug)]
pub struct Cut {
    /// The request was written to the daemon, which may have started it.
    pub sent: bool,
    /// The handle the daemon named for the request before the connection dropped.
    pub handle: Option<Handle>,
    pub error: std::io::Error,
}

/// The client's side: sends `req` to the daemon listening on `socket` and reads its reply.
pub fn send(socket: &str, req: &Request) -> std::io::Result<Reply> {
    send_reading(socket, req, None)
}

/// [`send`], asking the daemon for each step of the verb as it starts and passing it to
/// `progress` before the reply comes.
pub fn send_reading(
    socket: &str,
    req: &Request,
    progress: Option<&mut dyn FnMut(&str)>,
) -> std::io::Result<Reply> {
    exchange(socket, req, progress).map_err(|cut| cut.error)
}

/// [`send_reading`], also asking for the verb's handle, so that a connection that drops before
/// the reply says which handle to wait on.
pub fn exchange(
    socket: &str,
    req: &Request,
    progress: Option<&mut dyn FnMut(&str)>,
) -> Result<Reply, Cut> {
    let unsent = |error| Cut {
        sent: false,
        handle: None,
        error,
    };
    let mut stream = ignite_os::Native.connect(socket).map_err(unsent)?;
    let mut value = serde_json::to_value(req)
        .map_err(std::io::Error::other)
        .map_err(unsent)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("reattach".to_owned(), serde_json::Value::Bool(true));
        if progress.is_some() {
            object.insert("progress".to_owned(), serde_json::Value::Bool(true));
        }
    }
    write_line(&mut stream, &value).map_err(unsent)?;
    let mut handle = None;
    read_reply(BufReader::new(&mut stream), progress, &mut handle).map_err(|error| Cut {
        sent: true,
        handle,
        error,
    })
}

/// Reads the lines before the reply, passing each step to `progress` and keeping the handle in
/// `handle`, until the reply line.
pub(super) fn read_reply(
    mut reader: impl BufRead,
    mut progress: Option<&mut dyn FnMut(&str)>,
    handle: &mut Option<Handle>,
) -> std::io::Result<Reply> {
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "the daemon closed the connection",
            ));
        }
        match Before::read(&line) {
            Some(Before::Progress(step)) => {
                if let Some(progress) = progress.as_mut() {
                    progress(&step);
                }
            }
            Some(Before::Handle(named)) => *handle = Some(named),
            None => return serde_json::from_str(&line).map_err(std::io::Error::other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_reply_reports_no_step_until_one_is_recorded() {
        let starting = Starting::new();
        let before = starting.starting_reply();
        assert!(
            before.text.contains("no step recorded"),
            "before step: {}",
            before.text
        );
        assert!(before.json.as_ref().unwrap()["step"].is_null());

        let _entered = starting.enter();
        crate::steps::step("marking unfinished trees");
        let during = starting.starting_reply();
        assert!(
            during.text.contains("marking unfinished trees"),
            "during step: {}",
            during.text
        );
        assert_eq!(
            during.json.as_ref().unwrap()["step"],
            "marking unfinished trees"
        );
    }
}
