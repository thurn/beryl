//! What happens around every request from a leased tree, and to tickets whose session stopped.
//!
//! - [`ClaimHook`] runs after every request from a leased tree: the command counts as activity,
//!   and its reply starts with the `changed` lines.
//! - [`Claims::changed`] returns the `changed` lines: the comments and edits that someone other
//!   than the session made to its ticket since the session's previous command. The hub is read
//!   only when the change feed names the ticket, or once after the daemon starts. What was shown
//!   is recorded per tree in the local database, so a restart shows nothing twice.
//! - The silence clock (`active_at` and the stall's place) is saved in the local database as it
//!   changes and read when the daemon starts (see `activity`), so a restart does not give a
//!   silent session another 30 minutes.
//! - [`Claims::tick`] runs the stall procedure. A session whose command is still running, such
//!   as a long `ignite run` or `ignite wait`, is active however long it runs. After 30 minutes
//!   with no command running or ended, it queues a `check` of the session with the dispatcher and acts on the answer:
//!   running, nothing; idle, one message, then a second `check` 15 minutes later and a release
//!   only if the session is still idle; gone, a release. A lookup that failed is no answer, so
//!   it is asked again 5 minutes later. A release saves the tree's work first, like
//!   `ignite release`.

use std::fmt::Write as _;
use std::sync::Arc;

use serde_json::Value;

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::Request;
use ignite_core::protocol::tickets::TicketDetail;
use ignite_core::time::Timestamp;
use ignite_store::local::{rows, shown};

use super::{Claims, Held, Liveness, Saved, Stall, lock};
use crate::server::{Context, Hook};

/// Seconds with no command before the dispatcher is asked to look at the session.
pub const QUIET_S: f64 = 30.0 * 60.0;
/// Seconds after the message before the second look.
pub const GRACE_S: f64 = 15.0 * 60.0;
/// Seconds before a `check` whose lookup failed is asked again.
pub const RETRY_S: f64 = 5.0 * 60.0;
/// The most changes printed on one command; the rest are counted.
pub const MAX_CHANGES: usize = 5;
/// The most lines of one comment printed.
const COMMENT_LINES: usize = 3;

/// The message an idle session gets.
pub const NUDGE: &str = "No `ignite` command has come from your tree for 30 minutes. Continue \
                         your ticket, or give it back with `ignite release -m \"<its exact \
                         state>\"`.";

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

/// The changes in `detail` after `marks` by anyone but `session`.
fn changes_after(detail: &TicketDetail, marks: &rows::Shown, session: &SessionId) -> Vec<Change> {
    let mut changes = Vec::new();
    for comment in detail.comments.iter().flatten() {
        if i64::from(comment.n) <= marks.comment || comment.author == session.as_str() {
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

/// What the stall procedure decided for one ticket.
enum Verdict {
    Release(Held, Liveness),
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
            held.stall = Stall::Active;
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

    /// Runs the stall procedure on every held ticket; the daemon calls it about once a minute.
    pub fn tick(&self, ctx: &Context) {
        self.forget_released();
        self.retry_failed_resets();
        self.free_closed_tickets();
        let now = self.clock.now();
        let mut verdicts = Vec::new();
        {
            let mut state = lock(&self.state);
            for held in state.held.values_mut() {
                if let Some(verdict) = self.step(held, now) {
                    verdicts.push(verdict);
                }
            }
        }
        let trees: Vec<_> = lock(&self.state).held.keys().cloned().collect();
        self.save_activity(&trees);
        for Verdict::Release(held, liveness) in verdicts {
            let quiet = (now.seconds_since(held.active_at) / 60.0).round();
            let state = if liveness == Liveness::Gone {
                "gone"
            } else {
                "idle"
            };
            let note = |saved: &Saved| {
                let mut note = format!(
                    "Released by the daemon: no `ignite` command for {quiet} minutes, and the \
                     desktop app says the session is {state}. The tree held the branch {} with \
                     {} commits",
                    held.branch, saved.commits
                );
                if saved.uncommitted.is_empty() {
                    note.push_str(" and no uncommitted files");
                } else {
                    let _ = write!(note, " and uncommitted {}", saved.uncommitted.join(", "));
                }
                let _ = write!(note, "; all of it is saved as {}.", saved.branch);
                note
            };
            if self.give_back(ctx, &held, note, None, None).is_err() {
                // The work could not be saved or the hub not told. The session keeps the
                // ticket, and the procedure starts again.
                let mut state = lock(&self.state);
                if let Some(h) = state.held.get_mut(&held.tree) {
                    h.stall = Stall::Active;
                    h.active_at = now;
                }
                drop(state);
                self.save_activity([&held.tree]);
            }
        }
    }

    /// Moves one ticket through the stall procedure.
    fn step(&self, held: &mut Held, now: Timestamp) -> Option<Verdict> {
        match held.stall {
            Stall::Active => {
                let running = self
                    .commands
                    .get()
                    .is_some_and(|commands| commands.running(&held.session));
                if running {
                    held.active_at = now;
                } else if now.seconds_since(held.active_at) >= QUIET_S {
                    held.stall = Stall::Asked {
                        check: self.dispatcher.check(&held.session),
                        second: false,
                        at: now,
                    };
                }
            }
            Stall::Asked { check, second, .. } => match self.dispatcher.answer(check)? {
                Liveness::Running => {
                    held.stall = Stall::Active;
                    held.active_at = now;
                }
                Liveness::Unknown => held.stall = Stall::Retry { at: now, second },
                Liveness::Idle if !second => {
                    self.dispatcher.message(&held.session, NUDGE);
                    held.stall = Stall::Messaged { at: now };
                }
                liveness @ (Liveness::Idle | Liveness::Gone) => {
                    return Some(Verdict::Release(held.clone(), liveness));
                }
            },
            Stall::Messaged { at } => {
                if now.seconds_since(at) >= GRACE_S {
                    held.stall = Stall::Asked {
                        check: self.dispatcher.check(&held.session),
                        second: true,
                        at: now,
                    };
                }
            }
            Stall::Retry { at, second } => {
                if now.seconds_since(at) >= RETRY_S {
                    held.stall = Stall::Asked {
                        check: self.dispatcher.check(&held.session),
                        second,
                        at: now,
                    };
                }
            }
        }
        None
    }
}
