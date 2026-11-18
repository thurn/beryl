//! A ticket's page: its text sections, its subtasks and dependencies, its comments and
//! changelog, and a rail of properties.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use std::collections::HashMap;

use rusqlite::Connection;
use serde_json::Value;

use ignite_core::protocol::tickets::Status;

use super::common::{
    PRIORITY_NAMES, SITE_ICONS, brand, children_progress, person, priority_chip, status_dot,
    ticket_heading,
};
use super::components::{self as c, Feed, PageOptions, Tone, esc};
use super::data::{
    Card, Progress, ReadError, TicketData, TicketRow, by_attention, ticket_data, ticket_row,
};
use super::history::timeline;
use super::markdown::render as render_markdown;
use super::references::{display_id, mentions, rewrite_ticket_paths, ticket_url};

/// A ticket's page for the project `project` at `root`, at feed position `seq`.
#[allow(clippy::too_many_lines)]
pub fn page(root: &str, project: &str, data: &TicketData, seq: &str) -> PageOptions {
    let TicketData {
        ticket,
        children,
        chain,
        comments,
        history,
        groups,
        counts,
        ..
    } = data;
    let card = &ticket.card;
    let display = display_id(&card.id);
    let progress: HashMap<&str, &Progress> = counts
        .iter()
        .map(|count| (count.id.as_str(), count))
        .collect();
    let references: HashMap<&str, &Card> = data
        .references
        .iter()
        .map(|card| (card.id.as_str(), card))
        .collect();
    let links: HashMap<&str, &str> = data
        .links
        .iter()
        .map(|(name, id)| (name.as_str(), id.as_str()))
        .collect();
    let templates = std::cell::RefCell::new(Vec::<(String, String)>::new());

    let hover_link = |card: &Card, body: &str| -> String {
        let key = format!("hc-{}", card.id);
        let mut templates = templates.borrow_mut();
        if !templates.iter().any(|(name, _)| *name == key) {
            let count = progress.get(card.id.as_str());
            let bar = count.map_or(String::new(), |count| {
                c::progress(count.closed, count.total, None)
            });
            let html = c::template(
                &key,
                &format!(
                    "{}<div class=\"c-hovercard-title\">{}</div><div class=\"c-hovercard-meta\">{}<span>{}</span>{bar}</div>",
                    ticket_heading(card),
                    esc(&card.title),
                    status_dot(card.status, true),
                    esc(&card.ticket_type)
                ),
            );
            templates.push((key.clone(), html));
        }
        c::link(&ticket_url(root, &card.id), body, Some(&key), "")
    };

    let lookup = |name: &str| links.get(name).and_then(|id| references.get(id).copied());
    let link_text = |text: &str| -> String {
        mentions(project, text, &mut |name| match lookup(name) {
            Some(found) => hover_link(found, &c::mono(&display_id(&found.id))),
            None => name.to_owned(),
        })
    };
    let ticket_markup = |source: &str| -> String {
        let rewritten = rewrite_ticket_paths(project, source, &|name| {
            links
                .get(name)
                .filter(|id| references.contains_key(*id))
                .map(|id| (*id).to_owned())
        });
        rewrite_code_and_labels(&rewritten, &|name| {
            lookup(name).map(|found| display_id(&found.id))
        })
    };
    let markdown = |text: &str| render_markdown(text, &link_text, &ticket_markup);

    let mut entries: Vec<c::TocEntry> = Vec::new();
    let mut main: Vec<String> = Vec::new();
    let mut add = |heading: &str, html: &str, count: Option<usize>| {
        let anchor = heading.to_lowercase().replace(' ', "-");
        entries.push((heading.to_owned(), anchor.clone(), count, 0));
        main.push(c::section(heading, html, count, Some(&anchor), 2));
    };

    let eyebrow = c::mono(&display)
        + &if card.ticket_type == "epic" {
            c::chip("Epic", Tone::Violet, None)
        } else {
            String::new()
        }
        + &status_dot(card.status, true);
    let close_reason = if card.status == Status::Closed {
        ticket.close_reason.as_deref().unwrap_or("")
    } else {
        ""
    };
    for (heading, text) in [
        ("Close reason", close_reason),
        ("Description", &ticket.body),
        ("Acceptance criteria", &ticket.acceptance),
        ("Design", &ticket.design),
        ("Notes", &ticket.notes),
    ] {
        if !text.trim().is_empty() {
            add(heading, &c::prose(&markdown(text)), None);
        }
    }

    let row = |card: &Card| -> String {
        let label = format!(
            "{}<span class=\"c-row-id c-mono c-hide-narrow\">{}</span><span class=\"c-row-text\">{}</span>",
            status_dot(card.status, false),
            esc(&display_id(&card.id)),
            esc(&card.title)
        );
        c::row(
            &label,
            &(children_progress(progress.get(card.id.as_str()).copied())
                + &priority_chip(i64::from(card.priority))),
            Some(&ticket_url(root, &card.id)),
        )
    };
    let is_container = |card: &&Card| card.ticket_type == "epic" || card.ticket_type == "milestone";
    let mut lists: Vec<(String, Vec<&Card>)> = vec![
        (
            "Milestones".to_owned(),
            children.iter().filter(is_container).collect(),
        ),
        (
            "Subtasks".to_owned(),
            children.iter().filter(|card| !is_container(card)).collect(),
        ),
    ];
    for (name, ids) in groups {
        let mut cards: Vec<&Card> = ids
            .iter()
            .filter_map(|id| references.get(id.as_str()).copied())
            .collect();
        cards.sort_by(|left, right| by_attention(left, right));
        lists.push(((*name).to_owned(), cards));
    }
    for (heading, cards) in &lists {
        if cards.is_empty() {
            continue;
        }
        let (closed, open): (Vec<&Card>, Vec<&Card>) =
            cards.iter().partition(|card| card.status == Status::Closed);
        let archived = if closed.is_empty() {
            String::new()
        } else {
            c::rows(closed.iter().map(|card| row(card)))
        };
        let body = if open.is_empty() {
            archived
        } else {
            let folded = if archived.is_empty() {
                String::new()
            } else {
                c::disclosure(&format!("{} closed", closed.len()), &archived, false)
            };
            c::rows(open.iter().map(|card| row(card))) + &folded
        };
        add(heading, &body, Some(cards.len()));
    }

    if !comments.is_empty() {
        let body: String = comments
            .iter()
            .map(|comment| {
                c::comment(
                    &comment.author,
                    &person(Some(&comment.author)),
                    &c::time(Some(&comment.at), ""),
                    &markdown(&comment.text),
                )
            })
            .collect();
        add("Comments", &body, Some(comments.len()));
    }

    // Tickets imported from the old tracker keep its owner and start time under one key.
    let legacy = ticket
        .metadata
        .iter()
        .find(|(key, _)| key == "_enact_beads")
        .map(|(_, value)| value);
    let legacy_text = |key: &str| {
        legacy
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .unwrap_or("")
    };
    let started = legacy_text("started_at");
    let link_detail = |escaped: &str| link_text(escaped);
    add(
        "Changelog",
        &timeline(ticket, history, &link_detail, started),
        None,
    );
    let mut details: Vec<(String, String)> = Vec::new();
    if !ticket.created_by.is_empty() {
        details.push(("Created by".into(), esc(&ticket.created_by)));
    }
    if !legacy_text("owner").is_empty() {
        details.push(("Owner".into(), esc(legacy_text("owner"))));
    }
    if !started.is_empty() {
        details.push(("Started".into(), c::time(Some(started), "")));
    }
    let mut metadata: Vec<&(String, Value)> = ticket
        .metadata
        .iter()
        .filter(|(key, _)| key != "_enact_beads")
        .collect();
    metadata.sort_by(|left, right| left.0.cmp(&right.0));
    for (key, value) in metadata {
        details.push((
            key.clone(),
            c::mono(
                &value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned),
            ),
        ));
    }
    if !details.is_empty() {
        main.push(c::section(
            "",
            &c::disclosure("Details", &c::meta(&details), false),
            None,
            None,
            2,
        ));
    }

    let mut properties: Vec<(String, String)> = vec![
        ("Status".into(), status_dot(card.status, true)),
        (
            "Priority".into(),
            priority_chip(i64::from(card.priority))
                + &c::muted(
                    PRIORITY_NAMES
                        .get(usize::from(card.priority))
                        .copied()
                        .unwrap_or(""),
                ),
        ),
        ("Assignee".into(), person(card.assignee.as_deref())),
    ];
    if let Some(parent) = card.parent.as_deref().and_then(|id| references.get(id)) {
        properties.push(("Epic".into(), hover_link(parent, &esc(&parent.title))));
    }
    if !ticket.labels.is_empty() {
        properties.push((
            "Labels".into(),
            ticket
                .labels
                .iter()
                .map(|label| c::chip(label, Tone::Muted, None))
                .collect(),
        ));
    }
    let kind: String = {
        let mut chars = card.ticket_type.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().chain(chars).collect()
        })
    };
    properties.push(("Type".into(), esc(&kind)));
    properties.push(("Created".into(), c::time(Some(&ticket.created_at), "")));
    properties.push(("Updated".into(), c::time(Some(&card.updated_at), "")));
    if let Some(closed_at) = &ticket.closed_at {
        properties.push(("Closed".into(), c::time(Some(closed_at), "")));
    }

    let layout = c::layout(&main.join(""), &c::meta(&properties));
    let mut crumbs: Vec<c::Crumb> = vec![("Tickets".to_owned(), Some(root.to_owned()))];
    crumbs.extend(
        chain
            .iter()
            .filter(|id| references.contains_key(id.as_str()))
            .map(|id| (display_id(id), Some(ticket_url(root, id)))),
    );
    crumbs.push((display.clone(), None));
    let mut watched = vec![card.id.clone()];
    watched.extend(data.watched.iter().cloned());
    let body = if entries.len() >= 3 {
        c::with_toc(&c::toc(&entries), &layout)
    } else {
        layout
    };
    let hovercards: String = templates
        .borrow()
        .iter()
        .map(|(_, html)| html.as_str())
        .collect();
    PageOptions {
        title: format!("{} · {display}", card.title),
        body: c::title(&card.title, &eyebrow, "") + &body + &hovercards,
        feed: Some(Feed {
            seq: seq.to_owned(),
            tickets: Some(watched),
        }),
        root: root.to_owned(),
        asset_root: Some("/".to_owned()),
        icons: Some(SITE_ICONS),
        brand: brand(),
        description: card.title.clone(),
        crumbs,
        ..PageOptions::default()
    }
}

