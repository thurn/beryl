//! `beryl_cli specs-serve`: the specs' moments over HTTP, each page instance its own live game,
//! for the spec page (`just specs`). It speaks `serve`'s protocol under a prefix per instance,
//! so the game screen's live transport (`?live=<prefix>`, `ui/src/ipc.ts`) plays a moment as
//! it plays an app game:
//!
//! - `GET /specs` lists the specs and their moments, each with when it was `reviewed` (null if
//!   not, [`super::reviews`]) and how many open bug `reports` are about it.
//! - `POST /review/<card>` with `{"reviewed": bool}` marks the card reviewed, or not.
//! - `GET /bug-reports.zip` downloads the active local reports.
//! - `POST /report` with `{"key": instance key, "description": ...}` writes a bug report about
//!   that instance's moment ([`beryl_session::bugs`]), with its game's replay and latest view if
//!   the instance still runs. A report from a moment's full-size page (the game screen's bug
//!   button, `report_bug`) names its moment the same way.
//! - `/m/<card>/<moment>/<view>/<instance>/events` and
//!   `/m/<card>/<moment>/<view>/<instance>/invoke/<cmd>` are `serve`'s two routes for one
//!   instance of a moment seen from `<view>` (`yours` or `theirs`, [`super::View`]), started
//!   by its first request.
//! - `POST /m/<card>/<moment>/<view>/<instance>/play` plays out what the instance holds.
//! - `GET /m/<card>/<moment>/<view>/<instance>/held` says whether it holds anything to play.
//! - `GET /stream/<page>` is one event stream for a whole page: every view of each instance
//!   named `<page>.<n>`, as a `game-update` event of `{"instance": key, "update": view}`. A
//!   browser allows a host only about six open connections, so a page of moments shares one
//!   stream and hands each frame its views, rather than each frame holding its own.
//!
//! A moment's setup plays before the page sees anything: `current_game` answers with the first
//! view after it, the one the page shows at once. The views after it, of the moment's own
//! actions and of the game playing on to the viewer's first decision (as it does from the
//! opponent's seat), are held until the viewer asks for them (`play`, or acting on the game in
//! the frame), so the page animates them from that view on, when the viewer is watching; a new
//! instance (the page's replay button) plays them again. An instance outside a page (a
//! full-size frame, or a headless screenshot) has no play button, so it sends them a beat after
//! its first view.

use std::collections::HashMap;
use std::io::{self, Seek, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use beryl_session::bugs::{self, BugReport, Images, PageInfo, SpecMoment};
use beryl_session::ipc;
use beryl_session::{GameUpdate, Sessions, Status};
use serde_json::{Value, json};

use super::{Phase, View, find, listing, reviews, slug, specs};
use crate::serve::{CORS, HttpRequest, invoke, read_request, respond};

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
    beryl_session::journal::install_panic_hook();
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
    let HttpRequest {
        method,
        path,
        headers,
        body,
    } = read_request(&stream)?;
    let moment_report = path.starts_with("/m/")
        && path
            .rsplit_once("/invoke/")
            .is_some_and(|(_, command)| command == ipc::REPORT_BUG);
    if (path == "/report" || path == "/bug-reports.zip" || moment_report)
        && !local_reports_request(&headers)
    {
        return respond(&mut stream, "403 Forbidden", "{}");
    }
    if method == "OPTIONS" {
        return respond(&mut stream, "204 No Content", "");
    }
    if method == "GET" && path == "/bug-reports.zip" {
        return export_reports(&mut stream);
    }
    if method == "GET" && path == "/specs" {
        let body = annotated_listing().to_string();
        return respond(&mut stream, "200 OK", &body);
    }
    if let ("POST", Some(card)) = (method.as_str(), path.strip_prefix("/review/")) {
        let answer = review(card, &body);
        return respond(&mut stream, "200 OK", &answer.to_string());
    }
    if method == "POST" && path == "/report" {
        let answer = report(instances, &body);
        return respond(&mut stream, "200 OK", &answer.to_string());
    }
    if let ("GET", Some(page)) = (method.as_str(), path.strip_prefix("/stream/")) {
        let (sent, received) = channel();
        lock(pages).insert(page.to_owned(), sent);
        return pump(stream, &received);
    }
    let Some(rest) = path.strip_prefix("/m/") else {
        return respond(&mut stream, "404 Not Found", "{}");
    };
    handle_moment(stream, instances, pages, &method, rest, &body)
}

