//! Machine registration and heartbeat replies.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde_json::json;

use ignite_core::fleet::MachineState;
use ignite_core::protocol::emergency::ending_refusal;
use ignite_core::protocol::hub::{
    Heartbeat, HeartbeatReply, MachineRegister, MachineRegistered, Pause,
};
use ignite_core::protocol::tickets::Status;
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::{emergency, fleet, fleet_settings, tickets, tokens};

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
    let reply = HeartbeatReply {
        fleet: Some(
            usage::notice(&snapshot, beat.machine.as_str(), now).map_err(|e| store_failure(&e))?,
        ),
        // A hub that never recorded its version asks no machine to update.
        hub_version: hub_version.unwrap_or_else(|| beat.version.clone()),
        emergency: emergency::in_force(&snapshot, now).map_err(|e| store_failure(&e))?,
        paused_everywhere: fleet::paused_everywhere(&snapshot, now)
            .map_err(|e| store_failure(&e))?,
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

/// Pauses or resumes hand-offs to one machine or to every machine. Resuming every machine also
/// ends the emergency, if one is in force.
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
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_failure)?;
        if !pause.paused
            && let Some(current) = emergency::in_force(&tx, now).map_err(|e| store_failure(&e))?
        {
            // Another session may have declared an emergency since the caller declared its own.
            let holds = tickets::load(&tx, current.ticket.as_str())
                .map_err(|e| store_failure(&e))?
                .is_some_and(|held| {
                    held.status == Status::InProgress
                        && pause.session.is_some()
                        && held.session == pause.session
                });
            if let Some(message) = ending_refusal(
                &current,
                pause.session.as_ref(),
                holds,
                pause.emergency.as_ref(),
            ) {
                return Err(failure(409, &message));
            }
        }
        fleet::set_paused_everywhere(&tx, pause.paused, now).map_err(|e| store_failure(&e))?;
        if !pause.paused {
            emergency::clear(&tx).map_err(|e| store_failure(&e))?;
        }
        tx.commit().map_err(sql_failure)?;
    }
    Ok(done())
}
