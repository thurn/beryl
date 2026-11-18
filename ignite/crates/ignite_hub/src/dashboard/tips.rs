//! The `tips` page: the newest `staging` commit, the last verified one, where the last rounds'
//! time went, the batches each machine holds, open regressions and the checks switched off as
//! flaky.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;
use ignite_store::hub::tip_rounds;

use super::attention::{age, frame};
use super::components::{self as c, PageOptions, Tone, esc};
use super::data::ReadError;
use super::references::{display_id, ticket_url};

fn short(commit: &str) -> String {
    commit.chars().take(9).collect()
}

fn tip_tone(state: &str) -> Tone {
    match state {
        "verified" => Tone::Green,
        "failed" => Tone::Red,
        "verifying" => Tone::Blue,
        _ => Tone::Muted,
    }
}

/// The batches machines hold, and how many there are.
fn claims_section(conn: &Connection, now: Timestamp) -> Result<(String, usize), ReadError> {
    let mut claims = conn.prepare(
        "SELECT c.machine, COALESCE(m.name, c.machine), c.commit_sha, c.family, c.batch, c.expires_at
         FROM tip_claims c LEFT JOIN machines m ON m.id = c.machine
         WHERE c.expires_at > ?1 ORDER BY 2, c.family, c.batch",
    )?;
    let claims = claims
        .query_map([now.to_string()], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let running = if claims.is_empty() {
        c::empty("No machine holds a batch.")
    } else {
        c::rows(claims.iter().map(|(machine, commit, family, batch)| {
            c::row(
                &(esc(machine)
                    + &c::muted(" · ")
                    + &esc(family)
                    + &c::muted(&format!(" batch {batch}"))),
                &c::mono(&short(commit)),
                None,
            )
        }))
    };

    Ok((running, claims.len()))
}

/// How many recent rounds the page shows.
const ROUNDS_SHOWN: u32 = 5;

/// The newest rounds with where their time went, and how many are shown.
fn rounds_section(conn: &Connection, now: Timestamp) -> Result<(String, usize), ReadError> {
    let rounds = tip_rounds::recent(conn, ROUNDS_SHOWN)?;
    let shown = if rounds.is_empty() {
        c::empty("No round has been recorded.")
    } else {
        c::rows(rounds.iter().map(|round| {
            let (tone, label) = if round.overhead().is_some() {
                (Tone::Red, "slow")
            } else {
                (Tone::Green, "ok")
            };
            let at = round.at.to_string();
            c::row(
                &(c::dot(tone, label, None)
                    + " "
                    + &c::mono(&short(round.commit.as_str()))
                    + &c::muted(" · ")
                    + &esc(&round.summary())),
                &format!("{} ago", esc(&age(now, Some(&at)))),
                None,
            )
        }))
    };
    Ok((shown, rounds.len()))
}

/// The open regressions, and how many there are.
fn regressions_section(conn: &Connection, root: &str) -> Result<(String, usize), ReadError> {
    let mut regressions = conn.prepare(
        "SELECT check_name, ticket, first_bad, last_good, state FROM regressions
         WHERE state != 'fixed' ORDER BY check_name",
    )?;
    let regressions = regressions
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let open = if regressions.is_empty() {
        c::empty("No regression is open.")
    } else {
        c::rows(regressions.iter().map(|(check, ticket, bad, good, state)| {
            c::row(
                &(c::dot(Tone::Red, state, None)
                    + " "
                    + &esc(check)
                    + &c::muted(" · ")
                    + &c::link(
                        &ticket_url(root, ticket),
                        &esc(&display_id(ticket)),
                        None,
                        "",
                    )),
                &format!("good {}, bad {}", esc(&short(good)), esc(&short(bad))),
                None,
            )
        }))
    };

    Ok((open, regressions.len()))
}

/// The checks switched off as flaky, and how many there are.
fn flaky_section(conn: &Connection) -> Result<(String, usize), ReadError> {
    let mut flaky =
        conn.prepare("SELECT name, family FROM checks WHERE state = 'off' ORDER BY name")?;
    let flaky = flaky
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let off = if flaky.is_empty() {
        c::empty("No check is switched off.")
    } else {
        c::rows(
            flaky
                .iter()
                .map(|(name, family)| c::row(&esc(name), &esc(family), None)),
        )
    };

    Ok((off, flaky.len()))
}

/// The `tips` page.
pub(crate) fn options(
    conn: &Connection,
    root: &str,
    seq: &str,
    now: Timestamp,
) -> Result<PageOptions, ReadError> {
    let mut statement =
        conn.prepare("SELECT commit_sha, seen, verified, state FROM tips ORDER BY seen DESC, commit_sha LIMIT 10")?;
    let tips = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let last_verified: Option<(String, String)> = conn
        .query_row(
            "SELECT commit_sha, verified FROM tips WHERE state = 'verified' ORDER BY verified DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();
    let summary = {
        let tip = tips
            .first()
            .map_or_else(|| c::muted("none yet"), |tip| c::mono(&short(&tip.0)));
        let verified = last_verified.as_ref().map_or_else(
            || c::muted("none yet"),
            |(commit, at)| {
                c::mono(&short(commit)) + &c::muted(&format!(" {} ago", esc(&age(now, Some(at)))))
            },
        );
        c::meta(&[("Tip".to_owned(), tip), ("Verified".to_owned(), verified)])
    };
    let recent = if tips.is_empty() {
        c::empty("No tip has been seen.")
    } else {
        c::rows(tips.iter().map(|(commit, seen, _, state)| {
            c::row(
                &(c::dot(tip_tone(state), state, None) + " " + &c::mono(&short(commit))),
                &format!("seen {} ago", esc(&age(now, Some(seen)))),
                None,
            )
        }))
    };

    let (rounds, round_count) = rounds_section(conn, now)?;
    let (running, held) = claims_section(conn, now)?;
    let (open, regression_count) = regressions_section(conn, root)?;
    let (off, flaky_count) = flaky_section(conn)?;
    let body = summary
        + &c::section("Recent tips", &recent, Some(tips.len()), None, 3)
        + &c::section("Recent rounds", &rounds, Some(round_count), None, 3)
        + &c::section("Batches machines hold", &running, Some(held), None, 3)
        + &c::section("Open regressions", &open, Some(regression_count), None, 3)
        + &c::section("Switched off as flaky", &off, Some(flaky_count), None, 3);
    Ok(frame(
        root,
        seq,
        "tips",
        "Tip verification, regressions and flaky checks.",
        &body,
    ))
}
