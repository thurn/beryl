//! The home page: what is in progress, then every open ticket grouped by kind, then the
//! project status, which the page loads as a fragment.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use std::collections::HashMap;

use rusqlite::Connection;

use ignite_core::protocol::tickets::Status;
use ignite_core::time::Timestamp;

use super::attention;
use super::common::{SITE_ICONS, brand, children_progress, person, status_dot, ticket_heading};
use super::components::{self as c, Feed, PageOptions, Tone, esc};
use super::data::{HomeCard, Progress, ReadError, home_data};
use super::references::ticket_url;
use crate::upkeep::filing::HUB_LABEL;
use crate::upkeep::justiciar;

/// Tickets that tools file themselves, grouped by the labels they carry. A ticket goes in the
/// first group one of whose labels it carries. The first groups hold the labels the hub puts on
/// the tickets it files (see `upkeep`); the rest hold labels that the repository's own tools and
/// tickets imported from earlier trackers carry.
const AUTOMATIC: [(&str, &[&str]); 10] = [
    ("Incidents", &[justiciar::LABEL, "ci-incident"]),
    ("Regressions", &["regression"]),
    ("Flaky checks", &["flaky", "flaky-check"]),
    ("Machine faults", &["machine"]),
    (
        "Sweep and nightly failures",
        &["sweep-failure", "nightly-failure", "b3d-sweep"],
    ),
    ("CI time budget", &["ci-cut", "check-budget"]),
    ("CI structure review", &["ci-structure", "ci-structure-log"]),
    ("Count gate", &["count-creep", "count-lower"]),
    ("Check verdicts", &["ci-check"]),
    ("Filed by the hub", &[HUB_LABEL]),
];

/// The index in [`AUTOMATIC`] of the group a ticket with `labels` goes in, if any.
fn automatic_group(labels: &[String]) -> Option<usize> {
    AUTOMATIC.iter().position(|(_, keys)| {
        keys.iter()
            .any(|key| labels.iter().any(|label| label == key))
    })
}

