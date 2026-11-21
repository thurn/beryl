//! Machine registration and heartbeat replies.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde_json::json;

use ignite_core::protocol::hub::{Heartbeat, HeartbeatReply, MachineRegister, MachineRegistered};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::{emergency, fleet, tokens};

use super::{Answer, HUB_VERSION, Request, Response, failure, sql_failure, store_failure, usage};

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
    let hub_version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [HUB_VERSION],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_failure)?;
    Ok(Response::json(
        200,
        &json!(HeartbeatReply {
            fleet: Some(
                usage::notice(conn, beat.machine.as_str(), now).map_err(|e| store_failure(&e))?
            ),
            // A hub that never recorded its version asks no machine to update.
            hub_version: hub_version.unwrap_or_else(|| beat.version.clone()),
            emergency: emergency::in_force(conn, now).map_err(|e| store_failure(&e))?,
            paused_everywhere: fleet::paused_everywhere(conn).map_err(|e| store_failure(&e))?,
        }),
    ))
}
