//! The `available` page: the tickets workers hold, then every ready ticket in the order the pool
//! will offer them, each with a chip saying where it goes next or why it waits, then the ready
//! tickets the pool is never offered. A waiting ticket's chip opens a card with what each
//! machine makes of it, such as no idle worker, no free ticket tree or too few cores; the
//! reasons come from `crate::routes::dispatch::view`, which follows `POST /next`.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use std::collections::HashMap;

use rusqlite::Connection;

use ignite_core::dispatch::Stage;
use ignite_core::time::Timestamp;

use super::attention::{age, frame};
use super::common::{status_dot, ticket_heading};
use super::components::{self as c, PageOptions, Tone, esc};
use super::data::{Card, ReadError, cards_by_id};
use super::references::{display_id, ticket_url};
use crate::routes::dispatch::view::{self, Available};

/// The page's path under the dashboard's root, and its label in the links between pages.
pub const PATH: &str = "available";

fn stage_chip(stage: Stage) -> String {
    match stage {
        Stage::Justiciar => c::chip(
            "Justiciar",
            Tone::Red,
            Some("A fault in Ignite itself, offered before everything else."),
        ),
        Stage::Regression => c::chip(
            "Regression",
            Tone::Red,
            Some("A check that tip verification found failing; it holds master back."),
        ),
        Stage::Resume => c::chip(
            "Resuming",
            Tone::Blue,
            Some(
                "Offered ahead of new work to finish and land a saved branch or reverted landing.",
            ),
        ),
        Stage::Improvement => c::chip(
            "Improvement",
            Tone::Blue,
            Some("Offered ahead of the epics because improvements are under their share."),
        ),
        Stage::Group => String::new(),
    }
}

/// A card that is not itself a link: its title links to the ticket, so the chips in its foot
/// can open their own cards on a tap.
fn card(root: &str, card: Option<&Card>, id: &str, top: &str, foot: &str) -> String {
    let (heading, title) = card.map_or_else(
        || (c::mono(&display_id(id)), String::new()),
        |card| (ticket_heading(card), esc(&card.title)),
    );
    format!(
        "<div class=\"c-card c-card-static\">{heading}<h3 class=\"c-card-title\">{}</h3>\
         <div class=\"c-card-foot\">{top}{foot}</div></div>",
        c::link(&ticket_url(root, id), &title, None, "")
    )
}

/// A chip that opens the card in the template `template` on hover, focus or tap.
fn disclosure_chip(text: &str, tone: Tone, template: &str) -> String {
    format!(
        "<span class=\"c-chip c-tone-{}\" tabindex=\"0\" role=\"button\" \
         data-hovercard=\"{}\">{}</span>",
        tone.name(),
        esc(template),
        esc(text)
    )
}

