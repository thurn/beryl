//! Saves CLI reservations and worker state across daemon restarts.

use std::collections::BTreeMap;

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::hub::Reservation;
use ignite_core::time::Timestamp;
use ignite_store::local::pool as store;
use serde::{Deserialize, Serialize};

use super::{Dispatcher, Pool, Reserved, Stall, State, cli, shared};

const KEY: &str = "cli_dispatch_state";

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Snapshot {
    reserved: BTreeMap<SessionId, Reserved>,
    unread: Vec<(Reservation, String)>,
    refused: BTreeMap<SessionId, Timestamp>,
    stalls: BTreeMap<SessionId, Stall>,
    released: BTreeMap<SessionId, TicketId>,
    waiting: BTreeMap<SessionId, Timestamp>,
    tells: BTreeMap<TicketId, u64>,
    starts: Vec<cli::Call>,
    justiciar: Option<(SessionId, Timestamp)>,
    shared: shared::Watch,
}

pub(super) fn load(pool: &Pool) -> State {
    let snapshot = pool.with_database(|conn| match store::setting(conn, KEY) {
        Ok(Some(value)) => serde_json::from_str(&value).unwrap_or_default(),
        Ok(None) => legacy_snapshot(&store::legacy_dispatch_state(conn).unwrap_or_default()),
        Err(error) => {
            eprintln!("ignite: could not read CLI worker state: {error}");
            Snapshot::default()
        }
    });
    State {
        reserved: snapshot.reserved,
        unread: snapshot.unread,
        refused: snapshot.refused,
        stalls: snapshot.stalls,
        released: snapshot.released,
        waiting: snapshot.waiting,
        tells: snapshot.tells,
        cli_calls: snapshot.starts,
        justiciar: snapshot.justiciar,
        shared: snapshot.shared,
        ..State::default()
    }
}

/// Carries over CLI work from the last release that used the instruction queue.
fn legacy_snapshot(rows: &[ignite_store::local::rows::DispatchEntry]) -> Snapshot {
    fn insert_cli<T: serde::de::DeserializeOwned>(
        map: &mut BTreeMap<SessionId, T>,
        row: &ignite_store::local::rows::DispatchEntry,
    ) {
        if let (Ok(session), Ok(value)) =
            (SessionId::new(&row.key), serde_json::from_str(&row.body))
            && (cli::is_cli_session(&session) || cli::is_cli_slot(&session))
        {
            map.insert(session, value);
        }
    }
    let mut snapshot = Snapshot::default();
    for row in rows {
        match row.kind.as_str() {
            "reserved" => insert_cli(&mut snapshot.reserved, row),
            "refused" => insert_cli(&mut snapshot.refused, row),
            "stall" => insert_cli(&mut snapshot.stalls, row),
            "released" => insert_cli(&mut snapshot.released, row),
            "tell" => {
                if let (Ok(ticket), Ok(number)) =
                    (TicketId::new(&row.key), serde_json::from_str(&row.body))
                {
                    snapshot.tells.insert(ticket, number);
                }
            }
            "cli_begin" => snapshot
                .starts
                .extend(serde_json::from_str::<cli::Call>(&row.body)),
            "meta" if row.key == "shared" => {
                snapshot.shared = serde_json::from_str(&row.body).unwrap_or_default();
            }
            "meta" if row.key == "justiciar" => {
                snapshot.justiciar = serde_json::from_str::<(SessionId, Timestamp)>(&row.body)
                    .ok()
                    .filter(|(session, _)| cli::is_cli_session(session));
            }
            _ => {}
        }
    }
    snapshot
}

impl Dispatcher {
    /// Saves pending starts before launch. Replaying one after a crash is safe because the CLI
    /// supervisor looks up the reservation code and returns the worker it already started.
    pub(super) fn save(&self) {
        let mut saved = self
            .saved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let holders = self.host.holders();
        let snapshot = {
            let state = self.state();
            Snapshot {
                reserved: state.reserved.clone(),
                unread: state.unread.clone(),
                refused: state.refused.clone(),
                stalls: state.stalls.clone(),
                released: state.released.clone(),
                waiting: state.waiting.clone(),
                tells: state
                    .tells
                    .iter()
                    .filter(|(ticket, _)| holders.values().any(|held| held == *ticket))
                    .map(|(ticket, number)| (ticket.clone(), *number))
                    .collect(),
                starts: state
                    .begins_running
                    .iter()
                    .chain(&state.cli_calls)
                    .filter(|call| matches!(call, cli::Call::Begin { .. }))
                    .cloned()
                    .collect(),
                justiciar: state.justiciar.clone(),
                shared: state.shared.clone(),
            }
        };
        let Ok(value) = serde_json::to_string(&snapshot) else {
            return;
        };
        if saved.as_deref() != Some(value.as_str())
            && self
                .pool
                .with_database(|conn| store::set_setting(conn, KEY, Some(&value)))
                .is_ok()
        {
            *saved = Some(value);
        }
    }
}
