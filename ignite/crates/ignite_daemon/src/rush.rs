//! Rush: the operator's way to have one session's work finish first (docs/ignite.md, "Rush").
//!
//! The operator types `/rush` in a desktop session, and the skill runs `ignite rush`. From then
//! on every heavy command of that session is scheduled ahead of every class, behind only a tip
//! round's build checks and the commands of `incident` and `justiciar` tickets
//! ([`crate::sched::Urgency`]). It may take every shared core, and while one of its commands
//! holds or waits for cores the machine starts no ordinary workers. A ticket tree it asks for
//! goes to it ahead of the queued tickets.
//!
//! - Only a session may rush, and never a headless worker (`cli-…`): the skill is invoked by
//!   the operator alone, and a worker that rushed itself would take the machine from the others.
//! - At most [`PER_MACHINE`] rushes are in force on a machine. Their commands are served in the
//!   usual order within a class, and their tree leases in the order they were asked for.
//! - A rush lapses [`LAPSE_S`] after it was last started, unless `ignite rush` renews it. It ends
//!   with `ignite rush end`, or when its ticket's tree is given back: the ticket landed, was
//!   released or was closed. A session that rushes before `ignite start` gets its ticket
//!   attached once it holds a tree.
//!
//! The rushes are kept as JSON in the local database's pool settings, so the verb, the
//! supervisor that asks for cores, and a restarted daemon all read the same list.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::local::{pool as settings, rows, trees};

use crate::doctor::StatusSection;
use crate::handles::lock;
use crate::server::{Context, Verb};

/// The pool setting that holds the rushes in force, as JSON.
const KEY: &str = "rushes";

/// How long a rush lasts after it was last started: long enough for a ticket's work and its
/// landing, short enough that a forgotten rush does not hold the pool back overnight.
pub const LAPSE_S: f64 = 4.0 * 3_600.0;

/// The most rushes in force on one machine at once. With more, each would wait for the others
/// and none would be first.
pub const PER_MACHINE: usize = 2;

/// One session's rush.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rush {
    pub session: SessionId,
    /// The ticket whose tree the session holds; `None` until it holds one.
    pub ticket: Option<TicketId>,
    pub started_at: Timestamp,
    pub renewed_at: Timestamp,
}

impl Rush {
    pub fn expires_at(&self) -> Timestamp {
        self.renewed_at.plus_seconds(LAPSE_S)
    }
}

fn load(conn: &Connection) -> Vec<Rush> {
    settings::setting(conn, KEY)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn save(conn: &Connection, rushes: &[Rush]) {
    let json = (!rushes.is_empty())
        .then(|| serde_json::to_string(rushes).ok())
        .flatten();
    if let Err(error) = settings::set_setting(conn, KEY, json.as_deref()) {
        eprintln!("ignite: the rushes were not saved: {error}");
    }
}

/// Whether `row` is a ticket tree leased to a session now.
fn leased(row: &rows::Tree) -> bool {
    row.kind == "ticket" && row.state == trees::State::Leased.as_str()
}

/// The rushes in force at `now`, in the order they were started. Drops those that lapsed or
/// whose ticket no longer holds a tree here, attaches the ticket of a session that has leased
/// a tree since it rushed, and saves the list when that changed it.
pub fn in_force(conn: &Connection, now: Timestamp) -> Vec<Rush> {
    let stored = load(conn);
    if stored.is_empty() {
        return stored;
    }
    let rows = trees::all(conn).unwrap_or_default();
    let mut kept = Vec::with_capacity(stored.len());
    for mut rush in stored.iter().cloned() {
        if now >= rush.expires_at() {
            continue;
        }
        match &rush.ticket {
            Some(ticket) => {
                let holds = rows
                    .iter()
                    .any(|row| leased(row) && row.ticket.as_deref() == Some(ticket.as_str()));
                if !holds {
                    continue;
                }
            }
            None => {
                rush.ticket = rows
                    .iter()
                    .filter(|row| leased(row))
                    .find(|row| row.session.as_deref() == Some(rush.session.as_str()))
                    .and_then(|row| row.ticket.as_deref())
                    .and_then(|ticket| TicketId::new(ticket).ok());
            }
        }
        kept.push(rush);
    }
    if kept != stored {
        save(conn, &kept);
    }
    kept
}

/// Whether a command for `ticket`, or from `session`, belongs to a rush in force.
pub fn rushed(
    conn: &Connection,
    now: Timestamp,
    ticket: Option<&TicketId>,
    session: Option<&str>,
) -> bool {
    in_force(conn, now).iter().any(|rush| {
        ticket.is_some_and(|t| rush.ticket.as_ref() == Some(t))
            || session.is_some_and(|s| rush.session.as_str() == s)
    })
}

/// Why [`start`] refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A headless worker asked.
    Worker,
    /// [`PER_MACHINE`] other sessions hold a rush.
    Full(Vec<Rush>),
}

