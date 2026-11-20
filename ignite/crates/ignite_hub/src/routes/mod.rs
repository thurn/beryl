//! The hub's API routes under `/<project>/api`. Each module answers the paths it owns and
//! returns `None` for any other, so [`crate::server`] tries them in turn.

pub mod check_state;
pub mod checks;
pub mod commands;
pub mod dispatch;
pub mod feed;
pub mod listings;
pub mod tickets;
pub mod tip_rounds;
pub mod verdicts;
pub mod versions;

use crate::auth::Writer;

/// One request, already read from the connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// The path after `/<project>/api`, such as `/tickets/a.1`.
    pub path: String,
    /// Query parameters in order; a key may repeat.
    pub query: Vec<(String, String)>,
    /// Present on an authorized write.
    pub writer: Option<Writer>,
    pub body: Vec<u8>,
    /// Request headers in order, with their names in lowercase.
    pub headers: Vec<(String, String)>,
}

impl Request {
    /// The first value of a header, by its lowercase name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The first value of a query parameter.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// One response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    /// Headers besides `Content-Type`, such as `ETag`.
    pub headers: Vec<(&'static str, String)>,
}

impl Response {
    /// A JSON response.
    pub fn json(status: u16, value: &serde_json::Value) -> Response {
        Response {
            status,
            content_type: "application/json",
            body: serde_json::to_vec(value).unwrap_or_default(),
            headers: Vec::new(),
        }
    }

    /// The same response with one more header.
    #[must_use]
    pub fn with_header(mut self, name: &'static str, value: String) -> Response {
        self.headers.push((name, value));
        self
    }
}
