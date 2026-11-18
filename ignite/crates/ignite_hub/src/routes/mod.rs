//! The hub's API routes under `/<project>/api`. Each module answers the paths it owns and
//! returns `None` for any other, so [`crate::server`] tries them in turn.

pub mod checks;
pub mod dispatch;
pub mod feed;
pub mod tickets;

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
}

/// One response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    /// A JSON response.
    pub fn json(status: u16, value: &serde_json::Value) -> Response {
        Response {
            status,
            content_type: "application/json",
            body: serde_json::to_vec(value).unwrap_or_default(),
        }
    }
}
