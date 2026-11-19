//! The ticket mirror: every daemon keeps an append-only copy of the hub's tickets in its own
//! database, `mirror.db` beside `daemon.db`, so that losing the hub's machine, or a bad edit
//! made through the hub's API, never loses a ticket or its earlier state.
//!
//! The mirror follows the hub on a loop of its own (`ignite-mirror`), apart from the
//! dispatcher's feed loop, so the dispatcher stays as it was and a slow first copy never holds
//! up a hand-off:
//!
//! 1. **First copy.** An empty mirror reads the hub's feed position, then pages through
//!    `GET /export` one page a round and stores every ticket at that position.
//! 2. **Following.** It long-polls `GET /feed` from its position, fetches each ticket a
//!    `ticket` change names with `GET /export?id=...`, and stores it.
//! 3. **Append only.** A ticket is stored only when it differs from its newest stored version,
//!    leaving aside `version`, the hub's feed position of its last change. Nothing stored is
//!    changed or removed (the store's triggers refuse it).
//! 4. **A restored hub.** When the hub's feed position drops below the mirror's, or the hub
//!    says the mirror's position is older than its journal, the mirror keeps every version it
//!    has and copies the hub again from the start, adding versions only where they differ.
//!
//! [`restore`] reads the mirror back for `ignite hub restore --from-mirror`.

pub mod restore;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use rusqlite::Connection;

use ignite_core::ids::{Seq, TicketId};
use ignite_core::protocol::tickets::WholeTicket;
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Error, Hub, Result};
use ignite_store::mirror;

/// The mirror's file in the Ignite home.
pub const FILE: &str = "mirror.db";

/// The most tickets one `GET /export?id=...` names, as the hub allows.
const PAGE: usize = 200;

/// The keys of the follower's state.
const FEED: &str = "feed";
const COPY_SEQ: &str = "copy_seq";
const COPY_AFTER: &str = "copy_after";
const CAUGHT_UP_AT: &str = "caught_up_at";
const STARTED_AT: &str = "started_at";

/// What one round did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Versions stored.
    pub stored: usize,
    /// Whether a copy is under way, so the next round should start at once.
    pub copying: bool,
}

/// Where a mirror is, for `ignite doctor`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The feed position it follows from; `None` before the first copy has finished.
    pub feed: Option<Seq>,
    /// The feed position a copy under way stores its tickets at.
    pub copying: Option<Seq>,
    /// The last round that read every change the hub had.
    pub caught_up_at: Option<Timestamp>,
    /// When the mirror was started.
    pub started_at: Option<Timestamp>,
    pub versions: u64,
    pub tickets: u64,
}

/// One machine's mirror.
pub struct Mirror {
    conn: Connection,
    clock: Arc<dyn Clock>,
}

impl Mirror {
    /// Opens, or creates, the mirror at `path`.
    pub fn open(path: &Path, clock: Arc<dyn Clock>) -> Result<Mirror> {
        Ok(Mirror {
            conn: mirror::open(path).map_err(|e| store(&e))?,
            clock,
        })
    }