/// Builds the page from `available` at `now`.
#[allow(clippy::too_many_lines)]
pub(crate) fn page(
    root: &str,
    seq: &str,
    available: &Available,
    cards: &HashMap<String, Card>,
    now: Timestamp,
) -> PageOptions {
    let mut templates = String::new();

    let machines = if available.machines.is_empty() {
        c::empty("No machine is reporting.")
    } else {
        c::rows(available.machines.iter().map(|machine| {
            let asked = machine
                .asked
                .as_ref()
                .map_or_else(String::new, |(at, answer)| {
                    c::tip(
                        &c::muted(&format!(
                            "asked {} ago",
                            esc(&age(now, Some(&at.to_string())))
                        )),
                        &format!("Answer: {answer}"),
                    )
                });
            let build_space = machine.build_space.map_or_else(
                || "Build slots unreported".to_owned(),
                |space| space.to_string(),
            );
            c::row(
                &(format!("<strong>{}</strong> ", esc(&machine.name))
                    + &c::muted(&esc(&format!("{} · {build_space}", machine.summary)))),
                &asked,
                None,
            )
        }))
    };

    let running = if available.running.is_empty() {
        c::empty("Nothing is in progress.")
    } else {
        c::grid(available.running.iter().map(|running| {
            let id = running.id.as_str();
            let found = cards.get(id);
            let state = match &running.reserved_for {
                Some(machine) => c::chip(
                    &format!("Reserved for {machine}"),
                    Tone::Blue,
                    Some("Handed to a worker that has not started it yet."),
                ),
                None => found.map_or_else(String::new, |card| status_dot(card.status, true)),
            };
            card(root, found, id, &state, "")
        }))
    };

    let waiting = if available.waiting.is_empty() {
        c::empty("No ready ticket is offered.")
    } else {
        c::grid(
            available
                .waiting
                .iter()
                .enumerate()
                .map(|(index, waiting)| {
                    let id = waiting.id.as_str();
                    let key = format!("why-{id}");
                    let lines: Vec<(String, String)> = waiting
                        .machines
                        .iter()
                        .map(|(machine, said)| (machine.clone(), esc(said)))
                        .collect();
                    let detail = if lines.is_empty() {
                        c::muted("No machine is reporting what it has free.")
                    } else {
                        c::meta(&lines)
                    };
                    templates += &c::template(
                        &key,
                        &format!(
                            "<div class=\"c-hovercard-title\">{}</div><p>{}</p>{detail}",
                            if waiting.next_on.is_some() {
                                "Where it goes next"
                            } else {
                                "Why it is not running"
                            },
                            esc(&waiting.summary)
                        ),
                    );
                    let tone = if waiting.next_on.is_some() {
                        Tone::Green
                    } else {
                        Tone::Amber
                    };
                    let place = c::tip(
                        &c::mono(&format!("#{}", index + 1)),
                        "Its place in the order the pool offers tickets",
                    );
                    let foot = format!(
                        "{place}{}{}",
                        stage_chip(waiting.stage),
                        summary_chips(&waiting.summary, tone, &key)
                    );
                    card(root, cards.get(id), id, "", &foot)
                }),
        )
    };

    let held = if available.held.is_empty() {
        c::empty("Every ready ticket is offered.")
    } else {
        c::grid(available.held.iter().map(|held| {
            let id = held.id.as_str();
            let reason = c::chip(&capitalized(&held.reason), Tone::Muted, None);
            card(root, cards.get(id), id, "", &reason)
        }))
    };

    let body = c::section(
        "Machines",
        &machines,
        Some(available.machines.len()),
        None,
        3,
    ) + &c::section(
        "Running",
        &running,
        Some(available.running.len()),
        Some("running"),
        3,
    ) + &c::section(
        "Waiting",
        &waiting,
        Some(available.waiting.len()),
        Some("waiting"),
        3,
    ) + &c::section(
        "Not offered",
        &held,
        Some(available.held.len()),
        Some("not-offered"),
        3,
    ) + &templates;
    frame(
        root,
        seq,
        PATH,
        "What workers hold, then every ready ticket in the order the pool offers them.",
        &body,
    )
}

/// Keep each wait visible on the card and leave its measurements in the hovercard.
fn summary_chips(text: &str, tone: Tone, template: &str) -> String {
    if !text.contains(" · ") {
        return disclosure_chip(first_clause(text), tone, template);
    }
    text.split(" · ")
        .map(|wait| {
            let label = wait
                .split_once(": ")
                .map_or_else(|| first_clause(wait), |(title, _)| title);
            disclosure_chip(&capitalized(label), tone, template)
        })
        .collect()
}

/// The part before the first comma or parenthesis.
fn first_clause(text: &str) -> &str {
    let end = [", ", " ("]
        .iter()
        .filter_map(|mark| text.find(mark))
        .min()
        .unwrap_or(text.len());
    &text[..end]
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// The page for the router.
pub(crate) fn options(
    conn: &Connection,
    root: &str,
    seq: &str,
    now: Timestamp,
) -> Result<PageOptions, ReadError> {
    let available = view::available(conn, now).map_err(|error| ReadError(error.to_string()))?;
    let ids: Vec<&str> = available
        .running
        .iter()
        .map(|r| r.id.as_str())
        .chain(available.waiting.iter().map(|w| w.id.as_str()))
        .chain(available.held.iter().map(|h| h.id.as_str()))
        .collect();
    let cards = cards_by_id(conn, &ids)?;
    Ok(page(root, seq, &available, &cards, now))
}
