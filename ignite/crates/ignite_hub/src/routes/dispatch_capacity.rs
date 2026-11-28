//! Account for claims and reservations together when admitting another ticket.

use rusqlite::Connection;

use ignite_core::admission::Worked;
use ignite_core::dispatch::ReadyTicket;
use ignite_core::dispatch_pressure::{Budget, HandoffWait};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet, tickets};

use super::{Candidate, Requirements, expected_cores, named_files, resolved_files};

pub(super) struct Capacity<'a> {
    pub cores: Vec<f64>,
    pub budget: Budget<'a>,
    /// Each ticket counted against the build-ticket limit, with when it was claimed or
    /// reserved and whether it is claimed rather than only reserved.
    pub builds: Vec<(String, Option<Timestamp>, bool)>,
}

impl Capacity<'_> {
    /// Try dispatch order, falling through past tickets that wait for a build slot or another machine.
    pub(super) fn choose<'a>(
        &self,
        machine: &str,
        candidates: impl Iterator<Item = (&'a ReadyTicket, Candidate)>,
        defer: impl Fn(&ReadyTicket, &Candidate) -> Result<Option<String>>,
    ) -> Result<Option<std::result::Result<Candidate, HandoffWait>>> {
        let mut first_wait = None;
        for (ticket, candidate) in candidates {
            if let Some(reason) = self.budget.blocked(ticket, machine) {
                first_wait.get_or_insert(reason);
            } else if let Some(reason) = defer(ticket, &candidate)? {
                first_wait.get_or_insert_with(|| reason.into());
            } else {
                return Ok(Some(Ok(candidate)));
            }
        }
        Ok(first_wait.map(Err))
    }
}

pub(super) fn read<'a>(
    conn: &Connection,
    machine: &str,
    history: &[Worked],
    landed: &[String],
    now: Timestamp,
    requirements: &Requirements,
    budget: Budget<'a>,
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
        budget,
        builds: Vec::new(),
    };
    for row in held {
        let (id, claimed, expires) = row?;
        if let Some(ticket) = tickets::load(conn, &id)? {
            let files = named_files(&format!("{}\n{}", ticket.body, ticket.acceptance));
            let files = resolved_files(&files, landed);
            let cores = expected_cores(&ticket, &files, history, now);
            capacity.cores.push(cores);
            let since = claimed
                .as_deref()
                .and_then(|text| Timestamp::parse(text).ok())
                .or_else(|| {
                    expires
                        .as_deref()
                        .and_then(|text| Timestamp::parse(text).ok())
                        .map(|expiry| expiry.plus_seconds(-fleet::RESERVATION_SECONDS))
                });
            let build = requirements.needs_build_slot(
                ticket.id.as_str(),
                ticket.ticket_type,
                ticket.needs_os,
                cores,
                &files,
            );
            if capacity.budget.hold(&ticket.id, build, since) {
                capacity.builds.push((id, since, claimed.is_some()));
            }
        }
    }
    Ok(capacity)
}
