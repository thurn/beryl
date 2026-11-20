//! The dispatcher's queue in the local database, so that a restart loses no hand-off.
//!
//! Before this, a restarted daemon forgot the instructions it had queued or given, so a worker
//! that had been told to take a ticket could wait ten minutes for the hub to lapse the
//! reservation and offer the ticket again. Now every entry that decides what the dispatcher does
//! next is written to `dispatch_queue` as a JSON row whenever it changes (see [`Dispatcher::save`])
//! and read when the dispatcher is built (see [`restore`]).
//!
//! Saved: the pending and given instructions, the reservations, the hand-offs that wait for
//! another worker (`orphan`), the workers that refused (`refused`), the stalls (when a silent
//! worker was checked and messaged), the sessions whose ticket Ignite released (`released`), so a
//! restart does not let one claim again, the next instruction ID, the justiciar worker and the
//! shared failure in force (see `shared`), so a restart does not lift it. An
//! instruction that was given and not acknowledged is queued again first, so that it is given
//! again at once. Instruction IDs continue where they were, so an acknowledgement of an old ID
//! cannot be taken for a new instruction. What is not saved is rebuilt from the hub and the
//! trees: who holds a tree, the feed position and the tells already delivered.

use std::collections::{BTreeMap, VecDeque};

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::time::Timestamp;
use ignite_store::local::rows::DispatchEntry;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{Dispatcher, Queued, Reserved, Stall, State};

const PENDING: &str = "pending";
const GIVEN: &str = "given";
const RESERVED: &str = "reserved";
const ORPHAN: &str = "orphan";
const REFUSED: &str = "refused";
const STALL: &str = "stall";
const RELEASED: &str = "released";
const META: &str = "meta";

fn entry(kind: &str, key: &impl ToString, body: &impl Serialize) -> Option<DispatchEntry> {
    Some(DispatchEntry {
        kind: kind.to_owned(),
        key: key.to_string(),
        body: serde_json::to_string(body).ok()?,
    })
}

/// The rows that record `state`, in the order [`restore`] needs them.
fn entries_of(state: &State) -> Vec<DispatchEntry> {
    let mut entries = Vec::new();
    for (index, queued) in state.pending.iter().enumerate() {
        entries.extend(entry(PENDING, &index, queued));
    }
    for (id, given) in &state.given {
        entries.extend(entry(GIVEN, id, &given.queued));
    }
    for (session, reserved) in &state.reserved {
        entries.extend(entry(RESERVED, session, reserved));
    }
    for (index, reserved) in state.orphans.iter().enumerate() {
        entries.extend(entry(ORPHAN, &index, reserved));
    }
    for (session, at) in &state.refused {
        entries.extend(entry(REFUSED, session, at));
    }
    for (session, stall) in &state.stalls {
        entries.extend(entry(STALL, session, stall));
    }
    for (session, ticket) in &state.released {
        entries.extend(entry(RELEASED, session, ticket));
    }
    entries.extend(entry(META, &"next_id", &state.next_id));
    if let Some(justiciar) = &state.justiciar {
        entries.extend(entry(META, &"justiciar", justiciar));
    }
    if state.shared != super::shared::Watch::default() {
        entries.extend(entry(META, &"shared", &state.shared));
    }
    entries
}

fn read<T: DeserializeOwned>(row: &DispatchEntry) -> Option<T> {
    serde_json::from_str(&row.body).ok()
}

/// The state recorded by `entries`. A row that cannot be read is skipped: the dispatcher then
/// knows less, as it did before it saved anything, and the hub's reservations lapse as they did.
pub(super) fn restore(entries: &[DispatchEntry]) -> State {
    let mut state = State::default();
    let mut pending: VecDeque<Queued> = VecDeque::new();
    let mut given: BTreeMap<u64, Queued> = BTreeMap::new();
    for row in entries {
        match row.kind.as_str() {
            PENDING => pending.extend(read(row)),
            GIVEN => {
                if let (Ok(id), Some(queued)) = (row.key.parse::<u64>(), read::<Queued>(row)) {
                    given.insert(id, queued);
                }
            }
            RESERVED => {
                if let (Ok(session), Some(reserved)) = (SessionId::new(&row.key), read(row)) {
                    state.reserved.insert(session, reserved);
                }
            }
            ORPHAN => state.orphans.extend(read::<Reserved>(row)),
            REFUSED => {
                if let (Ok(session), Some(at)) = (SessionId::new(&row.key), read::<Timestamp>(row))
                {
                    state.refused.insert(session, at);
                }
            }
            STALL => {
                if let (Ok(session), Some(stall)) = (SessionId::new(&row.key), read::<Stall>(row)) {
                    state.stalls.insert(session, stall);
                }
            }
            RELEASED => {
                if let (Ok(session), Some(ticket)) =
                    (SessionId::new(&row.key), read::<TicketId>(row))
                {
                    state.released.insert(session, ticket);
                }
            }
            META => match row.key.as_str() {
                "next_id" => state.next_id = read(row).unwrap_or(0),
                "justiciar" => state.justiciar = read(row),
                "shared" => state.shared = read(row).unwrap_or_default(),
                _ => {}
            },
            _ => {}
        }
    }
    // An instruction given before the restart was not acknowledged to this daemon, so it is
    // given again at once, ahead of what was waiting.
    state.pending = given.into_values().chain(pending).collect();
    // No instruction may reuse an ID that is queued.
    let newest = state.pending.iter().map(|q| q.body.id).max().unwrap_or(0);
    state.next_id = state.next_id.max(newest);
    state
}

impl Dispatcher {
    /// Writes the queue to the database if it differs from what was last written. A failed write
    /// is not an error: a restart then forgets a little, as it did before the queue was saved.
    pub(super) fn save(&self) {
        let entries = entries_of(&self.state());
        let mut saved = self
            .saved
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *saved != entries && self.pool.save_queue(&entries).is_ok() {
            *saved = entries;
        }
    }
}
