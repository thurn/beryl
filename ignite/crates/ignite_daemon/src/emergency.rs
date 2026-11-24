//! The daemon's side of the emergency pause (see `ignite_core::protocol::emergency`).
//!
//! - An incident commander or justiciar runs `ignite emergency declare <ticket>` to set the
//!   flag at the hub. Each daemon hears the flag in its heartbeat reply and keeps it in the local
//!   database across restarts and hub outages.
//! - A full emergency pauses hand-offs and refuses the [heavy verbs](HEAVY_VERBS). With
//!   `--landings-only`, only `land` refuses. Both use exit code 6 ([`Exit::Paused`]). Reads
//!   and running commands continue. To stop a running command, use `ignite stop`.
//! - The declaring session and sessions working its ticket can still run heavy verbs.
//!   The declaring session may grant other tickets exceptions with `ignite emergency allow`.
//! - The flag lapses two hours after it was last declared. `ignite pool resume --all` ends it on
//!   every machine, and `ignite emergency clear --local` lifts it on this machine alone, for an
//!   operator stuck behind a flag nobody can end.
//! - `ignite wait emergency` returns when the flag clears, with the all-clear.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use rusqlite::Connection;

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::emergency::{Allowance, Emergency, EmergencyChange, EmergencyScope};
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::local::{pool as settings, trees};

use crate::doctor::{Finding, StatusSection};
use crate::handles::lock;
use crate::server::{Context, Verb};

/// The verbs that start heavy work, which an emergency refuses: they claim a ticket and lease a
/// tree, build, or run checks.
pub const HEAVY_VERBS: &[&str] = &["start", "test", "check", "land", "run", "rebuild", "audit"];

/// The pool setting that holds the last emergency the hub sent, as JSON.
const HEARD: &str = "emergency";
/// The pool setting that holds the declaration time of the emergency lifted on this machine.
const CLEARED: &str = "emergency_cleared_locally";

/// The longest a waiting thread sleeps before it looks at the clock and its caller again.
const WAIT_SLICE: Duration = Duration::from_secs(1);

#[derive(Default)]
struct State {
    /// The last emergency the hub sent, whether or not it has lapsed since.
    heard: Option<Emergency>,
    /// The `declared_at` of an emergency the operator lifted on this machine.
    cleared: Option<Timestamp>,
}

/// The emergency as this daemon knows it, shared by the server, the heartbeat and the verbs.
pub struct Emergencies {
    clock: Arc<dyn Clock>,
    db: Option<Mutex<Connection>>,
    state: Mutex<State>,
    changed: Condvar,
}

impl Emergencies {
    /// What the daemon knew before it last stopped, read from `db`; nothing without one.
    pub fn new(clock: Arc<dyn Clock>, db: Option<Connection>) -> Emergencies {
        let mut state = State::default();
        if let Some(conn) = &db {
            state.heard = settings::setting(conn, HEARD)
                .ok()
                .flatten()
                .and_then(|json| serde_json::from_str(&json).ok());
            state.cleared = settings::setting(conn, CLEARED)
                .ok()
                .flatten()
                .and_then(|at| Timestamp::parse(&at).ok());
        }
        Emergencies {
            clock,
            db: db.map(Mutex::new),
            state: Mutex::new(state),
            changed: Condvar::new(),
        }
    }

    fn keep(&self, key: &str, value: Option<&str>) {
        if let Some(db) = &self.db
            && let Err(error) = settings::set_setting(&lock(db), key, value)
        {
            eprintln!("ignite: the emergency was not saved: {error}");
        }
    }

    /// Records what the hub said: the emergency in force, or `None` once it has ended.
    pub fn heard(&self, emergency: Option<Emergency>) {
        let mut state = lock(&self.state);
        if state.heard == emergency {
            return;
        }
        let json = emergency
            .as_ref()
            .and_then(|e| serde_json::to_string(e).ok());
        state.heard = emergency;
        drop(state);
        self.keep(HEARD, json.as_deref());
        self.changed.notify_all();
    }

    /// The emergency that has not lapsed, even one lifted on this machine.
    fn current(&self, state: &State) -> Option<Emergency> {
        let now = self.clock.now();
        state.heard.clone().filter(|e| e.active(now))
    }

    fn lifted(state: &State, emergency: &Emergency) -> bool {
        state.cleared == Some(emergency.declared_at)
    }

    /// The emergency that this machine enforces now, if any.
    pub fn in_force(&self) -> Option<Emergency> {
        let state = lock(&self.state);
        self.current(&state).filter(|e| !Self::lifted(&state, e))
    }

    /// Lifts the emergency in force on this machine alone, until another is declared. Returns
    /// the one lifted.
    pub fn clear_locally(&self) -> Option<Emergency> {
        let mut state = lock(&self.state);
        let lifted = self.current(&state)?;
        state.cleared = Some(lifted.declared_at);
        drop(state);
        self.keep(CLEARED, Some(&lifted.declared_at.to_string()));
        self.changed.notify_all();
        Some(lifted)
    }

