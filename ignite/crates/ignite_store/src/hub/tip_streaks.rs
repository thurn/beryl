//! Checks that keep failing at consecutive tips.
//!
//! A round that sees a required check fail lets a newer `staging` tip replace its candidate
//! (`tips::note_check_failures`), because a newer landing may already hold the fix. On a busy
//! day a newer tip almost always exists, so the round is replaced before it reaches its verdict
//! and its culprit search, and the next candidate sees the same failure and is replaced in
//! turn. `master` then stayed put for hours with no regression ticket. A check that failed at
//! two consecutive tips was not fixed by the landings between them, so the hub reports it
//! itself from the failures the rounds recorded (see `ignite_hub`'s `upkeep::regressions`).

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit, MachineId};
use ignite_core::protocol::hub::ResultKey;

use crate::hub::results::parse;
use crate::{Result, StoreError};

/// How many consecutive tips must fail a check before the hub reports it.
pub const STREAK: usize = 2;

/// A check that failed on one platform at [`STREAK`] or more consecutive tips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Streak {
    /// The failed result at the newest tip of the streak.
    pub key: ResultKey,
    /// The oldest tip of the streak.
    pub first_bad: Commit,
    /// The newest verified tip before the streak.
    pub last_good: Commit,
    /// How many consecutive tips failed the check.
    pub tips: usize,
    /// The machine that stored the failed result at the newest tip, and where it saved the
    /// output, when the result cache still holds it.
    pub output: Option<(MachineId, String)>,
}

/// A check failing at the newest tip, as the walk back through older tips sees it.
struct Open {
    /// Its failed result at the newest tip.
    key: ResultKey,
    /// The oldest tip of the streak so far.
    first_bad: Commit,
    /// How many consecutive tips failed it so far.
    tips: usize,
    /// Whether an older tip without this failure ended the streak.
    broken: bool,
}

/// One tip with evidence: verified, or with failures that a round observed or recorded.
struct Evidence {
    commit: Commit,
    verified: bool,
    failed: Vec<ResultKey>,
}

fn failed_keys(text: Option<String>, what: &str) -> Result<Vec<ResultKey>> {
    text.map_or(Ok(Vec::new()), |text| {
        serde_json::from_str(&text).map_err(|error| StoreError::Invalid(format!("{what}: {error}")))
    })
}