/// Starts or renews `session`'s rush at `now`. Returns it, and whether it renewed one.
pub fn start(
    conn: &Connection,
    now: Timestamp,
    session: &SessionId,
    ticket: Option<TicketId>,
) -> Result<(Rush, bool), Refusal> {
    if session
        .as_str()
        .starts_with(crate::dispatchd::CLI_SESSION_PREFIX)
    {
        return Err(Refusal::Worker);
    }
    let mut rushes = in_force(conn, now);
    let renewed = if let Some(rush) = rushes.iter_mut().find(|r| &r.session == session) {
        rush.renewed_at = now;
        if rush.ticket.is_none() {
            rush.ticket = ticket;
        }
        true
    } else if rushes.len() >= PER_MACHINE {
        return Err(Refusal::Full(rushes));
    } else {
        rushes.push(Rush {
            session: session.clone(),
            ticket,
            started_at: now,
            renewed_at: now,
        });
        false
    };
    save(conn, &rushes);
    let rush = rushes
        .into_iter()
        .find(|r| &r.session == session)
        .expect("the rush was just kept");
    Ok((rush, renewed))
}

/// Ends `session`'s rush. Returns whether it had one.
pub fn end(conn: &Connection, now: Timestamp, session: &SessionId) -> bool {
    let mut rushes = in_force(conn, now);
    let before = rushes.len();
    rushes.retain(|r| &r.session != session);
    let ended = rushes.len() != before;
    if ended {
        save(conn, &rushes);
    }
    ended
}

/// `seconds` as `5 min` or `1 h 20 min`.
fn span(seconds: f64) -> String {
    let minutes = (seconds.max(0.0) / 60.0).round() as u64;
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {} min", minutes / 60, minutes % 60)
    }
}

fn describe(rush: &Rush, now: Timestamp) -> String {
    let ticket = rush
        .ticket
        .as_ref()
        .map_or_else(|| "no ticket yet".to_owned(), ToString::to_string);
    format!(
        "{} ({ticket}), started {} ago, lapses in {} unless renewed",
        rush.session,
        span(now.seconds_since(rush.started_at)),
        span(rush.expires_at().seconds_since(now)),
    )
}

/// The local database the rushes are kept in, and the daemon's clock, shared by the verb and
/// `ignite status`.
pub struct Rushes {
    pub db: Mutex<Connection>,
    pub clock: Arc<dyn Clock>,
}

/// `ignite rush [end]`, which only the `/rush` skill runs.
pub struct RushVerb(pub Arc<Rushes>);

const USAGE: &str = "usage    ignite rush [end]";

impl Verb for RushVerb {
    fn name(&self) -> &'static str {
        "rush"
    }

    /// A session may rush before it holds a tree, from anywhere.
    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn blocks_update(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let Some(session) = ctx.session.clone().or_else(|| req.session.clone()) else {
            return Reply::text(
                Exit::Usage,
                "refused  only a session can rush; the operator starts one with /rush",
            );
        };
        let now = self.0.clock.now();
        let db = lock(&self.0.db);
        match req
            .args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            [] => {
                let ticket = ctx
                    .tree
                    .as_ref()
                    .and_then(|tree| trees::get(&db, tree.as_str()).ok().flatten())
                    .filter(leased)
                    .and_then(|row| row.ticket)
                    .and_then(|ticket| TicketId::new(ticket).ok());
                match start(&db, now, &session, ticket) {
                    Ok((rush, renewed)) => Reply::text(
                        Exit::Success,
                        format!(
                            "{}  {}\nends     when the ticket lands or is released, with \
                             `ignite rush end`, or at {}",
                            if renewed { "renewed" } else { "rushed " },
                            describe(&rush, now),
                            rush.expires_at()
                        ),
                    ),
                    Err(Refusal::Worker) => Reply::text(
                        Exit::Usage,
                        "refused  a headless worker cannot rush; only the operator starts one \
                         with /rush",
                    ),
                    Err(Refusal::Full(rushes)) => {
                        let holders: Vec<String> =
                            rushes.iter().map(|rush| describe(rush, now)).collect();
                        Reply::text(
                            Exit::Usage,
                            format!(
                                "refused  {PER_MACHINE} sessions already rush on this machine: {}",
                                holders.join("; ")
                            ),
                        )
                    }
                }
            }
            ["end"] => {
                if end(&db, now, &session) {
                    Reply::text(Exit::Success, format!("ended    the rush of {session}"))
                } else {
                    Reply::text(Exit::Success, format!("none     {session} has no rush"))
                }
            }
            _ => Reply::text(Exit::Usage, USAGE),
        }
    }
}

/// The rush section of `ignite status`: nothing while no session rushes.
pub struct RushSection(pub Arc<Rushes>);

impl StatusSection for RushSection {
    fn title(&self) -> &'static str {
        "rush"
    }

    fn lines(&self) -> Vec<String> {
        let now = self.0.clock.now();
        in_force(&lock(&self.0.db), now)
            .iter()
            .map(|rush| describe(rush, now))
            .collect()
    }
}

#[cfg(test)]
#[path = "rush_tests.rs"]
mod tests;
