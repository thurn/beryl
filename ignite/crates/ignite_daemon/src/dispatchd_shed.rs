//! Shedding build tickets when this machine holds more than its limit (see
//! `ignite_core::protocol::hub::shed`).
//!
//! The hub's limit on build tickets applies only at hand-off. Each heartbeat reply with a
//! sample says how many build tickets this machine holds and what its limit is. When the
//! machine has held more than twice its limit for five minutes, the daemon sheds down to the
//! limit by itself, and `ignite pool shed` sheds at once. For each ticket shed, newest claim
//! first, the daemon does on the worker's behalf what `ignite release` does: it ends the
//! worker's turn, stops the verbs it started, ends any `ignite wait` it is blocked in, saves the
//! tree's work on the remote, releases the tree and returns the ticket to open with a comment
//! that says who shed it and why. The worker's slot is freed, so the session ends. Nothing
//! waits for the worker, which is what failed when an incident commander had to tell each
//! worker to release its ticket and several could not read the order.

use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::shed::{AUTO_SHED_AFTER_S, BuildHoldings, ShedDone};
use ignite_core::protocol::hub::{HeartbeatReply, ReleaseCause};
use ignite_core::traits::{Error, Result};

use super::HeadlessDispatch;
use crate::server::Context;

/// Who sheds when the daemon does it by itself.
pub(crate) const AUTOMATIC: &str = "Ignite, because the machine held more than twice its limit \
                                    for 5 minutes";

/// What one shed did.
#[derive(Debug, Default)]
pub struct Shed {
    /// The tickets released, with their sessions.
    pub released: Vec<(TicketId, SessionId)>,
    /// One line per ticket that could not be shed, and per verb that was stopped.
    pub notes: Vec<String>,
    /// The machine's build tickets and limit before the shed.
    pub before: Option<BuildHoldings>,
    pub to: u32,
}

impl Shed {
    /// The reply lines of `ignite pool shed`.
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self.before.iter().map(BuildHoldings::line).collect();
        if self.released.is_empty() {
            lines.push(format!(
                "shed     nothing: no ticket above {} can be shed (a landing, a ticket that \
                 holds master back or a reservation is kept)",
                self.to
            ));
        }
        for (ticket, session) in &self.released {
            lines.push(format!(
                "shed     {ticket} from {session}; its work is saved and the ticket is open"
            ));
        }
        lines.extend(self.notes.iter().cloned());
        lines
    }
}

impl HeadlessDispatch {
    /// Acts on what a heartbeat's reply says about this machine's build tickets: keeps the
    /// count for `ignite pool list`, carries out an `ignite pool shed --machine` order, and
    /// sheds by itself once the machine has held more than twice its limit for
    /// [`AUTO_SHED_AFTER_S`].
    pub fn heard_builds(&self, ctx: &Context, reply: &HeartbeatReply) {
        let now = ctx.clock.now();
        if let Some(holdings) = reply.build_tickets.clone() {
            let mut state = self.state();
            if holdings.far_over() {
                state.far_over_since.get_or_insert(now);
            } else {
                state.far_over_since = None;
            }
            state.builds = Some(holdings);
        }
        if let Some(order) = &reply.shed {
            self.shed_and_log(ctx, order.to, &order.by);
        }
        let due = self
            .state()
            .far_over_since
            .is_some_and(|since| now.seconds_since(since) >= AUTO_SHED_AFTER_S);
        if due {
            self.state().far_over_since = None;
            self.shed_and_log(ctx, None, AUTOMATIC);
        }
    }

    fn shed_and_log(&self, ctx: &Context, to: Option<u32>, by: &str) {
        match self.shed(ctx, to, by) {
            Ok(shed) => {
                for line in shed.lines() {
                    eprintln!("ignite: {line}");
                }
            }
            Err(error) => eprintln!("ignite: build tickets not shed: {error}"),
        }
    }

    /// The line `ignite pool list` and `ignite status` show, once a heartbeat reply has
    /// counted this machine's build tickets.
    pub fn build_line(&self) -> Option<String> {
        self.state().builds.as_ref().map(BuildHoldings::line)
    }

