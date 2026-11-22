//! What happens around every request from a leased tree, and to tickets whose session stopped.
//!
//! - [`ClaimHook`] runs after every request from a leased tree: the command counts as activity,
//!   and its reply starts with the `changed` lines.
//! - [`Claims::changed`] returns the `changed` lines: the comments and edits that someone other
//!   than the session made to its ticket since the session's previous command. The hub is read
//!   only when the change feed names the ticket, or once after the daemon starts. What was shown
//!   is recorded per tree in the local database, so a restart shows nothing twice. A comment
//!   made by `ignite tell` is printed as a record of the message, not as its text, because the
//!   daemon delivers the message separately.
//! - The silence clock (`active_at`) is saved in the local database as it changes and read when
//!   the daemon starts (see `activity`), so a restart does not give a silent session another 30
//!   minutes.
//! - [`Claims::tick`] keeps the clock current: a session whose command is still running, such as
//!   a long `ignite run` or `ignite wait`, is active however long it runs. The daemon's stall
//!   procedure (see `dispatchd_stall`) decides what to do with a session that went silent, and
//!   with one whose command has run far past its time, which this clock does not measure.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::Value;

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::Request;
use ignite_core::protocol::tickets::TicketDetail;
use ignite_core::time::Timestamp;
use ignite_store::local::{rows, shown};

use super::{Claims, lock};
use crate::server::{Context, Hook};

/// The most changes printed on one command; the rest are counted.
pub const MAX_CHANGES: usize = 5;
/// The most lines of one comment printed.
const COMMENT_LINES: usize = 3;

/// The server hook: renews the lease of the request's ticket and returns its `changed` lines.
pub struct ClaimHook(pub Arc<Claims>);

impl Hook for ClaimHook {
    fn run(&self, ctx: &Context, req: &Request, _tree: &rows::Tree) -> Vec<String> {
        self.0.changed(ctx, req)
    }
}

/// The newest comment number in a ticket's detail.
pub fn newest_comment(detail: &TicketDetail) -> i64 {
    detail
        .comments
        .iter()
        .flatten()
        .map(|c| i64::from(c.n))
        .max()
        .unwrap_or(0)
}

/// The newest history sequence number in a ticket's detail.
pub fn newest_history(detail: &TicketDetail) -> i64 {
    detail
        .history
        .iter()
        .flatten()
        .map(|h| i64::try_from(h.seq.0).unwrap_or(i64::MAX))
        .max()
        .unwrap_or(0)
}

/// The edited fields a worker must hear of, by their name in a history entry.
fn edited_field(key: &str) -> Option<&'static str> {
    match key {
        "title" => Some("title"),
        "body" => Some("body"),
        "acceptance" => Some("acceptance"),
        "design" => Some("design"),
        "labels" => Some("labels"),
        _ if key.contains("dep") || key.contains("edge") => Some("dependencies"),
        _ => None,
    }
}

/// One change to print.
struct Change {
    at: Timestamp,
    lines: Vec<String>,
    edit: bool,
}

/// The numbers of the comments that `ignite tell` marked for delivery: every value the
/// ticket's metadata key `tell` has held. The history records each metadata change as
/// `{"metadata": [before, after]}`.
fn tell_comments(detail: &TicketDetail) -> BTreeSet<u64> {
    let tell = |metadata: &Value| metadata.get("tell").and_then(Value::as_u64);
    let mut numbers: BTreeSet<u64> = detail
        .history
        .iter()
        .flatten()
        .filter_map(|entry| tell(entry.change.get("metadata")?.get(1)?))
        .collect();
    numbers.extend(detail.ticket.metadata.get("tell").and_then(Value::as_u64));
    numbers
}

/// The changes in `detail` after `marks` by anyone but `session`.
fn changes_after(detail: &TicketDetail, marks: &rows::Shown, session: &SessionId) -> Vec<Change> {
    let mut changes = Vec::new();
    let tells = tell_comments(detail);
    for comment in detail.comments.iter().flatten() {
        if i64::from(comment.n) <= marks.comment || comment.author == session.as_str() {
            continue;
        }
        if tells.contains(&u64::from(comment.n)) {
            // The daemon delivers a tell separately as a message the agent treats as a
            // teammate's request. Here it is tool output, which they rightly treat as data, so
            // the line records that the message exists and quotes only its first line.
            let first = comment
                .text
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("");
            changes.push(Change {
                at: comment.at,
                lines: vec![format!(
                    "comment: message from {}, delivered by the daemon, starting \"{first}\"",
                    comment.author
                )],
                edit: false,
            });
            continue;
        }
        let mut text = comment.text.lines().filter(|l| !l.trim().is_empty());
        let mut lines = vec![format!(
            "comment by {}: {}",
            comment.author,
            text.next().unwrap_or("").trim()
        )];
        lines.extend(text.take(COMMENT_LINES - 1).map(|l| l.trim().to_owned()));
        changes.push(Change {
            at: comment.at,
            lines,
            edit: false,
        });
    }
    for entry in detail.history.iter().flatten() {
        let seq = i64::try_from(entry.seq.0).unwrap_or(i64::MAX);
        if seq <= marks.history || entry.actor == session.as_str() {
            continue;
        }
        let Value::Object(change) = &entry.change else {
            continue;
        };
        let mut fields: Vec<&str> = change.keys().filter_map(|k| edited_field(k)).collect();
        fields.dedup();
        if fields.is_empty() {
            continue;
        }
        changes.push(Change {
            at: entry.at,
            lines: vec![format!(
                "edit by {}: {} (ignite brief prints the ticket as it is now)",
                entry.actor,
                fields.join(", ")
            )],
            edit: true,
        });
    }
    changes.sort_by_key(|c| c.at);
    changes
}

