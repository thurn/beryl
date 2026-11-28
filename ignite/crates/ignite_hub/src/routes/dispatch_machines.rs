//! Machine registration and heartbeat replies.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde_json::json;

use ignite_core::fleet::MachineState;
use ignite_core::protocol::hub::{
    Heartbeat, HeartbeatReply, MachineRegister, MachineRegistered, Pause,
};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::{emergency, fleet, fleet_settings, tokens};

use super::{
    Answer, HUB_VERSION, Request, Response, done, failure, sql_failure, store_failure, usage,
};

/// Refuses only the named reservation. A retry cannot affect a replacement or a claim.
pub(super) fn refuse_reservation(
    conn: &Connection,
    request: &Request,
    reservation: &ignite_core::protocol::hub::Reservation,
) -> Answer {
    super::same_machine(request, &reservation.machine)?;
    conn.execute(
        "DELETE FROM reservations
         WHERE ticket = ?1 AND machine = ?2 AND session = ?3 AND code = ?4",
        rusqlite::params![
            reservation.ticket.as_str(),
            reservation.machine.as_str(),
            reservation.session.as_str(),
            reservation.code,
        ],
    )
    .map_err(super::sql_failure)?;
    Ok(super::done())
}

/// Adds or updates the machine and answers with its write token. A new machine ID needs no token
/// and is issued one. A machine ID that already has a token registers again only with that token
/// as the bearer, so that nobody who can reach the hub can replace another machine's token, and
/// it keeps that token: `ignite install` registers the machine again while the daemon and its
/// verb processes still send the token they read when they started, and a new token would make
/// the hub refuse each of their writes with 401 until they restarted.
pub(super) fn register(
    conn: &mut Connection,
    request: &Request,
    machine: &MachineRegister,
    now: Timestamp,
) -> Answer {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_failure)?;
    let mut kept = None;
    if tokens::has_token(&tx, &machine.machine).map_err(|e| store_failure(&e))? {
        let bearer = request
            .headers
            .iter()
            .find(|(name, _)| name == "authorization")
            .and_then(|(_, value)| value.split_once(' '))
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
            .map(|(_, token)| token.trim());
        // `writer` is set when the request carried a valid token and an `X-Actor`; a client
        // that sends only the bearer (`ignite install`) is checked here.
        let owner = match (&request.writer, bearer) {
            (Some(writer), _) => Some(writer.machine.clone()),
            (None, Some(token)) => tokens::verify(&tx, token).map_err(|e| store_failure(&e))?,
            (None, None) => None,
        };
        if owner.as_ref() != Some(&machine.machine) {
            return Err(failure(
                401,
                &format!(
                    "{} is already registered; send its current token as the bearer",
                    machine.machine
                ),
            ));
        }
        // Keeps the bearer only once it is known to be the machine's current token.
        if let Some(token) = bearer
            && tokens::verify(&tx, token)
                .map_err(|e| store_failure(&e))?
                .as_ref()
                == Some(&machine.machine)
        {
            kept = Some(token.to_owned());
        }
    }
    fleet::register(&tx, machine, now).map_err(|e| store_failure(&e))?;
    let token = match kept {
        Some(token) => token,
        None => tokens::issue(&tx, &machine.machine).map_err(|e| store_failure(&e))?,
    };
    tx.commit().map_err(sql_failure)?;
    Ok(Response::json(
        200,
        &json!(MachineRegistered {
            machine: machine.machine.clone(),
            token,
        }),
    ))
}

pub(super) fn heartbeat(conn: &mut Connection, beat: &Heartbeat, now: Timestamp) -> Answer {
    fleet::heartbeat(conn, beat, now).map_err(|error| match error {
        StoreError::Invalid(message) => failure(404, &message),
        other => store_failure(&other),
    })?;
    // A failure here must not cost the machine its heartbeat; the next one tries again.
    let reconciled = crate::upkeep::returning::reconcile(conn, beat, now).unwrap_or_else(|error| {
        eprintln!("ignite: claims of {} not reconciled: {error}", beat.machine);
        Vec::new()
    });
    // An order goes out with the build tickets it is carried out against.
    let shed = beat
        .sample
        .is_some()
        .then(|| sheds::take_order(conn, beat))
        .flatten();
    // The heartbeat was recorded above. The reply only reads, so it can take a snapshot
    // without holding the write lock while assembling the fleet policy.
    let snapshot = conn
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(sql_failure)?;
    let hub_version: Option<String> = snapshot
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [HUB_VERSION],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_failure)?;
    let build_tickets = beat
        .sample
        .as_ref()
        .and_then(|sample| sheds::holdings(&snapshot, beat, sample, now));
    let pause = fleet::fleet_pause(&snapshot, now).map_err(|e| store_failure(&e))?;
    let reply = HeartbeatReply {
        fleet: Some(
            usage::notice(&snapshot, beat.machine.as_str(), now).map_err(|e| store_failure(&e))?,
        ),
        // A hub that never recorded its version asks no machine to update.
        hub_version: hub_version.unwrap_or_else(|| beat.version.clone()),
        emergency: emergency::in_force(&snapshot, now).map_err(|e| store_failure(&e))?,
        paused_everywhere: pause == fleet::FleetPause::Everything,
        paused_except_master: pause == fleet::FleetPause::ExceptHoldingMaster,
        reconciled,
        build_tickets,
        shed,
    };
    snapshot.commit().map_err(sql_failure)?;
    if let Some(sample) = &beat.sample
        && let Err(error) = super::queue::machine_snapshot::record(conn, beat, sample, now)
    {
        eprintln!(
            "ignite: machine snapshot for {} was not recorded: {error}",
            beat.machine
        );
    }
    Ok(Response::json(200, &json!(reply)))
}

/// Pauses or resumes hand-offs to one machine or to every machine. The pause of every machine
/// is the operator's own and leaves any emergency in force alone: in November 2026, resuming
/// hand-offs ended the automatic landings freeze too, and the incident commander had to declare
/// a new one and copy its allowances by hand.
pub(super) fn pause(
    conn: &mut Connection,
    pause: &Pause,
    request: &Request,
    now: Timestamp,
) -> Answer {
    if let Some(machine) = &pause.machine {
        let state = if pause.paused {
            MachineState::ChecksOnly
        } else {
            MachineState::Active
        };
        let actor = request
            .writer
            .as_ref()
            .map_or(machine.as_str(), |writer| writer.actor.as_str());
        fleet_settings::set_machine_state(conn, machine.as_str(), state, actor, now)
            .map_err(|e| store_failure(&e))?;
    } else {
        let fleet_pause = match (pause.paused, pause.except_holding_master) {
            (false, _) => fleet::FleetPause::None,
            (true, true) => fleet::FleetPause::ExceptHoldingMaster,
            (true, false) => fleet::FleetPause::Everything,
        };
        fleet::set_fleet_pause(conn, fleet_pause, now).map_err(|e| store_failure(&e))?;
    }
    Ok(done())
}

#[path = "dispatch_machines_shed.rs"]
pub(super) mod sheds;
