//! The hub's HTTP server, on `tiny_http`, with one thread per request and a pool of database
//! connections that the threads share.
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
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

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

/// The largest request body the hub reads, as the ticket service allowed.
const MAX_BODY: u64 = 100 * 1024 * 1024;

/// Where and how the hub serves.
#[derive(Clone, Debug)]
pub struct HubServer {
    pub config: Config,
    /// The address to listen on, such as `127.0.0.1:7345`; port 0 picks a free one.
    pub listen: String,
    pub database: PathBuf,
}

impl HubServer {
    /// Serves until the process ends.
    pub fn serve(&self) -> io::Result<()> {
        self.start()?.wait();
        Ok(())
    }

    /// Opens the database and starts serving in the background.
    pub fn start(&self) -> io::Result<RunningHub> {
        let first = hub::open(&self.database).map_err(io::Error::other)?;
        let server = Server::http(&self.listen).map_err(io::Error::other)?;
        let address = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| io::Error::other("the hub must listen on an IP address"))?;
        let server = Arc::new(server);
        let shared = Arc::new(Shared {
            project: self.config.hub.project.clone(),
            rules: self.config.rules.clone(),
            database: self.database.clone(),
            pool: Mutex::new(vec![first]),
        });
        let accepting = Arc::clone(&server);
        let thread = thread::spawn(move || {
            for request in accepting.incoming_requests() {
                let shared = Arc::clone(&shared);
                thread::spawn(move || answer(&shared, request));
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

    fn wait(self) {
        let _ = self.thread.join();
    }
}

struct Shared {
    project: String,
    /// The improvement rules' settings, evaluated when records or efficiency rows arrive.
    rules: Rules,
    database: PathBuf,
    /// Idle connections; a request takes one or opens a new one, and gives it back.
    pool: Mutex<Vec<Connection>>,
}

impl Shared {
    fn connection(&self) -> Result<Connection, Response> {
        let idle = self
            .pool
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop();
        match idle {
            Some(conn) => Ok(conn),
            None => hub::open(&self.database).map_err(|e| failure(500, &e.to_string())),
        }
    }

    fn give_back(&self, conn: Connection) {
        self.pool
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(conn);
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
/// stored, and the next arrival tries again.
fn evaluate_rules(shared: &Shared, now: Timestamp) {
    let Ok(mut conn) = shared.connection() else {
        return;
    };
    if let Err(error) = rules::filing::evaluate_if_due(&mut conn, &shared.rules, now) {
        eprintln!("ignite hub: evaluating the improvement rules failed: {error}");
    }
    shared.give_back(conn);
}

fn respond(shared: &Shared, http: &mut tiny_http::Request, now: Timestamp) -> Response {
    let url = http.url().to_owned();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let path = percent_decode(path);
    // The stylesheet, script, service worker and icons live at the hub's root.
    if let Some(file) = dashboard::serve::static_file(http.method().as_str(), &path) {
        return file;
    }
    let Some(rest) = path
        .strip_prefix('/')
        .and_then(|p| p.strip_prefix(shared.project.as_str()))
        .filter(|rest| rest.is_empty() || rest.starts_with('/'))
    else {
        return failure(404, "Unknown route");
    };
    let mut body = Vec::new();
    if http
        .as_reader()
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .is_err()
    {
        return failure(400, "could not read the request body");
    }
    if body.len() as u64 > MAX_BODY {
        return failure(413, "the request body is too large");
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
    let writer = auth::authorize(&conn, header("authorization"), header("x-actor")).ok();
    let mut request = Request {
        method,
        path: String::new(),
        query: parse_query(query),
        writer,
        body,
        headers,
    };
    let (response, conn) = if let Some(api) = rest.strip_prefix("/api") {
        api.clone_into(&mut request.path);
        caught(conn, |conn| api_route(conn, &request, now))
    } else {
        request.path = if rest.is_empty() {
            "/".to_owned()
        } else {
            rest.to_owned()
        };
        caught(conn, |conn| page(conn, &request, &shared.project, now))
    };
    if let Some(conn) = conn {
        shared.give_back(conn);
    }
    if request.method != "GET" && request.method != "HEAD" {
        routes::feed::notify();
    }
    response
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

/// Tries each API module in turn.
fn api_route(conn: &mut Connection, request: &Request, now: Timestamp) -> Response {
    routes::tickets::handle(conn, request, now)
        .or_else(|| routes::feed::handle(conn, request, now))
        .or_else(|| routes::checks::handle(conn, request, now))
        .or_else(|| routes::dispatch::handle(conn, request, now))
        .unwrap_or_else(|| failure(404, "Unknown route"))
}

/// One dashboard page's renderer: its HTML, or `None` for a path that is not its own.
type Page = fn(&Connection, &str, Timestamp) -> Option<String>;

/// The dashboard's pages. The ticket views need the project, the query and the request's
/// headers, so [`dashboard::serve::serve`] answers them; each other page renders from the path
/// after `/<project>`.
fn page(conn: &mut Connection, request: &Request, project: &str, now: Timestamp) -> Response {
    if let Some(response) = dashboard::serve::serve(conn, request, project, now) {
        return response;
    }
    if request.method != "GET" {
        return failure(404, "Unknown route");
    }
    let pages: [Page; 5] = [
        dashboard::machines::render,
        dashboard::pool::render,
        dashboard::landings::render,
        dashboard::tips::render,
        dashboard::explore::render,
    ];
    pages
        .iter()
        .find_map(|render| render(conn, &request.path, now))
        .map_or_else(
            || failure(404, "Unknown route"),
            |html| Response {
                status: 200,
                content_type: "text/html; charset=utf-8",
                body: html.into_bytes(),
                headers: Vec::new(),
            },
        )
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
}
