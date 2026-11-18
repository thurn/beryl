//! The result cache client: the machine's local copy first, then one hub request for the rest.
//!
//! The hub holds the result cache (see "The result cache" in `docs/ignite.md`). Each daemon keeps
//! a copy of the entries it has seen in its local database, so a lookup asks the hub only for
//! the keys the copy lacks, all in one request, and checks keep working from the copy while the
//! hub cannot be reached.
//!
//! Only passes and failures are cached. An `incomplete` run says nothing about the check, and a
//! `flaky` one is reported to the hub's flaky table instead (see [`ResultCache::flaky`]).
//!
//! Results, records and flaky reports that the hub cannot take now wait in the local outbox
//! ([`crate::outbox`]), which sends them once it is reachable.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rusqlite::Connection;

use ignite_core::config::Os;
use ignite_core::ids::{MachineId, RecordId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{CachedResult, FlakyReport, NewResult, Record, ResultKey};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Hub};
use ignite_store::local::{outbox as queue, results, rows};

use crate::outbox;

/// The result cache as one daemon sees it. See the module documentation.
pub struct ResultCache {
    conn: Mutex<Connection>,
    hub: Arc<dyn Hub>,
    machine: MachineId,
    clock: Arc<dyn Clock>,
}

impl ResultCache {
    /// A cache over the local database `conn` and the hub, for results this machine produces.
    pub fn new(
        conn: Connection,
        hub: Arc<dyn Hub>,
        machine: MachineId,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            conn: Mutex::new(conn),
            hub,
            machine,
            clock,
        }
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One entry per key, in order. Keys the local copy lacks go to the hub in one request, and
    /// what the hub answers is copied locally. When the hub cannot be reached, those keys are
    /// misses.
    ///
    /// A local entry does not record which machine produced it, so a hit from the copy names
    /// this machine.
    pub fn lookup(&self, keys: &[ResultKey]) -> Vec<Option<CachedResult>> {
        let mut found: Vec<Option<CachedResult>> = {
            let conn = self.conn();
            keys.iter().map(|key| self.local(&conn, key)).collect()
        };
        let missing: Vec<usize> = (0..keys.len()).filter(|&i| found[i].is_none()).collect();
        if missing.is_empty() {
            return found;
        }
        let asked: Vec<ResultKey> = missing.iter().map(|&i| keys[i].clone()).collect();
        let Ok(answers) = self.hub.lookup(&asked) else {
            return found;
        };
        let conn = self.conn();
        for (index, answer) in missing.into_iter().zip(answers) {
            if let Some(result) = answer.filter(|r| cacheable(r.outcome)) {
                // A failure to copy only costs a hub request next time.
                let _ = results::put(&conn, &row(&result));
                found[index] = Some(result);
            }
        }
        found
    }

    /// Stores passes and failures locally and at the hub. Results the hub does not take now are
    /// queued in the outbox and sent when it is reachable. Other outcomes are ignored.
    pub fn store(&self, new: &[NewResult]) {
        let new: Vec<NewResult> = new
            .iter()
            .filter(|r| cacheable(r.outcome))
            .cloned()
            .collect();
        if new.is_empty() {
            return;
        }
        let at = self.clock.now();
        {
            let conn = self.conn();
            for result in &new {
                let _ = results::put(&conn, &row(&cached(result, at)));
            }
        }
        if self.hub.store(&new).is_err() {
            let conn = self.conn();
            for result in &new {
                let _ = outbox::push_result(&conn, result, at);
            }
        }
    }

    /// Sends a command's record to the hub, or queues it when the hub cannot take it.
    pub fn record(&self, record: &Record) {
        if self.hub.records(std::slice::from_ref(record)).is_err() {
            let _ = outbox::push_record(&self.conn(), record, self.clock.now());
        }
    }

    /// Reports a flaky check to the hub, or queues the report when the hub cannot take it.
    pub fn flaky(&self, report: &FlakyReport) {
        if self.hub.flaky(report).is_err() {
            let _ = outbox::push_flaky(&self.conn(), report, self.clock.now());
        }
    }

    /// The outbox entries not yet sent, oldest first, as (kind, body).
    pub fn queued(&self) -> Vec<(String, String)> {
        queue::peek(&self.conn(), u32::MAX)
            .unwrap_or_default()
            .into_iter()
            .map(|entry| (entry.kind, entry.body))
            .collect()
    }

