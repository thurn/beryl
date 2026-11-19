//! The tip round routes (see "How long a round takes" in `docs/ignite.md`):
//!
//! - `POST /tips/costs` answers the check costs and claim length a round at one tip cuts one
//!   family's batches by, frozen at the first request, so every machine cuts the same batches.
//! - `POST /tips/rounds` stores one finished round. When too much of the round went to listing
//!   and Ignite's own work ([`TipRound::overhead`]), it files one improvement ticket naming the
//!   larger part, or reuses the open one. Waiting for cores never files: the `tip` class yields
//!   to landings by design. For [`COOLDOWN_SECONDS`] after a person closes that ticket, slow
//!   rounds file nothing ([`closed_by_a_person_since`]); upkeep's own closes reopen as
//!   `filing` describes.
//! - `GET /tips/rounds/last` answers the newest round, for `ignite status`.
//! - `GET /rates` answers each class's rate of check commands for today, which converts the
//!   gate's and the tip's budgets from seconds to instructions. It is computed on the first
//!   request of a day and kept for the rest of it ([`ignite_store::hub::rates`]).
//!
//! The two `POST` routes write, so they need an authorized writer (HTTP 401 otherwise).

use rusqlite::{Connection, OptionalExtension, params};
use serde::de::DeserializeOwned;
use serde_json::json;

use ignite_core::protocol::tickets::{TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_core::tipround::{
    LastTipRound, OVERHEAD_SHARE, TipCostsRequest, TipRound, TipRoundFiled,
};
use ignite_store::hub::{rates, tip_rounds};

use super::{Request, Response};
use crate::upkeep::filing::{self, Author, Filing};
use crate::upkeep::signature::Signature;

/// The subject and category of the one improvement ticket slow rounds share.
pub const SUBJECT: &str = "tip-rounds";
pub const CATEGORY: &str = "slow-tip-round";

/// How long after a person closes the slow-round ticket no new one is filed: a week. Round
/// times vary from round to round, so a ticket a person closed after finding nothing to cut was
/// filed again by the next slow round, about once an hour.
pub const COOLDOWN_SECONDS: f64 = 7.0 * 86_400.0;

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    let method = match request.path.as_str() {
        "/tips/costs" | "/tips/rounds" => "POST",
        "/tips/rounds/last" | "/rates" => "GET",
        _ => return None,
    };
    if request.method != method {
        return Some(failure(405, &format!("use {method} for {}", request.path)));
    }
    let writer = request.writer.as_ref();
    if method == "POST" && writer.is_none() {
        return Some(failure(401, "writes need a registered machine's token"));
    }
    let answer = match request.path.as_str() {
        "/tips/costs" => body::<TipCostsRequest>(request).and_then(|asked| {
            tip_rounds::costs(conn, &asked, now)
                .map(|costs| Response::json(200, &json!(costs)))
                .map_err(|e| failure(500, &e.to_string()))
        }),
        "/tips/rounds" => body::<TipRound>(request).and_then(|round| {
            let author = writer.map_or(filing::UPKEEP, |w| Author {
                actor: &w.actor,
                machine: w.machine.as_str(),
            });
            record(conn, author, &round, now)
                .map(|filed| Response::json(200, &json!(filed)))
                .map_err(|e| failure(500, &e.to_string()))
        }),
        "/rates" => rates::rates(conn, now)
            .map(|rates| Response::json(200, &json!(rates)))
            .map_err(|e| failure(500, &e.to_string())),
        _ => tip_rounds::last(conn)
            .map(|round| Response::json(200, &json!(LastTipRound { round })))
            .map_err(|e| failure(500, &e.to_string())),
    };
    Some(answer.unwrap_or_else(|response| response))
}

