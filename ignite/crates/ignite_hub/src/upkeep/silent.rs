//! A ticket for a person when a machine that should be working stops reporting.
//!
//! On 2026-11-25 the drive holding a Windows machine's trees stopped answering. Its daemon hung,
//! its heartbeats stopped, and nothing in the fleet said so: people noticed only because
//! `master` stopped moving, and a person had to turn the machine off and reboot it. The existing
//! machine ticket in `stuck.rs` is filed only after a tip has waited an hour, and tip
//! verification now defers a platform whose machines have all gone silent
//! ([`fleet_settings::unavailable_platforms`]), so a tip may never wait that long.
//!
//! [`machines_silent`] files a P0 ticket labelled `needs:person`, which the dashboard shows as an
//! attention card, for each machine that is not set off and has sent no heartbeat for
//! [`fleet_settings::ONLINE_SECONDS`]. The ticket closes by itself once the machine has reported
//! again or been set off ([`verdict`], called from `clearing_kinds.rs`). It has a signature of
//! its own rather than sharing the one in `stuck.rs`: a new sighting moves a ticket's clearing
//! time, and silence is not what that ticket waits on.

use std::collections::BTreeMap;

use rusqlite::Connection;

use ignite_core::fleet::{FleetSettings, MachineState};
use ignite_core::pins::machine_label;
use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::{fleet, fleet_settings, rows};

use super::clearing_kinds::Verdict;
use super::filing::{self, Filed, Filing, UPKEEP};
use super::parse_time;
use super::signature::Signature;

/// The signature category of these tickets, whose subject is the machine's ID.
pub const CATEGORY: &str = "silent";

/// A machine silent for longer than this is taken to be retired, not failing, and files nothing.
pub const RETIRED_SECONDS: f64 = 24.0 * 3_600.0;

/// Files one ticket for each machine that is not off and has been silent for between
/// [`fleet_settings::ONLINE_SECONDS`] and [`RETIRED_SECONDS`].
pub fn machines_silent(conn: &mut Connection, now: Timestamp) -> Result<Vec<Filed>> {
    let settings = fleet_settings::settings(conn)?;
    // A machine registered again under a new ID keeps its old row, so only the row that
    // reported last speaks for a name.
    let mut latest: BTreeMap<String, rows::Machine> = BTreeMap::new();
    for machine in fleet::machines(conn)? {
        let newer = latest
            .get(&machine.name)
            .is_none_or(|seen| seen.last_seen < machine.last_seen);
        if newer {
            latest.insert(machine.name.clone(), machine);
        }
    }
    let mut filed = Vec::new();
    for machine in latest.into_values() {
        if settings.machine(&machine.name).state == MachineState::Off {
            continue;
        }
        let silent_s = now.seconds_since(parse_time(&machine.last_seen)?);
        if !(fleet_settings::ONLINE_SECONDS..RETIRED_SECONDS).contains(&silent_s) {
            continue;
        }
        let body = format!(
            "Machine {name} ({id}) has sent no heartbeat since {last_seen}, {minutes:.0} minutes \
             ago, and it is not set off. Its daemon is hung or stopped, or the machine itself is \
             down. Until it reports again, tip verification defers checks that only its \
             platform can run if no other machine of that platform is reporting, and after two \
             hours the hub gives its claimed tickets back to the queue.\n\n\
             What to do: reach the machine (over SSH, or in person) and run `ignite doctor`. If \
             the machine does not answer, or a drive holding its trees does not answer, reboot \
             it. Set it `off` (`ignite fleet machine {name} --state off`) if it will stay down. \
             This ticket closes by itself an hour after the machine reports again.",
            name = machine.name,
            id = machine.id,
            last_seen = machine.last_seen,
            minutes = silent_s / 60.0,
        );
        let filing = Filing {
            signature: Signature::of_subject(&machine.id, CATEGORY),
            evidence: machine.last_seen.clone(),
            fields: TicketFields {
                title: Some(format!("Machine {} has stopped reporting", machine.name)),
                body: Some(body),
                ticket_type: Some(TicketType::Bug),
                priority: Some(0),
                labels: Some(vec![
                    "fleet".to_owned(),
                    "needs:person".to_owned(),
                    machine_label(&machine.name),
                ]),
                ..TicketFields::default()
            },
            deps: Vec::new(),
            released: true,
        };
        filed.push(filing::file(conn, UPKEEP, &filing, now)?);
    }
    Ok(filed)
}

