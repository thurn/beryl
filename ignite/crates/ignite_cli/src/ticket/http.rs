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
            retry_for,
        }
    }

    /// The hub named by the `ignite.toml` above `cwd`, with the token from `IGNITE_TOKEN` or
    /// `~/.config/ignite/token`. Reads need no token, so a missing one is reported only when a
    /// write is attempted.
    pub fn load(cwd: &Path, actor: String, env: impl Fn(&str) -> Option<String>) -> Result<Hub> {
        let (file, text) = find_config(cwd)?;
        let config = Config::parse(&file.display().to_string(), &text).map_err(usage)?;
        let retry_for = env("IGNITE_RETRY_FOR_S")
            .and_then(|seconds| seconds.parse::<f64>().ok())
            .filter(|seconds| Duration::try_from_secs_f64(*seconds).is_ok())
            .unwrap_or(DEFAULT_RETRY_S);
        let token = env("IGNITE_TOKEN")
            .filter(|token| !token.is_empty())
            .or_else(|| {
                let home = env("HOME")?;
                std::fs::read_to_string(PathBuf::from(home).join(".config/ignite/token")).ok()
            })
            .map(|token| token.trim().to_owned())
            .filter(|token| !token.is_empty());
        Ok(Hub::new(
            &config.hub.url,
            &config.hub.project,
            token,
            actor,
            Duration::from_secs_f64(retry_for),
        ))
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
            Some(body),
            replay_safe,
            self.retry_for,
            None,
        )
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
        body: Option<&Value>,
        replay_safe: bool,
        budget: Duration,
        hold: Option<Duration>,
    ) -> Result<Response> {
        let authorization = match body {
            Some(_) => Some(format!(
                "Bearer {}",
                self.token.as_deref().ok_or_else(|| usage(
                    "no write token: set IGNITE_TOKEN or put it in ~/.config/ignite/token"
                ))?
            )),
            None => None,
        };
        let encoded = body.map(Value::to_string);
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
            let response = match (&encoded, &authorization) {
                (Some(encoded), Some(authorization)) => self
                    .agent
                    .post(url)
                    .header("Authorization", authorization)
                    .header("X-Actor", &self.actor)
                    .header("Content-Type", "application/json")
                    .config()
                    .timeout_global(Some(timeout))
                    .build()
                    .send(encoded.as_bytes()),
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
                    let text = response
                        .body_mut()
                        .with_config()
                        .limit(64 * 1024 * 1024)
                        .read_to_string();
                    match text {
                        Ok(body) if (200..300).contains(&status) => {
                            return Ok(Response { body });
                        }
                        Ok(body) => {
                            let failure = http_failure(status, &body);
                            if !(failure.exit == Exit::Unreachable && replay_safe) {
                                return Err(failure);
                            }
                            failure
                        }
                        Err(error) => transport_failure(&error, replay_safe)?,
                    }
                }
                Err(error) => transport_failure(&error, replay_safe)?,
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

/// An unreachable hub is retried when the request was never sent or can be repeated; any other
/// failure to talk to it ends the command.
fn transport_failure(error: &ureq::Error, replay_safe: bool) -> Result<Failure> {
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
            let failure = Failure::new(Exit::Unreachable, format!("hub unavailable: {error}"));
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

/// The text of the `ignite.toml` in `cwd` or the nearest directory above it.
fn find_config(cwd: &Path) -> Result<(PathBuf, String)> {
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