    fn local(&self, conn: &Connection, key: &ResultKey) -> Option<CachedResult> {
        let platform = key.platform.map_or("", Os::as_str);
        let row = results::get(conn, key.check.as_str(), key.hash.as_str(), platform).ok()??;
        let outcome = match row.outcome.as_str() {
            "pass" => Outcome::Pass,
            "fail" => Outcome::Fail,
            _ => return None,
        };
        Some(CachedResult {
            key: key.clone(),
            outcome,
            machine: self.machine.clone(),
            record: RecordId(
                row.record
                    .and_then(|id| u64::try_from(id).ok())
                    .unwrap_or(0),
            ),
            at: Timestamp::parse(&row.at).unwrap_or(Timestamp::EPOCH),
            output: row.output,
        })
    }
}

/// Whether an outcome is reused: only a pass or a failure says something about the inputs.
fn cacheable(outcome: Outcome) -> bool {
    matches!(outcome, Outcome::Pass | Outcome::Fail)
}

fn cached(result: &NewResult, at: Timestamp) -> CachedResult {
    CachedResult {
        key: result.key.clone(),
        outcome: result.outcome,
        machine: result.machine.clone(),
        record: result.record,
        at,
        output: result.output.clone(),
    }
}

fn row(result: &CachedResult) -> rows::CachedResult {
    rows::CachedResult {
        check_name: result.key.check.as_str().to_owned(),
        hash: result.key.hash.as_str().to_owned(),
        platform: result.key.platform.map_or("", Os::as_str).to_owned(),
        outcome: if result.outcome == Outcome::Pass {
            "pass"
        } else {
            "fail"
        }
        .to_owned(),
        // Record 0 means the hub's record ID is not known.
        record: i64::try_from(result.record.0).ok().filter(|id| *id != 0),
        at: result.at.to_string(),
        output: result.output.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::fakes::{FakeClock, FakeHub};
    use ignite_core::ids::{CheckName, InputHash};

    fn key(name: &str) -> ResultKey {
        ResultKey {
            check: CheckName::new(name).unwrap(),
            hash: InputHash::new("c".repeat(64)).unwrap(),
            platform: None,
        }
    }

    fn new(name: &str, outcome: Outcome) -> NewResult {
        NewResult {
            key: key(name),
            outcome,
            machine: MachineId::new("m-1").unwrap(),
            record: RecordId(0),
            output: None,
        }
    }

    fn cache(hub: &Arc<FakeHub>) -> ResultCache {
        ResultCache::new(
            ignite_store::local::open_in_memory().unwrap(),
            hub.clone(),
            MachineId::new("m-1").unwrap(),
            Arc::new(FakeClock::new(Timestamp::EPOCH)),
        )
    }

    #[test]
    fn hub_hits_are_copied_and_found_locally_while_the_hub_is_down() {
        let hub = Arc::new(FakeHub::default());
        hub.store(&[new("f:a", Outcome::Pass)]).unwrap();
        let cache = cache(&hub);
        let keys = [key("f:a"), key("f:b")];
        let found = cache.lookup(&keys);
        assert_eq!(found[0].as_ref().unwrap().outcome, Outcome::Pass);
        assert!(found[1].is_none());
        hub.set_unreachable(true);
        let found = cache.lookup(&keys);
        assert_eq!(found[0].as_ref().unwrap().outcome, Outcome::Pass);
        assert!(found[1].is_none());
    }

    #[test]
    fn only_passes_and_failures_are_stored() {
        let hub = Arc::new(FakeHub::default());
        let cache = cache(&hub);
        cache.store(&[
            new("f:pass", Outcome::Pass),
            new("f:fail", Outcome::Fail),
            new("f:flaky", Outcome::Flaky),
            new("f:incomplete", Outcome::Incomplete),
        ]);
        hub.set_unreachable(true);
        let outcomes: Vec<Option<Outcome>> = cache
            .lookup(&[
                key("f:pass"),
                key("f:fail"),
                key("f:flaky"),
                key("f:incomplete"),
            ])
            .into_iter()
            .map(|r| r.map(|r| r.outcome))
            .collect();
        assert_eq!(
            outcomes,
            [Some(Outcome::Pass), Some(Outcome::Fail), None, None]
        );
    }

    #[test]
    fn results_the_hub_refuses_wait_in_the_outbox() {
        let hub = Arc::new(FakeHub::default());
        hub.set_unreachable(true);
        let cache = cache(&hub);
        cache.store(&[new("f:a", Outcome::Pass)]);
        let queued = cache.queued();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].0, "result");
        let body: NewResult = serde_json::from_str(&queued[0].1).unwrap();
        assert_eq!(body.key, key("f:a"));
        // The local copy already answers.
        assert!(cache.lookup(&[key("f:a")])[0].is_some());
    }
}