    /// The refusal of `verb` from `session`, whose ticket is `ticket`, while an emergency is in
    /// force; `None` when the verb may run.
    pub fn refusal(
        &self,
        verb: &str,
        session: Option<&SessionId>,
        ticket: Option<&TicketId>,
    ) -> Option<Reply> {
        if !HEAVY_VERBS.contains(&verb) {
            return None;
        }
        let emergency = self.in_force()?;
        if (emergency.scope == EmergencyScope::Landings && verb != "land")
            || session == Some(&emergency.session)
            || ticket.is_some_and(|t| emergency.allows(t, verb))
        {
            return None;
        }
        Some(Reply::text(
            Exit::Paused,
            format!(
                "paused   emergency {} (declared by {}) pauses {}. `ignite wait \
                 emergency` returns when it ends. Commands already running continue.",
                emergency.ticket,
                emergency.session,
                paused_work(emergency.scope)
            ),
        ))
    }

    /// Waits up to `wait_s` seconds for the emergency to end, or until the caller has `left`.
    pub fn wait(&self, wait_s: u32, left: &dyn Fn() -> bool) -> Reply {
        let deadline = self.clock.now().plus_seconds(f64::from(wait_s));
        let mut state = lock(&self.state);
        loop {
            let Some(emergency) = self.current(&state).filter(|e| !Self::lifted(&state, e)) else {
                return Reply::text(
                    Exit::Success,
                    "all clear the emergency is over; carry on with your ticket under the normal \
                     rules: `ignite sync`, then go on",
                );
            };
            let now = self.clock.now();
            if now >= deadline || left() {
                return Reply::text(
                    Exit::Running,
                    format!(
                        "paused   still in emergency {} (declared by {}); run `ignite wait \
                         emergency` again",
                        emergency.ticket, emergency.session
                    ),
                );
            }
            let until_lapse = emergency.expires_at().seconds_since(now).max(0.0);
            let slice = WAIT_SLICE.min(Duration::from_secs_f64(until_lapse));
            state = self
                .changed
                .wait_timeout(state, slice)
                .map_or_else(|poisoned| poisoned.into_inner().0, |(guard, _)| guard);
        }
    }

    /// The lines `ignite status` shows: nothing outside an emergency.
    pub fn lines(&self) -> Vec<String> {
        let state = lock(&self.state);
        let Some(emergency) = self.current(&state) else {
            return Vec::new();
        };
        let mut lines = vec![self.describe(&emergency)];
        if !emergency.allowed.is_empty() {
            let allowed: Vec<String> = emergency.allowed.iter().map(allowance).collect();
            lines.push(format!("allowed {}", allowed.join(", ")));
        }
        if Self::lifted(&state, &emergency) {
            lines.push("lifted on this machine by `ignite emergency clear --local`".to_owned());
        }
        lines
    }

    /// `doctor`'s finding during an emergency.
    pub fn finding(&self) -> Option<Finding> {
        let emergency = self.in_force()?;
        Some(Finding::warning(
            "emergency",
            self.describe(&emergency),
            "wait with `ignite wait emergency`. If the declaring session is gone, \
             `ignite emergency clear --local` lifts it on this machine.",
        ))
    }

    fn describe(&self, emergency: &Emergency) -> String {
        let now = self.clock.now();
        format!(
            "{}, declared by {} on {} {} ago, pauses {}, lapses in {} unless renewed",
            emergency.ticket,
            emergency.session,
            emergency.machine,
            span(now.seconds_since(emergency.declared_at)),
            paused_work(emergency.scope),
            span(emergency.expires_at().seconds_since(now)),
        )
    }
}

fn paused_work(scope: EmergencyScope) -> &'static str {
    match scope {
        EmergencyScope::All => "heavy verbs and worker hand-offs",
        EmergencyScope::Landings => "landings only",
    }
}

fn allowance(allowance: &Allowance) -> String {
    match &allowance.verb {
        Some(verb) => format!("{} ({verb})", allowance.ticket),
        None => allowance.ticket.to_string(),
    }
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

/// The ticket the request works on, for the exemptions: the one `start` names, or the one the
/// tree it comes from holds.
pub fn caller_ticket(
    db: Option<&Arc<Mutex<Connection>>>,
    ctx: &Context,
    req: &Request,
) -> Option<TicketId> {
    if req.verb == "start" {
        let named = req.args.iter().find(|a| !a.starts_with('-'))?;
        return TicketId::new(named.as_str()).ok();
    }
    let tree = trees::get(&lock(db?), ctx.tree.as_ref()?.as_str()).ok()??;
    TicketId::new(tree.ticket?).ok()
}

/// The emergency section of `ignite status`.
pub struct EmergencySection(pub Arc<Emergencies>);

impl StatusSection for EmergencySection {
    fn title(&self) -> &'static str {
        "emergency"
    }

    fn lines(&self) -> Vec<String> {
        self.0.lines()
    }
}