/// Stores the round, and files or finds the improvement ticket a slow round asks for.
fn record(
    conn: &mut Connection,
    author: Author,
    round: &TipRound,
    now: Timestamp,
) -> ignite_store::Result<TipRoundFiled> {
    tip_rounds::record(conn, round)?;
    let Some((part, share)) = round.overhead() else {
        return Ok(TipRoundFiled::default());
    };
    let signature = Signature::of_subject(SUBJECT, CATEGORY);
    if let Some(open) = open_ticket(conn, &signature.key())? {
        return Ok(TipRoundFiled {
            ticket: Some(open),
            created: false,
        });
    }
    if closed_by_a_person_since(conn, &signature.key(), now.plus_seconds(-COOLDOWN_SECONDS))? {
        return Ok(TipRoundFiled::default());
    }
    let percent = (share * 100.0).round();
    let body = format!(
        "Tip verification round at {commit} on {machine} spent {percent:.0}% of its time \
         {part}, and more than {limit:.0}% on listing families and Ignite's own work between \
         commands (waiting for cores is reported apart and counts for neither). \
         Find what makes that part slow and cut it, as \"How long a round takes\" in \
         docs/ignite.md describes.\n\nThe round: {summary}.",
        commit = round.commit,
        machine = round.machine,
        part = part.words(),
        limit = OVERHEAD_SHARE * 100.0,
        summary = round.summary(),
    );
    let filing = Filing {
        signature,
        evidence: round.commit.to_string(),
        fields: TicketFields {
            title: Some(format!(
                "Tip rounds spend {percent:.0}% of their time {}",
                part.words()
            )),
            body: Some(body),
            ticket_type: Some(TicketType::Task),
            priority: Some(2),
            labels: Some(vec!["improvement".to_owned(), format!("rule:{CATEGORY}")]),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: false,
    };
    let filed = filing::file(conn, author, &filing, now)?;
    Ok(TipRoundFiled {
        ticket: Some(filed.ticket),
        created: filed.change == filing::Change::Created,
    })
}

/// The open ticket with this signature, if there is one.
fn open_ticket(conn: &Connection, key: &str) -> ignite_store::Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT id FROM tickets
             WHERE json_extract(metadata, '$.ignite_signature') = ?1 AND status != 'closed'
             ORDER BY created_at DESC, id DESC LIMIT 1",
            params![key],
            |row| row.get(0),
        )
        .optional()?)
}

/// Whether a person closed a ticket with this signature after `since`. Upkeep's closes, which
/// carry [`filing::AUTO_CLOSED_KEY`], don't count: `filing::file` reopens those.
fn closed_by_a_person_since(
    conn: &Connection,
    key: &str,
    since: Timestamp,
) -> ignite_store::Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM tickets
             WHERE json_extract(metadata, '$.ignite_signature') = ?1 AND status = 'closed'
               AND closed_at > ?2
               AND json_extract(metadata, '$.' || ?3) IS NULL
             LIMIT 1",
            params![key, since.to_string(), filing::AUTO_CLOSED_KEY],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}

fn body<T: DeserializeOwned>(request: &Request) -> Result<T, Response> {
    serde_json::from_slice(&request.body)
        .map_err(|error| failure(400, &format!("bad request body: {error}")))
}

#[cfg(test)]
mod tests {
    use ignite_core::ids::{Commit, MachineId};
    use ignite_core::tipround::TipCosts;
    use ignite_store::hub;

    use super::*;
    use crate::auth::Writer;

    fn request(method: &str, path: &str, body: &impl serde::Serialize) -> Request {
        Request {
            method: method.to_owned(),
            path: path.to_owned(),
            query: Vec::new(),
            writer: Some(Writer {
                machine: MachineId::new("m-1").unwrap(),
                actor: "daemon".to_owned(),
            }),
            body: serde_json::to_vec(body).unwrap(),
            headers: Vec::new(),
        }
    }

    fn round(commit: &str, listing_s: f64) -> TipRound {
        TipRound {
            commit: Commit::new(commit).unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            at: Timestamp::EPOCH,
            wall_s: 600.0,
            ran: 10,
            cached: 2,
            checks_s: 600.0 - listing_s,
            listing_s,
            listing_alone_s: None,
            building_s: 0.0,
            waiting_s: 0.0,
            budget: None,
        }
    }

    fn post_round(conn: &mut Connection, round: &TipRound) -> TipRoundFiled {
        post_round_at(conn, round, Timestamp::EPOCH)
    }

    fn post_round_at(conn: &mut Connection, round: &TipRound, now: Timestamp) -> TipRoundFiled {
        let reply = handle(conn, &request("POST", "/tips/rounds", round), now);
        let reply = reply.unwrap();
        assert_eq!(reply.status, 200);
        serde_json::from_slice(&reply.body).unwrap()
    }