fn handle_moment(
    mut stream: TcpStream,
    instances: &Instances,
    pages: &Pages,
    method: &str,
    rest: &str,
    body: &[u8],
) -> Result<(), String> {
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
    match (method, route) {
        ("GET", "events") => events(stream, &feed),
        ("POST", "play") => {
            release(&feed);
            respond(&mut stream, "200 OK", "{\"ok\":null}")
        }
        ("GET", "held") => {
            let answer = json!({ "ok": holds(&lock(&feed.0)) });
            respond(&mut stream, "200 OK", &answer.to_string())
        }
        ("POST", command) if command.starts_with("invoke/") => {
            let command = &command["invoke/".len()..];
            let args: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            let answer = match command {
                ipc::CURRENT_GAME => current(&feed),
                // The moment is the game: the page never starts another.
                ipc::START_GAME | ipc::CONCEDE | ipc::RESUME_GAME => json!({ "ok": null }),
                ipc::LIST_AGENTS => json!({ "ok": [] }),
                _ => {
                    // The viewer acts on the game: it plays on from where it truly is.
                    if matches!(
                        command,
                        ipc::CHOOSE | ipc::CANCEL | ipc::UNDO | ipc::PASS_ON
                    ) {
                        release(&feed);
                    }
                    let list = lock(instances);
                    let Some(i) = list.iter().find(|i| i.key == key) else {
                        return respond(&mut stream, "200 OK", "{\"err\":{\"kind\":\"ended\"}}");
                    };
                    if command == ipc::REPORT_BUG {
                        // The full-size page's bug button: the report names its moment.
                        let spec = SpecMoment {
                            card: card.to_owned(),
                            moment: name.to_owned(),
                            view: view.to_owned(),
                        };
                        write_report(&i.sessions, args, spec)
                    } else {
                        invoke(&i.sessions, command, args)
                    }
                }
            };
            respond(&mut stream, "200 OK", &answer.to_string())
        }
        _ => respond(&mut stream, "404 Not Found", "{}"),
    }
}

/// Report writes can create local beads, and bundles contain private diagnostics.
/// Accept the loopback Vite proxy's same-origin requests and local command-line clients.
fn local_reports_request(headers: &HashMap<String, String>) -> bool {
    let Some(host) = headers.get("host") else {
        return false;
    };
    let (name, port) = host.split_once(':').unwrap_or((host, "80"));
    matches!(name, "localhost" | "127.0.0.1")
        && port.parse::<u16>().is_ok()
        && headers
            .get("origin")
            .is_none_or(|origin| origin.strip_prefix("http://") == Some(host))
        && headers
            .get("sec-fetch-site")
            .is_none_or(|site| matches!(site.as_str(), "same-origin" | "none"))
}

fn export_reports(stream: &mut TcpStream) -> Result<(), String> {
    let bundle = (|| -> io::Result<_> {
        let mut bundle = tempfile::tempfile()?;
        let folders = bugs::archive::active(&bugs::dir())?;
        bugs::archive::write(&folders, &mut bundle)?;
        bundle.rewind()?;
        Ok((bundle.metadata()?.len(), bundle))
    })();
    let (length, mut bundle) = match bundle {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("specs: could not export bug reports: {error}");
            return respond(stream, "500 Internal Server Error", "{}");
        }
    };
    // Keep the ZIP off the spec API's cross-origin JSON response path.
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\n\
         Content-Disposition: attachment; filename=beryl-bug-reports.zip\r\n\
         Cache-Control: no-store\r\nCross-Origin-Resource-Policy: same-origin\r\n\
         Content-Length: {length}\r\nConnection: close\r\n\r\n"
    )
    .and_then(|()| io::copy(&mut bundle, stream).map(|_| ()))
    .map_err(|e| e.to_string())
}

/// The specs as `GET /specs` lists them: each with when it was reviewed and its open reports.
fn annotated_listing() -> Value {
    let reviewed = reviews::load(&reviews::path());
    let reports = bugs::open_by_card(&bugs::dir());
    let mut listing = serde_json::to_value(listing()).unwrap_or_else(|_| json!([]));
    if let Some(specs) = listing.as_array_mut() {
        for spec in specs {
            let card = spec["card"].as_str().unwrap_or_default().to_owned();
            let slug = spec["slug"].as_str().unwrap_or_default().to_owned();
            spec["reviewed"] = json!(reviewed.get(&card));
            spec["reports"] = json!(reports.get(&slug).copied().unwrap_or(0));
        }
    }
    listing
}

