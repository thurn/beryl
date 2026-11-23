//! Whether regression tickets that hold the verified branch back still have a fix in progress.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};
use serde_json::Value;

use ignite_core::protocol::tickets::CULPRIT_HANDOFF_KEY;
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{tickets, tips};

use super::{parse_time, regressions};

/// Thresholds calibrated against the hub's claim, activity and landing history.
pub(super) const UNCLAIMED_SECONDS: f64 = 10.0 * 60.0;
pub(super) const INACTIVE_SECONDS: f64 = 50.0 * 60.0;
pub(super) const FAILED_FIXES: u32 = 2;
pub(super) const FALLBACK_AGE_SECONDS: f64 = 20.0 * 3600.0;

/// Use the 90th percentile of recent regression lifetimes from the culprit-resume workflow
/// once there is a sample big enough for two observations in its upper tenth.
const MIN_AGE_SAMPLE: usize = 20;
const MAX_AGE_SAMPLE: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stall {
    Unclaimed(Timestamp),
    Inactive(Timestamp),
    FailedFixes(u32),
    Old(Timestamp),
}

impl Stall {
    pub(super) fn reason(self) -> String {
        match self {
            Self::Unclaimed(since) => format!("It has been open without a claim since {since}."),
            Self::Inactive(since) => {
                format!("Its holder last recorded activity on the ticket at {since}.")
            }
            Self::FailedFixes(count) => format!(
                "{count} separate fixes landed, and tip verification still failed the same check \
                 after each one."
            ),
            Self::Old(since) => format!(
                "It has been open since {since}, longer than the historical 90th-percentile \
                 regression lifetime."
            ),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Progress {
    filed: Timestamp,
    unclaimed_since: Option<Timestamp>,
    last_activity: Option<Timestamp>,
    culprit_resumed: bool,
    failed_fixes: u32,
}

impl Progress {
    fn stall(self, now: Timestamp, age_seconds: f64) -> Option<Stall> {
        if self.culprit_resumed {
            return None;
        }
        if self.failed_fixes >= FAILED_FIXES {
            return Some(Stall::FailedFixes(self.failed_fixes));
        }
        if let Some(since) = self.unclaimed_since
            && now.seconds_since(since) >= UNCLAIMED_SECONDS
        {
            return Some(Stall::Unclaimed(since));
        }
        if let Some(since) = self.last_activity
            && now.seconds_since(since) >= INACTIVE_SECONDS
        {
            return Some(Stall::Inactive(since));
        }
        (now.seconds_since(self.filed) >= age_seconds).then_some(Stall::Old(self.filed))
    }
}

struct Regression {
    id: String,
    filed: Timestamp,
    ready: Timestamp,
    claimed: Option<Timestamp>,
    session: Option<String>,
    culprit_resumed: bool,
    status: String,
}

pub(super) fn open(conn: &Connection, now: Timestamp) -> Result<BTreeMap<String, Option<Stall>>> {
    observe(conn, now, None, passed)
}

/// A repair can proceed normally if the regression is waiting for a claim or has resumed
/// progress. Failed fixes, an inactive holder and an unusually old episode need attention.
pub(super) fn can_prioritize_repair(
    conn: &Connection,
    ticket: &str,
    now: Timestamp,
) -> Result<bool> {
    let stalls = observe(conn, now, Some(ticket), passed)?;
    Ok(matches!(
        stalls.get(ticket),
        Some(None | Some(Stall::Unclaimed(_)))
    ))
}

fn passed(conn: &Connection, id: &str) -> Result<bool> {
    let Some(ticket) = tickets::load(conn, id)? else {
        return Ok(true);
    };
    Ok(regressions::passing(conn, &ticket)?.is_some())
}

#[cfg(test)]
pub(super) fn open_with(
    conn: &Connection,
    now: Timestamp,
    passed: impl Fn(&Connection, &str) -> Result<bool>,
) -> Result<BTreeMap<String, Option<Stall>>> {
    observe(conn, now, None, passed)
}

fn observe(
    conn: &Connection,
    now: Timestamp,
    ticket: Option<&str>,
    passed: impl Fn(&Connection, &str) -> Result<bool>,
) -> Result<BTreeMap<String, Option<Stall>>> {
    let mut select = conn.prepare(
        "WITH open_tickets AS (
             SELECT DISTINCT r.ticket
             FROM regressions r JOIN tickets t ON t.id = r.ticket
             WHERE r.state != ?1 AND t.status != 'closed' AND (?4 IS NULL OR t.id = ?4)
         )
         SELECT t.id, t.created_at, t.status, coalesce(t.session, t.assignee), t.claimed_at,
                (SELECT max(h.at) FROM history h WHERE h.ticket = t.id
                 AND json_extract(h.change, '$.status[1]') = 'open') AS ready,
                (SELECT max(h.at) FROM history h WHERE h.ticket = t.id
                 AND json_extract(h.change, '$.status[0]') = 'closed'
                 AND json_extract(h.change, '$.status[1]') = 'open') AS reopened,
                json_extract(t.metadata, ?2) AS added,
                json_extract(t.metadata, ?3) AS handoff
         FROM open_tickets o JOIN tickets t ON t.id = o.ticket",
    )?;
    let added_key = format!("$.{}", regressions::ADDED_KEY);
    let handoff_key = format!("$.{CULPRIT_HANDOFF_KEY}");
    let mut rows = select.query(params![tips::FIXED, added_key, handoff_key, ticket])?;
    let mut regressions = Vec::new();
    let last_pass_seen: Option<String> = conn.query_row(
        "SELECT max(seen) FROM tips WHERE state = 'verified' AND verified <= ?1",
        [now.to_string()],
        |row| row.get(0),
    )?;
    let last_pass_seen = optional_time(last_pass_seen.as_deref())?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let filed = parse_time(&row.get::<_, String>(1)?)?;
        let reopened = optional_time(row.get::<_, Option<String>>(6)?.as_deref())?;
        let added = optional_time(row.get::<_, Option<String>>(7)?.as_deref())?;
        let latest_check = reopened
            .into_iter()
            .chain(added)
            .fold(filed, Timestamp::max);
        // A passing tip that includes the regression clears its fault even while the
        // regression ticket is still open for its worker or waiting for automatic closure.
        if last_pass_seen.is_some_and(|seen| seen > latest_check) && passed(conn, &id)? {
            continue;
        }
        // A regression can reuse its old ticket after a passing tip. The new failure gets
        // its own lifetime, while a reopen after an unsuccessful fix keeps the original one.
        let episode = if last_pass_seen.is_some_and(|seen| seen >= filed) {
            latest_check
        } else {
            filed
        };
        let session: Option<String> = row.get(3)?;
        let handoff: Option<String> = row.get(8)?;
        let status: String = row.get(2)?;
        regressions.push(Regression {
            id,
            filed: episode,
            ready: optional_time(row.get::<_, Option<String>>(5)?.as_deref())?.unwrap_or(filed),
            claimed: optional_time(row.get::<_, Option<String>>(4)?.as_deref())?,
            culprit_resumed: status == "in_progress"
                && resumed(handoff.as_deref(), session.as_deref()),
            session,
            status,
        });
    }
    if regressions.is_empty() {
        return Ok(BTreeMap::new());
    }
    let age_seconds = age_threshold(conn)?;
    regressions
        .into_iter()
        .map(|regression| {
            let progress = regression.progress(conn, now)?;
            Ok((regression.id, progress.stall(now, age_seconds)))
        })
        .collect()
}

fn optional_time(time: Option<&str>) -> Result<Option<Timestamp>> {
    time.map(parse_time).transpose()
}

/// Earlier daemons recorded the hand-off's note and session before adding a structured flag.
fn resumed(handoff: Option<&str>, holder: Option<&str>) -> bool {
    let Some(holder) = holder else {
        return false;
    };
    let Some(handoff) = handoff.and_then(|data| serde_json::from_str::<Value>(data).ok()) else {
        return false;
    };
    handoff["session"].as_str() == Some(holder)
        && handoff["resumed"].as_bool().unwrap_or_else(|| {
            handoff["note"]
                .as_str()
                .is_some_and(|note| note.starts_with("Resumed the culprit's transcript from "))
        })
}

impl Regression {
    fn progress(&self, conn: &Connection, now: Timestamp) -> Result<Progress> {
        let mut progress = Progress {
            filed: self.filed,
            unclaimed_since: (self.status == "open").then_some(self.ready),
            last_activity: None,
            culprit_resumed: self.culprit_resumed,
            failed_fixes: 0,
        };
        if progress.culprit_resumed {
            return Ok(progress);
        }
        if let Some(session) = self
            .session
            .as_deref()
            .filter(|_| self.status == "in_progress")
        {
            let claimed = self.claimed.unwrap_or(self.ready);
            let last: Option<String> = conn.query_row(
                "SELECT max(at) FROM (
                     SELECT at FROM records WHERE ticket = ?1 AND session = ?2 AND at <= ?3
                     UNION ALL
                     SELECT at FROM comments WHERE ticket = ?1 AND author = ?2 AND at <= ?3
                     UNION ALL
                     SELECT at FROM history WHERE ticket = ?1 AND actor = ?2 AND at <= ?3
                 )",
                params![self.id, session, now.to_string()],
                |row| row.get(0),
            )?;
            progress.last_activity = Some(
                optional_time(last.as_deref())?
                    .unwrap_or(claimed)
                    .max(claimed),
            );
        }
        progress.failed_fixes = failed_fixes(conn, &self.id, self.filed, now)?;
        Ok(progress)
    }
}

