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
use std::path::Path;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use beryl_session::bugs::{self, BugReport, Images, UiLogRequest};
use beryl_session::card_records::CardRecordsRequest;
use beryl_session::ipc::decks::{
    CardSearchRequest, CardTokensRequest, DeckDeleteRequest, DeckImportRequest, DeckOpenReply,
};
use beryl_session::ipc::{
    self, AppInfo, CancelRequest, ChooseRequest, ConcedeRequest, PassOnRequest, Platform,
    SetStopsRequest, TableActRequest, TableHostRequest, TableJoinRequest, TableKeysRequest,
    TableResumeRequest, TableSettings, TableSoloRequest, UndoRequest,
};
use beryl_session::prompt::ClickRequest;
use beryl_session::{GameUpdate, SessionError, Sessions, StartRequest, agents};
use beryl_table_session::connections::{MemoryInvites, TableConnections};
use beryl_table_session::decks::{self, Decks};
use serde::Deserialize;
use serde_json::{Value, json};

/// The open event streams, each a channel to its connection's thread.
type Streams = Arc<Mutex<Vec<Sender<String>>>>;

/// Serves the game commands on loopback until the process is killed.
pub fn serve(port: u16) -> Result<(), String> {
    beryl_session::journal::install_panic_hook();
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let data = tempfile::tempdir().map_err(|error| error.to_string())?;
    let memory = MemoryInvites::default();
    let seats = Arc::new([
        BrowserSeat::new(&data.path().join("host"), memory.clone()),
        BrowserSeat::new(&data.path().join("guest"), memory),
    ]);
    println!("beryl_cli: serving on http://{address}");
    for connection in listener.incoming() {
        let Ok(connection) = connection else { continue };
        let seats = Arc::clone(&seats);
        std::thread::spawn(move || {
            if let Err(e) = handle(connection, &seats) {
                eprintln!("beryl_cli: {e}");
            }
        });
    }
    Ok(())
}

struct BrowserSeat {
    sessions: Sessions,
    tables: TableConnections,
    decks: Decks,
    streams: Streams,
}

impl BrowserSeat {
    fn new(data: &Path, memory: MemoryInvites) -> Self {
        let streams: Streams = Arc::default();
        let events = Arc::clone(&streams);
        let game_streams = Arc::clone(&streams);
        let emit = move |update: GameUpdate| broadcast(&game_streams, ipc::GAME_UPDATE, &update);
        let sessions = Sessions::new(None, emit.clone());
        let tables =
            TableConnections::new(sessions.session_ids(), data.into(), emit, move |event| {
                let (name, payload) = event.into_message();
                broadcast(&events, name, &payload);
            })
            .for_server(memory);
        Self {
            sessions,
            tables,
            decks: Decks::new(data),
            streams,
        }
    }
}

fn broadcast(streams: &Streams, event: &str, payload: &impl serde::Serialize) {
    if let Ok(data) = serde_json::to_string(payload) {
        let message = format!("event: {event}\ndata: {data}\n\n");
        lock(streams).retain(|sender| sender.send(message.clone()).is_ok());
    }
}

fn lock(streams: &Streams) -> std::sync::MutexGuard<'_, Vec<Sender<String>>> {
    streams.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The headers every response carries: the UI is served from another port.
pub(crate) const CORS: &str = "Access-Control-Allow-Origin: *\r\n\
                    Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
                    Access-Control-Allow-Headers: Content-Type\r\n";

