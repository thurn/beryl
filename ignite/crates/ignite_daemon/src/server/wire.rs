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

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

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

/// How long a client that has connected has to send its request. A client that sends nothing
/// would otherwise hold its thread for as long as it stays connected.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest pause after failed accepts in a row.
const ACCEPT_PAUSE_MAX: Duration = Duration::from_secs(2);

/// [`serve`] on a listener that is already open. A failed accept (as when the process has run
/// out of file descriptors for a moment) is reported and tried again after a pause that grows
/// while they continue; it never ends the daemon.
pub fn serve_listener(server: &Arc<Server>, listener: &dyn Listener) -> std::io::Result<()> {
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
        let server = server.clone();
        let spawned = std::thread::Builder::new()
            .name("ignite-connection".to_owned())
            .spawn(move || answer(&server, stream));
        if let Err(error) = spawned {
            eprintln!("ignite: cannot start a thread for a connection: {error}");
        }
    }
}

/// Reads one request line from `stream` and writes the reply line, with the lines before it
/// that the client asked for.
fn answer(server: &Server, mut stream: Box<dyn Stream>) {
    let _ = stream.set_read_timeout(Some(REQUEST_TIMEOUT));
    let mut line = String::new();
    if BufReader::new(&mut stream).read_line(&mut line).is_err() {
        return;
    }
    let _ = stream.set_read_timeout(None);
    let caller: Option<CallerProbe> = stream.closed_probe().map(Arc::from);
    let wants = serde_json::from_str::<Wants>(&line).unwrap_or_default();
    let reply = match serde_json::from_str::<Request>(&line) {
        Ok(req) => {
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
        }
        Err(error) => Reply::text(Exit::Usage, format!("failed   unreadable request: {error}")),
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