/// Count fixes separately for each check. A failed round proves only the newest attempt it
/// contains failed. If that attempt reopens the regression, keep counting it until a tip passes.
fn failed_fixes(conn: &Connection, ticket: &str, since: Timestamp, now: Timestamp) -> Result<u32> {
    Ok(conn.query_row(
        "WITH fixes AS (
             SELECT json_extract(change, '$.landed[1].commits[#-1]') AS commit_sha,
                    min(at) AS first_landed_at
             FROM history
             WHERE ticket = ?1 AND at >= ?2 AND at <= ?3
               AND json_type(change, '$.landed[1].commits') = 'array'
             GROUP BY commit_sha
         ), failed_attempts AS (
             SELECT r.check_name,
                    (SELECT fix.commit_sha FROM fixes fix
                     WHERE fix.first_landed_at <= t.verified
                       AND (fix.commit_sha = t.commit_sha OR fix.commit_sha IN (
                           SELECT value FROM json_each(t.failure, '$.unverified_commits')
                       ))
                     ORDER BY fix.first_landed_at DESC, fix.commit_sha LIMIT 1) AS attempt
             FROM tips t, json_each(t.failure, '$.checks') failed
             JOIN regressions r ON r.check_name = json_extract(failed.value, '$.check')
               AND (r.platform IS NULL OR r.platform = coalesce(json_extract(failed.value, '$.platform'), ''))
             WHERE t.state = 'failed' AND t.verified >= ?2 AND t.verified <= ?3
               AND r.ticket = ?1 AND r.state != ?4
         )
         SELECT coalesce(max(attempts), 0)
         FROM (SELECT count(DISTINCT attempt) AS attempts FROM failed_attempts GROUP BY check_name)",
        params![ticket, since.to_string(), now.to_string(), tips::FIXED],
        |row| row.get(0),
    )?)
}

