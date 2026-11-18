//! The `landings` page: who holds the `staging` lease and who waits, and the last landings
//! with how many of their gate's checks were answered from the cache and how many ran.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;
use ignite_store::hub::leases;

use super::attention::{age, frame};
use super::components::{self as c, PageOptions, Tone, esc};
use super::data::ReadError;
use super::references::{display_id, ticket_url};

/// How many landings the page lists.
const SHOWN: usize = 25;

/// The `landings` page.
pub(crate) fn options(
    conn: &Connection,
    root: &str,
    seq: &str,
    now: Timestamp,
) -> Result<PageOptions, ReadError> {
    let held = leases::list(conn, now)?;
    let staging = held
        .iter()
        .find(|lease| lease.name == "staging" && lease.expires_at > now);
    let queue = match staging {
        None => c::empty("No landing is in progress."),
        Some(lease) => {
            let mut rows = vec![c::row(
                &(c::dot(Tone::Green, "Landing", None)
                    + " "
                    + &esc(&lease.holder)
                    + &c::muted(" · ")
                    + &esc(&lease.machine)),
                &format!("until {}", c::time(Some(&lease.expires_at.to_string()), "")),
                None,
            )];
            rows.extend(lease.waiters.iter().map(|waiter| {
                c::row(
                    &(c::dot(Tone::Muted, "Waiting", None) + " " + &esc(&waiter.holder)),
                    "",
                    None,
                )
            }));
            c::rows(rows)
        }
    };
    let mut statement = conn.prepare(
        "SELECT id, title, landed_at, landed, claimed_at FROM tickets
         WHERE landed_at IS NOT NULL ORDER BY landed_at DESC, id LIMIT ?1",
    )?;
    let landed = statement
        .query_map([SHOWN as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut gate = conn.prepare(
        "SELECT COALESCE(SUM(cpu_s = 0), 0), COALESCE(SUM(cpu_s > 0), 0) FROM records
         WHERE ticket = ?1 AND verb IN ('test', 'check', 'land') AND at >= COALESCE(?2, '')",
    )?;
    let mut rows = Vec::new();
    for (id, title, landed_at, work, claimed_at) in &landed {
        let (cached, ran): (i64, i64) = gate
            .query_row(rusqlite::params![id, claimed_at], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        let commit = work
            .as_deref()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
            .and_then(|value| {
                value["commits"][0]
                    .as_str()
                    .map(|sha| sha.chars().take(9).collect::<String>())
            })
            .unwrap_or_default();
        let label = c::link(
            &ticket_url(root, id),
            &esc(&format!("{} {title}", display_id(id))),
            None,
            "",
        );
        rows.push(c::row(
            &(label + &c::muted(" · ") + &c::mono(&commit)),
            &format!(
                "{cached} cached, {ran} run · {} ago",
                esc(&age(now, Some(landed_at)))
            ),
            None,
        ));
    }
    let last = if rows.is_empty() {
        c::empty("Nothing has landed.")
    } else {
        c::rows(rows)
    };
    let body = c::section("Queue", &queue, None, None, 3)
        + &c::section("Last landings", &last, Some(landed.len()), None, 3);
    Ok(frame(
        root,
        seq,
        "landings",
        "The landing queue and what each gate ran.",
        &body,
    ))
}