/// The `changed` lines for `changes`, at most [`MAX_CHANGES`] of them.
fn render(changes: &[Change], ticket: &TicketId) -> Vec<String> {
    let mut out = Vec::new();
    for change in changes.iter().take(MAX_CHANGES) {
        for (i, line) in change.lines.iter().enumerate() {
            let label = if i == 0 { "changed" } else { "" };
            out.push(format!("{label:<8} {line}").trim_end().to_owned());
        }
    }
    if changes.len() > MAX_CHANGES {
        let more = changes.len() - MAX_CHANGES;
        out.push(format!(
            "changed  and {more} more: ignite ticket show {ticket}"
        ));
    }
    out
}

impl Claims {
    /// Records a command from a leased tree as activity, and returns the `changed` lines to
    /// print before the verb's own output. A failure to reach the hub prints nothing and fails
    /// nothing.
    pub fn changed(&self, ctx: &Context, req: &Request) -> Vec<String> {
        let now = self.clock.now();
        let (tree, ticket, session, cursor) = {
            let mut state = lock(&self.state);
            let Some(held) = state
                .held
                .values_mut()
                .find(|h| req.cwd.starts_with(&h.path))
                .filter(|h| req.session.as_ref() == Some(&h.session))
            else {
                return Vec::new();
            };
            held.active_at = now;
            (
                held.tree.clone(),
                held.ticket.clone(),
                held.session.clone(),
                state.cursor,
            )
        };
        self.save_activity([&tree]);
        if let Some(cursor) = cursor
            && let Ok(batch) = ctx.hub.feed(cursor, 0)
        {
            let mut state = lock(&self.state);
            let held: Vec<TicketId> = state.held.values().map(|h| h.ticket.clone()).collect();
            for change in batch.changes {
                if let Some(t) = change.ticket
                    && held.contains(&t)
                {
                    state.dirty.insert(t);
                }
            }
            state.cursor = Some(batch.seq.max(cursor));
        }
        let needed = {
            let state = lock(&self.state);
            state.dirty.contains(&ticket) || state.held.get(&tree).is_some_and(|h| !h.synced)
        };
        if !needed {
            return Vec::new();
        }
        let Ok(detail) = self.desk.detail(&ticket) else {
            return Vec::new();
        };
        let newest = rows::Shown {
            tree: tree.to_string(),
            ticket: ticket.to_string(),
            comment: newest_comment(&detail),
            history: newest_history(&detail),
        };
        let marks = shown::get(&lock(&self.db), tree.as_str())
            .ok()
            .flatten()
            .filter(|m| m.ticket == ticket.as_str());
        // With no marks for this lease, nothing is known to be new: start counting from now.
        let changes = marks.map_or_else(Vec::new, |marks| changes_after(&detail, &marks, &session));
        let _ = shown::set(&lock(&self.db), &newest);
        let mut state = lock(&self.state);
        state.dirty.remove(&ticket);
        let version = detail.ticket.version;
        state.cursor = Some(state.cursor.map_or(version, |c| c.max(version)));
        if let Some(held) = state.held.get_mut(&tree) {
            held.synced = true;
            if changes.iter().any(|c| c.edit) {
                held.brief = None;
            }
        }
        render(&changes, &ticket)
    }

    /// Tidies the held tickets and moves the silence clock of every session with a command
    /// running; the daemon calls it about once a minute.
    pub fn tick(&self, _ctx: &Context) {
        self.forget_released();
        self.retry_failed_resets();
        self.free_lost_tickets();
        let now = self.clock.now();
        let trees: Vec<_> = {
            let mut state = lock(&self.state);
            for held in state.held.values_mut() {
                let running = self
                    .commands
                    .get()
                    .is_some_and(|commands| commands.running(&held.session));
                if running {
                    held.active_at = now;
                }
            }
            state.held.keys().cloned().collect()
        };
        self.save_activity(&trees);
    }
}