    /// Sheds this machine's build tickets down to `to`, or to its limit when `None`, as `by`.
    pub fn shed(&self, ctx: &Context, to: Option<u32>, by: &str) -> Result<Shed> {
        let Some(holdings) = self.state().builds.clone() else {
            return Err(Error::NotFound(
                "no heartbeat reply has counted this machine's build tickets yet; try again in \
                 a minute"
                    .to_owned(),
            ));
        };
        let to = to.unwrap_or(holdings.limit);
        let holders = self.host.holders();
        // A ticket this machine holds no tree for cannot be shed here, and one whose worker is
        // landing, releasing or closing it is about to end anyway.
        let picked: Vec<(TicketId, SessionId)> = holdings
            .to_shed(to, |held| {
                held.session.as_ref().is_none_or(|session| {
                    holders.get(session) != Some(&held.ticket) || self.host.ending_ticket(session)
                })
            })
            .into_iter()
            .filter_map(|held| Some((held.ticket.clone(), held.session.clone()?)))
            .collect();
        let mut shed = Shed {
            before: Some(holdings.clone()),
            to,
            ..Shed::default()
        };
        let reason = format!(
            "Shed by {by}: machine {} held {} build tickets against a limit of {}, so the newest \
             were taken back to bring it to {to}. Nothing is wrong with the ticket.",
            ctx.machine,
            holdings.held(),
            holdings.limit
        );
        for (ticket, session) in picked {
            match self.shed_one(&session, &ticket, &reason, &mut shed.notes) {
                Ok(()) => shed.released.push((ticket, session)),
                Err(error) => shed
                    .notes
                    .push(format!("kept     {ticket} with {session}: {error}")),
            }
        }
        if !shed.released.is_empty() {
            self.forget_shed(&shed.released);
            Self::record_shed(ctx, &shed, by);
        }
        Ok(shed)
    }

    /// Takes one ticket back without the worker's help. The turn ends first, so the worker
    /// cannot start another command meanwhile.
    fn shed_one(
        &self,
        session: &SessionId,
        ticket: &TicketId,
        reason: &str,
        notes: &mut Vec<String>,
    ) -> Result<()> {
        if let Some(cli) = self.cli.get()
            && let Err(error) = cli.pool.retire(session)
        {
            notes.push(format!("warning  {session}'s turn was not ended: {error}"));
        }
        notes.extend(self.host.stop_verbs(session));
        let told = format!("refused  {ticket} was shed: {reason}");
        self.host
            .interrupt(session, Reply::text(Exit::Conflict, told));
        self.host
            .release(session, ticket, reason, ReleaseCause::Shed)
    }

    /// Drops the released tickets from the count and from what the daemon tracks of their
    /// sessions, so that a second shed before the next count does not pick them again.
    fn forget_shed(&self, released: &[(TicketId, SessionId)]) {
        let mut state = self.state();
        for (ticket, session) in released {
            state.held.remove(session);
            state.stalls.remove(session);
            state.waiting.remove(session);
            if let Some(builds) = state.builds.as_mut() {
                builds.tickets.retain(|held| &held.ticket != ticket);
            }
        }
        drop(state);
        self.save();
    }

    /// Records the shed in the fleet's recent changes, so that `ignite fleet` shows it.
    fn record_shed(ctx: &Context, shed: &Shed, by: &str) {
        let tickets: Vec<&str> = shed.released.iter().map(|(t, _)| t.as_str()).collect();
        let before = shed
            .before
            .as_ref()
            .map_or_else(String::new, |holdings| format!("{}, ", holdings.line()));
        let done = ShedDone {
            machine: ctx.machine.clone(),
            summary: format!(
                "shed {}: {before}down to {}; by {by}",
                tickets.join(", "),
                shed.to
            ),
        };
        if let Err(error) = ctx.hub.shed_done(&done) {
            eprintln!(
                "ignite: the shed of {} was not recorded: {error}",
                tickets.join(", ")
            );
        }
    }
}
