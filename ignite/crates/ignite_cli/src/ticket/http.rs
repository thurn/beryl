//! The hub's HTTP client for the ticket verbs: where the hub is (`ignite.toml`), the write token,
//! and one request with the retry rules of the `tk` client it replaces.
//!
//! A request that failed before it was sent, or that is safe to repeat, is retried with a
//! growing pause until the retry budget is spent. A write that may have been applied is never
//! repeated unless it carries a `request_key` or a version precondition, which make a replay
//! harmless.

use std::fmt::{Display, Write as _};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use ignite_core::config::Config;
use ignite_core::protocol::client::{Exit, Reply};
use serde_json::{Value, json};

/// A command that failed: the exit code and what to tell the caller.
#[derive(Debug)]
pub struct Failure {
    pub exit: Exit,
    pub message: String,
    /// The hub's JSON error body, when it sent one.
    pub body: Option<Value>,
}

impl Failure {
    pub fn new(exit: Exit, message: impl Display) -> Failure {
        Failure {
            exit,
            message: message.to_string(),
            body: None,
        }
    }

    /// The reply the client prints for this failure.
    pub fn into_reply(self) -> Reply {
        let json = self.body.unwrap_or_else(|| json!({"error": self.message}));
        Reply {
            exit: self.exit,
            text: self.message,
            json: Some(json),
            handle: None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Failure>;

/// A usage or configuration error (exit 1).
pub fn usage(error: impl Display) -> Failure {
    Failure::new(Exit::Usage, error)
}

/// What a successful request returned.
pub struct Response {
    pub body: String,
}

impl Response {
    pub fn json(&self) -> Result<Value> {
        serde_json::from_str(&self.body).map_err(usage)
    }
}

/// The retry budget when `IGNITE_RETRY_FOR_S` does not say otherwise.
const DEFAULT_RETRY_S: f64 = 120.0;

/// One hub, as one machine's ticket commands see it.
pub struct Hub {
    /// `http://host:port/<project>/api`, without a trailing slash.
    base: String,
    agent: ureq::Agent,
    token: Option<String>,
    pub actor: String,
    /// The title the desktop app shows for the actor's session, sent with each write so the
    /// dashboard can name the session by its title (`ignite_daemon::session_title`).
    pub actor_title: Option<String>,
    pub retry_for: Duration,
}

impl Hub {
    pub fn new(
        url: &str,
        project: &str,
        token: Option<String>,
        actor: String,
        retry_for: Duration,
    ) -> Hub {
        let base = format!("{}/{project}/api", url.trim_end_matches('/'));
        let mut config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0);
        if ["//127.0.0.1", "//localhost", "//[::1]"]
            .iter()
            .any(|local| url.contains(local))
        {
            config = config.proxy(None);
        }
        Hub {
            base,
            agent: config.build().into(),
            token,
            actor,
            actor_title: None,
            retry_for,
        }
    }

    /// The hub named by the `ignite.toml` above `cwd`, with the token from `IGNITE_TOKEN`, the
    /// machine's identity that `ignite install` wrote (`machine.json` in `$IGNITE_HOME` or
    /// `~/.config/ignite`), or the file `token` beside it. Reads need no token, so a missing one is reported only when a
    /// write is attempted.
    pub fn load(cwd: &Path, actor: String, env: impl Fn(&str) -> Option<String>) -> Result<Hub> {
        let (file, text) = find_config(cwd)?;
        let config = Config::parse(&file.display().to_string(), &text).map_err(usage)?;
        let retry_for = env("IGNITE_RETRY_FOR_S")
            .and_then(|seconds| seconds.parse::<f64>().ok())
            .filter(|seconds| Duration::try_from_secs_f64(*seconds).is_ok())
            .unwrap_or(DEFAULT_RETRY_S);
        let ignite_home = env("IGNITE_HOME").map(PathBuf::from).or_else(|| {
            ignite_core::config::home_dir(|name| env(name).map(Into::into))
                .map(|home| home.join(".config").join("ignite"))
        });
        let token = env("IGNITE_TOKEN")
            .filter(|token| !token.is_empty())
            .or_else(|| machine_token(&ignite_home.clone()?.join("machine.json")))
            .or_else(|| std::fs::read_to_string(ignite_home?.join("token")).ok())
            .map(|token| token.trim().to_owned())
            .filter(|token| !token.is_empty());
        let actor_title = ignite_daemon::session_title::app_data(&env)
            .and_then(|dir| ignite_daemon::session_title::title(&dir, &actor, &env));
        let mut hub = Hub::new(
            &config.hub.url,
            &config.hub.project,
            token,
            actor,
            Duration::from_secs_f64(retry_for),
        );
        hub.actor_title = actor_title;
        Ok(hub)
    }

    /// `GET path?query`, retried while the hub is unreachable.
    pub fn get(&self, path: &[&str], query: &[(&str, String)]) -> Result<Response> {
        self.get_within(path, query, self.retry_for)
    }

    /// `GET` with its own retry budget, for a long poll that must give up by a deadline.
    pub fn get_within(
        &self,
        path: &[&str],
        query: &[(&str, String)],
        budget: Duration,
    ) -> Result<Response> {
        self.send(&self.url(path, query), None, true, budget, None)
    }

    /// `GET`, with each long poll allowed `wait` more than the hub holds it.
    pub fn poll(
        &self,
        path: &[&str],
        query: &[(&str, String)],
        wait: Duration,
    ) -> Result<Response> {
        self.send(
            &self.url(path, query),
            None,
            true,
            self.retry_for,
            Some(wait),
        )
    }

    /// `POST path`. `replay_safe` says whether repeating the request after an unclear failure
    /// can do no harm.
    pub fn post(&self, path: &[&str], body: &Value, replay_safe: bool) -> Result<Response> {
        self.send(
            &self.url(path, &[]),
            Some((WriteMethod::Post, body)),
            replay_safe,
            self.retry_for,
            None,
        )
    }

    /// `PUT path` with a version precondition, safe to replay after an unclear failure.
    pub fn put_versioned(&self, path: &[&str], body: &Value) -> Result<Response> {
        self.send(
            &self.url(path, &[]),
            Some((WriteMethod::Put, body)),
            true,
            self.retry_for,
            None,
        )
    }

    /// `POST path?query` with raw bytes of `content_type` as the body, such as an image. It is
    /// sent again after an unclear failure, since the hub keeps an image uploaded twice to the
    /// same ticket under the same name once.
    pub fn post_bytes(
        &self,
        path: &[&str],
        query: &[(&str, String)],
        bytes: &[u8],
        content_type: &str,
    ) -> Result<Response> {
        let body = Body {
            method: WriteMethod::Post,
            bytes,
            content_type,
        };
        let raw = self.send_raw(
            &self.url(path, query),
            Some(body),
            true,
            self.retry_for,
            None,
        )?;
        Ok(Response {
            body: String::from_utf8_lossy(&raw).into_owned(),
        })
    }

    /// `GET path?query`, answering the body's bytes, such as an image.
    pub fn get_bytes(&self, path: &[&str], query: &[(&str, String)]) -> Result<Vec<u8>> {
        self.send_raw(&self.url(path, query), None, true, self.retry_for, None)
    }

    /// `POST path` once, waiting at most `timeout`, for a report whose loss does no harm.
    /// Returns whether the hub accepted it.
    pub fn post_once(&self, path: &[&str], body: &Value, timeout: Duration) -> bool {
        let Some(token) = &self.token else {
            return false;
        };
        self.agent
            .post(&self.url(path, &[]))
            .header("Authorization", &format!("Bearer {token}"))
            .header("X-Actor", &self.actor)
            .header("Content-Type", "application/json")
            .config()
            .timeout_global(Some(timeout))
            .build()
            .send(body.to_string().as_bytes())
            .is_ok_and(|response| response.status().is_success())
    }

    fn url(&self, path: &[&str], query: &[(&str, String)]) -> String {
        let mut url = self.base.clone();
        for segment in path {
            url.push('/');
            url.push_str(&encode(segment));
        }
        for (index, (name, value)) in query.iter().enumerate() {
            url.push(if index == 0 { '?' } else { '&' });
            url.push_str(&encode(name));
            url.push('=');
            url.push_str(&encode(value));
        }
        url
    }

    fn send(
        &self,
        url: &str,
        body: Option<(WriteMethod, &Value)>,
        replay_safe: bool,
        budget: Duration,
        hold: Option<Duration>,
    ) -> Result<Response> {
        let encoded = body.map(|(_, value)| value.to_string());
        let body = body
            .zip(encoded.as_deref())
            .map(|((method, _), text)| Body {
                method,
                bytes: text.as_bytes(),
                content_type: "application/json",
            });
        let raw = self.send_raw(url, body, replay_safe, budget, hold)?;
        String::from_utf8(raw)
            .map(|body| Response { body })
            .map_err(usage)
    }

    fn send_raw(
        &self,
        url: &str,
        body: Option<Body<'_>>,
        replay_safe: bool,
        budget: Duration,
        hold: Option<Duration>,
    ) -> Result<Vec<u8>> {
        let authorization = match body {
            Some(_) => Some(format!(
                "Bearer {}",
                self.token.as_deref().ok_or_else(|| usage(
                    "no write token: run `ignite install`, or set IGNITE_TOKEN"
                ))?
            )),
            None => None,
        };
        let start = Instant::now();
        let mut delay = Duration::from_millis(200);
        loop {
            let left = budget.saturating_sub(start.elapsed());
            let timeout = hold.map_or_else(
                || {
                    if budget.is_zero() {
                        Duration::from_secs(10)
                    } else {
                        left.clamp(Duration::from_millis(1), Duration::from_secs(10))
                    }
                },
                |hold| hold + Duration::from_secs(10),
            );
            let response = match (body, &authorization) {
                (Some(body), Some(authorization)) => {
                    let request = match body.method {
                        WriteMethod::Post => self.agent.post(url),
                        WriteMethod::Put => self.agent.put(url),
                    };
                    request
                        .header("Authorization", authorization)
                        .header("X-Actor", &self.actor)
                        .header("Content-Type", body.content_type)
                        .config()
                        .timeout_global(Some(timeout))
                        .build()
                        .send(body.bytes)
                }
                _ => self
                    .agent
                    .get(url)
                    .config()
                    .timeout_global(Some(timeout))
                    .build()
                    .call(),
            };
            let failure = match response {
                Ok(mut response) => {
                    let status = response.status().as_u16();
                    let read = response
                        .body_mut()
                        .with_config()
                        .limit(64 * 1024 * 1024)
                        .read_to_vec();
                    match read {
                        Ok(body) if (200..300).contains(&status) => {
                            return Ok(body);
                        }
                        Ok(body) => {
                            let failure = http_failure(status, &String::from_utf8_lossy(&body));
                            if !(failure.exit == Exit::Unreachable && replay_safe) {
                                return Err(failure);
                            }
                            failure
                        }
                        Err(error) => transport_failure(&error, replay_safe, url)?,
                    }
                }
                Err(error) => transport_failure(&error, replay_safe, url)?,
            };
            let left = budget.saturating_sub(start.elapsed());
            if delay >= left {
                thread::sleep(left);
                return Err(failure);
            }
            thread::sleep(delay);
            delay = (delay * 2).min(Duration::from_secs(10));
        }
    }
}

/// A request body: its bytes and their content type.
#[derive(Clone, Copy)]
struct Body<'a> {
    method: WriteMethod,
    bytes: &'a [u8],
    content_type: &'a str,
}

#[derive(Clone, Copy)]
enum WriteMethod {
    Post,
    Put,
}

/// An unreachable hub is retried when the request was never sent or can be repeated; any other
/// failure to talk to it ends the command.
fn transport_failure(error: &ureq::Error, replay_safe: bool, url: &str) -> Result<Failure> {
    let before_send = matches!(
        error,
        ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
            | ureq::Error::Timeout(ureq::Timeout::Resolve | ureq::Timeout::Connect)
    ) || matches!(error, ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused);
    match error {
        ureq::Error::Timeout(_)
        | ureq::Error::Io(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed => {
            let mut message = format!("hub unavailable: {error}");
            if let Some(hint) = ignite_core::tailnet::diagnose_url(
                url,
                ignite_core::tailnet::FailureKind::ConnectOrTls,
            ) {
                message = format!("{message}. {hint}");
            }
            let failure = Failure::new(Exit::Unreachable, message);
            if replay_safe || before_send {
                Ok(failure)
            } else {
                Err(failure)
            }
        }
        _ => Err(usage(error)),
    }
}

/// A refused request: 409 is a conflict (exit 3), 5xx an unreachable hub (exit 4) and anything
/// else a usage error.
fn http_failure(status: u16, body: &str) -> Failure {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let exit = match status {
        409 => Exit::Conflict,
        500..=599 => Exit::Unreachable,
        _ => Exit::Usage,
    };
    let message = match parsed.as_ref().and_then(|body| body["error"].as_str()) {
        Some(detail) => format!("hub returned HTTP {status}: {detail}"),
        None => format!("hub returned HTTP {status}"),
    };
    Failure {
        exit,
        message,
        body: parsed,
    }
}

/// The write token in the machine's identity file, if it has one.
fn machine_token(path: &Path) -> Option<String> {
    let identity: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    identity.get("token")?.as_str().map(str::to_owned)
}

/// The text of the `ignite.toml` in `cwd` or the nearest directory above it.
pub fn find_config(cwd: &Path) -> Result<(PathBuf, String)> {
    for directory in cwd.ancestors() {
        let file = directory.join("ignite.toml");
        match std::fs::read_to_string(&file) {
            Ok(text) => return Ok((file, text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(usage(format!("{}: {error}", file.display()))),
        }
    }
    Err(usage("no ignite.toml found above the working directory"))
}

/// Percent-encodes everything but unreserved characters.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_encode_segments_and_query_values() {
        let hub = Hub::new("http://h:1/", "pj", None, "a".into(), Duration::ZERO);
        let url = hub.url(&["tickets", "a b"], &[("search", "x&y=z".into())]);
        assert_eq!(url, "http://h:1/pj/api/tickets/a%20b?search=x%26y%3Dz");
    }

    #[test]
    fn statuses_map_to_the_shared_exit_codes() {
        assert_eq!(http_failure(409, "{}").exit, Exit::Conflict);
        assert_eq!(http_failure(503, "").exit, Exit::Unreachable);
        let refused = http_failure(404, r#"{"error": "Ticket x not found"}"#);
        assert_eq!(refused.exit, Exit::Usage);
        assert_eq!(refused.message, "hub returned HTTP 404: Ticket x not found");
    }
}
