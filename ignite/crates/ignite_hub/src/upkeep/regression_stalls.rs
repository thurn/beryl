//! Whether regression tickets that hold the verified branch back still have a fix in progress.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};
use serde_json::Value;

use ignite_core::config::Os;
use ignite_core::protocol::tickets::CULPRIT_HANDOFF_KEY;
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet, fleet_settings, rows, tickets, tips};

use super::{parse_time, regressions};

/// Thresholds calibrated against the hub's claim, activity and landing history.
pub(super) const UNCLAIMED_SECONDS: f64 = 10.0 * 60.0;
pub(super) const INACTIVE_SECONDS: f64 = 50.0 * 60.0;
pub(super) const FAILED_FIXES: u32 = 2;
pub(super) const FALLBACK_AGE_SECONDS: f64 = 4.0 * 3600.0;
pub(super) const MAX_AGE_SECONDS: f64 = 4.0 * 3600.0;

/// Use the 90th percentile of recent regression lifetimes from the culprit-resume workflow
/// once there is a sample big enough for two observations in its upper tenth.
const MIN_AGE_SAMPLE: usize = 20;
const MAX_AGE_SAMPLE: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Stall {
    Unclaimed(Timestamp),
    Inactive(Timestamp),
    HolderUnavailable(String),
    FailedFixes(u32),
    Old(Timestamp),
}

impl Stall {
    pub(super) fn reason(&self) -> String {
        match self {
            Self::Unclaimed(since) => format!("It has been open without a claim since {since}."),
            Self::Inactive(since) => {
                format!("Its holder last recorded activity on the ticket at {since}.")
            }
            Self::HolderUnavailable(machine) => format!(
                "Its holder is on machine {machine}, which is off, silent or has stuck storage."
            ),
            Self::FailedFixes(count) => format!(
                "{count} separate fixes landed or closed it without a landing, and tip \
                 verification still failed the same check after each one."
            ),
            Self::Old(since) => format!(
                "It has been open since {since}, longer than the historical 90th-percentile \
                 regression lifetime."
            ),
        }
    }
}

#[derive(Clone, Debug)]
struct Progress {
    filed: Timestamp,
    unclaimed_since: Option<Timestamp>,
    last_activity: Option<Timestamp>,
    holder_unavailable: Option<String>,
    culprit_resumed: bool,
    failed_fixes: u32,
}

