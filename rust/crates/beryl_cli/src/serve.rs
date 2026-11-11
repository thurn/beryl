//! `beryl_cli serve`: the app's game commands over HTTP, so the UI can play live games in a
//! plain browser (`just ui-live`). It stands in for `beryl_app`'s Tauri commands and events:
//! the same [`Sessions`] behind the same command names (`beryl_session::ipc`), so a reviewer
//! driving a headless browser sees the app's real timing, the agents' thinking included.
//!
//! - `POST /invoke/<command>` takes the command's arguments as JSON (`{}` or
//!   `{"request": ...}`) and answers `{"ok": result}` or `{"err": error}`.
//! - `GET /events` is a server-sent event stream: each view as a `game-update` event, sent
//!   only to the streams open when it is emitted, as Tauri emits it.
//!
//! Requests are small and local, so this is a minimal HTTP/1.1 server on the standard library:
//! one thread per connection, one request per connection.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use beryl_session::bugs::{self, BugReport, Images, UiLogRequest};
use beryl_session::card_records::CardRecordsRequest;
use beryl_session::ipc::{
    self, AppInfo, CancelRequest, ChooseRequest, ConcedeRequest, PassOnRequest, SetStopsRequest,
    UndoRequest,
};
use beryl_session::prompt::ClickRequest;
use beryl_session::{GameUpdate, SessionError, Sessions, StartRequest, agents};
use serde::Deserialize;
use serde_json::{Value, json};

/// The open event streams, each a channel to its connection's thread.
type Streams = Arc<Mutex<Vec<Sender<String>>>>;

/// Serves the game commands on `127.0.0.1:port` until the process is killed.
pub fn serve(port: u16) -> Result<(), String> {
    beryl_session::journal::install_panic_hook();
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    let streams: Streams = Arc::default();
    let sessions = {
        let streams = Arc::clone(&streams);
        Arc::new(Sessions::new(None, move |update: GameUpdate| {
            let Ok(data) = serde_json::to_string(&update) else {
                return;
            };
            let message = format!("event: {}\ndata: {data}\n\n", ipc::GAME_UPDATE);
            lock(&streams).retain(|s| s.send(message.clone()).is_ok());
        }))
    };
    println!("beryl_cli: serving on http://127.0.0.1:{port}");
    for connection in listener.incoming() {
        let Ok(connection) = connection else { continue };
        let sessions = Arc::clone(&sessions);
        let streams = Arc::clone(&streams);
        std::thread::spawn(move || {
            if let Err(e) = handle(connection, &sessions, &streams) {
                eprintln!("beryl_cli: {e}");
            }
        });
    }
    Ok(())
}

fn lock(streams: &Streams) -> std::sync::MutexGuard<'_, Vec<Sender<String>>> {
    streams.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The headers every response carries: the UI is served from another port.
pub(crate) const CORS: &str = "Access-Control-Allow-Origin: *\r\n\
                    Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
                    Access-Control-Allow-Headers: Content-Type\r\n";

fn handle(stream: TcpStream, sessions: &Sessions, streams: &Streams) -> Result<(), String> {
    let HttpRequest {
        method, path, body, ..
    } = read_request(&stream)?;
    let mut stream = stream;
    match (method.as_str(), path.as_str()) {
        ("OPTIONS", _) => respond(&mut stream, "204 No Content", ""),
        ("GET", "/events") => events(stream, streams),
        ("POST", path) if path.starts_with("/invoke/") => {
            let args: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let answer = invoke(sessions, &path["/invoke/".len()..], args);
            respond(&mut stream, "200 OK", &answer.to_string())
        }
        _ => respond(&mut stream, "404 Not Found", "{}"),
    }
}

pub(crate) struct HttpRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub(crate) fn read_request(stream: &TcpStream) -> Result<HttpRequest, String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut headers = HashMap::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).map_err(|e| e.to_string())?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let length = headers.get("content-length").map_or(Ok(0), |value| {
        value.parse().map_err(|_| "bad Content-Length")
    })?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok(HttpRequest {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        body,
    })
}

pub(crate) fn respond(stream: &mut TcpStream, status: &str, body: &str) -> Result<(), String> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\n{CORS}Content-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .map_err(|e| e.to_string())
}

