//! `beryl_cli specs-serve`: the specs' moments over HTTP, each page instance its own live game,
//! for the spec page (`just specs`). It speaks `serve`'s protocol under a prefix per instance,
//! so the game screen's live transport (`?live=<prefix>`, `ui/src/ipc.ts`) plays a moment as
//! it plays an app game:
//!
//! - `GET /specs` lists the specs and their moments.
//! - `/m/<card>/<moment>/<view>/<instance>/events` and
//!   `/m/<card>/<moment>/<view>/<instance>/invoke/<cmd>` are `serve`'s two routes for one
//!   instance of a moment seen from `<view>` (`yours` or `theirs`, [`super::View`]), started
//!   by its first request.
//! - `POST /m/<card>/<moment>/<view>/<instance>/play` plays out the moment's own actions.
//! - `GET /stream/<page>` is one event stream for a whole page: every view of each instance
//!   named `<page>.<n>`, as a `game-update` event of `{"instance": key, "update": view}`. A
//!   browser allows a host only about six open connections, so a page of moments shares one
//!   stream and hands each frame its views, rather than each frame holding its own.
//!
//! A moment's setup plays before the page sees anything: `current_game` answers with the first
//! view after it, the one the page shows at once. The views of the moment's own actions are
//! held until the viewer asks for them (`play`, or acting on the game in the frame), so the page
//! animates them from that view on, when the viewer is watching; a new instance (the page's
//! replay button) plays them again. An instance outside a page (a full-size frame, or a
//! headless screenshot), or of a moment with nothing to play, has no play button, so it sends
//! them a beat after its first view: seen from the opponent's seat, the game plays on to the
//! viewer's first decision.

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use beryl_session::ipc;
use beryl_session::{GameUpdate, Sessions};
use serde_json::{Value, json};

use super::{Phase, View, find, listing};
use crate::serve::{CORS, invoke, read_request, respond};

/// How many instances live at once; the least recently used goes first. A page keeps each
/// moment it has shown.
const INSTANCES: usize = 64;
/// How long `current_game` waits for a moment's setup.
const SETUP: Duration = Duration::from_secs(20);
/// The beat between a moment outside a page showing and its actions playing out.
const BEAT: Duration = Duration::from_millis(900);
/// How often an idle event stream is pinged, which finds a page gone.
const PING: Duration = Duration::from_secs(5);

/// One instance's views, as its game emits them.
#[derive(Default)]
struct Feed {
    views: Vec<GameUpdate>,
    /// The first view after the setup: what the page shows at once.
    start: Option<usize>,
    /// Whether views past the start go to the streams yet.
    released: bool,
    /// Whether its moment has actions of its own to play, which a page holds for its button.
    plays: bool,
    streams: Vec<Sender<String>>,
    /// Its key, and the page stream it goes to as well, if its key names a page.
    key: String,
    page: Option<(Pages, String)>,
}

/// Each page's stream, by the page's name.
type Pages = Arc<Mutex<HashMap<String, Sender<String>>>>;

type SharedFeed = Arc<(Mutex<Feed>, Condvar)>;

struct Instance {
    key: String,
    /// Keeps the game running; dropped, it stops.
    sessions: Sessions,
    feed: SharedFeed,
    used: Instant,
}

type Instances = Arc<Mutex<Vec<Instance>>>;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Serves the specs on `127.0.0.1:port` until the process is killed.
pub fn serve(port: u16) -> Result<(), String> {
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    println!("beryl_cli: serving specs on http://127.0.0.1:{port}");
    let instances: Instances = Arc::default();
    let pages: Pages = Arc::default();
    for connection in listener.incoming() {
        let Ok(connection) = connection else { continue };
        let instances = Arc::clone(&instances);
        let pages = Arc::clone(&pages);
        std::thread::spawn(move || {
            if let Err(e) = handle(connection, &instances, &pages) {
                eprintln!("beryl_cli: {e}");
            }
        });
    }
    Ok(())
}

fn handle(mut stream: TcpStream, instances: &Instances, pages: &Pages) -> Result<(), String> {
    let (method, path, body) = read_request(&stream)?;
    if method == "OPTIONS" {
        return respond(&mut stream, "204 No Content", "");
    }
    if method == "GET" && path == "/specs" {
        let body = serde_json::to_string(&listing()).map_err(|e| e.to_string())?;
        return respond(&mut stream, "200 OK", &body);
    }
    if let ("GET", Some(page)) = (method.as_str(), path.strip_prefix("/stream/")) {
        let (sent, received) = channel();
        lock(pages).insert(page.to_owned(), sent);
        return pump(stream, &received);
    }
    let Some(rest) = path.strip_prefix("/m/") else {
        return respond(&mut stream, "404 Not Found", "{}");
    };
    let mut parts = rest.splitn(5, '/');
    let (Some(card), Some(name), Some(view), Some(id), Some(route)) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return respond(&mut stream, "404 Not Found", "{}");
    };
    let key = format!("{card}/{name}/{view}/{id}");
    let (sessions_feed, error) = match instance(instances, pages, &key, card, name, view) {
        Ok(found) => (Some(found), None),
        Err(e) => (None, Some(e)),
    };
    let Some(feed) = sessions_feed else {
        let error = error.unwrap_or_default();
        eprintln!("beryl_cli: {key}: {error}");
        let body = json!({ "err": { "kind": "spec", "message": error } });
        return respond(&mut stream, "200 OK", &body.to_string());
    };
    match (method.as_str(), route) {
        ("GET", "events") => events(stream, &feed),
        ("POST", "play") => {
            release(&feed);
            respond(&mut stream, "200 OK", "{\"ok\":null}")
        }
        ("POST", command) if command.starts_with("invoke/") => {
            let command = &command["invoke/".len()..];
            let args: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let answer = match command {
                ipc::CURRENT_GAME => current(&feed),
                // The moment is the game: the page never starts another.
                ipc::START_GAME | ipc::CONCEDE => json!({ "ok": null }),
                ipc::LIST_AGENTS => json!({ "ok": [] }),
                _ => {
                    // The viewer acts on the game: it plays on from where it truly is.
                    if matches!(command, ipc::CHOOSE | ipc::CANCEL | ipc::UNDO) {
                        release(&feed);
                    }
                    let list = lock(instances);
                    let Some(i) = list.iter().find(|i| i.key == key) else {
                        return respond(&mut stream, "200 OK", "{\"err\":{\"kind\":\"ended\"}}");
                    };
                    invoke(&i.sessions, command, args)
                }
            };
            respond(&mut stream, "200 OK", &answer.to_string())
        }
        _ => respond(&mut stream, "404 Not Found", "{}"),
    }
}

