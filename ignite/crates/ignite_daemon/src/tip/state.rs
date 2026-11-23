//! What the tip verifier remembers between rounds, kept in the local database so that a restart
//! of the daemon does not repeat reports, end a hold-off or start the platform wait over.
//!
//! The memory is one JSON row (`tip_state`). It is loaded when the verifier is built and saved
//! after each report and at the end of each round. A row from an older build that lacks a field
//! loads with that field empty, and a row that cannot be read is ignored, so the verifier starts
//! as it did before it kept anything. Maps and sets are stored as lists so that keys that are
//! not strings fit in JSON.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;
use std::sync::{Mutex, PoisonError};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit, Handle, Seq};
use ignite_core::protocol::hub::{ResultKey, Reverted};
use ignite_core::time::Timestamp;
use ignite_store::local::tip_state;

use super::TipVerifier;
use crate::server::Context;

/// The fields of [`TipVerifier`] that outlive a round, as saved. Every field has a default, so
/// a field added later never makes an older row unreadable.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Memory {
    since: Seq,
    settled: Option<Commit>,
    failed: Option<FailedTip>,
    fleet_version: Option<u64>,
    reported: Vec<ResultKey>,
    deferred: Vec<CheckName>,
    waiting_since: Vec<(Os, Timestamp)>,
    flagged: Vec<(Commit, Os)>,
    held_off: Vec<(ResultKey, Timestamp)>,
    incomplete_runs: Vec<(ResultKey, u32)>,
    reported_incomplete: Vec<ResultKey>,
    reported_stuck: Vec<(Commit, CheckName)>,
    rechecked: Vec<ResultKey>,
    resting: Option<(Commit, Timestamp)>,
    unsent_reverts: Vec<Reverted>,
}

/// The round publishes this after recording the failure, before it starts any culprit search.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct FailedTip {
    pub commit: Commit,
    pub checks: Vec<ResultKey>,
    /// Identifies the round the loop can stop. A later round at the same SHA has its own handle.
    pub round: Option<Handle>,
}

/// The database the memory is saved in, and the text last saved, so that a round that changed
/// nothing writes nothing.
pub(super) struct Saved {
    /// Behind a lock because a round's families run on scoped threads that share the verifier.
    conn: Mutex<Connection>,
    last: Mutex<Option<String>>,
}

/// The items in a stable order, so that equal memories save as equal text.
fn listed<T: Serialize>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    let mut items: Vec<T> = items.into_iter().collect();
    items.sort_by_cached_key(|item| serde_json::to_string(item).unwrap_or_default());
    items
}

fn set<T: Hash + Eq>(items: Vec<T>) -> HashSet<T> {
    items.into_iter().collect()
}

impl TipVerifier {
    /// Keeps the verifier's memory in `conn`, starting from what an earlier verifier saved.
    #[must_use]
    pub fn with_memory(mut self, conn: Connection) -> Self {
        let last = match tip_state::get(&conn) {
            Ok(Some(row)) => {
                match serde_json::from_str::<Memory>(&row.body) {
                    Ok(memory) => self.restore(memory),
                    Err(error) => {
                        eprintln!("ignite: the tip verifier's memory is unreadable: {error}");
                    }
                }
                Some(row.body)
            }
            Ok(None) => None,
            Err(error) => {
                eprintln!("ignite: the tip verifier's memory was not loaded: {error}");
                None
            }
        };
        self.saved = Some(Saved {
            conn: Mutex::new(conn),
            last: Mutex::new(last),
        });
        self
    }

    pub(super) fn fleet_notice(&self) -> Option<ignite_core::fleet::FleetNotice> {
        let saved = self.saved.as_ref()?;
        let conn = saved.conn.lock().unwrap_or_else(PoisonError::into_inner);
        crate::fleet_notice::read(&conn)
    }

    pub(super) fn machine_is_off(&self) -> bool {
        self.fleet_notice()
            .is_some_and(|notice| notice.state == ignite_core::fleet::MachineState::Off)
    }

    pub(super) fn require_machine_on(&self) -> ignite_core::traits::Result<()> {
        if self.machine_is_off() {
            return Err(ignite_core::traits::Error::Conflict(
                "tip verification waits while this machine is off".to_owned(),
            ));
        }
        Ok(())
    }

    fn restore(&mut self, memory: Memory) {
        self.since = memory.since;
        self.settled = memory.settled;
        self.failed = memory.failed;
        self.fleet_version = memory.fleet_version;
        self.reported = set(memory.reported);
        self.deferred = set(memory.deferred);
        self.waiting_since = memory.waiting_since.into_iter().collect::<BTreeMap<_, _>>();
        self.flagged = set(memory.flagged);
        self.held_off = memory.held_off.into_iter().collect::<HashMap<_, _>>();
        self.incomplete_runs = memory
            .incomplete_runs
            .into_iter()
            .collect::<HashMap<_, _>>();
        self.reported_incomplete = set(memory.reported_incomplete);
        self.reported_stuck = set(memory.reported_stuck);
        self.rechecked = set(memory.rechecked);
        self.resting = memory.resting;
        self.unsent_reverts = memory.unsent_reverts;
    }

    fn memory(&self) -> Memory {
        Memory {
            since: self.since,
            settled: self.settled.clone(),
            failed: self.failed.clone(),
            fleet_version: self.fleet_version,
            reported: listed(self.reported.iter().cloned()),
            deferred: listed(self.deferred.iter().cloned()),
            waiting_since: self.waiting_since.iter().map(|(o, t)| (*o, *t)).collect(),
            flagged: listed(self.flagged.iter().cloned()),
            held_off: listed(self.held_off.iter().map(|(k, t)| (k.clone(), *t))),
            incomplete_runs: listed(self.incomplete_runs.iter().map(|(k, n)| (k.clone(), *n))),
            reported_incomplete: listed(self.reported_incomplete.iter().cloned()),
            reported_stuck: listed(self.reported_stuck.iter().cloned()),
            rechecked: listed(self.rechecked.iter().cloned()),
            resting: self.resting.clone(),
            unsent_reverts: self.unsent_reverts.clone(),
        }
    }

    /// Saves the memory when it changed since the last save. A failure is logged and never
    /// fails a round: the worst it costs is a repeated report after a restart.
    pub(super) fn save(&mut self, ctx: &Context) {
        if self.saved.is_none() {
            return;
        }
        let body = match serde_json::to_string(&self.memory()) {
            Ok(body) => body,
            Err(error) => {
                eprintln!("ignite: the tip verifier's memory was not saved: {error}");
                return;
            }
        };
        let Some(saved) = self.saved.as_ref() else {
            return;
        };
        let mut last = saved.last.lock().unwrap_or_else(PoisonError::into_inner);
        if last.as_deref() == Some(body.as_str()) {
            return;
        }
        let at = String::from(ctx.clock.now());
        let conn = saved.conn.lock().unwrap_or_else(PoisonError::into_inner);
        match tip_state::put(&conn, &body, &at) {
            Ok(()) => *last = Some(body),
            Err(error) => eprintln!("ignite: the tip verifier's memory was not saved: {error}"),
        }
    }
}
