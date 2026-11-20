//! The hub's HTTP server, on `tiny_http`, with one thread per request and a pool of database
//! connections that the threads share.
//!
//! Load is bounded: at most [`MAX_REQUEST_THREADS`] requests are answered at once (the next
//! gets 503), at most [`MAX_CONNECTIONS`] database connections are open (a request waits a few
//! seconds for one, then gets 503), and a request without a valid token may send only
//! [`MAX_UNAUTHENTICATED_BODY`] bytes.
//!
//! There are no socket timeouts, so a client that stalls mid-body holds one of the request
//! threads until it disconnects; the thread cap bounds the harm. `tiny_http` 0.12 never hands
//! out an accepted socket, and its `Listener` is a closed enum, so a server cannot set
//! `SO_RCVTIMEO` or `SO_SNDTIMEO` on each connection. Setting them on the listening socket,
//! which accepted sockets inherit, would also make an idle `accept` time out, and `tiny_http`'s
//! accept thread stops for good at its first error. Timeouts need a different HTTP server.
//!
//! The server listens on the address it is given, normally the loopback address, and Tailscale
//! publishes it on the private network, as it did the ticket service. API requests live under
//! `/<project>/api`; other requests under `/<project>` are the dashboard's pages. The project is
//! the one in `ignite.toml`'s `[hub]`; any other answers 404 without touching the database.
//!
//! A route that panics answers 500 instead of dropping the connection.

use std::io::{self, Read};
use std::net::SocketAddr;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::json;
use tiny_http::{Header, Server};

use ignite_core::config::{Config, Rules};
use ignite_core::time::{SystemClock, Timestamp};
use ignite_core::traits::Clock;
use ignite_store::hub;

use crate::auth;
use crate::dashboard;
use crate::routes::{self, Request, Response};
use crate::rules;

/// The largest request body the hub reads, which its clients check before sending.
const MAX_BODY: u64 = ignite_core::protocol::hub::MAX_BODY_BYTES;

/// The most requests answered at the same time, each on its own thread.
pub const MAX_REQUEST_THREADS: usize = 64;

/// The most database connections open at once. The long-poll waiters of `/feed` hold some of
/// them (`routes::feed::MAX_WAITERS`), so this stays well above that.
pub const MAX_CONNECTIONS: usize = 32;

/// How long a request waits for a free database connection before it gets 503.
const CONNECTION_WAIT: Duration = Duration::from_secs(5);

/// The shortest time between two attempts to evaluate the improvement rules.
const EVALUATION_RETRY: Duration = Duration::from_secs(600);

/// The largest body the hub reads from a request that carries no valid token, enough for a
/// machine's first registration.
pub const MAX_UNAUTHENTICATED_BODY: u64 = 64 * 1024;

/// Where and how the hub serves.
#[derive(Clone, Debug)]
pub struct HubServer {
    pub config: Config,
    /// The address to listen on, such as `127.0.0.1:7345`; port 0 picks a free one.
    pub listen: String,
    pub database: PathBuf,
    /// The HTML shown at the left of every dashboard page's header in place of the name
    /// "Ignite": the contents of the file `[hub] wordmark` names, or `None` for the name.
    pub wordmark: Option<String>,
}

impl HubServer {
    /// Serves until the process ends.
    pub fn serve(&self) -> io::Result<()> {
        self.start()?.wait();
        Ok(())
    }

    /// Opens the database and starts serving in the background.
    pub fn start(&self) -> io::Result<RunningHub> {
        self.start_with(false)
    }

    /// Like [`HubServer::start`], but the server answers only `GET` and `HEAD` requests and
    /// refuses `/<project>/api/export`, so it is safe to publish beyond the private network
    /// (Tailscale Funnel). It has its own request-thread cap and connection pool, so a flood
    /// on it cannot starve the private listener.
    pub fn start_read_only(&self) -> io::Result<RunningHub> {
        self.start_with(true)
    }