impl Progress {
    fn stall(self, now: Timestamp, age_seconds: f64) -> Option<Stall> {
        if let Some(machine) = self.holder_unavailable {
            return Some(Stall::HolderUnavailable(machine));
        }
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
    holder_unavailable: Option<String>,
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
    let settings = fleet_settings::settings(conn)?;
    let stuck = fleet_settings::storage_stuck_map(conn)?;
    let machines: BTreeMap<String, rows::Machine> = fleet::machines(conn)?
        .into_iter()
        .map(|machine| (machine.id.clone(), machine))
        .collect();
    let unavailable = fleet_settings::unavailable_platforms(conn, &settings, now)?;
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
                json_extract(t.metadata, ?3) AS handoff,
                t.machine
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
        // A regression whose failing checks all belong to currently deferred/unavailable platforms
        // does not hold `master` back, so it must not silence `master_lag`.
        if !unavailable.is_empty() && on_unavailable_platforms_only(conn, &id, &unavailable)? {
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
        let machine_id: Option<String> = row.get(9)?;
        let holder_unavailable = (status == "in_progress")
            .then_some(machine_id)
            .flatten()
            .and_then(|id| machines.get(&id))
            .filter(|machine| {
                !fleet_settings::machine_online_and_active(machine, &settings, &stuck, now)
            })
            .map(|machine| machine.name.clone());
        regressions.push(Regression {
            id,
            filed: episode,
            ready: optional_time(row.get::<_, Option<String>>(5)?.as_deref())?.unwrap_or(filed),
            claimed: optional_time(row.get::<_, Option<String>>(4)?.as_deref())?,
            holder_unavailable,
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
            holder_unavailable: self.holder_unavailable.clone(),
            culprit_resumed: self.culprit_resumed,
            failed_fixes: 0,
        };
        if progress.holder_unavailable.is_some() || progress.culprit_resumed {
            return Ok(progress);
        }
        if let Some(session) = self
            .session
            .as_deref()
            .filter(|_| self.status == "in_progress")
        {
            let claimed = self.claimed.unwrap_or(self.ready);
            // Activity is anything the holder did after the claim. Daemons record `test` and
            // most `build` runs without a ticket, and `run` leaves no record at all, so a
            // record of the holder's with no ticket counts, and so does the newest command
            // its daemon reports on its worker row (when the command started, or ended once
            // it has ended). Without these a worker that only tests and measures looked idle.
            let last: Option<String> = conn.query_row(
                "SELECT max(at) FROM (
                     SELECT at FROM records
                     WHERE (ticket = ?1 OR ticket IS NULL) AND session = ?2 AND at <= ?3
                     UNION ALL
                     SELECT command_since FROM workers
                     WHERE ticket = ?1 AND session = ?2 AND command_since <= ?3
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

/// Whether every unfixed regression row for `ticket` belongs to a platform in `unavailable`.
fn on_unavailable_platforms_only(
    conn: &Connection,
    ticket: &str,
    unavailable: &BTreeSet<Os>,
) -> Result<bool> {
    let mut stmt =
        conn.prepare_cached("SELECT platform FROM regressions WHERE ticket = ?1 AND state != ?2")?;
    let platforms: Vec<Option<String>> = stmt
        .query_map(params![ticket, tips::FIXED], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if platforms.is_empty() {
        return Ok(false);
    }
    Ok(platforms.iter().all(|platform| match platform.as_deref() {
        Some("macos") => unavailable.contains(&Os::Macos),
        Some("windows") => unavailable.contains(&Os::Windows),
        _ => false,
    }))
}

/// Count fixes separately for each check. A failed round proves only the newest attempt it
/// contains failed. Also count closes with no landed commit that tip verification reopened.
fn failed_fixes(conn: &Connection, ticket: &str, since: Timestamp, now: Timestamp) -> Result<u32> {
    let landed: u32 = conn.query_row(
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
    )?;
    let reopened_without_landing: u32 = conn.query_row(
        "SELECT count(*) FROM history r
         WHERE r.ticket = ?1 AND r.at >= ?2 AND r.at <= ?3
           AND json_extract(r.change, '$.status[0]') = 'closed'
           AND json_extract(r.change, '$.status[1]') = 'open'
           AND NOT EXISTS (
               SELECT 1 FROM history l
               WHERE l.ticket = ?1 AND l.at >= ?2 AND l.at <= r.at
                 AND json_type(l.change, '$.landed[1].commits') = 'array'
                 AND l.at >= coalesce((
                     SELECT max(prev.at) FROM history prev
                     WHERE prev.ticket = ?1 AND prev.at >= ?2 AND prev.at < r.at
                       AND json_extract(prev.change, '$.status[0]') = 'closed'
                       AND json_extract(prev.change, '$.status[1]') = 'open'
                 ), ?2)
           )",
        params![ticket, since.to_string(), now.to_string()],
        |row| row.get(0),
    )?;
    Ok(landed.saturating_add(reopened_without_landing))
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
    Ok(ages[(9 * ages.len()).div_ceil(10) - 1].min(MAX_AGE_SECONDS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::config::Os;
    use ignite_core::fleet::MachineState;
    use ignite_core::ids::MachineId;
    use ignite_core::protocol::hub::MachineRegister;
    use ignite_core::protocol::tickets::{Operation, Status, TicketFields, TicketType};
    use ignite_store::hub;
    use serde_json::json;

    use crate::upkeep::tests::{apply, create};

    fn at(seconds: f64) -> Timestamp {
        Timestamp::parse("2026-11-25T14:00:00Z")
            .unwrap()
            .plus_seconds(seconds)
    }

    fn register(conn: &Connection, id: &str, name: &str, os: Os, now: Timestamp) {
        fleet::register(
            conn,
            &MachineRegister {
                machine: MachineId::new(id).unwrap(),
                name: name.to_owned(),
                os,
                cores: 12,
                memory_mb: 32_768,
                families: Vec::new(),
                version: "v".to_owned(),
            },
            now,
        )
        .unwrap();
    }

    fn insert_regression(
        conn: &mut Connection,
        check: &str,
        platform: Option<&str>,
        now: Timestamp,
    ) -> String {
        let ids = apply(
            conn,
            vec![create(
                &format!("Regression: {check}"),
                TicketType::Bug,
                None,
                &[],
            )],
            now,
        );
        let id = ids[0].clone();
        conn.execute(
            "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state, platform)
             VALUES (?1, ?2, 'bad1', 'good1', 'open', ?3)",
            params![check, id, platform],
        )
        .unwrap();
        id
    }

    #[test]
    fn age_threshold_is_capped_at_max_age_even_with_multiday_outliers() {
        let mut conn = hub::open_in_memory().unwrap();
        let start = at(0.0);
        for i in 0..25 {
            let ids = apply(
                &mut conn,
                vec![create("old reg", TicketType::Bug, None, &[])],
                start,
            );
            let id = &ids[0];
            let hours = if i < 20 { 1.0 } else { 90.0 };
            conn.execute(
                "UPDATE tickets SET status = 'closed', labels = '[\"regression\"]', closed_at = ?1,
                 session = 'w', metadata = ?2 WHERE id = ?3",
                params![
                    start.plus_seconds(hours * 3600.0).to_string(),
                    json!({CULPRIT_HANDOFF_KEY: {"session": "w", "resumed": true}}).to_string(),
                    id
                ],
            )
            .unwrap();
        }
        assert!((age_threshold(&conn).unwrap() - MAX_AGE_SECONDS).abs() < f64::EPSILON);
    }

    #[test]
    fn repeated_reopens_without_a_landed_fix_escalate_as_failed_fixes() {
        let mut conn = hub::open_in_memory().unwrap();
        let id = insert_regression(&mut conn, "clippy:kit", None, at(0.0));
        let close_and_reopen = |conn: &mut Connection, close_s: f64, reopen_s: f64| {
            apply(
                conn,
                vec![Operation::Close {
                    id: id.clone(),
                    reason: "passed on rerun".to_owned(),
                    include_ready: false,
                }],
                at(close_s),
            );
            apply(
                conn,
                vec![Operation::Update {
                    id: id.clone(),
                    set: Some(TicketFields {
                        status: Some(Status::Open),
                        ..TicketFields::default()
                    }),
                    add_labels: Vec::new(),
                    remove_labels: Vec::new(),
                    set_meta: BTreeMap::new(),
                    unset_meta: Vec::new(),
                }],
                at(reopen_s),
            );
        };
        close_and_reopen(&mut conn, 60.0, 120.0);
        assert_eq!(failed_fixes(&conn, &id, at(0.0), at(180.0)).unwrap(), 1);
        close_and_reopen(&mut conn, 240.0, 300.0);
        assert_eq!(failed_fixes(&conn, &id, at(0.0), at(360.0)).unwrap(), 2);

        let open = open(&conn, at(360.0)).unwrap();
        assert_eq!(open.get(&id), Some(&Some(Stall::FailedFixes(2))));
    }

    #[test]
    fn a_regression_on_a_deferred_platform_is_ignored_and_an_offline_holder_stalls_at_once() {
        let mut conn = hub::open_in_memory().unwrap();
        register(&conn, "m-mac", "mac", Os::Macos, at(0.0));
        register(&conn, "m-win", "win", Os::Windows, at(0.0));

        // A Windows-only regression stops holding master when Windows is deferred.
        let win_reg = insert_regression(&mut conn, "clippy:kit_win", Some("windows"), at(0.0));
        assert!(open(&conn, at(10.0)).unwrap().contains_key(&win_reg));
        fleet_settings::record_storage_stuck(&conn, "m-win", at(20.0)).unwrap();
        assert!(!open(&conn, at(30.0)).unwrap().contains_key(&win_reg));

        // A portable regression held by a machine that goes off stalls immediately.
        let portable = insert_regression(&mut conn, "rust:unit", None, at(30.0));
        conn.execute(
            "UPDATE tickets SET status = 'in_progress', assignee = 'w', session = 'w',
             machine = 'm-mac', claimed_at = ?1 WHERE id = ?2",
            params![at(35.0).to_string(), portable],
        )
        .unwrap();
        assert_eq!(open(&conn, at(40.0)).unwrap().get(&portable), Some(&None));
        fleet_settings::set_machine_state(&mut conn, "m-mac", MachineState::Off, "ic", at(45.0))
            .unwrap();
        assert_eq!(
            open(&conn, at(50.0)).unwrap().get(&portable),
            Some(&Some(Stall::HolderUnavailable("mac".to_owned())))
        );
    }
}
