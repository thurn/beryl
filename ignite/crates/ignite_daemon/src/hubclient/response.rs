//! Decoding hub replies and classifying their errors.

use serde::de::DeserializeOwned;

use ignite_core::traits::{Error, Result};

use super::{Answer, HubClient};

/// How an error for a body too large for the hub begins; see [`too_large`].
pub const TOO_LARGE: &str = "413 ";

/// Whether the body exceeded [`MAX_BODY_BYTES`](ignite_core::protocol::hub::MAX_BODY_BYTES)
/// locally or the hub answered 413.
pub fn too_large(error: &Error) -> bool {
    matches!(error, Error::Invalid(message) if message.starts_with(TOO_LARGE))
}

/// What the hub's read-only listener says in the body of its 405 answer to a write.
const READ_ONLY_BODY: &str = "this listener is read-only";

/// How an error for a write that reached the hub's read-only listener begins; see
/// [`read_only`].
pub const READ_ONLY: &str = "hub URL reaches the read-only listener: ";

/// Whether `error` says that the configured hub URL reaches the hub's read-only listener. That
/// listener refuses every write and never stops doing so, so a request does not try again (a 405
/// is never retried), and a loop that repeats the write reports it once (see [`FailureLog`](super::FailureLog)).
pub fn read_only(error: &Error) -> bool {
    matches!(error, Error::Unreachable(message) if message.starts_with(READ_ONLY))
}

impl HubClient {
    /// The answer if its status is a success, else the error its status means.
    pub(super) fn ok(&self, path: &str, answer: Answer) -> Result<Answer> {
        let message = || format!("{path}: {}", String::from_utf8_lossy(&answer.body));
        match answer.status {
            200..=299 => Ok(answer),
            404 => Err(Error::NotFound(message())),
            409 => Err(Error::Conflict(message())),
            // A refused token is the machine's problem to fix (`ignite install` registers it again),
            // not a fault of what was sent, so it is reported like an unreachable hub: callers that
            // queue work keep it queued instead of dropping it as malformed.
            401 | 403 => Err(Error::Unreachable(format!(
                "the hub refused this machine's token ({}){}; run `ignite install` to register it \
             again: {}",
                answer.status,
                if self.token_file.is_some() && answer.status == 401 {
                    " even after it was read again"
                } else {
                    ""
                },
                message()
            ))),
            // Only 400 and 422 say that what was sent is malformed, which is the one answer after
            // which a caller may drop a queued entry, and 413 says that it is too large. Every
            // other refusal (a timeout, a rate limit, a proxy's 4xx) is about the hub's state, not
            // the entry, so the entry is kept and sent again later.
            400 | 422 => Err(Error::Invalid(format!("{} {}", answer.status, message()))),
            // A body too large is a verdict on what was sent, but not that it is malformed: the
            // same content may go in smaller pieces (see `too_large`).
            413 => Err(Error::Invalid(format!("{TOO_LARGE}{}", message()))),
            // The hub's read-only listener answers every write with 405. Pointing `[hub] url` at it
            // is a mistake that no retry fixes, so it gets a message of its own (see `read_only`).
            405 if String::from_utf8_lossy(&answer.body).contains(READ_ONLY_BODY) => {
                Err(Error::Unreachable(format!(
                    "{READ_ONLY}the hub answered {path} with 405 \"{READ_ONLY_BODY}\"; \
                 point [hub] url at the writable listener ({}), since the read-only one is for \
                 the public address",
                    self.base
                )))
            }
            status => Err(Error::Unreachable(format!(
                "the hub answered {status}, which is no verdict on what was sent: {}",
                message()
            ))),
        }
    }
}

pub(super) fn decode<R: DeserializeOwned>(path: &str, body: &[u8]) -> Result<R> {
    serde_json::from_slice(body).map_err(|e| Error::Invalid(format!("{path}: bad reply: {e}")))
}
