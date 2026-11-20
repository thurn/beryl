//! The page `/pool/session/<id>` of a CLI worker, laid out like a conversation in the Claude
//! desktop app: a compact header with its ticket, app, model, turn and state, the digest of its
//! transcript as the daemon sends it, and a box at the bottom that tells the worker something.
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

/// The address of the page of `session`, under the dashboard's `root`.
pub fn session_url(root: &str, session: &str) -> String {
    format!("{root}pool/session/{}", percent_encode(session))
}

/// The page of `session`, or `None` when the hub knows nothing of it: no worker of that name and
/// no transcript line.
pub(crate) fn options(
    conn: &Connection,
    root: &str,
    project: &str,
    session: &str,
    editable: bool,
) -> Result<Option<PageOptions>, ReadError> {
    let worker: Option<(String, String, Option<String>)> = conn
        .query_row(
            "SELECT app, state, ticket FROM workers WHERE session = ?1 LIMIT 1",
            [session],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let lines = transcripts::tail(conn, session, INITIAL_LINES)?;
    if worker.is_none() && lines.is_empty() {
        return Ok(None);
    }
    let last = lines.last();
    let ticket = worker
        .as_ref()
        .and_then(|(_, _, ticket)| ticket.clone())
        .or_else(|| last.and_then(|line| line.line.ticket.as_ref().map(ToString::to_string)));
    let held = worker
        .as_ref()
        .is_some_and(|(_, _, ticket)| ticket.is_some());
    let app = worker
        .as_ref()
        .map_or("unknown", |(app, _, _)| app.as_str());
    let state = worker
        .as_ref()
        .map_or("gone", |(_, state, _)| state.as_str());
    let tier = match &ticket {
        Some(id) => tier_of(conn, id)?,
        None => None,
    };
    let ticket_link = ticket.as_ref().map_or_else(
        || c::muted("no ticket"),
        |id| c::link(&ticket_url(root, id), &esc(&display_id(id)), None, ""),
    );
    let tone = match state {
        "down" | "stalled" => Tone::Red,
        "idle" | "gone" => Tone::Muted,
        _ => Tone::Green,
    };
    let model = tier.map_or_else(|| c::muted("model unknown"), |tier| esc(&tier));
    let turn = last.map_or_else(
        || c::muted("no turn yet"),
        |line| format!("turn {}", line.line.turn),
    );
    let header = format!(
        "<header class=\"c-session-head\"><h1 class=\"c-session-name\">{}</h1>\
         <p class=\"c-session-facts\"><span>{ticket_link}</span><span>{}</span>\
         <span>{model}</span><span>{turn}</span><span>{}</span></p></header>",
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
    } else if ticket.is_some() && held {
        tell_box()
    } else {
        "<p class=\"c-session-composer c-muted\">\
         The worker holds no ticket, so there is nothing to tell it.</p>"
            .to_owned()
    };
    let context = format!(
        "<div id=\"c-session\" hidden data-session=\"{}\" data-ticket=\"{}\" data-api=\"/{}/api\"></div>",
        esc(session),
        esc(ticket.as_deref().unwrap_or_default()),
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
            ("Pool".to_owned(), Some(format!("{root}pool"))),
            (session.to_owned(), None),
        ],
        brand: brand(),
        ..PageOptions::default()
    }))
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
