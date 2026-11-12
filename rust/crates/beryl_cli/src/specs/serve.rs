//! `beryl_cli specs-serve`: the specs' moments over HTTP, each page instance its own live game,
//! for the spec page (`just specs`). It speaks `serve`'s protocol under a prefix per instance,
//! so the game screen's live transport (`?live=<prefix>`, `ui/src/ipc.ts`) plays a moment as
//! it plays an app game:
//!
//! - `GET /specs` lists the cards, generating none, each with when it was `reviewed` (null if
//!   not, [`super::reviews`]) and how many open bug `reports` are about it; `GET /specs/<card>`
//!   generates that card's spec and lists its moments ([`super::listing`]).
//! - `POST /review/<card>` with `{"reviewed": bool}` marks the card reviewed, or not.
//! - `GET /bug-reports.zip` downloads the active local reports ([`reports`]).
//! - `POST /invoke/card_records` answers the card store's records for the names asked, as the
//!   app's command does: for the page's list of cards, which is no moment's.
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
//! A moment's setup plays before the page sees anything: `current_game` answers with the first view
//! after it, the one the page shows at once. The views after it, of the moment's own actions and of
//! the game playing on to the viewer's first decision (as it does from the opponent's seat), are
//! held until the viewer asks for them (`play`, or acting on the game in the frame), so the page
//! animates them from that view on, when the viewer is watching; a new instance (the page's replay
//! button) plays them again. Full-size and headless views hold their replay the same way.

use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use beryl_scenario::Action;
use beryl_session::bugs::SpecMoment;
use beryl_session::ipc;
use beryl_session::protocol::{Candidate, GameEvent, PlayerView, Side};
use beryl_session::script::Follow;
use beryl_session::{CandidateIndex, DecisionId, GameUpdate, Sessions, Status};
use serde_json::{Value, json};

use super::{Phase, View, cards, find, listing, reviews, spec};
use crate::serve::{CORS, HttpRequest, card_records, invoke, read_request, respond};

mod reports;

/// How many instances live at once; the least recently used goes first. A page keeps each
/// moment it has shown.
const INSTANCES: usize = 64;
/// How long `current_game` waits for a moment's setup.
const SETUP: Duration = Duration::from_secs(20);
/// How often an idle event stream is pinged, which finds a page gone.
const PING: Duration = Duration::from_secs(5);

#[derive(Default, PartialEq, Eq)]
enum FeedMode {
    #[default]
    Replay,
    Manual,
    Playground,
}

