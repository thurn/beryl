//! The page `/<ticket>/session/<id>`: the transcript of one CLI worker's session on one ticket,
//! laid out like a conversation in the Claude desktop app. It has a compact header with its
//! ticket, app, model, turn and state, links to the ticket's other sessions, the digest of the
//! transcript as the daemon sends it, and a box at the bottom that tells the worker something.
//! `/<ticket>/session` shows the ticket's latest session, and `/pool/session/<id>`, the page's
//! first address, redirects to the session's page under the ticket it worked last.
//!
//! A ticket can have several sessions (one that gave it up and the next that claimed it, or a
//! regression's session after its landing broke the build), and along a `lane:` chain one
//! session works several tickets. Every transcript line names its ticket, so the page shows only
//! the session's lines on this ticket, and the script skips later lines on another ticket.
//!
//! A transcript can hold secrets, so the page is served on the private listener only (see
//! `Site::private`); the public read-only copy answers 404 for it. The box is shown only on a
//! page with edit controls, to a Tailscale user.
//!
//! The page carries its first lines as JSON, and `static/session.js` renders them and every
//! later line, which it follows with a long poll on
//! `GET /transcripts/<session>?after=<seq>&wait=<seconds>`. Rendering only in the script keeps
//! one copy of the grouping of tool calls and of the Markdown of what the worker wrote.
//!
//! The box does what `ignite tell` does: it comments on the ticket and marks the comment for
//! delivery with the ticket's `tell` key, and with the "now" box ticked also its `tell_now` key,
//! so the daemon ends the running turn and resumes with the message.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use rusqlite::{Connection, OptionalExtension};

use ignite_core::protocol::cli::{StoredTranscriptLine, TELL_NOW_KEY};
use ignite_store::hub::transcripts;

use super::common::brand;
use super::components::{self as c, PageOptions, Tone, esc};
use super::data::ReadError;
use super::references::{display_id, percent_encode, ticket_url};
use crate::dashboard::common::SITE_ICONS;

/// How many lines of the digest the page starts with.
const INITIAL_LINES: usize = 400;

/// The address of the page of the ticket's latest session, under the dashboard's `root`.
pub fn ticket_sessions_url(root: &str, ticket: &str) -> String {
    format!("{}/session", ticket_url(root, ticket))
}

/// The address of the page of `session` on `ticket`, under the dashboard's `root`.
pub fn session_url(root: &str, ticket: &str, session: &str) -> String {
    format!(
        "{}/{}",
        ticket_sessions_url(root, ticket),
        percent_encode(session)
    )
}

/// The page's first address, `/pool/session/<id>`, which redirects to the page under the
/// session's ticket. A link from a worker that holds no ticket goes there.
pub fn pool_session_url(root: &str, session: &str) -> String {
    format!("{root}pool/session/{}", percent_encode(session))
}

/// The ticket whose page `/pool/session/<session>` redirects to: the one the worker holds, or
/// else the ticket of its last line. `None` when the hub knows of neither.
pub(crate) fn ticket_of(conn: &Connection, session: &str) -> Result<Option<String>, ReadError> {
    let held: Option<String> = conn
        .query_row(
            "SELECT ticket FROM workers WHERE session = ?1 AND ticket IS NOT NULL LIMIT 1",
            [session],
            |row| row.get(0),
        )
        .optional()?;
    if held.is_some() {
        return Ok(held);
    }
    Ok(transcripts::last_ticket(conn, session)?.map(|ticket| ticket.to_string()))
}

