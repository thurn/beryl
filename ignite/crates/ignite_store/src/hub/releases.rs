//! The `ticket_releases` table: the counted releases of each ticket. A ticket that a worker
//! cannot complete is released and returns to the pool, where the next worker can meet the same
//! obstacle. So the hub counts each release by a worker giving up or by Ignite taking the
//! ticket back (`ReleaseCause::counts`), and once [`HOLD_AFTER_SESSIONS`] different sessions
//! have released a ticket it is held back: the dispatch queue no longer offers it, and a
//! justiciar ticket asks for the ticket to be fixed. Changing the ticket's body or acceptance,
//! or a person releasing it again, forgets its releases ([`reset`]).

use std::collections::HashSet;

use rusqlite::{Connection, params};

use ignite_core::ids::TicketId;
use ignite_core::protocol::hub::ReleaseCause;
use ignite_core::time::Timestamp;

use crate::Result;

/// How many different sessions must have released a ticket before it is held back.
pub const HOLD_AFTER_SESSIONS: usize = 2;

/// One counted release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counted {
    pub session: String,
    pub cause: String,
    pub note: String,
    pub at: String,
}

/// A ticket's counted releases, oldest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Releases {
    pub releases: Vec<Counted>,
}

impl Releases {
    /// The sessions that released the ticket, each once, in the order of their first release.
    #[must_use]
    pub fn sessions(&self) -> Vec<&str> {
        let mut seen = HashSet::new();
        self.releases
            .iter()
            .map(|release| release.session.as_str())
            .filter(|session| seen.insert(*session))
            .collect()
    }

    /// Whether enough different sessions released the ticket for it to be held back.
    #[must_use]
    pub fn held_back(&self) -> bool {
        self.sessions().len() >= HOLD_AFTER_SESSIONS
    }

    /// Why the ticket is held back, as `ignite ticket show` and the dashboard say it, or
    /// `None` while it is not.
    #[must_use]
    pub fn reason(&self) -> Option<String> {
        self.held_back().then(|| {
            format!(
                "released {} times by {} sessions ({}), so the pool is not offered it until its \
                 body or acceptance changes or a person releases it again",
                self.releases.len(),
                self.sessions().len(),
                self.sessions().join(", ")
            )
        })
    }
}

/// Records a counted release of `ticket` by `session` and returns the ticket's releases with it.
pub fn record(
    conn: &Connection,
    ticket: &str,
    session: &str,
    cause: ReleaseCause,
    note: &str,
    now: Timestamp,
) -> Result<Releases> {
    conn.execute(
        "INSERT INTO ticket_releases (ticket, n, session, cause, note, at)
         VALUES (?1, (SELECT coalesce(max(n), 0) + 1 FROM ticket_releases WHERE ticket = ?1),
                 ?2, ?3, ?4, ?5)",
        params![ticket, session, cause.as_str(), note, now.to_string()],
    )?;
    of(conn, ticket)
}

/// The counted releases of `ticket`, oldest first.
pub fn of(conn: &Connection, ticket: &str) -> Result<Releases> {
    let mut statement = conn.prepare(
        "SELECT session, cause, note, at FROM ticket_releases WHERE ticket = ?1 ORDER BY n",
    )?;
    let releases = statement
        .query_map([ticket], |row| {
            Ok(Counted {
                session: row.get(0)?,
                cause: row.get(1)?,
                note: row.get(2)?,
                at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Releases { releases })
}

/// Forgets the releases of `ticket`, so it is offered again. Returns whether it had any.
pub fn reset(conn: &Connection, ticket: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM ticket_releases WHERE ticket = ?1", [ticket])? > 0)
}

/// The tickets that are held back.
pub fn held_back(conn: &Connection) -> Result<HashSet<String>> {
    let mut statement = conn.prepare(&format!(
        "SELECT ticket FROM ticket_releases GROUP BY ticket \
         HAVING count(DISTINCT session) >= {HOLD_AFTER_SESSIONS}"
    ))?;
    let held = statement
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(held)
}

/// `tickets` without the ones that are held back.
pub fn offerable(conn: &Connection, mut tickets: Vec<TicketId>) -> Result<Vec<TicketId>> {
    let held = held_back(conn)?;
    tickets.retain(|ticket| !held.contains(ticket.as_str()));
    Ok(tickets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Timestamp {
        Timestamp::parse("2026-11-20T06:00:00Z").unwrap()
    }

    #[test]
    fn two_sessions_hold_a_ticket_back_and_one_session_twice_does_not() {
        let conn = crate::hub::open_in_memory().unwrap();
        let gave_up = ReleaseCause::GaveUp;
        record(&conn, "TA", "local_a", gave_up, "first", now()).unwrap();
        let again = record(&conn, "TA", "local_a", gave_up, "again", now()).unwrap();
        assert!(!again.held_back());
        assert!(held_back(&conn).unwrap().is_empty());
        let second = record(&conn, "TA", "local_b", ReleaseCause::TakenBack, "x", now()).unwrap();
        assert!(second.held_back());
        assert_eq!(second.sessions(), ["local_a", "local_b"]);
        assert_eq!(held_back(&conn).unwrap(), HashSet::from(["TA".to_owned()]));
        assert!(reset(&conn, "TA").unwrap());
        assert!(held_back(&conn).unwrap().is_empty());
        assert!(!reset(&conn, "TA").unwrap());
    }
}