fn age_threshold(conn: &Connection) -> Result<f64> {
    let mut select = conn.prepare(
        "SELECT t.created_at, t.closed_at, t.session, json_extract(t.metadata, ?1)
         FROM tickets t
         WHERE t.status = 'closed' AND t.closed_at IS NOT NULL
           AND json_type(t.metadata, ?1) = 'object'
           AND EXISTS (SELECT 1 FROM json_each(t.labels) WHERE value = 'regression')
         ORDER BY t.closed_at DESC, t.id",
    )?;
    let mut rows = select.query([format!("$.{CULPRIT_HANDOFF_KEY}")])?;
    let mut ages = Vec::new();
    while let Some(row) = rows.next()? {
        let holder: Option<String> = row.get(2)?;
        let handoff: String = row.get(3)?;
        if resumed(Some(&handoff), holder.as_deref()) {
            let created = parse_time(&row.get::<_, String>(0)?)?;
            let closed = parse_time(&row.get::<_, String>(1)?)?;
            ages.push(closed.seconds_since(created));
            if ages.len() == MAX_AGE_SAMPLE {
                break;
            }
        }
    }
    if ages.len() < MIN_AGE_SAMPLE {
        return Ok(FALLBACK_AGE_SECONDS);
    }
    ages.sort_by(f64::total_cmp);
    Ok(ages[(9 * ages.len()).div_ceil(10) - 1])
}
