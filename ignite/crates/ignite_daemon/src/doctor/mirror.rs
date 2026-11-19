//! The ticket mirror in `ignite doctor`: how far this machine's append-only copy of the hub's
//! tickets has followed the hub, and a warning when it has not caught up for over an hour.

use std::path::Path;

use ignite_core::ids::Seq;
use ignite_core::time::Timestamp;
use ignite_core::traits::Hub;

use super::Finding;
use crate::mirror::{self, Status};

/// How long a mirror may go without catching up with the hub before `doctor` warns.
const BEHIND_AFTER_S: f64 = 3600.0;

/// What to do about a mirror that is behind.
const FIX: &str = "look at the `ignite-mirror` loop in `ignite status` and the daemon's log; the \
                   mirror is this machine's copy to restore the hub's tickets from";

/// The `mirror` finding for the mirror at `path`, compared with the hub's feed position.
pub fn finding(path: &Path, hub: &dyn Hub, now: Timestamp) -> Finding {
    if !path.exists() {
        return Finding::warning(
            "mirror",
            format!("no ticket mirror at {}.", path.display()),
            "restart the daemon, whose `ignite-mirror` loop creates it",
        );
    }
    let status = ignite_store::mirror::open(path)
        .map_err(|e| e.to_string())
        .and_then(|conn| mirror::status(&conn).map_err(|e| e.to_string()));
    match status {
        Ok(status) => judge(&status, hub.feed(Seq(u64::MAX), 0).ok().map(|b| b.seq), now),
        Err(error) => Finding::problem(
            "mirror",
            format!(
                "the ticket mirror at {} cannot be read: {error}.",
                path.display()
            ),
            FIX,
        ),
    }
}

/// The finding for a mirror in `status` when the hub is at `hub` (`None` if unreachable).
fn judge(status: &Status, hub: Option<Seq>, now: Timestamp) -> Finding {
    let hub_text = hub.map_or_else(|| "unknown".to_owned(), |seq| seq.0.to_string());
    let held = format!("{} versions of {} tickets", status.versions, status.tickets);
    let position = match (status.copying, status.feed) {
        (Some(seq), _) => format!("copying the hub's tickets at position {}", seq.0),
        (None, Some(seq)) => format!("at position {} of the hub's {hub_text}", seq.0),
        (None, None) => "not started".to_owned(),
    };
    let since = status.caught_up_at.or(status.started_at);
    let behind_s = since.map(|since| now.seconds_since(since));
    let caught_up = match (status.caught_up_at, behind_s) {
        (Some(_), Some(seconds)) => format!("caught up {} ago", minutes(seconds)),
        _ => "not caught up yet".to_owned(),
    };
    let text = format!("{position}, {held}, {caught_up}.");
    if behind_s.is_none_or(|seconds| seconds > BEHIND_AFTER_S) {
        Finding::warning(
            "mirror",
            format!("more than an hour behind the hub: {text}"),
            FIX,
        )
    } else {
        Finding::ok("mirror", text)
    }
}

/// `seconds` as whole minutes, or seconds under a minute.
fn minutes(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{}s", seconds.max(0.0).round())
    } else {
        format!("{}m", (seconds / 60.0).floor())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        Timestamp::parse(text).unwrap()
    }

    #[test]
    fn a_mirror_that_caught_up_lately_is_ok_and_one_an_hour_behind_is_a_warning() {
        let status = Status {
            feed: Some(Seq(40)),
            caught_up_at: Some(at("2026-11-18T10:00:00.000Z")),
            started_at: Some(at("2026-11-18T09:00:00.000Z")),
            versions: 12,
            tickets: 10,
            ..Status::default()
        };
        let fresh = judge(&status, Some(Seq(41)), at("2026-11-18T10:00:05.000Z"));
        assert!(fresh.ok && !fresh.warning, "{fresh:?}");
        assert_eq!(
            fresh.text,
            "at position 40 of the hub's 41, 12 versions of 10 tickets, caught up 5s ago."
        );
        let stale = judge(&status, Some(Seq(90)), at("2026-11-18T11:30:00.000Z"));
        assert!(stale.warning, "{stale:?}");
        assert!(stale.text.contains("caught up 90m ago"), "{}", stale.text);
    }

    #[test]
    fn a_first_copy_is_ok_for_an_hour() {
        let status = Status {
            copying: Some(Seq(7)),
            started_at: Some(at("2026-11-18T10:00:00.000Z")),
            ..Status::default()
        };
        assert!(!judge(&status, None, at("2026-11-18T10:10:00.000Z")).warning);
        assert!(judge(&status, None, at("2026-11-18T11:10:00.000Z")).warning);
    }
}