/// One instance's views, as its game emits them.
#[derive(Default)]
struct Feed {
    views: Vec<GameUpdate>,
    /// Views delivered to this board, retained for reproducible UI bug fixtures.
    history: Vec<GameUpdate>,
    /// The first view after the setup: what the page shows at once.
    start: Option<usize>,
    /// Whether views past the start go to the streams yet.
    released: bool,
    /// Whether its moment has actions of its own to play, which a page holds for its button.
    plays: bool,
    mode: FeedMode,
    opponent_move: Option<(DecisionId, CandidateIndex)>,
    streams: Vec<Sender<String>>,
    /// The seat's screen as the views so far leave it, which the view shown at once starts
    /// over from.
    follow: Follow,
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
    mirror: Option<SharedFeed>,
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
        && !reports::local_request(&headers)
    {
        return respond(&mut stream, "403 Forbidden", "{}");
    }
    if method == "OPTIONS" {
        return respond(&mut stream, "204 No Content", "");
    }
    if method == "GET" && path == "/bug-reports.zip" {
        return reports::export(&mut stream);
    }
    if method == "POST" && path == "/invoke/card_records" {
        // The list of cards asks for their records, which no moment's game is needed for.
        let args = serde_json::from_slice(&body).unwrap_or(Value::Null);
        return respond(&mut stream, "200 OK", &card_records(args).to_string());
    }
    if method == "GET" && path == "/specs" {
        let body = listing::cards().to_string();
        return respond(&mut stream, "200 OK", &body);
    }
    if let ("GET", Some(card)) = (method.as_str(), path.strip_prefix("/specs/")) {
        let Some(spec) = spec(card) else {
            return respond(&mut stream, "404 Not Found", "{}");
        };
        return respond(&mut stream, "200 OK", &listing::moments(spec).to_string());
    }
    if let ("GET", Some(card)) = (method.as_str(), path.strip_prefix("/playground/")) {
        let Some(spec) = spec(card) else {
            return respond(&mut stream, "404 Not Found", "{}");
        };
        return respond(
            &mut stream,
            "200 OK",
            &super::playground::listing(spec.playground()).to_string(),
        );
    }
    if let ("GET", Some(card)) = (method.as_str(), path.strip_prefix("/checkpoints/")) {
        let Some(spec) = spec(card) else {
            return respond(&mut stream, "404 Not Found", "{}");
        };
        let checkpoints = spec.checkpoints();
        let listing: Vec<_> = checkpoints
            .examples
            .iter()
            .map(|c| {
                json!({
                    "name": c.moment.name,
                    "title": c.moment.title,
                    "group": c.moment.group,
                    "plays": false,
                    "first": c.first.name(),
                    "roles": c.roles,
                    "actions": c.actions,
                    "opponentMove": !c.review_casts.is_empty(),
                    "replay": spec.replay_from_hand(c.moment.group),
                })
            })
            .collect();
        return respond(
            &mut stream,
            "200 OK",
            &json!({"examples": listing, "unreached": checkpoints.unreached}).to_string(),
        );
    }
    if let ("POST", Some(card)) = (method.as_str(), path.strip_prefix("/review/")) {
        let answer = review(card, &body);
        return respond(&mut stream, "200 OK", &answer.to_string());
    }
    if method == "POST" && path == "/report" {
        let answer = reports::report(instances, &body);
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
        ("GET", "opponent-move") => {
            let decision = lock(&feed.0).opponent_move.map(|(id, _)| id);
            respond(
                &mut stream,
                "200 OK",
                &json!({ "ok": decision }).to_string(),
            )
        }
        ("POST", "opponent-move") => {
            let expected = serde_json::from_slice::<DecisionId>(body).ok();
            let list = lock(instances);
            let step = lock(&feed.0).opponent_move;
            let played = step.is_some_and(|(decision, index)| {
                expected == Some(decision)
                    && list
                        .iter()
                        .find(|i| i.key == instance_key(card, name, view, id))
                        .is_some_and(|i| i.sessions.choose(decision, index).is_ok())
            });
            respond(&mut stream, "200 OK", &json!({ "ok": played }).to_string())
        }
        ("POST", command) if command.starts_with("invoke/") => {
            let command = &command["invoke/".len()..];
            let args: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            let answer = match command {
                ipc::CURRENT_GAME => current(&feed),
                // The moment is the game: the page never starts another.
                ipc::START_GAME | ipc::CONCEDE | ipc::RESUME_GAME => json!({ "ok": null }),
                ipc::LIST_AGENTS => json!({ "ok": [] }),
                ipc::CARD_RECORDS => card_records(args),
                _ => {
                    // The viewer acts on the game: it plays on from where it truly is.
                    if matches!(
                        command,
                        ipc::CHOOSE | ipc::CLICK | ipc::CANCEL | ipc::UNDO | ipc::PASS_ON
                    ) {
                        release(&feed);
                    }
                    let list = lock(instances);
                    let Some(i) = list
                        .iter()
                        .find(|i| i.key == instance_key(card, name, view, id))
                    else {
                        return respond(&mut stream, "200 OK", "{\"err\":{\"kind\":\"ended\"}}");
                    };
                    if command == ipc::REPORT_BUG {
                        // The full-size page's bug button: the report names its moment.
                        let spec = SpecMoment {
                            card: card.to_owned(),
                            moment: name.to_owned(),
                            view: view.to_owned(),
                        };
                        reports::write_report(&i.sessions, args, spec, false)
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

/// `POST /review/<card>`: sets the review mark and, when marking, finds the next unreviewed card.
fn review(card: &str, body: &[u8]) -> Value {
    let specs = cards();
    let Some(index) = specs.iter().position(|s| s.slug == card) else {
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
                    .map(|s| s.slug.clone())
            } else {
                None
            };
            json!({ "ok": { "reviewed": all.get(spec.card), "next": next } })
        }
        Err(message) => json!({ "err": { "kind": "io", "message": message } }),
    }
}

fn checkpoint(card: &str, name: &str) -> Option<&'static super::auto::Checkpoint> {
    if !name.starts_with("review-") {
        return None;
    }
    spec(card)?
        .checkpoints()
        .examples
        .iter()
        .find(|c| c.moment.name == name)
}

fn instance_key(card: &str, name: &str, view: &str, id: &str) -> String {
    let view = if name.starts_with("review-") {
        "yours"
    } else {
        view
    };
    format!("{card}/{name}/{view}/{id}")
}

fn new_feed(pages: &Pages, key: &str, plays: bool, manual: bool) -> SharedFeed {
    let feed: SharedFeed = Arc::default();
    let mut f = lock(&feed.0);
    key.clone_into(&mut f.key);
    f.plays = plays;
    f.mode = if manual {
        FeedMode::Manual
    } else {
        FeedMode::Replay
    };
    // An instance `<page>.<n>` goes to its page's stream.
    f.page = key
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split_once('.')
        .map(|(page, _)| (Arc::clone(pages), page.to_owned()));
    drop(f);
    feed
}

/// Both seats of a manual checkpoint share one session and have their own feeds.
fn instance(
    instances: &Instances,
    pages: &Pages,
    key: &str,
    card: &str,
    name: &str,
    view: &str,
) -> Result<SharedFeed, String> {
    let view = View::parse(view).ok_or_else(|| format!("no view {view:?}"))?;
    let manual = name.starts_with("review-");
    let id = key.rsplit('/').next().unwrap_or_default();
    let key = instance_key(card, name, view.name(), id);
    // Generate the card's checkpoints outside the shared instances lock.
    let review = checkpoint(card, name);
    let playground = super::playground::find(card, name);
    let moment = if let Some(example) = playground {
        Some(&example.moment)
    } else if manual {
        review.map(|c| &c.moment)
    } else {
        find(card, name).map(|(_, m)| m)
    }
    .ok_or_else(|| format!("no moment {card}/{name}"))?;
    let mut list = lock(instances);
    let pick = |i: &Instance| {
        Arc::clone(if view == View::Theirs {
            i.mirror.as_ref().unwrap_or(&i.feed)
        } else {
            &i.feed
        })
    };
    if let Some(i) = list.iter_mut().find(|i| i.key == key) {
        i.used = Instant::now();
        return Ok(pick(i));
    }
    let plays = playground.map_or(!manual && !moment.play.is_empty(), |v| {
        super::playground::automated(v, view)
    });
    let feed = new_feed(pages, &key, plays, manual);
    if playground.is_some() {
        lock(&feed.0).mode = FeedMode::Playground;
    }
    let emitted = Arc::clone(&feed);
    let (sessions, mirror) = if let Some(example) = playground {
        let sessions = super::playground::start(example, view, move |update, phase| {
            record_view(&emitted, &update, phase);
        })?;
        (sessions, None)
    } else if manual {
        let mirror = new_feed(pages, &format!("{card}/{name}/theirs/{id}"), false, true);
        let other = Arc::clone(&mirror);
        let casts = Mutex::new(review.map_or(&[][..], |c| c.review_casts));
        let sessions = super::start_review(moment, move |updates, live| {
            let ready = super::at_checkpoint(&updates, live);
            let moves = if live {
                recorded_cast_moves(&mut lock(&casts), &updates)
            } else {
                [None; 2]
            };
            for ((shared, update), step) in [&emitted, &other].into_iter().zip(updates).zip(moves) {
                let (feed, changed) = &**shared;
                let mut f = lock(feed);
                f.opponent_move = step;
                if f.start.is_some() {
                    broadcast(&mut f, &update);
                } else if !ready {
                    follow(&mut f, &update);
                    continue;
                }
                // What a page opened from here on shows at once.
                let mut shown = update;
                shown_first(&mut f, &mut shown);
                f.views.clear();
                f.views.push(shown);
                f.start = Some(0);
                changed.notify_all();
            }
        })?;
        (sessions, Some(mirror))
    } else {
        let (sessions, _) = super::start(moment, view, move |update, phase| {
            record_view(&emitted, &update, phase);
        })?;
        (sessions, None)
    };
    if list.len() >= INSTANCES
        && let Some(oldest) = list
            .iter()
            .enumerate()
            .min_by_key(|(_, i)| i.used)
            .map(|(n, _)| n)
    {
        list.swap_remove(oldest);
    }
    let instance = Instance {
        key,
        sessions,
        feed,
        mirror,
        used: Instant::now(),
    };
    let selected = pick(&instance);
    list.push(instance);
    Ok(selected)
}

fn record_view(shared: &SharedFeed, update: &GameUpdate, phase: Phase) {
    let (feed, ready) = &**shared;
    let mut f = lock(feed);
    if f.start.is_none()
        && phase != Phase::Setup
        && !(f.mode == FeedMode::Playground && matches!(update.view.status, Status::Playing))
    {
        let mut initial = update.clone();
        shown_first(&mut f, &mut initial);
        f.views.push(initial.clone());
        f.start = Some(f.views.len() - 1);
        initial.view.events.clear();
        remember(&mut f, &initial);
        ready.notify_all();
    } else {
        if f.start.is_none() {
            follow(&mut f, update);
        }
        f.views.push(update.clone());
        if f.released {
            broadcast(&mut f, update);
        }
    }
}

/// Follows the seat's screen through `update`, a view the page never shows at once.
fn follow(feed: &mut Feed, update: &GameUpdate) {
    if let Err(error) = feed.follow.play(&update.view) {
        eprintln!("beryl_cli: {}: {error}", feed.key);
    }
}

/// Gives `update`, which the page shows at once, the script that starts the page's stage over
/// where the views before it left the seat's screen, in place of the one it came with (a
/// reset numbering the abilities on its stack as the scripts after it name them).
fn shown_first(feed: &mut Feed, update: &mut GameUpdate) {
    if let Err(error) = super::describe::restart(&mut feed.follow, &mut update.view) {
        eprintln!("beryl_cli: {}: {error}", feed.key);
    }
}

/// Follow the recorded casts only while the game takes them in order. The other seat may
/// take the next legal cast explicitly, by the same decision ID as a click in the game.
pub(super) fn recorded_cast_moves(
    remaining: &mut &'static [Action],
    updates: &[GameUpdate; 2],
) -> [Option<(DecisionId, CandidateIndex)>; 2] {
    if remaining.is_empty() {
        return [None; 2];
    }
    let first = &updates[0].view;
    for event in &first.events {
        if let GameEvent::Cast {
            slot, controller, ..
        } = *event
        {
            let matches = remaining.first().is_some_and(|action| {
                matches!(*action, Action::Cast { player, card, .. }
                    if (player.index() == 0) == (controller == Side::You)
                        && named(first, slot, card))
            });
            *remaining = if matches { &remaining[1..] } else { &[] };
        }
    }
    let Some(Action::Cast {
        player,
        card,
        face,
        costs,
    }) = remaining.first().copied()
    else {
        return [None; 2];
    };
    let view = &updates[player.index()].view;
    let Status::Deciding { decision } = &view.status else {
        return [None; 2];
    };
    let mut choices = decision.candidates.iter().enumerate().filter(|(_, c)| {
        matches!(&c.option, Candidate::Cast { slot, face: f, costs: paid }
            if *f == face.get() && paid.iter().copied().eq(costs.indices().map(beryl_engine::card::CastingIx::get)) && named(view, *slot, card))
    });
    let mut moves = [None; 2];
    if let Some((i, _)) = choices.next()
        && choices.next().is_none()
        && let Ok(index) = u16::try_from(i)
    {
        moves[1 - player.index()] = Some((decision.id, CandidateIndex(index)));
    }
    moves
}

fn named(view: &PlayerView, slot: u16, card: &str) -> bool {
    view.objects
        .iter()
        .any(|o| o.slot == slot && o.name.as_deref() == Some(card))
}

fn remember(feed: &mut Feed, update: &GameUpdate) {
    const HISTORY: usize = 128;
    if feed.history.len() == HISTORY {
        feed.history.remove(0);
    }
    feed.history.push(update.clone());
}

fn broadcast(feed: &mut Feed, update: &GameUpdate) {
    remember(feed, update);
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

/// The view the page shows at once, once the setup is done.
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
    json!({ "ok": shown })
}

/// Whether the instance, its first view shown, holds views to play: its moment's own actions,
/// or a game that goes on without the viewer, not yet waiting on their decision.
fn holds(f: &Feed) -> bool {
    f.mode != FeedMode::Manual
        && !f.released
        && f.start.is_some_and(|start| {
            (f.plays && f.mode != FeedMode::Playground)
                || !matches!(f.views[start].view.status, Status::Deciding { .. })
        })
}

/// Sends the views held since the first one shown to the instance's streams, once its setup
/// is done; later views go as they come.
fn release(feed: &SharedFeed) {
    let mut f = lock(&feed.0);
    let Some(start) = f.start else { return };
    if f.mode == FeedMode::Manual || f.released {
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

    /// A view shown at once starts the page's stage over, from each seat; one that opens with
    /// an ability on the stack numbers it as the scripts after the setup do, so the stage has
    /// a node for what the next script resolves.
    #[test]
    fn the_view_shown_at_once_starts_over_with_the_stack_numbered() {
        let instances = Arc::new(Mutex::new(Vec::new()));
        let pages = Arc::new(Mutex::new(HashMap::new()));
        let card = "the-wandering-emperor";
        let moment = "1-grows-a-creature-and-gives-it-first-strike-pass";
        for (view, abilities) in [("yours", json!([1])), ("theirs", json!([]))] {
            let key = format!("{card}/{moment}/{view}/reset-test");
            let feed = instance(&instances, &pages, &key, card, moment, view).unwrap();
            let shown = current(&feed);
            assert_eq!(
                shown["ok"]["view"]["script"]["reset"],
                json!({ "abilities": abilities }),
                "{view}: {}",
                shown["ok"]["view"]["script"]
            );
        }
    }

    #[test]
    fn playground_priority_needs_no_play_button() {
        let instances = Arc::new(Mutex::new(Vec::new()));
        let pages = Arc::new(Mutex::new(HashMap::new()));
        let card = "aangs-iceberg";
        let moment =
            "playground-exiles-a-permanent-until-it-leaves-waterbend-taps-a-creature-to-pay-4";
        let key = format!("{card}/{moment}/yours/priority-test");
        let feed = instance(&instances, &pages, &key, card, moment, "yours").unwrap();
        let shown = current(&feed);
        assert_eq!(shown["ok"]["view"]["status"]["kind"], "deciding");
        assert!(
            !holds(&lock(&feed.0)),
            "a human decision is already playable"
        );
    }
}