/// Streams each view emitted from now on until the browser goes away. The stream subscribes
/// before it answers, so a page that sees it open misses no view after that.
fn events(mut stream: TcpStream, streams: &Streams) -> Result<(), String> {
    let (sent, received) = channel();
    lock(streams).push(sent);
    write!(
        stream,
        "HTTP/1.1 200 OK\r\n{CORS}Content-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n"
    )
    .map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    for message in received {
        if stream.write_all(message.as_bytes()).is_err() || stream.flush().is_err() {
            break;
        }
    }
    Ok(())
}

/// The [`ipc::CARD_RECORDS`] command's answer, which needs no game: the spec server's too.
pub(crate) fn card_records(args: Value) -> Value {
    match serde_json::from_value::<Request<CardRecordsRequest>>(args) {
        Ok(r) => json!({ "ok": beryl_session::card_records::card_records(&r.request) }),
        Err(e) => json!({ "err": { "kind": "badRequest", "message": e.to_string() } }),
    }
}

/// A command's request argument, as the UI sends it.
#[derive(Deserialize)]
struct Request<T> {
    request: T,
}

/// Runs `command` as `beryl_app` would: `{"ok": result}` or `{"err": error}`.
pub(crate) fn invoke(sessions: &Sessions, command: &str, args: Value) -> Value {
    fn request<T: for<'de> Deserialize<'de>>(args: Value) -> Result<T, Value> {
        serde_json::from_value::<Request<T>>(args)
            .map(|r| r.request)
            .map_err(|e| json!({"kind": "badRequest", "message": e.to_string()}))
    }
    fn answer<T: serde::Serialize, E: serde::Serialize>(result: Result<T, E>) -> Value {
        match result {
            Ok(v) => json!({ "ok": v }),
            Err(e) => json!({ "err": e }),
        }
    }
    match command {
        ipc::APP_INFO => answer::<_, ()>(Ok(AppInfo {
            version: format!("{} (live)", env!("CARGO_PKG_VERSION")),
            smoke: false,
            mobile: false,
        })),
        ipc::UI_READY => answer::<_, ()>(Ok(())),
        ipc::LIST_AGENTS => answer::<_, ()>(Ok(agents())),
        ipc::START_GAME => answer(
            request::<StartRequest>(args).and_then(|r| sessions.start(&r).map_err(|e| json!(e))),
        ),
        ipc::CLICK => answer(
            request::<ClickRequest>(args).and_then(|r| sessions.click(r).map_err(|e| json!(e))),
        ),
        ipc::CHOOSE => answer(
            request::<ChooseRequest>(args)
                .and_then(|r| sessions.choose_request(r).map_err(|e| json!(e))),
        ),
        ipc::SET_STOPS => answer(
            request::<SetStopsRequest>(args)
                .and_then(|r| sessions.set_stops(r.session, r.stops).map_err(|e| json!(e))),
        ),
        ipc::CANCEL => answer(
            request::<CancelRequest>(args)
                .and_then(|r| sessions.cancel(r.decision).map_err(|e| json!(e))),
        ),
        ipc::UNDO => answer(
            request::<UndoRequest>(args)
                .and_then(|r| sessions.undo(r.decision).map_err(|e| json!(e))),
        ),
        ipc::PASS_ON => answer(
            request::<PassOnRequest>(args)
                .and_then(|r| sessions.pass_on(r.decision).map_err(|e| json!(e))),
        ),
        ipc::CONCEDE => answer(
            request::<ConcedeRequest>(args)
                .and_then(|r| sessions.concede(r.session).map_err(|e| json!(e))),
        ),
        ipc::CURRENT_GAME => answer::<_, ()>(Ok(sessions.current())),
        ipc::RESUME_GAME => answer::<_, ()>(Ok(sessions.resume_latest())),
        // A browser's page has no window to snapshot: its report goes without the frame.
        ipc::CAPTURE_WINDOW => answer::<(), _>(Err(SessionError::Io {
            reason: "no window snapshot in a browser".to_owned(),
        })),
        ipc::REPORT_BUG => answer(request::<BugReport>(args).and_then(|r| {
            bugs::write(&bugs::dir(), &r, &Images::default(), sessions).map_err(|e| json!(e))
        })),
        ipc::UI_LOG => answer(request::<UiLogRequest>(args).map(bugs::ui_log)),
        ipc::CARD_RECORDS => card_records(args),
        _ => json!({ "err": { "kind": "unknownCommand", "command": command } }),
    }
}