/// The instance `key` names, started from the moment if new.
fn instance(
    instances: &Instances,
    pages: &Pages,
    key: &str,
    card: &str,
    name: &str,
    view: &str,
) -> Result<SharedFeed, String> {
    let mut list = lock(instances);
    if let Some(i) = list.iter_mut().find(|i| i.key == key) {
        i.used = Instant::now();
        return Ok(Arc::clone(&i.feed));
    }
    let (_, moment) = find(card, name).ok_or_else(|| format!("no moment {card}/{name}"))?;
    let view = View::parse(view).ok_or_else(|| format!("no view {view:?}"))?;
    let feed: SharedFeed = Arc::default();
    {
        let mut f = lock(&feed.0);
        key.clone_into(&mut f.key);
        f.plays = !moment.play.is_empty();
        // An instance `<page>.<n>` goes to its page's stream.
        let id = key.rsplit('/').next().unwrap_or_default();
        f.page = id
            .split_once('.')
            .map(|(page, _)| (Arc::clone(pages), page.to_owned()));
    }
    let emitted = Arc::clone(&feed);
    let (sessions, _) = super::start(moment, view, move |update, phase| {
        let (feed, ready) = &*emitted;
        let mut f = lock(feed);
        f.views.push(update.clone());
        if f.start.is_none() && phase != Phase::Setup {
            f.start = Some(f.views.len() - 1);
            ready.notify_all();
        } else if f.released {
            broadcast(&mut f, &update);
        }
    })?;
    if list.len() >= INSTANCES
        && let Some(oldest) = list
            .iter()
            .enumerate()
            .min_by_key(|(_, i)| i.used)
            .map(|(n, _)| n)
    {
        list.swap_remove(oldest);
    }
    list.push(Instance {
        key: key.to_owned(),
        sessions,
        feed: Arc::clone(&feed),
        used: Instant::now(),
    });
    Ok(feed)
}

fn broadcast(feed: &mut Feed, update: &GameUpdate) {
    let Ok(data) = serde_json::to_string(update) else {
        return;
    };
    let message = format!("event: {}\ndata: {data}\n\n", ipc::GAME_UPDATE);
    feed.streams.retain(|s| s.send(message.clone()).is_ok());
    if let Some((pages, page)) = &feed.page {
        let message = format!(
            "event: {}\ndata: {{\"instance\":{},\"update\":{data}}}\n\n",
            ipc::GAME_UPDATE,
            json!(feed.key),
        );
        let mut pages = lock(pages);
        if pages.get(page).is_some_and(|s| s.send(message).is_err()) {
            pages.remove(page);
        }
    }
}

/// The view the page shows at once, once the setup is done. Outside a page, the views since
/// go to its streams a beat later.
fn current(feed: &SharedFeed) -> Value {
    let (lockable, ready) = &**feed;
    let deadline = Instant::now() + SETUP;
    let mut f = lock(lockable);
    while f.start.is_none() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return json!({ "err": { "kind": "spec", "message": "the setup never finished" } });
        }
        f = ready
            .wait_timeout(f, left)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
    let start = f.start.unwrap_or_default();
    let mut shown = f.views[start].clone();
    // Shown at once, it animates nothing.
    shown.view.events.clear();
    if !f.released && (f.page.is_none() || !f.plays) {
        let feed = Arc::clone(feed);
        std::thread::spawn(move || {
            std::thread::sleep(BEAT);
            release(&feed);
        });
    }
    json!({ "ok": shown })
}

/// Sends the views held since the first one shown to the instance's streams, once its setup
/// is done; later views go as they come.
fn release(feed: &SharedFeed) {
    let mut f = lock(&feed.0);
    let Some(start) = f.start else { return };
    if f.released {
        return;
    }
    f.released = true;
    let later = f.views.split_off(start + 1);
    for update in &later {
        broadcast(&mut f, update);
    }
}

/// The instance's views from now on, pinged while idle, until the page goes away.
fn events(stream: TcpStream, feed: &SharedFeed) -> Result<(), String> {
    let (sent, received) = channel();
    lock(&feed.0).streams.push(sent);
    pump(stream, &received)
}

/// Writes an event stream of what `received` gets, pinged while idle, until the page goes
/// away. A first comment goes at once: a browser behind a proxy (Vite's) hears the stream open
/// only once bytes arrive, and the page asks for its first view only once it has.
fn pump(mut stream: TcpStream, received: &std::sync::mpsc::Receiver<String>) -> Result<(), String> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\n{CORS}Content-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n: open\n\n"
    )
    .map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;
    loop {
        let message = match received.recv_timeout(PING) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => ": ping\n\n".to_owned(),
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if stream.write_all(message.as_bytes()).is_err() || stream.flush().is_err() {
            break;
        }
    }
    Ok(())
}
