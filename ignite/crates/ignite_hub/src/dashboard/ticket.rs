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
    PRIORITY_NAMES, SITE_ICONS, brand, person, priority_chip, status_dot, ticket_heading,
    work_progress, worker,
};
use super::components::{self as c, Feed, PageOptions, Tone, esc};
use super::data::{
    Card, NextCopy, ReadError, TicketData, TicketRow, by_attention, ticket_data, ticket_row,
};
use super::edit;
use super::history::timeline;
use super::markdown::render_with_images as render_markdown;
use super::milestones::is_milestone;
use super::pins;
use super::references::{display_id, mentions, rewrite_ticket_paths, ticket_url};

/// A ticket's page for the project `project` at `root`, at feed position `seq`. An `editable`
/// page carries the controls that edit the ticket (see [`edit`]). Only a `private` page, from
/// the private listener, shows the ticket's images (see [`super::attachments`]).
#[allow(clippy::too_many_lines)]
pub fn page(
    root: &str,
    project: &str,
    data: &TicketData,
    pin_properties: [(String, String); 2],
    seq: &str,
    (editable, private): (bool, bool),
) -> PageOptions {
    let TicketData {
        ticket,
        children,
        chain,
        comments,
        history,
        groups,
        forecasts,
        ..
    } = data;
    let card = &ticket.card;
    let display = display_id(&card.id);
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
            let remaining = forecasts.get(&card.id).map_or_else(String::new, |work| {
                format!("<span>{}</span>", esc(&work.breakdown()))
            });
            let html = c::template(
                &key,
                &format!(
                    "{}<div class=\"c-hovercard-title\">{}</div><div class=\"c-hovercard-meta\">{}<span>{}</span>{remaining}</div>",
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
    let image =
        |alt: &str, name: &str| super::attachments::embedded(&data.attachments, name, alt, private);
    let markdown = |text: &str| render_markdown(text, &link_text, &ticket_markup, &image);

    let mut entries: Vec<c::TocEntry> = Vec::new();
    let mut main: Vec<String> = Vec::new();

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
    if !close_reason.trim().is_empty() {
        let (entry, section) =
            plain_section("Close reason", &c::prose(&markdown(close_reason)), None);
        entries.push(entry);
        main.push(section);
    }
    if let Some(reason) = data.releases.reason() {
        let (entry, section) = plain_section("Held back", &c::prose(&esc(&reason)), None);
        entries.push(entry);
        main.push(section);
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
            &(work_progress(forecasts.get(&card.id)) + &priority_chip(i64::from(card.priority))),
            Some(&ticket_url(root, &card.id)),
        )
    };
    // Closed tickets follow the open ones in the same list, with no
    // collapsed group or count header.
    let list_html = |cards: &Vec<&Card>| -> String {
        let (closed, open): (Vec<&Card>, Vec<&Card>) =
            cards.iter().partition(|card| card.status == Status::Closed);
        c::rows(open.iter().chain(closed.iter()).map(|card| row(card)))
    };
    // Milestones stay in plan order (see `milestones::order`), finished ones included.
    let milestones: Vec<&Card> = children.iter().filter(|card| is_milestone(card)).collect();
    let subtasks: Vec<&Card> = children.iter().filter(|card| !is_milestone(card)).collect();
    for (heading, html, count) in [
        (
            "Milestones",
            c::rows(milestones.iter().map(|card| row(card))),
            milestones.len(),
        ),
        ("Subtasks", list_html(&subtasks), subtasks.len()),
    ] {
        if count == 0 {
            continue;
        }
        let (entry, section) = plain_section(heading, &html, Some(count));
        entries.push(entry);
        main.push(section);
    }
    let mut missing: Vec<(&str, &str)> = Vec::new();
    for (heading, field, text) in [
        ("Description", "body", &ticket.body),
        ("Acceptance criteria", "acceptance", &ticket.acceptance),
        ("Design", "design", &ticket.design),
        ("Notes", "notes", &ticket.notes),
    ] {
        let empty = text.trim().is_empty();
        if editable {
            // An empty section is on the page but hidden, for its "add" button to show.
            let anchor = heading.to_lowercase().replace(' ', "-");
            let view = if empty {
                String::new()
            } else {
                c::prose(&markdown(text))
            };
            if empty {
                missing.push((field, heading));
            } else {
                entries.push((heading.to_owned(), anchor.clone(), None, 0));
            }
            main.push(edit::section(heading, &anchor, field, &view, text));
        } else if !empty {
            let (entry, section) = plain_section(heading, &c::prose(&markdown(text)), None);
            entries.push(entry);
            main.push(section);
        }
    }
    if editable {
        main.push(edit::adders(&missing));
    }
    let mut add = |heading: &str, html: &str, count: Option<usize>| {
        let (entry, section) = plain_section(heading, html, count);
        entries.push(entry);
        main.push(section);
    };

    let mut lists: Vec<(String, Vec<&Card>)> = Vec::new();
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
        add(heading, &list_html(cards), Some(cards.len()));
    }

    if !data.attachments.is_empty() {
        add(
            "Attachments",
            &super::attachments::grid(&data.attachments, private),
            Some(data.attachments.len()),
        );
    }
    if !comments.is_empty() || editable {
        let mut body: String = comments
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
        if editable {
            body.push_str(&edit::composer());
        }
        add("Comments", &body, Some(comments.len()));
    }
    if data.spend {
        add(
            "Efficiency",
            &super::spend_view::placeholder(&format!(
                "{}?fragment=spend",
                ticket_url(root, &card.id)
            )),
            None,
        );
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
    let filer_title = ticket
        .metadata
        .iter()
        .find(|(key, _)| key == CREATED_BY_TITLE)
        .and_then(|(_, value)| value.as_str());
    if !ticket.created_by.is_empty() {
        details.push((
            "Created by".into(),
            created_by(&ticket.created_by, filer_title),
        ));
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
        .filter(|(key, _)| key != "_enact_beads" && key != CREATED_BY_TITLE)
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

    let status = status_dot(card.status, true);
    let priority = priority_chip(i64::from(card.priority))
        + &c::muted(
            PRIORITY_NAMES
                .get(usize::from(card.priority))
                .copied()
                .unwrap_or(""),
        );
    // On an editable page a value carries its pencil; a closed ticket's status changes only by
    // reopening it.
    let editable_value = |field: &str, label: &str, kind: &str, view: String, raw: &str| {
        if editable {
            edit::property(field, label, kind, &view, raw)
        } else {
            view
        }
    };
    let status = if card.status == Status::Closed {
        status
    } else {
        editable_value(
            "status",
            "Status",
            "status",
            status,
            edit::status_value(card.status),
        )
    };
    let mut properties: Vec<(String, String)> = vec![
        ("Status".into(), status),
        (
            "Priority".into(),
            editable_value(
                "priority",
                "Priority",
                "priority",
                priority,
                &card.priority.to_string(),
            ),
        ),
        (
            "Assignee".into(),
            assignee_view(card.assignee.as_deref(), &card.id, root, private),
        ),
    ];
    properties.extend(pin_properties);
    if let Some(work) = data.forecasts.get(&card.id) {
        properties.push(("Complete".into(), work_progress(Some(work))));
    }
    if editable && card.ticket_type == "epic" {
        for (label, field, value) in [
            ("Allocation", "workers", ticket.workers),
            ("Max workers", "max_workers", ticket.max_workers),
        ] {
            let view = value.map_or_else(|| c::muted("None"), |n| esc(&n.to_string()));
            let raw = value.map(|n| n.to_string()).unwrap_or_default();
            properties.push((
                label.into(),
                editable_value(field, label, "count", view, &raw),
            ));
        }
    }
    if let Some(next) = &data.next_copy {
        let every = ticket.every_hours.unwrap_or_default();
        let view = esc(&format!(
            "{every} {}",
            if every == 1 { "hour" } else { "hours" }
        ));
        properties.push((
            "Every".into(),
            editable_value("every_hours", "Every", "count", view, &every.to_string()),
        ));
        let next = match next {
            NextCopy::Paused => c::muted("Paused"),
            NextCopy::At(at) => c::time(Some(at), ""),
            NextCopy::AfterLiveCopy => c::muted("When the open copy closes"),
        };
        properties.push(("Next copy".into(), next));
    }
    if let Some(parent) = card.parent.as_deref().and_then(|id| references.get(id)) {
        let label = if parent.ticket_type == "meta" {
            "Schedule"
        } else {
            "Epic"
        };
        properties.push((label.into(), hover_link(parent, &esc(&parent.title))));
    }
    if !ticket.labels.is_empty() || editable {
        let chips: String = ticket
            .labels
            .iter()
            .map(|label| c::chip(label, Tone::Muted, None))
            .collect();
        let view = if chips.is_empty() {
            c::muted("None")
        } else {
            chips
        };
        properties.push((
            "Labels".into(),
            editable_value("labels", "Labels", "labels", view, &ticket.labels.join(" ")),
        ));
    }
    let kind: String = {
        let mut chars = card.ticket_type.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().chain(chars).collect()
        })
    };
    if !data.releases.releases.is_empty() {
        let count = data.releases.releases.len();
        let view = esc(&count.to_string())
            + &if data.releases.held_back() {
                c::chip("Held back", Tone::Amber, None)
            } else {
                String::new()
            };
        properties.push(("Releases".into(), view));
    }
    if private && !data.sessions.is_empty() {
        properties.push((
            "Sessions".into(),
            sessions_view(&data.sessions, &card.id, root),
        ));
    }
    properties.push(("Type".into(), esc(&kind)));
    properties.push(("Created".into(), c::time(Some(&ticket.created_at), "")));
    properties.push(("Updated".into(), c::time(Some(&card.updated_at), "")));
    if let Some(closed_at) = &ticket.closed_at {
        properties.push(("Closed".into(), c::time(Some(closed_at), "")));
    }

    let aside = if editable {
        edit::actions(card.status) + &c::meta(&properties)
    } else {
        c::meta(&properties)
    };
    let layout = c::layout(&main.join(""), &aside);
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
    let mut heading = c::title(&card.title, &eyebrow, "");
    if editable {
        // The title is edited in place, with its pencil after it.
        let plain = format!("<h1 class=\"c-title\">{}</h1>", esc(&card.title));
        let editable_title = format!(
            "<h1 class=\"c-title\">{}{}</h1>",
            edit::unit("title", "line", &esc(&card.title), &card.title, false),
            edit::pencil("title", "Title")
        );
        heading = heading.replacen(&plain, &editable_title, 1)
            + &edit::context(project, &card.id, ticket.version);
    }
    PageOptions {
        title: format!("{} · {display}", card.title),
        body: heading + &body + &hovercards,
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

/// A titled section of the page's main column and its entry in the contents.
/// The metadata key under which `ignite ticket create` saves the title of the Claude desktop
/// session that filed the ticket.
const CREATED_BY_TITLE: &str = "created_by_title";

/// Who filed a ticket: the filing session's title saved with the ticket, else the session's
/// latest title, otherwise the actor. A Claude desktop session (`local_<uuid>`) links to the
/// app, which opens that session.
fn created_by(actor: &str, title: Option<&str>) -> String {
    let latest = super::common::session_title(actor);
    let text = esc(title.or(latest.as_deref()).unwrap_or(actor));
    if actor.starts_with("local_") {
        format!(
            "<a href=\"claude://claude.ai/epitaxy/{}\" title=\"{}\">{text}</a>",
            esc(actor),
            esc(actor)
        )
    } else {
        text
    }
}

fn plain_section(heading: &str, html: &str, count: Option<usize>) -> (c::TocEntry, String) {
    let anchor = heading.to_lowercase().replace(' ', "-");
    let section = c::section(heading, html, count, Some(&anchor), 2);
    ((heading.to_owned(), anchor, count, 0), section)
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
    (editable, private): (bool, bool),
) -> Result<Option<PageOptions>, ReadError> {
    let Some(row): Option<TicketRow> = ticket_row(conn, id)? else {
        return Ok(None);
    };
    let data = ticket_data(conn, project, row)?;
    let pin_properties = pins::properties(conn, id, &data.ticket.labels, editable)?;
    Ok(Some(page(
        root,
        project,
        &data,
        pin_properties,
        seq,
        (editable, private),
    )))
}

/// The assignee of a ticket. A CLI worker's links to the ticket's session page, which only the
/// private listener serves.
fn assignee_view(assignee: Option<&str>, ticket: &str, root: &str, private: bool) -> String {
    let view = worker(assignee, ticket);
    match assignee {
        Some(session) if private && session.starts_with("cli-") => c::link(
            &super::session::ticket_sessions_url(root, ticket),
            &view,
            None,
            "",
        ),
        _ => view,
    }
}

/// The sidebar's link to the ticket's session page: "Transcript" for one session, or how many.
fn sessions_view(sessions: &[String], ticket: &str, root: &str) -> String {
    let label = match sessions.len() {
        1 => "Transcript".to_owned(),
        count => format!("{count} sessions"),
    };
    c::link(
        &super::session::ticket_sessions_url(root, ticket),
        &label,
        None,
        "",
    )
}
