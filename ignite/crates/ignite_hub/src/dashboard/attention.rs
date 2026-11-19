//! What every fleet page shares: the page frame, the links between the pages, and the line at
//! the top of the dashboard that says what needs a person.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;

use super::common::{SITE_ICONS, brand};
use super::components::{self as c, Feed, PageOptions, Tone, esc};
use super::data::ReadError;
use super::references::{display_id, ticket_url};

/// Workers whose command ran, or who stayed silent, longer than this are marked.
pub const STUCK_SECONDS: f64 = 600.0;

/// How long a machine may go without a heartbeat before it counts as gone.
pub const MACHINE_SILENT_SECONDS: f64 = 600.0;

/// The fleet pages, as `(path after the project home, label)`.
pub const PAGES: [(&str, &str); 6] = [
    ("machines", "Machines"),
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

/// A machine that stopped reporting, has no dispatcher, or has paused its pool.
fn machine_items(conn: &Connection, now: Timestamp) -> Result<Vec<(Tone, String)>, ReadError> {
    let mut found = Vec::new();
    let mut machines =
        conn.prepare("SELECT id, name, os, last_seen FROM machines ORDER BY name")?;
    let machines = machines
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut dispatchers =
        conn.prepare("SELECT state FROM workers WHERE machine = ?1 AND role = 'dispatcher'")?;
    for (id, name, _, last_seen) in &machines {
        let states = dispatchers
            .query_map([id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let silent = seconds_since(now, Some(last_seen)).is_none_or(|s| s > MACHINE_SILENT_SECONDS);
        if silent {
            found.push((
                Tone::Red,
                format!(
                    "{} has not reported for {}",
                    esc(name),
                    age(now, Some(last_seen))
                ),
            ));
        } else if states.iter().all(|state| state == "down") {
            found.push((Tone::Red, format!("{} has no dispatcher", esc(name))));
        } else if states.iter().any(|state| state == "paused") {
            found.push((Tone::Amber, format!("{}'s pool is paused", esc(name))));
        }
    }

    Ok(found)
}

/// Open regressions that no worker can take: none exists on a machine with a running
/// dispatcher.
fn regression_items(conn: &Connection, root: &str) -> Result<Vec<(Tone, String)>, ReadError> {
    let mut found = Vec::new();
    let takers: i64 = conn.query_row(
        "SELECT COUNT(*) FROM workers w JOIN machines m ON m.id = w.machine
         WHERE w.role = 'worker' AND w.state != 'down'
           AND EXISTS (SELECT 1 FROM workers d WHERE d.machine = w.machine
                       AND d.role = 'dispatcher' AND d.state NOT IN ('down', 'paused'))",
        [],
        |row| row.get(0),
    )?;
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
    if takers == 0 {
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
    }

    Ok(found)
}

/// The live tickets that hold `master` back (the tickets of failing regressions and justiciar
/// tickets), named while any pool is paused. A paused machine still hands them to one worker at
/// a time, but a pause of every machine hands them to nobody, so `master` stays where it is.
fn held_items(conn: &Connection, root: &str) -> Result<Vec<(Tone, String)>, ReadError> {
    let everywhere: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM pauses WHERE machine = ?1)",
        [ignite_store::hub::fleet::ALL_MACHINES],
        |row| row.get(0),
    )?;
    let mut paused = conn.prepare(
        "SELECT m.name FROM machines m
         WHERE EXISTS (SELECT 1 FROM pauses p WHERE p.machine = m.id)
            OR EXISTS (SELECT 1 FROM workers d WHERE d.machine = m.id
                       AND d.role = 'dispatcher' AND d.state = 'paused')
         ORDER BY m.name",
    )?;
    let paused = paused
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if !everywhere && paused.is_empty() {
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
        let names: Vec<String> = paused.iter().map(|name| esc(name)).collect();
        (
            Tone::Amber,
            format!(
                "master is held back while {} paused; a paused pool still hands \
                 these to one worker at a time",
                if names.len() == 1 {
                    format!("{}'s pool is", names[0])
                } else {
                    format!("the pools of {} are", names.join(", "))
                }
            ),
        )
    };
    Ok(vec![(tone, format!("{text}: {}", links.join(", ")))])
}

/// Live tickets that no worker can take, because their owner names a machine or app that
/// `ignite.toml` does not declare (see `upkeep::machines::unassignable`).
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
    found.extend(machine_items(conn, now)?);
    found.extend(regression_items(conn, root)?);
    found.extend(held_items(conn, root)?);
    found.extend(stranded_items(conn, root)?);
    Ok(found)
}

/// The first line of the dashboard: each item that needs a person, or a note that none does.
pub fn line(conn: &Connection, root: &str, now: Timestamp) -> Result<String, ReadError> {
    let items = items(conn, root, now)?;
    let body = if items.is_empty() {
        c::empty("Nothing needs a person.")
    } else {
        c::rows(
            items
                .iter()
                .map(|(tone, text)| c::row(&(c::dot(*tone, "", None) + text), "", None)),
        )
    };
    Ok(c::section(
        "Needs a person",
        &body,
        Some(items.len()),
        Some("attention"),
        3,
    ) + &nav(root))
}