/// Whether the machine `id` is still silent since the ticket last saw it at `sighted`.
pub fn verdict(
    machines: &[rows::Machine],
    settings: &FleetSettings,
    id: &str,
    sighted: Timestamp,
) -> Verdict {
    let Some(machine) = machines.iter().find(|machine| machine.id == id) else {
        return Verdict::Cleared(format!("machine {id} is no longer registered."));
    };
    if settings.machine(&machine.name).state == MachineState::Off {
        return Verdict::Cleared(format!("machine {} was set off.", machine.name));
    }
    match parse_time(&machine.last_seen) {
        Ok(seen) if seen > sighted => Verdict::Cleared(format!(
            "machine {} reported at {}.",
            machine.name, machine.last_seen
        )),
        _ => Verdict::Present,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upkeep::filing::Change;
    use ignite_core::config::Os;
    use ignite_core::ids::MachineId;
    use ignite_core::protocol::hub::MachineRegister;
    use ignite_core::protocol::tickets::Status;
    use ignite_store::hub::{self, tickets};

    fn at(seconds: f64) -> Timestamp {
        Timestamp::parse("2026-11-25T05:58:00Z")
            .unwrap()
            .plus_seconds(seconds)
    }

    fn register(conn: &Connection, id: &str, name: &str, now: Timestamp) {
        let register = MachineRegister {
            machine: MachineId::new(id).unwrap(),
            name: name.to_owned(),
            os: Os::Windows,
            cores: 12,
            memory_mb: 32_768,
            families: Vec::new(),
            version: "v".to_owned(),
        };
        fleet::register(conn, &register, now).unwrap();
    }

    #[test]
    fn a_machine_silent_for_half_an_hour_gets_one_ticket_for_a_person() {
        let mut conn = hub::open_in_memory().unwrap();
        register(&conn, "m-w", "cygnus", at(0.0));
        register(&conn, "m-1", "jarvis", at(0.0));

        assert!(
            machines_silent(&mut conn, at(fleet_settings::ONLINE_SECONDS - 1.0))
                .unwrap()
                .is_empty()
        );

        // jarvis keeps reporting; cygnus does not.
        register(&conn, "m-1", "jarvis", at(1_700.0));
        let filed = machines_silent(&mut conn, at(fleet_settings::ONLINE_SECONDS)).unwrap();
        assert_eq!(filed.len(), 1);
        assert_eq!(filed[0].change, Change::Created);
        let ticket = tickets::load(&conn, &filed[0].ticket).unwrap().unwrap();
        assert_eq!(ticket.title, "Machine cygnus has stopped reporting");
        assert_eq!(ticket.priority, 0);
        assert_eq!(ticket.status, Status::Open);
        for label in ["fleet", "needs:person", "machine:cygnus"] {
            assert!(ticket.labels.iter().any(|l| l == label), "{label}");
        }

        // Later passes add nothing while the machine stays silent.
        let again = machines_silent(&mut conn, at(3_000.0)).unwrap();
        assert!(again.iter().all(|f| f.change == Change::Unchanged));
    }

    #[test]
    fn the_ticket_closes_once_the_machine_has_reported_again_for_an_hour() {
        let mut conn = hub::open_in_memory().unwrap();
        let mut upkeep = crate::upkeep::Upkeep::new(None).unwrap();
        register(&conn, "m-w", "cygnus", at(0.0));
        let report = upkeep
            .pass(&mut conn, at(fleet_settings::ONLINE_SECONDS))
            .unwrap();
        assert_eq!(report.tickets_filed.len(), 1);
        let id = report.tickets_filed[0].clone();

        let mut closed = Vec::new();
        for step in 1..=10 {
            let now = at(fleet_settings::ONLINE_SECONDS + 600.0 * f64::from(step));
            register(&conn, "m-w", "cygnus", now);
            closed.extend(upkeep.pass(&mut conn, now).unwrap().tickets_closed);
        }
        assert_eq!(closed, vec![id.clone()]);
        let reason = tickets::load(&conn, &id)
            .unwrap()
            .unwrap()
            .close_reason
            .unwrap();
        assert!(reason.contains("cygnus reported"), "{reason}");
    }

    #[test]
    fn a_machine_set_off_or_long_retired_files_nothing() {
        let mut conn = hub::open_in_memory().unwrap();
        register(&conn, "m-w", "cygnus", at(0.0));
        register(&conn, "m-old", "hal", at(0.0));
        hub::fleet_settings::set_machine_state(
            &mut conn,
            "m-w",
            MachineState::Off,
            "operator",
            at(0.0),
        )
        .unwrap();
        // hal was last heard from more than a day ago.
        assert!(
            machines_silent(&mut conn, at(RETIRED_SECONDS))
                .unwrap()
                .is_empty()
        );
        // cygnus is off, so its silence is expected.
        assert!(
            machines_silent(&mut conn, at(fleet_settings::ONLINE_SECONDS))
                .unwrap()
                .iter()
                .all(|f| tickets::load(&conn, &f.ticket)
                    .unwrap()
                    .unwrap()
                    .title
                    .contains("hal"))
        );
    }

    #[test]
    fn an_old_registration_of_a_reporting_machine_files_nothing() {
        let mut conn = hub::open_in_memory().unwrap();
        register(&conn, "m-old", "cygnus", at(0.0));
        register(&conn, "m-new", "cygnus", at(3_000.0));
        assert!(machines_silent(&mut conn, at(3_600.0)).unwrap().is_empty());
    }
}
