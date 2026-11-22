//! What `ignite pool list` and `ignite pool watch` show of the apps in CLI mode: each slot's
//! state (the turn number and how long it has run, waiting on an `ignite` verb, backing off,
//! parked or free) and each app's CLI hold.

use std::fmt::Write as _;

use ignite_core::cli::{Hold, Slot, SlotState};
use ignite_core::ids::SessionId;
use ignite_core::protocol::cli::{TranscriptKind, TranscriptLine};
use ignite_core::time::Timestamp;
use ignite_core::traits::Result;

use super::HeadlessDispatch;

/// `seconds` as the largest whole unit, such as `45s`, `12m` or `3h`.
pub(crate) fn duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    if seconds < 60.0 {
        format!("{seconds:.0}s")
    } else if seconds < 3600.0 {
        format!("{:.0}m", seconds / 60.0)
    } else {
        format!("{:.0}h", seconds / 3600.0)
    }
}

/// The words after `slot` that say what the slot is doing at `now`.
fn describe(slot: &Slot, now: Timestamp) -> String {
    let age = slot
        .since
        .map_or_else(String::new, |since| duration(now.seconds_since(since)));
    let turn = slot.turn;
    match slot.state {
        SlotState::Starting => format!("turn {turn} starting"),
        SlotState::Running => format!("turn {turn} running for {age}"),
        SlotState::WaitingVerb => format!("turn {turn} ended, waiting on a verb for {age}"),
        SlotState::BackingOff => format!("backing off for {age} after turn {turn}"),
        SlotState::Parked => format!("parked for {age} after turn {turn}"),
        SlotState::Free => "free".to_owned(),
    }
}

/// The digest lines after the first `after`, as `ignite pool watch` prints them: a `turn` line
/// before the first line of each turn, then one line each for what the worker said, each tool
/// call, the start of each call's output and each turn's result. Empty when there is nothing after `after`.
pub fn digest_text(lines: &[TranscriptLine], after: usize) -> String {
    let mut out = Vec::new();
    for line in lines.iter().skip(after) {
        if line.index == 0 {
            out.push(format!("turn     {}", line.turn));
        }
        let label = match line.kind {
            TranscriptKind::Assistant => "say",
            TranscriptKind::Tool => "tool",
            TranscriptKind::Result => "result",
            TranscriptKind::Prompt => "prompt",
            TranscriptKind::ToolResult => "output",
        };
        out.push(format!("{label:<8} {}", line.text));
    }
    out.join("\n")
}

fn describe_hold(hold: &Hold, now: Timestamp) -> String {
    format!(
        "hold     {} CLI hold for {} ({}): {}",
        hold.app,
        duration(now.seconds_since(hold.since)),
        hold.reason.as_str(),
        hold.message
    )
}

impl HeadlessDispatch {
    /// One line per slot of each app in CLI mode, and one per CLI hold, as `ignite pool list`
    /// prints them. Empty when no app runs in CLI mode.
    pub fn cli_lines(&self, now: Timestamp) -> Vec<String> {
        let Some(cli) = self.cli.get() else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        for app in self.cli_apps() {
            if let Some(hold) = cli.pool.held(app) {
                lines.push(describe_hold(&hold, now));
            }
            for slot in cli.pool.slots(app) {
                let mut line = format!("slot     {app} {}  {}", slot.index, describe(&slot, now));
                if let Some(ticket) = &slot.ticket {
                    let _ = write!(line, "  {ticket}");
                }
                if let Some(session) = &slot.session {
                    let _ = write!(line, "  {session}");
                }
                lines.push(line);
            }
        }
        lines
    }

    /// The local CLI holds shown by `ignite status`.
    pub fn cli_hold_lines(&self, now: Timestamp) -> impl Iterator<Item = String> + '_ {
        self.cli.get().into_iter().flat_map(move |cli| {
            self.cli_apps()
                .filter_map(move |app| cli.pool.held(app).map(|hold| describe_hold(&hold, now)))
        })
    }

    /// The digest of the CLI session `target` names (a session ID or a ticket), from whichever
    /// app's pool has it.
    pub fn cli_digest(&self, target: &str) -> Result<Option<(SessionId, Vec<TranscriptLine>)>> {
        match self.cli.get() {
            Some(cli) => cli.pool.digest(target),
            None => Ok(None),
        }
    }
}