/// The checks failing at `commit` that also failed on the same platform at the tips with
/// evidence just before it, back to the last verified tip. A check is the same by name and
/// platform: its input hash changes with almost every landing. A tip whose rounds recorded
/// other failures but not this one ends its streak, since its round may have passed it.
pub fn streaks(conn: &Connection, commit: &Commit) -> Result<Vec<Streak>> {
    let mut statement = conn.prepare(
        "SELECT commit_sha, state = 'verified', json_extract(failure, '$.checks'),
                observed_failures
         FROM tips
         WHERE (seen, commit_sha) <= (SELECT seen, commit_sha FROM tips WHERE commit_sha = ?1)
           AND (state = 'verified' OR failure IS NOT NULL OR observed_failures IS NOT NULL)
         ORDER BY seen DESC, commit_sha DESC",
    )?;
    let mut rows = statement.query_map(params![commit.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, bool>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut next = || -> Result<Option<Evidence>> {
        let Some(row) = rows.next().transpose()? else {
            return Ok(None);
        };
        let (sha, verified, recorded, observed) = row;
        let mut failed = failed_keys(recorded, "a tip's failed checks")?;
        failed.extend(failed_keys(observed, "observed tip failures")?);
        Ok(Some(Evidence {
            commit: parse("tip", &sha)?,
            verified,
            failed,
        }))
    };
    let Some(newest) = next()?.filter(|tip| tip.commit == *commit && !tip.verified) else {
        return Ok(Vec::new());
    };
    let mut open: BTreeMap<(CheckName, Option<Os>), Open> = BTreeMap::new();
    for key in newest.failed {
        open.entry((key.check.clone(), key.platform))
            .or_insert_with(|| Open {
                key,
                first_bad: commit.clone(),
                tips: 1,
                broken: false,
            });
    }
    let mut last_good = None;
    while let Some(tip) = next()? {
        if tip.verified {
            last_good = Some(tip.commit);
            break;
        }
        for ((check, platform), streak) in open.iter_mut().filter(|(_, s)| !s.broken) {
            if tip
                .failed
                .iter()
                .any(|key| key.check == *check && key.platform == *platform)
            {
                streak.first_bad.clone_from(&tip.commit);
                streak.tips += 1;
            } else {
                streak.broken = true;
            }
        }
        if open.values().all(|s| s.broken && s.tips < STREAK) {
            return Ok(Vec::new());
        }
    }
    let Some(last_good) = last_good else {
        // With no verified tip on record there is no range to report.
        return Ok(Vec::new());
    };
    open.into_values()
        .filter(|streak| streak.tips >= STREAK)
        .map(|streak| {
            Ok(Streak {
                output: failed_output(conn, &streak.key)?,
                key: streak.key,
                first_bad: streak.first_bad,
                last_good: last_good.clone(),
                tips: streak.tips,
            })
        })
        .collect()
}

/// The machine that stored `key`'s failure and where it saved the output.
fn failed_output(conn: &Connection, key: &ResultKey) -> Result<Option<(MachineId, String)>> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT machine, output FROM results
             WHERE check_name = ?1 AND hash = ?2 AND platform = ?3
               AND outcome = 'fail' AND output IS NOT NULL",
            params![
                key.check.as_str(),
                key.hash.as_str(),
                key.platform.map_or("", Os::as_str)
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    row.map(|(machine, output)| Ok((parse("machine", &machine)?, output)))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{self, tips};
    use ignite_core::ids::InputHash;
    use ignite_core::protocol::hub::{TipCheckFailures, TipState, TipVerified};
    use ignite_core::time::Timestamp;

    fn key(check: &str, hash: char, platform: Option<Os>) -> ResultKey {
        ResultKey {
            check: CheckName::new(check).unwrap(),
            hash: InputHash::new(hash.to_string().repeat(64)).unwrap(),
            platform,
        }
    }

    fn tip(conn: &mut Connection, second: f64, sha: &str, failed: Vec<ResultKey>) -> Commit {
        let commit = Commit::new(sha).unwrap();
        tips::seen(conn, sha, Timestamp::EPOCH.plus_seconds(second)).unwrap();
        if failed.is_empty() {
            let verified = TipVerified {
                commit: commit.clone(),
                machine: MachineId::new("m-1").unwrap(),
                state: TipState::Verified,
                regression_passes: Some(Vec::new()),
                failure: None,
                failed_descendant: None,
            };
            tips::verified(conn, &verified, Timestamp::EPOCH.plus_seconds(second)).unwrap();
        } else {
            let failures = TipCheckFailures {
                commit: commit.clone(),
                checks: failed,
            };
            tips::note_check_failures(conn, &failures).unwrap();
        }
        commit
    }

    #[test]
    fn a_streak_needs_the_same_check_and_platform_at_consecutive_tips() {
        let mut conn = hub::open_in_memory().unwrap();
        let mac = Some(Os::Macos);
        let good = tip(&mut conn, 1.0, "aaaa", Vec::new());
        let first = tip(&mut conn, 2.0, "bbbb", vec![key("x:a", 'a', mac)]);
        assert_eq!(streaks(&conn, &first).unwrap(), Vec::new());

        // Another platform's failure of the same check is another check.
        let windows = tip(
            &mut conn,
            3.0,
            "cccc",
            vec![key("x:a", 'b', Some(Os::Windows))],
        );
        assert_eq!(streaks(&conn, &windows).unwrap(), Vec::new());

        let again = tip(&mut conn, 4.0, "dddd", vec![key("x:a", 'c', mac)]);
        assert_eq!(streaks(&conn, &again).unwrap(), Vec::new());
        let third = tip(&mut conn, 5.0, "eeee", vec![key("x:a", 'd', mac)]);
        assert_eq!(
            streaks(&conn, &third).unwrap(),
            vec![Streak {
                key: key("x:a", 'd', mac),
                first_bad: again,
                last_good: good,
                tips: 2,
                output: None,
            }]
        );

        // A verified tip ends every streak.
        tip(&mut conn, 6.0, "ffff", Vec::new());
        let after = tip(&mut conn, 7.0, "abab", vec![key("x:a", 'e', mac)]);
        assert_eq!(streaks(&conn, &after).unwrap(), Vec::new());
    }
}