fn handle(mut stream: TcpStream, seats: &[BrowserSeat; 2]) -> Result<(), String> {
    let HttpRequest {
        method, path, body, ..
    } = read_request(&stream)?;
    let (path, query) = path.split_once('?').unwrap_or((&path, ""));
    let key = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find_map(|(name, value)| (name == "session").then_some(value));
    let seat = match key {
        None | Some("host") => &seats[0],
        Some("guest") => &seats[1],
        Some(_) => return respond(&mut stream, "400 Bad Request", "{}"),
    };
    let BrowserSeat {
        sessions,
        tables,
        decks,
        streams,
    } = seat;
    match (method.as_str(), path) {
        ("OPTIONS", _) => respond(&mut stream, "204 No Content", ""),
        ("GET", "/events") => events(stream, streams),
        ("POST", path) if path.starts_with("/invoke/") => {
            let args: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let command = &path["/invoke/".len()..];
            let answer = match command {
                ipc::TABLE_SOLO => table_command(args, |r: TableSoloRequest| tables.solo(r)),
                ipc::TABLE_HOST => table_command(args, |r: TableHostRequest| {
                    tables.host(Platform::current(), r)
                }),
                ipc::TABLE_JOIN => table_command(args, |r: TableJoinRequest| tables.join(r)),
                ipc::TABLE_RESUME => table_command(args, |r: TableResumeRequest| {
                    tables.resume(Platform::current(), &r)
                }),
                ipc::TABLE_SAVED => answer(tables.saved()),
                ipc::TABLE_SETTINGS_GET => answer(tables.settings()),
                ipc::TABLE_SETTINGS_SET => {
                    table_command(args, |r: TableSettings| tables.set_settings(&r))
                }
                ipc::TABLE_ACT => {
                    answer(request::<TableActRequest>(args).map(|r| tables.act(r.action)))
                }
                ipc::TABLE_CURRENT => answer::<_, ()>(Ok(tables.current())),
                ipc::TABLE_LEAVE => {
                    tables.leave();
                    answer::<_, ()>(Ok(()))
                }
                ipc::TABLE_KEYS => answer(request::<TableKeysRequest>(args).map(|_| ())),
                ipc::TABLE_QUIT => answer::<_, ()>(Ok(())),
                ipc::DECK_IMPORT => answer(
                    request::<DeckImportRequest>(args)
                        .and_then(|r| decks::import(decks, r).map_err(|e| json!(e))),
                ),
                ipc::DECK_LIST => answer(decks::list(decks)),
                ipc::DECK_DELETE => answer(
                    request::<DeckDeleteRequest>(args)
                        .and_then(|r| decks::delete(decks, &r).map_err(|e| json!(e))),
                ),
                // A browser cannot open the app's native file picker.
                ipc::DECK_OPEN_FILE => answer::<_, ()>(Ok(DeckOpenReply::Unavailable)),
                ipc::CARD_SEARCH => {
                    answer(request::<CardSearchRequest>(args).map(|r| decks::search(&r)))
                }
                ipc::CARD_TOKENS => {
                    answer(request::<CardTokensRequest>(args).map(|r| decks::tokens(&r)))
                }
                _ => invoke(sessions, command, args),
            };
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

fn table_command<R, T, E>(args: Value, handle: impl FnOnce(R) -> Result<T, E>) -> Value
where
    R: for<'de> Deserialize<'de>,
    T: serde::Serialize,
    E: serde::Serialize,
{
    answer(request(args).and_then(|request| handle(request).map_err(|error| json!(error))))
}

/// Runs `command` as `beryl_app` would: `{"ok": result}` or `{"err": error}`.
pub(crate) fn invoke(sessions: &Sessions, command: &str, args: Value) -> Value {
    match command {
        ipc::APP_INFO => answer::<_, ()>(Ok(AppInfo {
            version: format!("{} (live)", env!("CARGO_PKG_VERSION")),
            smoke: false,
            platform: Platform::current(),
            simulator: false,
            wire: beryl_session::wire_hash(&[]),
            profile: None,
        })),
        ipc::UI_READY => answer::<_, ()>(Ok(())),
        ipc::LIST_AGENTS => answer::<_, ()>(Ok(agents())),
        ipc::STAGE_TIMINGS => answer::<_, ()>(Ok(beryl_session::script::own_timings())),
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
            bugs::write(
                &bugs::dir(),
                &r,
                &Images::default(),
                sessions,
                &bugs::Environment::current(),
            )
            .map_err(|e| json!(e))
        })),
        ipc::UI_LOG => answer(request::<UiLogRequest>(args).map(bugs::ui_log)),
        ipc::CARD_RECORDS => card_records(args),
        _ => json!({ "err": { "kind": "unknownCommand", "command": command } }),
    }
}
