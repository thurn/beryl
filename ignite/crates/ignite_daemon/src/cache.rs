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
//! Results, records and flaky reports that the hub cannot take now wait in the local outbox ([`crate::outbox`]), which sends them once it is reachable.
//!
//! The local database also keeps each check family's listing by family and listing hash, and
//! the hub keeps them for every machine ([`ResultCache::listings`]).
//!
//! The local database also keeps how long each check command took when it passed, from which
//! the check runner computes the command's hang limit ([`ResultCache::slowest_pass`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rusqlite::Connection;

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit, MachineId, RecordId, SessionId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{
    CachedResult, FlakyReport, ListingKey, NewResult, Record, ResultKey, StoredListing,
};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Hub};
use ignite_store::local::{listings, outbox as queue, passes, results, rows};

use crate::outbox;

/// The hash a listing that must not be kept is stored under on this machine. A listing hash is
/// hex, so it never equals this word.
const UNKEPT: &str = "unkept";

/// The message of a failure found in the result cache instead of run.
pub const FAILED_BEFORE: &str = "failed before on these inputs";

/// The verbs whose outcomes help detect shared failures (see `note_sightings`).
const SIGHTED_VERBS: &[&str] = &["test", "check", "land"];

/// How far back a command's passes count toward its hang limit: 30 days.
const PASSES_KEPT_S: f64 = 30.0 * 24.0 * 60.0 * 60.0;

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

    /// The daemon's local database, which the landing gate's memory also lives in (see
    /// `checks::spent`).
    pub(crate) fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One entry per key, in order. Keys the local copy lacks go to the hub in one request, and
    /// what the hub answers is copied locally. When the hub cannot be reached, those keys are
    /// misses.
    ///
    /// A local entry stored before entries recorded their machine is used only if it is a
    /// pass, and then names this machine; such a failure is asked of the hub again.
    ///
    /// A local failure is always asked of the hub too, and the hub's entry wins when it has
    /// one: the failure may have been repaired since (the hub then holds a newer pass), and a
    /// failure that was only a fault of this machine must not outlive it. The local failure
    /// stands only while the hub says nothing or cannot be reached.
    pub fn lookup(&self, keys: &[ResultKey]) -> Vec<Option<CachedResult>> {
        let mut found: Vec<Option<CachedResult>> = {
            let conn = self.conn();
            keys.iter().map(|key| self.local(&conn, key)).collect()
        };
        let missing: Vec<usize> = (0..keys.len())
            .filter(|&i| found[i].as_ref().is_none_or(|r| r.outcome == Outcome::Fail))
            .collect();
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

    /// Records the latest outcome of each check for `session` when `verb` is `test`, `check`,
    /// or `land`, so the daemon can detect shared failures (see `dispatchd_shared`). `hashes`
    /// gives each check's input hash. `head` is the tested tree's checked-out commit.
    pub fn note_sightings(
        &self,
        session: Option<&SessionId>,
        verb: &str,
        run: &crate::checks::CheckRun,
        hashes: &HashMap<CheckName, String>,
        head: Option<&Commit>,
    ) {
        let Some(session) = session else {
            return;
        };
        if !SIGHTED_VERBS.contains(&verb) {
            return;
        }
        let at = self.clock.now().to_string();
        let conn = self.conn();
        for (check, outcome) in &run.outcomes {
            let outcome = match outcome {
                Outcome::Pass => "pass",
                Outcome::Fail => "fail",
                Outcome::Flaky | Outcome::Incomplete => continue,
            };
            let first_line = run
                .failures
                .iter()
                .find(|f| f.check == *check)
                .and_then(|f| f.message.as_deref())
                .filter(|message| *message != FAILED_BEFORE)
                .and_then(|message| message.lines().map(str::trim).find(|l| !l.is_empty()))
                .map(str::to_owned);
            let _ = results::note_sighting(
                &conn,
                &rows::CheckSighting {
                    session: session.to_string(),
                    check_name: check.to_string(),
                    verb: verb.to_owned(),
                    outcome: outcome.to_owned(),
                    hash: hashes.get(check).cloned(),
                    first_line,
                    tip_unknown: 0,
                    at: at.clone(),
                    head: head.map(ToString::to_string),
                },
            );
        }
    }

    /// Marks `session`'s failures of `checks` as ones whose result at the tip could not be
    /// found or run (see `dispatchd_shared`).
    pub fn mark_tip_unknown(&self, session: Option<&SessionId>, checks: &[CheckName]) {
        let Some(session) = session else {
            return;
        };
        let conn = self.conn();
        for check in checks {
            let _ = results::mark_tip_unknown(&conn, session.as_str(), check.as_str());
        }
    }

    /// Every session's latest outcome of every check recorded at or after `since`, oldest
    /// first; empty when the database cannot be read.
    pub fn sightings_since(&self, since: Timestamp) -> Vec<rows::CheckSighting> {
        results::sightings_since(&self.conn(), &since.to_string()).unwrap_or_default()
    }

    /// Sends a command's record to the hub, or queues it when the hub cannot take it. Returns
    /// the ID the hub gave it, or `None` when it was queued.
    pub fn record(&self, record: &Record) -> Option<RecordId> {
        if let Ok(ids) = self.hub.numbered_records(std::slice::from_ref(record)) {
            return ids.first().copied();
        }
        let _ = outbox::push_record(&self.conn(), record, self.clock.now());
        None
    }

    /// Reports a run's flakes together, or queues the batch if the hub cannot take it.
    pub fn flaky(&self, reports: &[FlakyReport]) {
        if !reports.is_empty() && self.hub.flaky_batch(reports).is_err() {
            let _ = outbox::push_flaky(&self.conn(), reports, self.clock.now());
        }
    }

    /// The stored listing of each key, in order: the local copy first, then one hub request for
    /// the keys it lacks. What the hub answers is copied locally. A hub that cannot be reached
    /// leaves those keys missing, which only means the family is listed again.
    pub fn listings(&self, keys: &[ListingKey]) -> Vec<Option<String>> {
        let mut found: Vec<Option<String>> = {
            let conn = self.conn();
            keys.iter()
                .map(|k| {
                    listings::get(&conn, &k.family, k.hash.as_str())
                        .ok()
                        .flatten()
                })
                .collect()
        };
        let missing: Vec<usize> = (0..keys.len()).filter(|&i| found[i].is_none()).collect();
        if missing.is_empty() {
            return found;
        }
        let asked: Vec<ListingKey> = missing.iter().map(|&i| keys[i].clone()).collect();
        let Ok(answers) = self.hub.listings(&asked) else {
            return found;
        };
        let at = self.clock.now().to_string();
        let conn = self.conn();
        for (index, answer) in missing.into_iter().zip(answers) {
            if let Some(listing) = answer {
                let key = &keys[index];
                // A failure to copy only costs a hub request next time.
                let _ = listings::put(&conn, &key.family, key.hash.as_str(), &listing, &at);
                found[index] = Some(listing);
            }
        }
        found
    }

    /// The newest listing of `family` in the local copy, whatever inputs it was made from.
    pub fn latest_listing(&self, family: &str) -> Option<String> {
        listings::latest(&self.conn(), family).ok().flatten()
    }

    /// Stores, on this machine only, a listing of `family` that must not be kept for its inputs
    /// (see "Checks" in `docs/ignite.md`), so that [`Cache::latest_listing`] can still say which
    /// checks the family holds. It is stored under [`UNKEPT`], which no listing hash equals, so
    /// no tree is ever offered it as its listing.
    pub fn store_unkept_listing(&self, family: &str, listing: &str) {
        let at = self.clock.now().to_string();
        let _ = listings::put(&self.conn(), family, UNKEPT, listing, &at);
    }

    /// Stores a family's listing locally and at the hub. A hub that cannot take it now is not
    /// asked again: a missing listing only costs another machine a listing of its own.
    ///
    /// The hub gets its copy from a thread of its own, so the gate that made the listing does
    /// not wait for it: a 4 MB listing took the hub 6 to 10 s to receive, and the gate waited
    /// that long before it planned anything. A copy the hub never receives, because it could
    /// not be reached or the daemon stopped first, only means another machine lists the family
    /// itself. Tests send it before returning, so what the hub holds afterwards is certain.
    pub fn store_listing(&self, key: &ListingKey, listing: &str) {
        let at = self.clock.now().to_string();
        let _ = listings::put(&self.conn(), &key.family, key.hash.as_str(), listing, &at);
        let hub = Arc::clone(&self.hub);
        let stored = StoredListing {
            key: key.clone(),
            listing: listing.to_owned(),
        };
        let send = move || {
            let _ = hub.store_listing(&stored);
        };
        if cfg!(test) {
            send();
        } else {
            std::thread::spawn(send);
        }
    }

    /// Records that `family`'s `command` (`list` or `run`) passed in `wall_s` seconds.
    pub fn note_pass(&self, family: &str, command: &str, wall_s: f64) {
        let now = self.clock.now();
        let forget = now.plus_seconds(-PASSES_KEPT_S);
        let _ = passes::add(
            &self.conn(),
            family,
            command,
            wall_s,
            &now.to_string(),
            &forget.to_string(),
        );
    }

    /// The longest pass of `family`'s `command` in the last 30 days, if any.
    pub fn slowest_pass(&self, family: &str, command: &str) -> Option<f64> {
        let since = self.clock.now().plus_seconds(-PASSES_KEPT_S);
        passes::slowest(&self.conn(), family, command, &since.to_string())
            .ok()
            .flatten()
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
        let machine = match row.machine.map(MachineId::new) {
            Some(Ok(machine)) => machine,
            // Whose failure this is is not known, so the hub is asked.
            _ if outcome == Outcome::Fail => return None,
            _ => self.machine.clone(),
        };
        Some(CachedResult {
            key: key.clone(),
            outcome,
            machine,
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
        machine: Some(result.machine.as_str().to_owned()),
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
            cost: ignite_core::protocol::hub::ResultCost::default(),
            areas: Vec::new(),
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
    fn a_local_failure_names_its_machine_and_an_unattributed_one_is_asked_again() {
        let hub = Arc::new(FakeHub::default());
        let cache = cache(&hub);
        let mut other = new("f:a", Outcome::Fail);
        other.machine = MachineId::new("m-2").unwrap();
        cache.store(&[other]);
        hub.set_unreachable(true);
        let found = cache.lookup(&[key("f:a")]);
        assert_eq!(found[0].as_ref().unwrap().machine.as_str(), "m-2");
        // A row from before machines were kept.
        let mut old = row(&cached(&new("f:b", Outcome::Fail), Timestamp::EPOCH));
        old.machine = None;
        results::put(&cache.conn(), &old).unwrap();
        assert!(cache.lookup(&[key("f:b")])[0].is_none());
    }

    #[test]
    fn a_newer_hub_pass_replaces_a_stale_local_failure() {
        let hub = Arc::new(FakeHub::default());
        let cache = cache(&hub);
        cache.store(&[new("f:a", Outcome::Fail)]);
        // Another machine repaired it on the same inputs; only the hub knows.
        let mut pass = new("f:a", Outcome::Pass);
        pass.machine = MachineId::new("m-2").unwrap();
        hub.store(&[pass]).unwrap();
        let found = cache.lookup(&[key("f:a")]);
        assert_eq!(found[0].as_ref().unwrap().outcome, Outcome::Pass);
        // The pass is copied, so it holds while the hub is down.
        hub.set_unreachable(true);
        let found = cache.lookup(&[key("f:a")]);
        assert_eq!(found[0].as_ref().unwrap().outcome, Outcome::Pass);
    }

    #[test]
    fn a_local_failure_the_hub_has_no_answer_for_stands() {
        let hub = Arc::new(FakeHub::default());
        let cache = cache(&hub);
        cache.store(&[new("f:a", Outcome::Fail)]);
        let found = cache.lookup(&[key("f:a")]);
        assert_eq!(found[0].as_ref().unwrap().outcome, Outcome::Fail);
    }

    #[test]
    fn a_listing_is_kept_locally_and_shared_through_the_hub() {
        let hub = Arc::new(FakeHub::default());
        let key = ListingKey {
            family: "unit".to_owned(),
            hash: InputHash::new("d".repeat(64)).unwrap(),
        };
        let first = cache(&hub);
        assert_eq!(first.listings(std::slice::from_ref(&key)), [None]);
        first.store_listing(&key, "[1]");
        // A second daemon with the same hub finds it, and keeps a copy for when the hub is down.
        let second = cache(&hub);
        assert_eq!(
            second.listings(std::slice::from_ref(&key)),
            [Some("[1]".to_owned())]
        );
        hub.set_unreachable(true);
        assert_eq!(
            second.listings(std::slice::from_ref(&key)),
            [Some("[1]".to_owned())]
        );
        let third = cache(&hub);
        assert_eq!(
            third.listings(std::slice::from_ref(&key)),
            [None],
            "a down hub is a miss"
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

    #[test]
    fn a_sessions_outcomes_are_noted_with_the_first_line_of_a_run_failure_only() {
        use crate::checks::{CheckRun, Failure};
        let hub = Arc::new(FakeHub::default());
        let cache = cache(&hub);
        let failure = |name: &str, message: &str| Failure {
            check: CheckName::new(name).unwrap(),
            outcome: Outcome::Fail,
            at: None,
            message: Some(message.to_owned()),
            output: None,
            reason: None,
        };
        let run = CheckRun {
            outcomes: vec![
                (CheckName::new("rust:a").unwrap(), Outcome::Fail),
                (CheckName::new("rust:b").unwrap(), Outcome::Fail),
                (CheckName::new("rust:c").unwrap(), Outcome::Pass),
                (CheckName::new("rust:d").unwrap(), Outcome::Incomplete),
            ],
            failures: vec![
                failure("rust:a", "\nassertion failed\nmore"),
                failure("rust:b", FAILED_BEFORE),
            ],
            ..CheckRun::default()
        };
        let session = SessionId::new("local_s1").unwrap();
        let hashes = HashMap::from([(CheckName::new("rust:b").unwrap(), "h".to_owned())]);
        let head = Commit::new("abc1234").unwrap();
        cache.note_sightings(Some(&session), "status", &run, &hashes, Some(&head));
        assert!(
            cache.sightings_since(Timestamp::EPOCH).is_empty(),
            "not a sighted verb"
        );
        cache.note_sightings(Some(&session), "test", &run, &hashes, Some(&head));
        let sightings = cache.sightings_since(Timestamp::EPOCH);
        assert!(
            sightings
                .iter()
                .all(|s| s.head.as_deref() == Some(head.as_str()))
        );
        let seen: Vec<_> = sightings
            .into_iter()
            .map(|s| (s.check_name, s.outcome, s.first_line, s.hash))
            .collect();
        let text = |s: &str| s.to_owned();
        assert_eq!(
            seen,
            [
                (
                    text("rust:a"),
                    text("fail"),
                    Some(text("assertion failed")),
                    None
                ),
                (text("rust:b"), text("fail"), None, Some(text("h"))),
                (text("rust:c"), text("pass"), None, None),
            ]
        );
    }
}
