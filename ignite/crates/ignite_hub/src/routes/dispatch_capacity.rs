//! Account for claims and reservations together when admitting another ticket.

use rusqlite::Connection;

use ignite_core::admission::Worked;
use ignite_core::dispatch::ReadyTicket;
use ignite_core::dispatch_pressure::{Budget, DispatchPressure, HandoffWait};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet, tickets};

use super::{Candidate, expected_cores, named_files};

pub(super) struct Capacity<'a> {
    pub cores: Vec<f64>,
    pub disk: Option<Budget<'a>>,
}

impl Capacity<'_> {
    /// Try the usual dispatch order, falling through to smaller tickets that still fit.
    pub(super) fn choose(
        &self,
        ready: &mut Vec<ReadyTicket>,
        pick: impl Fn(&[ReadyTicket]) -> Option<Candidate>,
    ) -> Option<std::result::Result<Candidate, HandoffWait>> {
        let preferred = pick(ready)?;
        if let Some(budget) = &self.disk
            && let Some(reason) = ready
                .iter()
                .find(|ticket| ticket.id == preferred.ticket)
                .and_then(|ticket| budget.blocked(ticket))
        {
            ready.retain(|ticket| budget.blocked(ticket).is_none());
            return Some(pick(ready).ok_or(reason));
        }
        Some(Ok(preferred))
    }
}

pub(super) fn read<'a>(
    conn: &Connection,
    machine: &str,
    history: &[Worked],
    landed: &[String],
    now: Timestamp,
    pressure: Option<&'a DispatchPressure>,
) -> Result<Capacity<'a>> {
    let mut statement = conn.prepare(
        "SELECT id, claimed_at, NULL FROM tickets WHERE status = 'in_progress' AND machine = ?1
         UNION ALL
         SELECT ticket, NULL, expires_at FROM reservations WHERE machine = ?1 AND expires_at > ?2
         ORDER BY 2 DESC NULLS LAST, 3, 1",
    )?;
    let held = statement.query_map((machine, now.to_string()), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut capacity = Capacity {
        cores: Vec::new(),
        disk: pressure.map(|pressure| Budget::new(pressure, now)),
    };
    for row in held {
        let (id, claimed, expires) = row?;
        if let Some(ticket) = tickets::load(conn, &id)? {
            let files = named_files(&format!("{}\n{}", ticket.body, ticket.acceptance));
            let cores = expected_cores(&ticket, &files, history, landed, now);
            capacity.cores.push(cores);
            if let Some(budget) = capacity.disk.as_mut() {
                let since = claimed
                    .as_deref()
                    .and_then(|text| Timestamp::parse(text).ok())
                    .or_else(|| {
                        expires
                            .as_deref()
                            .and_then(|text| Timestamp::parse(text).ok())
                            .map(|expiry| expiry.plus_seconds(-fleet::RESERVATION_SECONDS))
                    });
                budget.hold(&ticket.id, cores, &files, since);
            }
        }
    }
    Ok(capacity)
}
