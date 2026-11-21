//! What every fleet page shares: the page frame, the links between the pages, and the line at
//! the top of the dashboard that says what needs a person.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use rusqlite::Connection;

use ignite_core::fleet::{FleetSettings, MachineState};
use ignite_core::time::Timestamp;
use ignite_store::hub::{fleet, fleet_settings, tip_coverage};

use super::common::{SITE_ICONS, brand};
use super::components::{self as c, Feed, PageOptions, Tone, esc};
use super::data::ReadError;
use super::references::{display_id, ticket_url};

/// Workers whose command ran, or who stayed silent, longer than this are marked.
pub const STUCK_SECONDS: f64 = 600.0;

/// How long a machine may go without a heartbeat before it counts as gone.
pub const MACHINE_SILENT_SECONDS: f64 = 600.0;

/// The fleet pages, as `(path after the project home, label)`.
pub const PAGES: [(&str, &str); 7] = [
    ("available", "Available"),
    ("fleet", "Fleet"),
    ("pool", "Pool"),
    ("epics", "Epics"),
    ("landings", "Landings"),
    ("tips", "Tips"),
    ("finds", "Finds"),
];

/// Links to the ticket home and every fleet page.
pub fn nav(root: &str) -> String {
    let mut links = vec![c::link(root, "Tickets", None, "")];
    links.extend(
        PAGES
            .iter()
            .map(|(path, label)| c::link(&format!("{root}{path}"), label, None, "")),
    );
    format!("<p class=\"c-muted\">{}</p>", links.join(" · "))
}

/// One fleet page: the title, the links to its siblings, then `body`.
pub fn frame(root: &str, seq: &str, path: &str, sub: &str, body: &str) -> PageOptions {
    let label = PAGES
        .iter()
        .find(|(candidate, _)| *candidate == path)
        .map_or(path, |(_, label)| *label);
    let name = brand();
    PageOptions {
        title: format!("{label} · {name}"),
        body: c::title(label, "", sub) + &nav(root) + body,
        feed: Some(Feed {
            seq: seq.to_owned(),
            tickets: None,
        }),
        root: root.to_owned(),
        asset_root: Some("/".to_owned()),
        icons: Some(SITE_ICONS),
        crumbs: vec![
            ("Tickets".to_owned(), Some(root.to_owned())),
            (label.to_owned(), None),
        ],
        brand: name,
        ..PageOptions::default()
    }
}

/// How long ago `iso` was, as "9 min", or "unknown".
pub fn age(now: Timestamp, iso: Option<&str>) -> String {
    let Some(then) = iso.and_then(|iso| Timestamp::parse(iso).ok()) else {
        return "unknown".to_owned();
    };
    let seconds = now.seconds_since(then).max(0.0);
    if seconds < 90.0 {
        format!("{} s", seconds.round())
    } else if seconds < 5400.0 {
        format!("{} min", (seconds / 60.0).round())
    } else if seconds < 129_600.0 {
        format!("{} h", (seconds / 3600.0).round())
    } else {
        format!("{} d", (seconds / 86_400.0).round())
    }
}

/// Seconds from `iso` to `now`, or `None` if `iso` is missing or unreadable.
pub fn seconds_since(now: Timestamp, iso: Option<&str>) -> Option<f64> {
    let then = Timestamp::parse(iso?).ok()?;
    Some(now.seconds_since(then))
}

/// Tickets marked `needs:person`, with their notes.
fn person_items(conn: &Connection, root: &str) -> Result<Vec<(Tone, String)>, ReadError> {
    let mut found = Vec::new();
    let mut person = conn.prepare(
        "SELECT id, title, notes FROM tickets
         WHERE status != 'closed' AND EXISTS
           (SELECT 1 FROM json_each(tickets.labels) WHERE value = 'needs:person')
         ORDER BY id",
    )?;
    let waiting = person
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (id, title, notes) in waiting {
        let mut text = format!(
            "{} needs a person: {}",
            c::link(&ticket_url(root, &id), &esc(&display_id(&id)), None, ""),
            esc(&title)
        );
        if !notes.trim().is_empty() {
            text += &c::muted(&format!(" — {}", esc(notes.trim())));
        }
        found.push((Tone::Red, text));
    }

    Ok(found)
}

pub(super) fn machine_state(
    conn: &Connection,
    name: &str,
    state: MachineState,
) -> Result<String, ReadError> {
    let actor = fleet_settings::state_actor(conn, name)?;
    Ok(actor.map_or_else(
        || state.to_string(),
        |actor| format!("{state} (set by {})", esc(&actor)),
    ))
}

/// Machines in a restricted state, or with a fault such as a missing heartbeat.
fn machine_items(
    conn: &Connection,
    settings: &FleetSettings,
    now: Timestamp,
) -> Result<Vec<(Tone, String)>, ReadError> {
    let mut found = Vec::new();
    let mut machines = conn.prepare(
        "SELECT name, last_seen
         FROM machines ORDER BY name",
    )?;
    let machines = machines
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (name, last_seen) in machines {
        let state = settings.machine(&name).state;
        if state != MachineState::Active {
            let label = machine_state(conn, &name, state)?;
            found.push((Tone::Amber, format!("{} is {label}", esc(&name))));
        }
        let silent =
            seconds_since(now, Some(&last_seen)).is_none_or(|s| s > MACHINE_SILENT_SECONDS);
        if silent {
            found.push((
                Tone::Red,
                format!(
                    "{} has not reported for {}",
                    esc(&name),
                    age(now, Some(&last_seen))
                ),
            ));
        }
    }

    Ok(found)
}

