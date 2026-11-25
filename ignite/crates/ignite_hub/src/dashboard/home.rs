//! The home page shows what needs a person, what is in progress, every open ticket grouped by
//! kind, and the project status, which the page loads as a fragment.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use std::collections::BTreeMap;

use rusqlite::Connection;

use ignite_core::config::PoolConfig;
use ignite_core::dispatch::label;
use ignite_core::fleet::FleetSettings;
use ignite_core::protocol::tickets::Status;
use ignite_core::shares::{DEFAULT_GENERAL_ALLOCATION, Source};
use ignite_core::time::Timestamp;
use ignite_store::hub::fleet_settings;

use super::attention;
use super::common::{
    SITE_ICONS, allocation_chip, brand, status_dot, work_progress, worker_unlinked,
};
use super::components::{self as c, Feed, PageOptions, Tone, esc};
use super::data::{Forecasts, HomeCard, ReadError, home_data};
use super::edit;
use super::references::{display_id, ticket_url};
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

// Incidents and regressions are shown first, before the pools for ordinary work.
const URGENT_GROUPS: usize = 2;

/// The index in [`AUTOMATIC`] of the group a ticket with `labels` goes in, if any.
fn automatic_group(labels: &[String]) -> Option<usize> {
    AUTOMATIC.iter().position(|(_, keys)| {
        keys.iter()
            .any(|key| labels.iter().any(|label| label == key))
    })
}

