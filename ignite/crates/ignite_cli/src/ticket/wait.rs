//! `ignite ticket wait`: block until a ticket has a status, using the hub's long-poll feed.

use std::time::{Duration, Instant};

use ignite_core::protocol::client::{Exit, Reply};
use serde_json::Value;

use super::http::{Hub, Result, usage};
use super::{ids, render};

/// The longest single poll, so that a dropped connection is noticed within about this long.
const POLL_S: u64 = 30;

/// Returns the ticket once its status is `status`, or exit 5 when `timeout_s` runs out first.
pub fn wait(hub: &Hub, id: &str, status: &str, timeout_s: u32) -> Result<Reply> {
    let deadline = Instant::now() + Duration::from_secs(u64::from(timeout_s));
    let mut since = 0;
    loop {
        let mut ticket = hub.get(&["tickets", id], &[])?.json()?;
        if ticket["status"] == status {
            ids::display(&mut ticket);
            return Ok(Reply {
                exit: Exit::Success,
                text: render::detail(&ticket),
                json: Some(ticket),
                handle: None,
            });
        }
        // Any later change to this ticket has a feed position above its version, so polling from
        // there cannot miss the one that sets the status, whatever happens between the read and
        // the poll.
        since = since.max(
            ticket["version"]
                .as_u64()
                .ok_or_else(|| usage("no version"))?,
        );
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            let message = format!(
                "timed out after {timeout_s}s: {id} is {}, not {status}",
                ticket["status"].as_str().unwrap_or("unknown")
            );
            return Ok(Reply::text(Exit::Running, message));
        }
        let seconds = left.as_secs().clamp(1, POLL_S);
        let batch: Value = hub
            .poll(
                &["feed"],
                &[("since", since.to_string()), ("wait", seconds.to_string())],
                Duration::from_secs(seconds),
            )?
            .json()?;
        since = since.max(batch["seq"].as_u64().unwrap_or(since));
    }
}