/// Builds the home page for the project at `root`, at feed position `seq`.
#[allow(clippy::too_many_lines)]
pub fn page(root: &str, cards: &[HomeCard], counts: &[Progress], seq: &str) -> PageOptions {
    let progress: HashMap<&str, &Progress> = counts
        .iter()
        .map(|count| (count.id.as_str(), count))
        .collect();
    let mut active: Vec<&HomeCard> = Vec::new();
    let mut epics: Vec<&HomeCard> = Vec::new();
    let mut tickets: Vec<&HomeCard> = Vec::new();
    let mut deferred: Vec<&HomeCard> = Vec::new();
    let mut bug_reports: Vec<&HomeCard> = Vec::new();
    let mut groups: Vec<Vec<&HomeCard>> = AUTOMATIC.iter().map(|_| Vec::new()).collect();

    for ticket in cards {
        let group = automatic_group(&ticket.labels);
        if ticket.card.status == Status::InProgress {
            active.push(ticket);
        }
        if ticket.card.status == Status::Deferred {
            deferred.push(ticket);
        } else if ticket.labels.iter().any(|label| label == "bug-report") {
            bug_reports.push(ticket);
        } else if let Some(group) = group {
            groups[group].push(ticket);
        } else if ticket.card.parent.is_none() {
            if ticket.card.ticket_type == "epic" {
                epics.push(ticket);
            } else {
                tickets.push(ticket);
            }
        }
    }

    let grid = |items: &[&HomeCard], show_kind: bool, show_state: bool| -> String {
        c::grid(items.iter().map(|ticket| {
            let card = &ticket.card;
            let state = if show_state && card.status != Status::Open {
                status_dot(card.status, true)
            } else {
                String::new()
            };
            let kind = if show_kind && card.ticket_type == "epic" {
                c::chip("Epic", Tone::Violet, None)
            } else {
                String::new()
            };
            let assignee = card
                .assignee
                .as_deref()
                .map_or(String::new(), |name| person(Some(name)));
            let foot = format!(
                "{state}{kind}{}{assignee}<span class=\"c-card-time\">{}</span>",
                children_progress(progress.get(card.id.as_str()).copied()),
                c::time(Some(&card.updated_at), "")
            );
            c::card(
                &ticket_url(root, &card.id),
                &format!(
                    "{}<h3 class=\"c-card-title\">{}</h3><div class=\"c-card-foot\">{foot}</div>",
                    ticket_heading(card),
                    esc(&card.title)
                ),
                None,
            )
        }))
    };

    let automatic_total: usize = groups.iter().map(Vec::len).sum();
    let automatic_body: String = groups
        .iter()
        .enumerate()
        .filter(|(_, group)| !group.is_empty())
        .map(|(index, group)| {
            c::subsection(
                AUTOMATIC[index].0,
                &grid(group, true, true),
                Some(group.len()),
                4,
            )
        })
        .collect();
    let sections = [
        ("Epics", "epics", epics.len(), grid(&epics, false, true)),
        (
            "Tickets",
            "tickets",
            tickets.len(),
            grid(&tickets, true, true),
        ),
        (
            "Filed automatically",
            "automatic",
            automatic_total,
            automatic_body,
        ),
        (
            "Bug reports",
            "bug-reports",
            bug_reports.len(),
            grid(&bug_reports, true, true),
        ),
        (
            "Deferred",
            "deferred",
            deferred.len(),
            grid(&deferred, true, true),
        ),
    ];
    let shown: Vec<_> = sections
        .iter()
        .filter(|(_, _, count, _)| *count > 0)
        .collect();
    let total: usize = shown.iter().map(|(_, _, count, _)| count).sum();
    let mut entries: Vec<c::TocEntry> = vec![
        ("Active".into(), "active".into(), Some(active.len()), 0),
        ("All".into(), "all".into(), Some(total), 0),
    ];
    entries.extend(
        shown.iter().map(|(heading, anchor, count, _)| {
            ((*heading).into(), (*anchor).into(), Some(*count), 1)
        }),
    );
    entries.push(("Project status".into(), "project-status".into(), None, 0));
    let nav = c::toc(&entries);

    let active_body = if active.is_empty() {
        c::empty("Nothing is in progress.")
    } else {
        grid(&active, true, false)
    };
    let active_part = c::part("Active", &active_body, "active", Some(active.len()));
    let all_body: String = shown
        .iter()
        .map(|(heading, anchor, count, body)| {
            c::section(heading, body, Some(*count), Some(anchor), 3)
        })
        .collect();
    let all_body = if all_body.is_empty() {
        c::empty("No open tickets.")
    } else {
        all_body
    };
    let all_part = c::part("All", &all_body, "all", Some(total));
    let status = c::part(
        "Project status",
        &c::include(
            &format!("{root}?fragment=status"),
            "Project status is unavailable.",
            3,
        ),
        "project-status",
        None,
    );
    let name = brand(root);
    PageOptions {
        title: format!("Tickets · {name}"),
        body: c::title("Tickets", "", "What is in progress, and everything open.")
            + &c::with_toc(&nav, &(active_part + &all_part + &status)),
        feed: Some(Feed {
            seq: seq.to_owned(),
            tickets: None,
        }),
        root: root.to_owned(),
        asset_root: Some("/".to_owned()),
        icons: Some(SITE_ICONS),
        crumbs: vec![("Tickets".to_owned(), None)],
        brand: name,
        ..PageOptions::default()
    }
}

/// The page for the router.
pub(crate) fn options(
    conn: &Connection,
    root: &str,
    seq: &str,
    now: Timestamp,
) -> Result<PageOptions, ReadError> {
    let (cards, counts) = home_data(conn)?;
    let mut options = page(root, &cards, &counts, seq);
    let attention = attention::line(conn, root, now)?;
    options.body = attention + &options.body;
    Ok(options)
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;