    fn improvement_tickets(conn: &Connection) -> u32 {
        conn.query_row(
            "SELECT count(*) FROM tickets WHERE labels LIKE '%improvement%'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    /// A 137-minute round spent 21 minutes listing families, and finding that took a query by
    /// hand.
    #[test]
    fn a_slow_round_files_one_ticket_and_the_next_reuses_it() {
        let mut conn = hub::open_in_memory().unwrap();
        let first = post_round(&mut conn, &round("abcd", 180.0));
        assert!(first.created, "{first:?}");
        let ticket = first.ticket.clone().unwrap();
        let title: String = conn
            .query_row(
                "SELECT title FROM tickets WHERE id = ?1",
                [&ticket],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "Tip rounds spend 30% of their time listing families");

        let second = post_round(&mut conn, &round("ef01", 200.0));
        assert_eq!(second.ticket.as_deref(), Some(ticket.as_str()));
        assert!(!second.created);
        assert_eq!(improvement_tickets(&conn), 1);
        // No comment was added to it either.
        let comments: u32 = conn
            .query_row("SELECT count(*) FROM comments", [], |row| row.get(0))
            .unwrap();
        assert_eq!(comments, 0);

        // A quick round files nothing; the last round is the newest stored.
        assert_eq!(
            post_round(&mut conn, &round("2345", 10.0)),
            TipRoundFiled::default()
        );
        let last = handle(
            &mut conn,
            &request("GET", "/tips/rounds/last", &()),
            Timestamp::EPOCH,
        );
        let last: LastTipRound = serde_json::from_slice(&last.unwrap().body).unwrap();
        assert_eq!(last.round, Some(round("2345", 10.0)));
    }

    /// A person closed the slow-round ticket after finding nothing to cut, and the next slow
    /// round filed it again within the hour, round after round.
    #[test]
    fn a_ticket_a_person_closed_is_not_filed_again_for_a_week() {
        let mut conn = hub::open_in_memory().unwrap();
        let first = post_round(&mut conn, &round("abcd", 180.0)).ticket.unwrap();
        let closed: Timestamp = "2026-11-19T08:00:00Z".parse().unwrap();
        conn.execute(
            "UPDATE tickets SET status = 'closed', closed_at = ?2 WHERE id = ?1",
            params![first, closed.to_string()],
        )
        .unwrap();

        let soon = post_round_at(
            &mut conn,
            &round("ef01", 180.0),
            closed.plus_seconds(3600.0),
        );
        assert_eq!(soon, TipRoundFiled::default());
        assert_eq!(improvement_tickets(&conn), 1);

        let later = closed.plus_seconds(COOLDOWN_SECONDS + 3600.0);
        let filed = post_round_at(&mut conn, &round("2345", 180.0), later);
        assert!(filed.created, "{filed:?}");
        assert_eq!(improvement_tickets(&conn), 2);
    }

    #[test]
    fn costs_answer_the_same_for_a_tip() {
        let mut conn = hub::open_in_memory().unwrap();
        let asked = TipCostsRequest {
            commit: Commit::new("abcd").unwrap(),
            family: "unit".to_owned(),
            claim_seconds: 60.0,
        };
        let reply = handle(
            &mut conn,
            &request("POST", "/tips/costs", &asked),
            Timestamp::EPOCH,
        );
        let costs: TipCosts = serde_json::from_slice(&reply.unwrap().body).unwrap();
        assert!((costs.claim_seconds - 60.0).abs() < f64::EPSILON);
        let mut anonymous = request("POST", "/tips/costs", &asked);
        anonymous.writer = None;
        let refused = handle(&mut conn, &anonymous, Timestamp::EPOCH).unwrap();
        assert_eq!(refused.status, 401);
    }

    /// The gate and tip verification convert their budgets with the rates the hub answers. With
    /// no record of a class's check commands it has none, and the daemon uses its default.
    #[test]
    fn rates_answer_for_the_day_to_any_reader() {
        let mut conn = hub::open_in_memory().unwrap();
        let mut asked = request("GET", "/rates", &());
        asked.writer = None;
        let now: Timestamp = "2026-11-19T08:00:00Z".parse().unwrap();
        let reply = handle(&mut conn, &asked, now).unwrap();
        assert_eq!(reply.status, 200);
        let rates: ignite_core::budget::ClassRates = serde_json::from_slice(&reply.body).unwrap();
        assert_eq!(rates.day, "2026-11-19");
        assert_eq!(rates.land, None);
        assert!((rates.land() - ignite_core::budget::DEFAULT_LAND_RATE).abs() < f64::EPSILON);
        let posted = handle(&mut conn, &request("POST", "/rates", &()), now).unwrap();
        assert_eq!(posted.status, 405);
    }
}