/// `ignite emergency [declare <ticket> [--landings-only] | allow <ticket> [<verb>] | clear --local]`.
pub struct EmergencyVerb(pub Arc<Emergencies>);

const USAGE: &str = "usage    ignite emergency [declare <ticket> [--landings-only] | allow \
                     <ticket> [<verb>] | clear --local]; `ignite pool resume --all` ends it";

impl Verb for EmergencyVerb {
    fn name(&self) -> &'static str {
        "emergency"
    }

    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn blocks_update(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let words: Vec<&str> = req.args.iter().map(String::as_str).collect();
        match words.as_slice() {
            [] => self.show(),
            ["declare", ticket, "--landings-only"] | ["declare", "--landings-only", ticket] => {
                self.declare(ctx, req, ticket, EmergencyScope::Landings)
            }
            ["declare", ticket] => self.declare(ctx, req, ticket, EmergencyScope::All),
            ["allow", ticket, verb @ ..] if verb.len() <= 1 => {
                self.allow(ctx, req, ticket, verb.first().copied())
            }
            ["clear", "--local"] => match self.0.clear_locally() {
                Some(lifted) => Reply::text(
                    Exit::Success,
                    format!(
                        "lifted   emergency {} on this machine only; other machines still enforce \
                         it until a session runs `ignite pool resume --all`",
                        lifted.ticket
                    ),
                ),
                None => Reply::text(Exit::Success, "none     no emergency is in force"),
            },
            _ => Reply::text(Exit::Usage, USAGE),
        }
    }
}

impl EmergencyVerb {
    fn show(&self) -> Reply {
        let lines = self.0.lines();
        if lines.is_empty() {
            return Reply::text(Exit::Success, "none     no emergency is in force");
        }
        Reply::text(Exit::Success, lines.join("\n"))
    }

    fn change(&self, ctx: &Context, change: &EmergencyChange) -> Result<Emergency, Reply> {
        match ctx.hub.emergency(change) {
            Ok(Some(emergency)) => {
                self.0.heard(Some(emergency.clone()));
                Ok(emergency)
            }
            Ok(None) => Err(Reply::text(
                Exit::Usage,
                "failed   the hub kept no emergency",
            )),
            Err(error) => Err(Reply::text(
                crate::dispatchd::hub_exit(&error),
                format!("failed   {error}"),
            )),
        }
    }

    fn declare(&self, ctx: &Context, req: &Request, ticket: &str, scope: EmergencyScope) -> Reply {
        let Some(session) = ctx.session.clone().or_else(|| req.session.clone()) else {
            return Reply::text(
                Exit::Usage,
                "failed   only a session can declare an emergency: set IGNITE_SESSION",
            );
        };
        let Ok(ticket) = TicketId::new(ticket) else {
            return Reply::text(Exit::Usage, format!("failed   not a ticket: {ticket}"));
        };
        let change = EmergencyChange::Declare {
            ticket,
            session,
            machine: ctx.machine.clone(),
            scope,
        };
        let emergency = match self.change(ctx, &change) {
            Ok(emergency) => emergency,
            Err(reply) => return reply,
        };
        Reply::text(
            Exit::Success,
            format!(
                "declared emergency {} pauses {} across the fleet\nlapses   {} unless you \
                 declare it again\nends     `ignite pool resume --all`",
                emergency.ticket,
                paused_work(emergency.scope),
                emergency.expires_at()
            ),
        )
    }

    fn allow(&self, ctx: &Context, req: &Request, ticket: &str, verb: Option<&str>) -> Reply {
        let Some(session) = ctx.session.clone().or_else(|| req.session.clone()) else {
            return Reply::text(
                Exit::Usage,
                "failed   only the declaring session can grant allowances",
            );
        };
        if let Some(verb) = verb
            && !HEAVY_VERBS.contains(&verb)
        {
            let heavy = HEAVY_VERBS.join(", ");
            return Reply::text(
                Exit::Usage,
                format!("failed   {verb} is never refused; name one of {heavy}"),
            );
        }
        let Ok(ticket) = TicketId::new(ticket) else {
            return Reply::text(Exit::Usage, format!("failed   not a ticket: {ticket}"));
        };
        let allowance = Allowance {
            ticket,
            verb: verb.map(str::to_owned),
        };
        let change = EmergencyChange::Allow {
            session,
            allowance: allowance.clone(),
        };
        match self.change(ctx, &change) {
            Ok(_) => Reply::text(
                Exit::Success,
                format!(
                    "allowed  {} during the emergency",
                    self::allowance(&allowance)
                ),
            ),
            Err(reply) => reply,
        }
    }
}

#[cfg(test)]
#[path = "emergency_tests.rs"]
mod tests;