    fn start_with(&self, read_only: bool) -> io::Result<RunningHub> {
        let first = hub::open(&self.database).map_err(io::Error::other)?;
        if !read_only {
            // Routes and upkeep read the machines' operating systems and the pool's apps from the
            // database, so the facts of this `ignite.toml` replace those of the last start.
            ignite_store::hub::owners::record_routing(
                &first,
                &ignite_core::owner::Routing::from_config(&self.config),
            )
            .map_err(io::Error::other)?;
        }
        let server = Server::http(&self.listen).map_err(io::Error::other)?;
        let address = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| io::Error::other("the hub must listen on an IP address"))?;
        let server = Arc::new(server);
        let shared = Arc::new(Shared {
            project: self.config.hub.project.clone(),
            aliases: self.config.hub.aliases.clone(),
            wordmark: self.wordmark.clone(),
            rules: self.config.rules.clone(),
            database: self.database.clone(),
            pool: Mutex::new(Pool {
                idle: vec![first],
                open: 1,
            }),
            returned: Condvar::new(),
            active: AtomicUsize::new(0),
            read_only,
            evaluations: EvaluationGate::default(),
        });
        let accepting = Arc::clone(&server);
        let thread = thread::spawn(move || {
            for request in accepting.incoming_requests() {
                if shared.active.fetch_add(1, Ordering::SeqCst) >= MAX_REQUEST_THREADS {
                    shared.active.fetch_sub(1, Ordering::SeqCst);
                    let busy = tiny_http::Response::from_string("{\"error\":\"the hub is busy\"}")
                        .with_status_code(503);
                    let _ = request.respond(busy);
                    continue;
                }
                let shared = Arc::clone(&shared);
                thread::spawn(move || {
                    answer(&shared, request);
                    shared.active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Ok(RunningHub {
            address,
            server,
            thread,
        })
    }
}

/// A hub serving in the background.
pub struct RunningHub {
    address: SocketAddr,
    server: Arc<Server>,
    thread: JoinHandle<()>,
}

impl RunningHub {
    /// The address it listens on, with the port it was given.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Stops accepting requests. Requests already being answered finish on their own threads.
    pub fn stop(self) {
        self.server.unblock();
        let _ = self.thread.join();
    }

    /// Blocks until the accept loop ends. That happens after [`RunningHub::stop`], and also on
    /// its own: `tiny_http` ends its accept thread at the first accept error, such as running
    /// out of file descriptors, and never accepts again, so a caller that expects the hub to
    /// serve forever treats a return from this as a failure.
    pub fn wait(self) {
        let _ = self.thread.join();
    }
}

struct Shared {
    project: String,
    /// Older names of the project, which redirect to `project`.
    aliases: Vec<String>,
    /// See [`HubServer::wordmark`].
    wordmark: Option<String>,
    /// The improvement rules' settings, evaluated when records or efficiency rows arrive.
    rules: Rules,
    database: PathBuf,
    /// Idle connections and how many are open; a request takes one, opens a new one while
    /// fewer than [`MAX_CONNECTIONS`] are open, and gives it back.
    pool: Mutex<Pool>,
    /// Signalled when a connection comes back or closes.
    returned: Condvar,
    /// Requests being answered now.
    active: AtomicUsize,
    /// Whether only reads are served; see [`HubServer::start_read_only`].
    read_only: bool,
    /// Lets one evaluation of the improvement rules run at a time; see [`evaluate_rules`].
    evaluations: EvaluationGate,
}

struct Pool {
    idle: Vec<Connection>,
    open: usize,
}

impl Shared {
    fn connection(&self) -> Result<Connection, Response> {
        let deadline = Instant::now() + CONNECTION_WAIT;
        let mut pool = self.pool.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(conn) = pool.idle.pop() {
                return Ok(conn);
            }
            if pool.open < MAX_CONNECTIONS {
                pool.open += 1;
                drop(pool);
                return hub::open(&self.database).map_err(|e| {
                    self.discard();
                    failure(500, &e.to_string())
                });
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(failure(503, "the hub is busy"));
            }
            pool = self
                .returned
                .wait_timeout(pool, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn give_back(&self, conn: Connection) {
        self.pool
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .idle
            .push(conn);
        self.returned.notify_one();
    }

    /// Records that a connection was closed instead of given back.
    fn discard(&self) {
        let mut pool = self.pool.lock().unwrap_or_else(PoisonError::into_inner);
        pool.open = pool.open.saturating_sub(1);
        drop(pool);
        self.returned.notify_one();
    }
}

fn answer(shared: &Shared, mut http: tiny_http::Request) {
    let now = SystemClock.now();
    let path = http.url().split('?').next().unwrap_or_default().to_owned();
    let response = respond(shared, &mut http, now);
    let new_records = path.ends_with("/api/records") || path.ends_with("/api/metrics/import");
    let evaluate = new_records && response.status == 200;
    let mut reply = tiny_http::Response::from_data(response.body).with_status_code(response.status);
    let headers =
        std::iter::once(("Content-Type", response.content_type.to_owned())).chain(response.headers);
    for (name, value) in headers {
        if let Ok(header) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            reply.add_header(header);
        }
    }
    let _ = http.respond(reply);
    if evaluate {
        evaluate_rules(shared, now);
    }
}

/// Evaluates the improvement rules after new records or efficiency rows have been stored and
/// answered, so the machine that sent them does not wait. A failed evaluation leaves the rows
/// stored, and a later arrival tries again.
///
/// An evaluation reads every efficiency row, which takes minutes once the `metrics` table is
/// gigabytes. So [`EvaluationGate`] lets only one run at a time and starts a new one at most every
/// [`EVALUATION_RETRY`], even after a failure. Before this, each arrival started another
/// evaluation while earlier ones were still reading, and their reads slowed the dispatch
/// transaction until other writers failed with `database is locked`.
fn evaluate_rules(shared: &Shared, now: Timestamp) {
    shared.evaluations.run(Instant::now(), || {
        let Ok(mut conn) = shared.connection() else {
            return;
        };
        if let Err(error) = rules::filing::evaluate_if_due(&mut conn, &shared.rules, now) {
            eprintln!("ignite hub: evaluating the improvement rules failed: {error}");
        }
        shared.give_back(conn);
    });
}

/// When the improvement rules were last tried, locked while an evaluation runs.
#[derive(Default)]
struct EvaluationGate {
    started: Mutex<Option<Instant>>,
}

impl EvaluationGate {
    /// Calls `evaluate` unless an evaluation is running or one started less than
    /// [`EVALUATION_RETRY`] before `now`, whether or not it succeeded. Returns whether it ran.
    fn run(&self, now: Instant, evaluate: impl FnOnce()) -> bool {
        let mut started = match self.started.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return false,
        };
        if started.is_some_and(|at| now.saturating_duration_since(at) < EVALUATION_RETRY) {
            return false;
        }
        *started = Some(now);
        evaluate();
        true
    }
}

fn respond(shared: &Shared, http: &mut tiny_http::Request, now: Timestamp) -> Response {
    let url = http.url().to_owned();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let path = percent_decode(path);
    if shared.read_only && !matches!(http.method().as_str(), "GET" | "HEAD") {
        return failure(405, "this listener is read-only");
    }
    // The stylesheet, script, service worker and icons live at the hub's root.
    if let Some(file) = dashboard::serve::static_file(http.method().as_str(), &path) {
        return file;
    }
    if let Some(redirect) = dashboard::serve::alias_redirect(
        &path,
        &parse_query(query),
        &shared.project,
        &shared.aliases,
    ) {
        return redirect;
    }
    // The API is under `/<project>/api`. Every other address is a dashboard page at the root.
    let api = path
        .strip_prefix('/')
        .and_then(|p| p.strip_prefix(shared.project.as_str()))
        .and_then(|rest| rest.strip_prefix("/api"))
        .filter(|api| api.is_empty() || api.starts_with('/'))
        // The dashboard's pages are at the root, and their script follows the change feed at
        // `api/feed` relative to the root.
        .or_else(|| (path == "/api/feed").then_some("/feed"));
    // `/export` dumps every ticket, which a public listener does not serve.
    if shared.read_only && api.is_some_and(|api| api == "/export" || api.starts_with("/export/")) {
        return failure(404, "Unknown route");
    }
    let headers: Vec<(String, String)> = http
        .headers()
        .iter()
        .map(|h| {
            (
                h.field.as_str().as_str().to_ascii_lowercase(),
                h.value.as_str().to_owned(),
            )
        })
        .collect();
    let method = http.method().as_str().to_owned();
    let conn = match shared.connection() {
        Ok(conn) => conn,
        Err(response) => return response,
    };
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    // The token is checked before any body is read, so a request without one cannot make the
    // hub buffer more than a small body.
    let (writer, editable) = writer(&conn, shared.read_only, api, &header);
    let limit = if writer.is_some() {
        MAX_BODY
    } else {
        MAX_UNAUTHENTICATED_BODY
    };
    let mut body = Vec::new();
    if http
        .body_length()
        .is_some_and(|length| length as u64 > limit)
    {
        shared.give_back(conn);
        return failure(413, "the request body is too large");
    }
    if http
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut body)
        .is_err()
    {
        shared.give_back(conn);
        return failure(400, "could not read the request body");
    }
    if body.len() as u64 > limit {
        shared.give_back(conn);
        return failure(413, "the request body is too large");
    }
    let mut request = Request {
        method,
        path: String::new(),
        query: parse_query(query),
        writer,
        body,
        headers,
    };
    let (response, conn) = if let Some(api) = api {
        api.clone_into(&mut request.path);
        caught(conn, |conn| {
            api_route(conn, &request, !shared.read_only, now)
        })
    } else {
        path.clone_into(&mut request.path);
        caught(conn, |conn| page(conn, &request, shared, editable, now))
    };
    match conn {
        Some(conn) => shared.give_back(conn),
        None => shared.discard(),
    }
    if request.method != "GET" && request.method != "HEAD" {
        routes::feed::notify();
    }
    response
}

/// Who may write with this request, and whether its pages carry the dashboard's edit controls.
/// A machine writes with its token. The dashboard's edit controls write as the Tailscale user, on
/// the private listener only, and only tickets (`/batch`; see `auth`).
fn writer<'h>(
    conn: &Connection,
    read_only: bool,
    api: Option<&str>,
    header: &impl Fn(&str) -> Option<&'h str>,
) -> (Option<auth::Writer>, bool) {
    let login = header("tailscale-user-login").filter(|_| !read_only);
    let writer = auth::authorize(conn, header("authorization"), header("x-actor"))
        .ok()
        .or_else(|| {
            (api == Some("/batch"))
                .then(|| auth::dashboard_writer(login, header(auth::DASHBOARD_HEADER)))
                .flatten()
        });
    (writer, auth::dashboard_editor(login).is_some())
}

/// Runs a route, turning a panic into a 500. The connection of a route that panicked is dropped,
/// since it may be inside a transaction.
fn caught(
    mut conn: Connection,
    route: impl FnOnce(&mut Connection) -> Response,
) -> (Response, Option<Connection>) {
    match panic::catch_unwind(AssertUnwindSafe(|| route(&mut conn))) {
        Ok(response) => (response, Some(conn)),
        Err(cause) => {
            let message = cause
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| cause.downcast_ref::<&str>().copied())
                .unwrap_or("the route panicked");
            (failure(500, message), None)
        }
    }
}

/// Tries each API module in turn. `private` says the request came on the private listener.
fn api_route(conn: &mut Connection, request: &Request, private: bool, now: Timestamp) -> Response {
    routes::tickets::handle(conn, request, now)
        .or_else(|| routes::feed::handle(conn, request, private, now))
        .or_else(|| routes::checks::handle(conn, request, now))
        .or_else(|| routes::listings::handle(conn, request, now))
        .or_else(|| routes::verdicts::handle(conn, request, now))
        .or_else(|| routes::tip_rounds::handle(conn, request, now))
        .or_else(|| routes::versions::handle(conn, request, now))
        .or_else(|| routes::check_state::handle(conn, request, now))
        .or_else(|| routes::dispatch::handle(conn, request, now))
        .or_else(|| routes::commands::handle(conn, request, now))
        .unwrap_or_else(|| failure(404, "Unknown route"))
}

/// The dashboard's pages, which [`dashboard::serve::serve`] routes and renders from the request
/// path. `editable` pages carry the controls that edit tickets.
fn page(
    conn: &mut Connection,
    request: &Request,
    shared: &Shared,
    editable: bool,
    now: Timestamp,
) -> Response {
    dashboard::serve::serve_with(
        conn,
        request,
        &dashboard::serve::Site {
            project: &shared.project,
            wordmark: shared.wordmark.as_deref(),
            editable,
        },
        now,
    )
    .unwrap_or_else(|| failure(404, "Unknown route"))
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let decode = |text: &str| percent_decode(&text.replace('+', " "));
            (decode(key), decode(value))
        })
        .collect()
}