/// The provider windows the hub is holding on every machine.
fn usage_items(conn: &Connection, now: Timestamp) -> Result<Vec<(Tone, String)>, ReadError> {
    use ignite_store::hub::{fleet_settings, provider_usage};
    let settings = fleet_settings::settings(conn)?;
    let holds = provider_usage::held(conn, &settings, now)?;
    Ok(holds
        .into_iter()
        .flat_map(|(provider, holds)| {
            holds.into_iter().map(move |hold| {
                let text = format!("{provider} is on a usage hold ({})", hold.describe());
                (Tone::Amber, esc(&text))
            })
        })
        .collect())
}

/// Open regressions when no reporting machine can start a CLI worker.
fn regression_items(
    conn: &Connection,
    settings: &FleetSettings,
    root: &str,
    now: Timestamp,
) -> Result<Vec<(Tone, String)>, ReadError> {
    if !fleet::paused_everywhere(conn)? {
        let mut candidates = conn.prepare(
            "SELECT name, last_seen FROM machines WHERE json_array_length(cli_apps) > 0",
        )?;
        let candidates = candidates
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if candidates.iter().any(|(name, last_seen)| {
            settings.machine(name).state != MachineState::Off
                && seconds_since(now, Some(last_seen))
                    .is_some_and(|age| age <= MACHINE_SILENT_SECONDS)
        }) {
            return Ok(Vec::new());
        }
    }
    let mut found = Vec::new();
    let mut regressions = conn.prepare(
        "SELECT r.check_name, r.ticket FROM regressions r JOIN tickets t ON t.id = r.ticket
         WHERE r.state IN ('open', 'reopened') AND t.status != 'in_progress'
         ORDER BY r.check_name",
    )?;
    let regressions = regressions
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (check, ticket) in regressions {
        found.push((
            Tone::Red,
            format!(
                "The regression in {} ({}) has no worker that can take it",
                esc(&check),
                c::link(
                    &ticket_url(root, &ticket),
                    &esc(&display_id(&ticket)),
                    None,
                    ""
                )
            ),
        ));
    }

    Ok(found)
}

/// Tickets holding `master` back, shown when a machine's fleet state limits its work
/// or hand-offs are paused across the fleet.
fn held_items(
    conn: &Connection,
    settings: &FleetSettings,
    root: &str,
) -> Result<Vec<(Tone, String)>, ReadError> {
    let everywhere = fleet::paused_everywhere(conn)?;
    let limited = settings
        .machines
        .values()
        .any(|setting| setting.state != MachineState::Active);
    if !everywhere && !limited {
        return Ok(Vec::new());
    }
    let mut holding = conn.prepare(
        "SELECT id FROM tickets t WHERE status != 'closed'
           AND (id IN (SELECT ticket FROM regressions WHERE state IN ('open', ?1))
                OR EXISTS (SELECT 1 FROM json_each(t.labels) WHERE value = 'justiciar'))
         ORDER BY id",
    )?;
    let holding = holding
        .query_map([ignite_store::hub::tips::REOPENED], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if holding.is_empty() {
        return Ok(Vec::new());
    }
    let links: Vec<String> = holding
        .iter()
        .map(|id| c::link(&ticket_url(root, id), &esc(&display_id(id)), None, ""))
        .collect();
    let (tone, text) = if everywhere {
        (
            Tone::Red,
            "Hand-offs are paused on every machine, so nobody takes the tickets holding \
             master back"
                .to_owned(),
        )
    } else {
        (Tone::Amber, "Tickets holding master back".to_owned())
    };
    Ok(vec![(tone, format!("{text}: {}", links.join(", ")))])
}

/// Live tickets pinned to an unknown machine or provider.
fn stranded_items(conn: &Connection, root: &str) -> Result<Vec<(Tone, String)>, ReadError> {
    let link = |id: &str| c::link(&ticket_url(root, id), &esc(&display_id(id)), None, "");
    Ok(crate::upkeep::machines::unassignable(conn)?
        .into_iter()
        .map(|(id, reason)| {
            (
                Tone::Orange,
                format!("No machine can run {}: {}", link(&id), esc(&reason)),
            )
        })
        .collect())
}

/// The things that need a person, each with the tone it is shown in.
pub fn items(
    conn: &Connection,
    root: &str,
    now: Timestamp,
) -> Result<Vec<(Tone, String)>, ReadError> {
    let mut found = person_items(conn, root)?;
    let settings = fleet_settings::settings(conn)?;
    found.extend(
        tip_coverage::get(conn)?
            .warnings()
            .map(|line| (Tone::Amber, esc(&line))),
    );
    found.extend(machine_items(conn, &settings, now)?);
    found.extend(usage_items(conn, now)?);
    found.extend(regression_items(conn, &settings, root, now)?);
    found.extend(held_items(conn, &settings, root)?);
    found.extend(stranded_items(conn, root)?);
    Ok(found)
}

/// The dashboard navigation, preceded by any items that need a person.
pub fn line(conn: &Connection, root: &str, now: Timestamp) -> Result<String, ReadError> {
    let items = items(conn, root, now)?;
    if items.is_empty() {
        return Ok(nav(root));
    }
    let body = c::rows(
        items
            .iter()
            .map(|(tone, text)| c::row(&(c::dot(*tone, "", None) + text), "", None)),
    );
    Ok(c::section(
        "Needs a person",
        &body,
        Some(items.len()),
        Some("attention"),
        3,
    ) + &nav(root))
}
