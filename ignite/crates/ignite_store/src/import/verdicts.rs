//! The old system's verdicts on check failures, read from the hub after the tickets are
//! imported.
//!
//! The old check tools kept one closed ticket per check that had ever failed, labeled
//! [`VERDICT_LABEL`] and titled with the check's name. Each comment on it is one JSON verdict:
//! `{"time", "host", "commit", "lane", "verdict", "fixed", ...}`, where `verdict` is one of
//!
//! - `flaky`: the check failed and passed on the same tree, which `fixed` names as `tree <id>`;
//! - `stale`, `catch` or `mixed`: a later change made the failing check pass again, by changing
//!   the check, the product, or both;
//! - `miss`: a note recorded beside one of the others, never a failure of its own.
//!
//! A later comment `{"corrects": <comment>, "verdict": ...}` replaces the verdict of the
//! comment it names, which is the comment's `legacy_id` or else `ticket:<n>`.

use std::collections::HashMap;

use rusqlite::Connection;
use serde::Deserialize;

use crate::Result;

/// The label of the old system's per-check verdict tickets.
pub const VERDICT_LABEL: &str = "ci-check";

/// One verdict on one failure of `check`.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub check: String,
    /// `flaky`, `stale`, `catch`, `mixed` or `miss`.
    pub kind: String,
    pub time: String,
    pub host: String,
    pub commit: String,
    pub lane: String,
    pub fixed: String,
}

#[derive(Deserialize)]
struct Comment {
    verdict: Option<String>,
    corrects: Option<String>,
    #[serde(default)]
    time: String,
    #[serde(default)]
    host: String,
    #[serde(default)]
    commit: String,
    #[serde(default)]
    lane: String,
    #[serde(default)]
    fixed: Option<serde_json::Value>,
}

/// Every verdict on the hub's verdict tickets, oldest comment first per check, corrected.
pub fn read(hub: &Connection) -> Result<Vec<Verdict>> {
    let mut tickets = hub.prepare(
        "SELECT t.title, c.n, c.legacy_id, c.text FROM tickets t
         JOIN comments c ON c.ticket = t.id
         WHERE EXISTS (SELECT 1 FROM json_each(t.labels) WHERE value = ?1)
         ORDER BY t.id, c.n",
    )?;
    let rows = tickets
        .query_map([VERDICT_LABEL], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut found: Vec<Verdict> = Vec::new();
    // Each comment's position in `found`, by the ID a correction names it with.
    let mut by_comment: HashMap<String, usize> = HashMap::new();
    for (check, n, legacy_id, text) in rows {
        let Ok(comment) = serde_json::from_str::<Comment>(&text) else {
            continue;
        };
        let Some(verdict) = comment.verdict else {
            continue;
        };
        if let Some(corrected) = comment.corrects {
            if let Some(&index) = by_comment.get(&corrected) {
                found[index].kind = verdict;
            }
            continue;
        }
        let id = legacy_id.unwrap_or_else(|| format!("ticket:{n}"));
        by_comment.insert(id, found.len());
        let fixed = match comment.fixed {
            Some(serde_json::Value::String(text)) => text,
            _ => String::new(),
        };
        found.push(Verdict {
            check,
            kind: verdict,
            time: comment.time,
            host: comment.host,
            commit: comment.commit,
            lane: comment.lane,
            fixed,
        });
    }
    Ok(found)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::hub;

    /// Adds a verdict ticket for `check` to the hub with these comments, as the ticket import
    /// would have.
    pub fn add_verdict_ticket(hub: &Connection, id: &str, check: &str, comments: &[&str]) {
        hub.execute(
            "INSERT INTO tickets (id, title, status, labels, created_at, created_by, updated_at,
                                  version)
             VALUES (?1, ?2, 'closed', '[\"ci-check\",\"ci-lane:x\"]', '2026-11-01T00:00:00Z',
                     'tk', '2026-11-01T00:00:00Z', 1)",
            [id, check],
        )
        .unwrap();
        for (n, text) in comments.iter().enumerate() {
            let legacy = format!("{id}-c{n}");
            hub.execute(
                "INSERT INTO comments (ticket, n, author, at, text, legacy_id)
                 VALUES (?1, ?2, 'tk', '2026-11-01T00:00:00Z', ?3, ?4)",
                rusqlite::params![id, n as i64 + 1, text, legacy],
            )
            .unwrap();
        }
    }

    #[test]
    fn corrections_replace_the_verdict_they_name_and_other_comments_are_skipped() {
        let hub = hub::open_in_memory().unwrap();
        add_verdict_ticket(
            &hub,
            "ci1",
            "lane:one",
            &[
                r#"{"time":"2026-11-02T00:00:00+00:00","host":"h","commit":"aaaa","lane":"lane","verdict":"catch"}"#,
                "not a verdict",
                r#"{"time":"2026-11-03T00:00:00+00:00","commit":"bbbb","verdict":"flaky","fixed":"tree cccc"}"#,
                r#"{"corrects":"ci1-c0","verdict":"stale"}"#,
            ],
        );
        let verdicts = read(&hub).unwrap();
        assert_eq!(verdicts.len(), 2);
        assert_eq!(verdicts[0].check, "lane:one");
        assert_eq!(verdicts[0].kind, "stale");
        assert_eq!(verdicts[0].commit, "aaaa");
        assert_eq!(verdicts[1].kind, "flaky");
        assert_eq!(verdicts[1].fixed, "tree cccc");
    }
}