/// A ticket named in backticks or as the label of a Markdown link is shown by its display name.
fn rewrite_code_and_labels(text: &str, display: &dyn Fn(&str) -> Option<String>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let name_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '.';
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let open = chars[index];
        // `name` not inside a longer run of backticks, and [name] followed by `(`.
        let (close_char, lone) = match open {
            '`' => ('`', (index == 0 || chars[index - 1] != '`')),
            '[' => (']', true),
            _ => ('\0', false),
        };
        if lone && close_char != '\0' {
            let length = chars[index + 1..]
                .iter()
                .take_while(|&&c| name_char(c))
                .count();
            let end = index + 1 + length;
            let closes = chars.get(end) == Some(&close_char)
                && length > 0
                && if open == '`' {
                    chars.get(end + 1) != Some(&'`')
                } else {
                    chars.get(end + 1) == Some(&'(')
                };
            let name: String = chars[index + 1..end].iter().collect();
            if closes
                && valid_name(&name)
                && let Some(shown) = display(&name)
            {
                out.push(open);
                out.push_str(&shown);
                out.push(close_char);
                index = end + 1;
                continue;
            }
        }
        out.push(open);
        index += 1;
    }
    out
}

/// `T?[A-Za-z0-9-]+(\.[0-9]+)*` exactly.
fn valid_name(name: &str) -> bool {
    let body = name.strip_prefix('T').unwrap_or(name);
    let mut parts = body.split('.');
    let head = parts.next().unwrap_or("");
    !head.is_empty()
        && head.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && parts.all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// The page for the router; `None` when the ticket does not exist.
pub(crate) fn options(
    conn: &Connection,
    root: &str,
    project: &str,
    id: &str,
    seq: &str,
) -> Result<Option<PageOptions>, ReadError> {
    let Some(row): Option<TicketRow> = ticket_row(conn, id)? else {
        return Ok(None);
    };
    let data = ticket_data(conn, project, row)?;
    Ok(Some(page(root, project, &data, seq)))
}