    /// A mirror held in memory, for tests.
    pub fn in_memory(clock: Arc<dyn Clock>) -> Result<Mirror> {
        Ok(Mirror {
            conn: mirror::open_in_memory().map_err(|e| store(&e))?,
            clock,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// One round: a page of a copy under way, or one long poll of the feed of up to `wait_s`
    /// seconds and the tickets it names.
    pub fn round(&mut self, hub: &dyn Hub, wait_s: u32) -> Result<Progress> {
        if let Some(seq) = self.seq_state(COPY_SEQ)? {
            return self.copy_page(hub, seq);
        }
        let Some(since) = self.seq_state(FEED)? else {
            // An empty mirror: note where the feed is before reading anything, so a change made
            // during the copy is read again from the feed afterwards.
            let batch = hub.feed(Seq(u64::MAX), 0)?;
            let now = self.now();
            self.set(STARTED_AT, Some(&now))?;
            self.start_copy(batch.seq)?;
            return Ok(Progress {
                stored: 0,
                copying: true,
            });
        };
        let batch = hub.feed(since, wait_s)?;
        let resync = batch
            .changes
            .iter()
            .any(|change| change.kind == ignite_store::hub::feed::RESYNC);
        if batch.seq < since || resync {
            eprintln!(
                "ignite: the hub's feed went back from position {} to {}; copying its tickets \
                 into the mirror again",
                since.0, batch.seq.0
            );
            self.start_copy(batch.seq)?;
            return Ok(Progress {
                stored: 0,
                copying: true,
            });
        }
        // Each ticket at the position of its last change in this batch.
        let mut changed: BTreeMap<TicketId, Seq> = BTreeMap::new();
        for change in &batch.changes {
            if change.kind == "ticket"
                && let Some(ticket) = &change.ticket
            {
                changed.insert(ticket.clone(), change.seq);
            }
        }
        let ids: Vec<TicketId> = changed.keys().cloned().collect();
        let mut fetched = Vec::new();
        // The hub cuts a response short once it is large, so ask again for the tickets after
        // the last one received. Tickets that no longer exist are skipped, so an empty reply
        // means none of the rest exist.
        for chunk in ids.chunks(PAGE) {
            let mut rest = chunk;
            while !rest.is_empty() {
                let page = hub.export(None, rest)?;
                let Some(last) = page.last() else { break };
                let position = rest.iter().position(|id| *id == last.ticket.id);
                fetched.extend(page);
                let Some(position) = position else { break };
                rest = &rest[position + 1..];
            }
        }
        let now = self.now();
        // A reply holds at most a set number of changes; the position is that of the last one
        // read, so the next round reads the rest.
        let position = batch.changes.last().map_or(batch.seq, |change| change.seq);
        let transaction = self.conn.transaction().map_err(sqlite)?;
        let mut stored = 0;
        for whole in &fetched {
            let seq = changed.get(&whole.ticket.id).copied().unwrap_or(batch.seq);
            stored += usize::from(keep(&transaction, whole, seq, &now)?);
        }
        set(&transaction, FEED, Some(&position.0.to_string()))?;
        if position >= batch.seq {
            set(&transaction, CAUGHT_UP_AT, Some(&now))?;
        }
        transaction.commit().map_err(sqlite)?;
        Ok(Progress {
            stored,
            copying: false,
        })
    }

    /// Stores one page of the copy at `seq`, or ends the copy when the hub has no more.
    fn copy_page(&mut self, hub: &dyn Hub, seq: Seq) -> Result<Progress> {
        let after = self
            .get(COPY_AFTER)?
            .filter(|after| !after.is_empty())
            .map(TicketId::new)
            .transpose()
            .map_err(|e| Error::Invalid(format!("the mirror's copy position: {e}")))?;
        let page = hub.export(after.as_ref(), &[])?;
        let now = self.now();
        let transaction = self.conn.transaction().map_err(sqlite)?;
        let mut stored = 0;
        for whole in &page {
            stored += usize::from(keep(&transaction, whole, seq, &now)?);
        }
        let copying = if let Some(last) = page.last() {
            set(&transaction, COPY_AFTER, Some(last.ticket.id.as_str()))?;
            true
        } else {
            set(&transaction, FEED, Some(&seq.0.to_string()))?;
            set(&transaction, COPY_SEQ, None)?;
            set(&transaction, COPY_AFTER, None)?;
            false
        };
        transaction.commit().map_err(sqlite)?;
        Ok(Progress { stored, copying })
    }

    fn start_copy(&self, seq: Seq) -> Result<()> {
        self.set(COPY_SEQ, Some(&seq.0.to_string()))?;
        self.set(COPY_AFTER, Some(""))
    }

    fn now(&self) -> String {
        self.clock.now().to_string()
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        mirror::state(&self.conn, key).map_err(|e| store(&e))
    }

    fn set(&self, key: &str, value: Option<&str>) -> Result<()> {
        set(&self.conn, key, value)
    }

    fn seq_state(&self, key: &str) -> Result<Option<Seq>> {
        seq_state(&self.conn, key)
    }
}

/// Where the mirror in `conn` is.
pub fn status(conn: &Connection) -> Result<Status> {
    let time = |key: &str| -> Result<Option<Timestamp>> {
        Ok(mirror::state(conn, key)
            .map_err(|e| store(&e))?
            .and_then(|text| Timestamp::parse(&text).ok()))
    };
    let (versions, tickets) = mirror::counts(conn).map_err(|e| store(&e))?;
    Ok(Status {
        feed: seq_state(conn, FEED)?,
        copying: seq_state(conn, COPY_SEQ)?,
        caught_up_at: time(CAUGHT_UP_AT)?,
        started_at: time(STARTED_AT)?,
        versions,
        tickets,
    })
}

/// Appends `whole` unless it equals the newest stored version of its ticket; whether it did.
fn keep(conn: &Connection, whole: &WholeTicket, seq: Seq, now: &str) -> Result<bool> {
    let id = whole.ticket.id.as_str();
    if let Some(latest) = mirror::latest(conn, id).map_err(|e| store(&e))?
        && serde_json::from_str::<WholeTicket>(&latest.whole_json)
            .is_ok_and(|stored| same(&stored, whole))
    {
        return Ok(false);
    }
    let json = serde_json::to_string(whole).map_err(|e| Error::Invalid(e.to_string()))?;
    mirror::append(conn, id, now, seq.0, &json).map_err(|e| store(&e))?;
    Ok(true)
}

/// Whether two copies of a ticket hold the same content. `version`, the feed position of the
/// ticket's last change, is left out: a hub restored from a snapshot, or one that imported the
/// ticket, numbers the same content anew.
fn same(a: &WholeTicket, b: &WholeTicket) -> bool {
    let mut a = a.clone();
    a.ticket.version = b.ticket.version;
    a == *b
}

fn set(conn: &Connection, key: &str, value: Option<&str>) -> Result<()> {
    mirror::set_state(conn, key, value).map_err(|e| store(&e))
}

fn seq_state(conn: &Connection, key: &str) -> Result<Option<Seq>> {
    mirror::state(conn, key)
        .map_err(|e| store(&e))?
        .map(|text| {
            text.parse::<u64>()
                .map(Seq)
                .map_err(|e| Error::Invalid(format!("the mirror's {key}: {e}")))
        })
        .transpose()
}

fn store(error: &ignite_store::StoreError) -> Error {
    Error::Io(format!("ticket mirror: {error}"))
}

fn sqlite(error: rusqlite::Error) -> Error {
    store(&error.into())
}

#[cfg(test)]
#[path = "mirror_tests.rs"]
mod tests;