/// A machine's hostname as a card shows it: without its domain, and capitalized (`Multivac`).
fn machine_label(hostname: &str) -> String {
    let host = hostname.split('.').next().unwrap_or(hostname);
    let mut chars = host.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A card's tags: its allocation, the machine that runs it or else the one it is pinned to, and
/// the model it runs on or is assigned to. Each shows once, in the allocation chip's tone.
fn card_tags(ticket: &HomeCard, pool: Option<&PoolConfig>) -> String {
    let card = &ticket.card;
    let allocation = allocation_chip(
        card.allocation
            .filter(|_| card.status != Status::PendingApproval),
    );
    let tag = |name: &str, tip: &str| c::chip(name, Tone::Neutral, Some(tip));
    let machine = match (&ticket.machine, &card.pins.machine) {
        (Some(name), _) => tag(&machine_label(name), "Running on this machine"),
        (None, Some(name)) => tag(&machine_label(name), "Pinned to this machine"),
        (None, None) => String::new(),
    };
    let model = model(ticket, pool).map_or_else(String::new, |(name, tip)| tag(&name, tip));
    allocation + &machine + &model
}

/// The model a ticket runs on, or else the one it is assigned to, with the tag's tip. The
/// provider working it, or else the one it is pinned to, maps the ticket's `tier:` label to a
/// model ID; an untiered ticket takes the provider's `sonnet` model, as workers do. With no
/// provider the tier is all that is known, so the tier is named.
fn model(ticket: &HomeCard, pool: Option<&PoolConfig>) -> Option<(String, &'static str)> {
    let tier = label(&ticket.labels, "tier:");
    let (provider, tip) = match (&ticket.provider, &ticket.card.pins.provider) {
        (Some(provider), _) => (provider, "Running on this model"),
        (None, Some(provider)) => (provider, "Assigned to this model"),
        (None, None) => return tier.map(|tier| (tier.to_owned(), "Assigned to this tier")),
    };
    let tier = tier.unwrap_or("sonnet");
    let id = pool
        .and_then(|pool| pool.model_id(provider, tier))
        .unwrap_or_else(|| tier.to_owned());
    Some((model_name(&id), tip))
}

/// A model ID as a tag names it: its words after the family name, without version numbers
/// (`claude-opus-5-5` is `opus`, `gemini-4-neon` is `neon`). An ID with no such word is shown
/// whole.
fn model_name(id: &str) -> String {
    let words: Vec<&str> = id
        .split('-')
        .skip(1)
        .filter(|word| word.starts_with(|first: char| first.is_ascii_alphabetic()))
        .collect();
    if words.is_empty() {
        id.to_owned()
    } else {
        words.join("-")
    }
}

/// Builds the home page for the project at `root`, at feed position `seq`.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
fn page(
    root: &str,
    cards: &[HomeCard],
    forecasts: &Forecasts,
    attention_cards: Vec<String>,
    seq: &str,
    settings: &FleetSettings,
    editable: Option<&str>,
    pool: Option<&PoolConfig>,
) -> PageOptions {
    let mut active: Vec<&HomeCard> = Vec::new();
    let mut epics: Vec<&HomeCard> = Vec::new();
    let mut scheduled: Vec<&HomeCard> = Vec::new();
    let mut tickets: Vec<&HomeCard> = Vec::new();
    let mut deferred: Vec<&HomeCard> = Vec::new();
    let mut bug_reports: Vec<&HomeCard> = Vec::new();
    let mut groups: Vec<Vec<&HomeCard>> = AUTOMATIC.iter().map(|_| Vec::new()).collect();
    let mut upkeep_groups = groups.clone();
    let mut sources: BTreeMap<Source, Vec<&HomeCard>> = BTreeMap::new();

    for ticket in cards {
        let group = automatic_group(&ticket.labels);
        if ticket.card.status == Status::InProgress {
            active.push(ticket);
        }
        if ticket.card.status == Status::Deferred {
            deferred.push(ticket);
        } else if ticket.labels.iter().any(|label| label == "bug-report") {
            bug_reports.push(ticket);
        } else if let Some(group) = group.filter(|index| *index < URGENT_GROUPS) {
            groups[group].push(ticket);
        } else if let Some(source) = ticket.source {
            if source == Source::Upkeep
                && let Some(group) = group.filter(|index| AUTOMATIC[*index].1 != [HUB_LABEL])
            {
                upkeep_groups[group].push(ticket);
            } else {
                sources.entry(source).or_default().push(ticket);
            }
        } else if let Some(group) = group {
            groups[group].push(ticket);
        } else if ticket.card.parent.is_none() {
            if ticket.card.ticket_type == "epic" {
                epics.push(ticket);
            } else if ticket.card.ticket_type == "meta" {
                scheduled.push(ticket);
            } else {
                tickets.push(ticket);
            }
        }
    }

    let grid = |items: &[&HomeCard], show_kind: bool| -> String {
        c::grid(items.iter().map(|ticket| {
            let card = &ticket.card;
            let state = status_dot(card.status, true);
            let kind = if show_kind && card.ticket_type == "epic" {
                c::chip("Epic", Tone::Violet, None)
            } else if show_kind && card.ticket_type == "meta" {
                c::chip("Scheduled", Tone::Blue, None)
            } else {
                String::new()
            };
            // A registered worker's machine shows as a tag, so the foot names only a worker
            // no machine has registered.
            let assignee = match (&ticket.machine, card.assignee.as_deref()) {
                (None, Some(name)) => worker_unlinked(Some(name), &card.id),
                _ => String::new(),
            };
            let foot = format!(
                "{state}{kind}{}{assignee}<span class=\"c-card-tags\">{}</span>",
                work_progress(forecasts.get(&card.id), forecasts.default),
                card_tags(ticket, pool)
            );
            c::card(
                &ticket_url(root, &card.id),
                &format!(
                    "<div class=\"c-card-top\">{}<span class=\"c-card-time\">{}</span></div>\
                     <h3 class=\"c-card-title\">{}</h3><div class=\"c-card-foot\">{foot}</div>",
                    c::mono(&display_id(&card.id)),
                    c::time(Some(&card.updated_at), ""),
                    esc(&card.title)
                ),
                None,
            )
        }))
    };

    let allocations = settings.allocations.clone().unwrap_or_default();
    let general = allocations.general.unwrap_or(DEFAULT_GENERAL_ALLOCATION);
    let general_chip = edit::allocation_chip(
        "general",
        "standalone tickets allocation",
        allocations.general,
        DEFAULT_GENERAL_ALLOCATION,
        editable.is_some(),
    );
    let grouped = |(index, group): (usize, &Vec<&HomeCard>), level| {
        c::subsection(
            AUTOMATIC[index].0,
            &grid(group, true),
            Some(group.len()),
            level,
        )
    };
    let upkeep_total = upkeep_groups.iter().map(Vec::len).sum::<usize>();
    let automatic_total = groups.iter().map(Vec::len).sum::<usize>()
        + sources.values().map(Vec::len).sum::<usize>()
        + upkeep_total;
    let automatic_body = groups
        .iter()
        .take(URGENT_GROUPS)
        .enumerate()
        .filter(|(_, group)| !group.is_empty())
        .map(|group| grouped(group, 4))
        .chain(Source::ALL.into_iter().map(|source| {
            let label = if source == Source::Flaky {
                "Flaky checks"
            } else {
                source.label()
            };
            let chip = edit::allocation_chip(
                source.key(),
                &format!("{label} allocation"),
                allocations.sources.get(&source).copied(),
                general,
                editable.is_some(),
            );
            let cards = sources.get(&source).map_or(&[][..], Vec::as_slice);
            let mut body = grid(cards, true);
            let mut count = cards.len();
            if source == Source::Upkeep {
                body.extend(
                    upkeep_groups
                        .iter()
                        .enumerate()
                        .filter(|(_, group)| !group.is_empty())
                        .map(|group| grouped(group, 5)),
                );
                count += upkeep_total;
            }
            c::subsection_with_control(label, &body, Some(count), 4, &chip)
        }))
        .chain(
            groups
                .iter()
                .enumerate()
                .skip(URGENT_GROUPS)
                .filter(|(_, group)| !group.is_empty())
                .map(|group| grouped(group, 4)),
        )
        .collect::<String>();
    let sections = [
        ("Epics", "epics", epics.len(), grid(&epics, false), ""),
        (
            "Standalone tickets",
            "tickets",
            tickets.len(),
            grid(&tickets, true),
            general_chip.as_str(),
        ),
        (
            "Scheduled",
            "scheduled",
            scheduled.len(),
            grid(&scheduled, false),
            "",
        ),
        (
            "Filed automatically",
            "automatic",
            automatic_total,
            automatic_body,
            "",
        ),
        (
            "Bug reports",
            "bug-reports",
            bug_reports.len(),
            grid(&bug_reports, true),
            "",
        ),
        (
            "Deferred",
            "deferred",
            deferred.len(),
            grid(&deferred, true),
            "",
        ),
    ];
    let shown: Vec<_> = sections
        .iter()
        .filter(|(_, anchor, count, _, _)| *count > 0 || matches!(*anchor, "tickets" | "automatic"))
        .collect();
    let total: usize = shown.iter().map(|(_, _, count, _, _)| count).sum();
    let mut entries: Vec<c::TocEntry> = Vec::new();
    let attention_part = if attention_cards.is_empty() {
        String::new()
    } else {
        let count = Some(attention_cards.len());
        entries.push(("Needs a person".into(), "attention".into(), count, 0));
        c::part(
            "Needs a person",
            &c::grid(attention_cards),
            "attention",
            count,
        )
    };
    entries.extend([
        ("Active".into(), "active".into(), Some(active.len()), 0),
        ("All".into(), "all".into(), Some(total), 0),
    ]);
    entries.extend(shown.iter().map(|(heading, anchor, count, _, _)| {
        ((*heading).into(), (*anchor).into(), Some(*count), 1)
    }));
    entries.push(("Project status".into(), "project-status".into(), None, 0));
    let nav = c::toc(&entries);

    let active_body = if active.is_empty() {
        c::empty("Nothing is in progress.")
    } else {
        grid(&active, true)
    };
    let active_part = c::part("Active", &active_body, "active", Some(active.len()));
    let mut all_body: String = shown
        .iter()
        .map(|(heading, anchor, count, body, control)| {
            c::section_with_control(heading, body, Some(*count), Some(anchor), 3, control)
        })
        .collect();
    if total == 0 {
        all_body.push_str(&c::empty("No open tickets."));
    }
    let all_body = match editable {
        Some(project) => edit::fleet_allocation_context(settings, project, &all_body),
        None => all_body,
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
    let name = brand();
    PageOptions {
        title: format!("Tickets · {name}"),
        body: attention::nav(root)
            + &c::title("Tickets", "", "What is in progress, and everything open.")
            + &c::with_toc(&nav, &(attention_part + &active_part + &all_part + &status)),
        feed: Some(Feed {
            seq: seq.to_owned(),
            tickets: None,
        }),
        root: root.to_owned(),
        asset_root: Some("/".to_owned()),
        icons: Some(SITE_ICONS),
        // The page's own title says "Tickets", so the header shows only the brand.
        crumbs: Vec::new(),
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
    editable: Option<&str>,
    pool: Option<&PoolConfig>,
) -> Result<PageOptions, ReadError> {
    let (cards, forecasts) = home_data(conn)?;
    let attention_cards = attention::cards(conn, root, now)?;
    let settings = fleet_settings::settings(conn)?;
    Ok(page(
        root,
        &cards,
        &forecasts,
        attention_cards,
        seq,
        &settings,
        editable,
        pool,
    ))
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;