/// `POST /review/<card>`: sets the review mark and, when marking, finds the next unreviewed spec.
fn review(card: &str, body: &[u8]) -> Value {
    let specs = specs();
    let Some(index) = specs.iter().position(|s| slug(s.card) == card) else {
        return json!({ "err": { "kind": "spec", "message": format!("no spec {card}") } });
    };
    let Some(reviewed) = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v["reviewed"].as_bool())
    else {
        return json!({ "err": { "kind": "badRequest", "message": "want {\"reviewed\": bool}" } });
    };
    let spec = &specs[index];
    match reviews::set(&reviews::path(), spec.card, reviewed) {
        Ok(all) => {
            let next = if reviewed {
                specs[index + 1..]
                    .iter()
                    .chain(&specs[..index])
                    .find(|s| !all.contains_key(s.card))
                    .map(|s| slug(s.card))
            } else {
                None
            };
            json!({ "ok": { "reviewed": all.get(spec.card), "next": next } })
        }
        Err(message) => json!({ "err": { "kind": "io", "message": message } }),
    }
}

/// `POST /report`: a note from the spec page about the moment an instance plays, written as a
/// bug report with what its game knows, if it still runs.
fn report(instances: &Instances, body: &[u8]) -> Value {
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (Some(key), Some(description)) = (request["key"].as_str(), request["description"].as_str())
    else {
        return json!({ "err": { "kind": "badRequest", "message": "want key and description" } });
    };
    if description.trim().is_empty() {
        return json!({ "err": { "kind": "badRequest", "message": "an empty note" } });
    }
    let mut parts = key.splitn(4, '/');
    let (Some(card), Some(moment), Some(view)) = (parts.next(), parts.next(), parts.next()) else {
        return json!({ "err": { "kind": "badRequest", "message": format!("no moment in {key}") } });
    };
    if find(card, moment).is_none() || View::parse(view).is_none() {
        return json!({ "err": { "kind": "spec", "message": format!("no moment {key}") } });
    }
    let spec = SpecMoment {
        card: card.to_owned(),
        moment: moment.to_owned(),
        view: view.to_owned(),
    };
    let args = json!({ "request": {
        "description": description,
        "ai": false,
        "snapshot": null,
        "region": null,
        "page": PageInfo { url: format!("/specs/{spec}"), width: 0.0, height: 0.0, scale: 1.0 },
        "uiLog": [],
        "updates": [],
    }});
    let list = lock(instances);
    match list.iter().find(|i| i.key == key) {
        Some(i) => write_report(&i.sessions, args, spec),
        None => write_report(&Sessions::new(None, |_| {}), args, spec),
    }
}

/// Writes the `report_bug` request in `args` as a report about `spec`, with what `sessions`
/// knows of its game.
fn write_report(sessions: &Sessions, args: Value, spec: SpecMoment) -> Value {
    #[derive(serde::Deserialize)]
    struct Request {
        request: BugReport,
    }
    let mut report = match serde_json::from_value::<Request>(args) {
        Ok(r) => r.request,
        Err(e) => return json!({ "err": { "kind": "badRequest", "message": e.to_string() } }),
    };
    report.spec = Some(spec);
    let root = bugs::dir();
    match bugs::write(&root, &report, &Images::default(), sessions) {
        Ok(saved) => {
            bugs::filing::enqueue(&root, &saved.id);
            json!({ "ok": saved })
        }
        Err(e) => json!({ "err": e }),
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
    // An instance named `hold-…` waits for its `play`, as a page's does: a driver starts it
    // when it is watching.
    let held = f
        .key
        .rsplit('/')
        .next()
        .is_some_and(|id| id.starts_with("hold-"));
    if !f.released && f.page.is_none() && !held {
        let feed = Arc::clone(feed);
        std::thread::spawn(move || {
            std::thread::sleep(BEAT);
            release(&feed);
        });
    }
    json!({ "ok": shown })
}

/// Whether the instance, its first view shown, holds views to play: its moment's own actions,
/// or a game that goes on without the viewer, not yet waiting on their decision.
fn holds(f: &Feed) -> bool {
    !f.released
        && f.start.is_some_and(|start| {
            f.plays || !matches!(f.views[start].view.status, Status::Deciding { .. })
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_routes_only_accept_local_requests() {
        let headers = |host: &str, origin: &str, site: &str| {
            [
                ("host".to_owned(), host.to_owned()),
                ("origin".to_owned(), origin.to_owned()),
                ("sec-fetch-site".to_owned(), site.to_owned()),
            ]
            .into_iter()
            .collect()
        };
        assert!(local_reports_request(&headers(
            "localhost:1430",
            "http://localhost:1430",
            "same-origin"
        )));
        assert!(!local_reports_request(&headers(
            "localhost:1430",
            "http://localhost:9999",
            "same-site"
        )));
        assert!(!local_reports_request(&headers(
            "example.org:1430",
            "http://example.org:1430",
            "same-origin"
        )));
        let mut navigation = HashMap::from([("host".to_owned(), "127.0.0.1:1430".to_owned())]);
        assert!(local_reports_request(&navigation));
        navigation.insert("sec-fetch-site".to_owned(), "cross-site".to_owned());
        assert!(!local_reports_request(&navigation));
    }
}