/// The sessions of `ticket`, oldest first: those with lines on it, then the CLI worker holding
/// it if it has sent no line yet.
pub(crate) fn sessions_of(conn: &Connection, ticket: &str) -> Result<Vec<String>, ReadError> {
    let mut sessions: Vec<String> = transcripts::sessions_of(conn, ticket)?
        .iter()
        .map(ToString::to_string)
        .collect();
    let holder: Option<String> = conn
        .query_row(
            "SELECT session FROM workers WHERE ticket = ?1 AND session LIKE 'cli-%' LIMIT 1",
            [ticket],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(holder) = holder.filter(|holder| !sessions.contains(holder)) {
        sessions.push(holder);
    }
    Ok(sessions)
}

/// The page of `session` on `ticket`, or of the ticket's latest session when `session` is
/// `None`. `None` when the ticket has no such session.
pub(crate) fn options(
    conn: &Connection,
    root: &str,
    project: &str,
    ticket: &str,
    session: Option<&str>,
    editable: bool,
) -> Result<Option<PageOptions>, ReadError> {
    let sessions = sessions_of(conn, ticket)?;
    let Some(session) = session.map_or_else(
        || sessions.last().cloned(),
        |session| sessions.iter().find(|known| *known == session).cloned(),
    ) else {
        return Ok(None);
    };
    let session = session.as_str();
    let worker: Option<(String, String, Option<String>)> = conn
        .query_row(
            "SELECT app, state, ticket FROM workers WHERE session = ?1 LIMIT 1",
            [session],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let lines = transcripts::tail_on(conn, session, ticket, INITIAL_LINES)?;
    let last = lines.last();
    let held = worker
        .as_ref()
        .is_some_and(|(_, _, holds)| holds.as_deref() == Some(ticket));
    let app = worker
        .as_ref()
        .map_or("unknown", |(app, _, _)| app.as_str());
    // A worker gone on to another ticket along a lane is done with this one.
    let state = match &worker {
        Some((_, state, _)) if held => state.as_str(),
        Some(_) => "done",
        None => "gone",
    };
    let tier = tier_of(conn, ticket)?;
    let ticket_link = c::link(
        &ticket_url(root, ticket),
        &esc(&display_id(ticket)),
        None,
        "",
    );
    let tone = match state {
        "down" | "stalled" => Tone::Red,
        "idle" | "gone" | "done" => Tone::Muted,
        _ => Tone::Green,
    };
    let model = tier.map_or_else(|| c::muted("model unknown"), |tier| esc(&tier));
    let turn = last.map_or_else(
        || c::muted("no turn yet"),
        |line| format!("turn {}", line.line.turn),
    );
    let others = session_tabs(root, ticket, &sessions, session);
    let header = format!(
        "<header class=\"c-session-head\"><h1 class=\"c-session-name\">{}</h1>\
         <p class=\"c-session-facts\"><span>{ticket_link}</span><span>{}</span>\
         <span>{model}</span><span>{turn}</span><span>{}</span></p>{others}</header>",
        c::mono(session),
        esc(app),
        c::dot(tone, state, None),
    );
    let after = last.map_or(0, |line| line.seq);
    let transcript = format!(
        "<section id=\"transcript\"><div class=\"c-convo\" id=\"c-transcript\" data-after=\"{after}\"></div>\
         <script type=\"application/json\" id=\"c-transcript-lines\">{}</script>\
         <p class=\"c-muted c-transcript-empty\"{}>No lines yet.</p></section>",
        lines_json(&lines),
        if lines.is_empty() { "" } else { " hidden" }
    );
    let tell = if !editable {
        String::new()
    } else if held {
        tell_box()
    } else {
        "<p class=\"c-session-composer c-muted\">\
         This session no longer holds the ticket, so there is nothing to tell it.</p>"
            .to_owned()
    };
    let context = format!(
        "<div id=\"c-session\" hidden data-session=\"{}\" data-ticket=\"{}\" data-api=\"/{}/api\"></div>",
        esc(session),
        esc(ticket),
        esc(project)
    );
    let body = format!("<div class=\"c-session\">{header}{transcript}{tell}{context}</div>");
    Ok(Some(PageOptions {
        title: format!("Session {session} · {}", brand()),
        body,
        root: root.to_owned(),
        asset_root: Some("/".to_owned()),
        icons: Some(SITE_ICONS),
        crumbs: vec![
            ("Tickets".to_owned(), Some(root.to_owned())),
            (display_id(ticket), Some(ticket_url(root, ticket))),
            ("Session".to_owned(), None),
        ],
        brand: brand(),
        ..PageOptions::default()
    }))
}

/// Links to each session of `ticket`, oldest first, with `current` marked; nothing when the
/// ticket has only one.
fn session_tabs(root: &str, ticket: &str, sessions: &[String], current: &str) -> String {
    if sessions.len() < 2 {
        return String::new();
    }
    let tabs: String = sessions
        .iter()
        .map(|other| {
            // `cli-claude-8da43b65` is shown as `8da43b65`.
            let label = c::mono(other.rsplit('-').next().unwrap_or(other));
            if other == current {
                format!("<span class=\"c-session-tab\" aria-current=\"page\">{label}</span>")
            } else {
                c::link(
                    &session_url(root, ticket, other),
                    &label,
                    None,
                    "c-session-tab",
                )
            }
        })
        .collect();
    format!("<nav class=\"c-session-tabs\" aria-label=\"Sessions\">Sessions {tabs}</nav>")
}

/// The model tier (`sonnet`, `opus`) named by the ticket's `tier:` label.
fn tier_of(conn: &Connection, ticket: &str) -> Result<Option<String>, ReadError> {
    let labels: Option<String> = conn
        .query_row(
            "SELECT labels FROM tickets WHERE id = ?1",
            [ticket],
            |row| row.get(0),
        )
        .optional()?;
    Ok(labels
        .and_then(|labels| serde_json::from_str::<Vec<String>>(&labels).ok())
        .and_then(|labels| {
            labels
                .into_iter()
                .find_map(|label| label.strip_prefix("tier:").map(str::to_owned))
        }))
}

/// The lines as JSON for a `<script type="application/json">` element, in the shape
/// `GET /transcripts/<session>` answers them. A transcript is untrusted text, so every `<`, `>`
/// and `&` is written as a JSON escape, which keeps a `</script>` in a line from ending the
/// element.
fn lines_json(lines: &[StoredTranscriptLine]) -> String {
    serde_json::to_string(lines)
        .unwrap_or_else(|_| "[]".to_owned())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

/// The box at the bottom of the page that tells the worker something.
fn tell_box() -> String {
    format!(
        "<form class=\"c-edit-composer c-tell c-session-composer\" data-tell data-now-key=\"{TELL_NOW_KEY}\">\
         <textarea name=\"text\" rows=\"2\" placeholder=\"Tell the worker\" aria-label=\"Message\"></textarea>\
         <div class=\"c-edit-buttons\">\
         <button type=\"submit\" class=\"c-edit-action c-primary\">Send</button>\
         <label class=\"c-tell-now\"><input type=\"checkbox\" name=\"now\"> Now</label>\
         <span class=\"c-edit-hint\">Sent after the turn ends, or at once with Now.</span>\
         </div></form>"
    )
}