/// Decodes `%XX` escapes; a malformed escape is kept as it is.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        if let Some(byte) = escape {
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parameters_are_decoded_and_keep_their_order() {
        assert_eq!(
            parse_query("label=a&search=two+words%21&label=b&ready"),
            [
                ("label".to_owned(), "a".to_owned()),
                ("search".to_owned(), "two words!".to_owned()),
                ("label".to_owned(), "b".to_owned()),
                ("ready".to_owned(), String::new()),
            ]
        );
        assert_eq!(percent_decode("/tickets/a%2E1%zz"), "/tickets/a.1%zz");
    }

    #[test]
    fn rule_evaluations_run_one_at_a_time_and_not_again_within_the_retry() {
        let gate = EvaluationGate::default();
        let start = Instant::now();
        // An arrival while an evaluation runs starts none.
        let mut nested = None;
        assert!(gate.run(start, || nested = Some(gate.run(start, || {}))));
        assert_eq!(nested, Some(false));
        // Nor does the next arrival right after it.
        assert!(!gate.run(start, || {}));
        // An evaluation that failed is not tried again until the retry interval has passed.
        let mut conn = ignite_store::hub::open_in_memory().unwrap();
        conn.execute("ALTER TABLE findings RENAME TO findings_away", [])
            .unwrap();
        let later = start + EVALUATION_RETRY;
        let mut failed = false;
        assert!(gate.run(later, || {
            failed = rules::filing::evaluate_if_due(
                &mut conn,
                &ignite_core::config::Rules::default(),
                SystemClock.now(),
            )
            .is_err();
        }));
        assert!(failed);
        let almost = later + EVALUATION_RETRY.saturating_sub(Duration::from_secs(1));
        assert!(!gate.run(almost, || {}));
        assert!(gate.run(later + EVALUATION_RETRY, || {}));
    }

    #[test]
    fn wait_returns_when_the_accept_loop_ends_by_itself() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-hub-wait-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = HubServer {
            config: Config::parse(
                "ignite.toml",
                "[repository]\nremote = \"origin\"\nstaging = \"staging\"\n\
                 master = \"master\"\ntrees = \"trees\"\nchecks = \"checks\"\n\
                 [hub]\nurl = \"http://localhost:1\"\nproject = \"test\"\n\
                 [gate]\nthreshold = 0.01\ncap = 10\n\
                 [machine.local]\nticket_trees = 4\nlanding_trees = 1\n",
            )
            .unwrap(),
            listen: "127.0.0.1:0".to_owned(),
            database: dir.join("hub.db"),
            wordmark: None,
        }
        .start()
        .unwrap();
        // `unblock` ends `incoming_requests` the way a failed accept does.
        let server = Arc::clone(&hub.server);
        let (ended, ends) = std::sync::mpsc::channel();
        thread::spawn(move || {
            hub.wait();
            let _ = ended.send(());
        });
        assert!(ends.recv_timeout(Duration::from_millis(200)).is_err());
        server.unblock();
        ends.recv_timeout(Duration::from_secs(10))
            .expect("wait returns once the accept loop has ended");
        let _ = std::fs::remove_dir_all(dir);
    }
}
