//! Connects the `ignite-cli` loop's [`Effects`] to [`Health`] and records each ended turn's
//! cost and token counts in the hub's metrics outbox.

use std::sync::Arc;

use serde_json::json;

use ignite_core::cli::{HoldReason, Slot, TurnEnd, TurnEvents, UsageSample};
use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::hub::MetricsImport;
use ignite_core::protocol::tickets::Status;
use ignite_core::traits::{Error, Result};

use super::{Health, PLAN_LIMIT_MESSAGE, TURN_SOURCE, reports_plan_usage};
use crate::clid::Effects;
use crate::pool::Pool;
use crate::server::Context;

/// Queues turn `turn` of `session`'s cost and token counts for the hub's metrics under
/// [`TURN_SOURCE`], with its ticket. A turn with no result has no cost and queues nothing.
pub fn record_turn(
    pool: &Pool,
    ctx: &Context,
    app: &str,
    session: &SessionId,
    ticket: Option<&TicketId>,
    turn: u32,
    events: &TurnEvents,
) -> Result<()> {
    if events.cost_usd.is_none() && events.tokens.is_none() {
        return Ok(());
    }
    let now = ctx.clock.now();
    let tokens = events.tokens.unwrap_or_default();
    let import = MetricsImport {
        machine: ctx.machine.clone(),
        source: TURN_SOURCE.to_owned(),
        rows: vec![json!({
            "at": now.to_string(),
            "app": app,
            "session": session.as_str(),
            "ticket": ticket.map(TicketId::as_str),
            "turn": turn,
            "model": events.model,
            "cli_version": events.cli_version,
            "is_error": events.result_error,
            "cost_usd": events.cost_usd,
            "input_tokens": tokens.input,
            "output_tokens": tokens.output,
            "cache_read_tokens": tokens.cache_read,
            "cache_write_tokens": tokens.cache_write,
        })],
    };
    pool.with_database(|conn| crate::outbox::push_metrics(conn, &import, now))
}

/// The `ignite-cli` loop's [`Effects`]: usage samples, each ended turn's cost and plan-limit
/// sample, and CLI holds go to [`Health`], and a slot is offered only once a probe has passed.
pub struct HealthEffects {
    pub health: Arc<Health>,
    /// The daemon's context, for the hub, the clock, the machine and `[pool]`.
    pub ctx: Context,
}

impl Effects for HealthEffects {
    fn usage(&self, slot: &Slot, samples: &[UsageSample]) {
        let now = self.ctx.clock.now();
        if let Err(error) = self.health.record_usage(&self.ctx, &slot.app, samples, now) {
            eprintln!("ignite: CLI usage samples not stored: {error}");
        }
    }

    fn ended(&self, slot: &Slot, end: &TurnEnd, events: &TurnEvents) {
        let now = self.ctx.clock.now();
        let dialect = self.ctx.config.cli_dialect(&slot.app);
        match end {
            TurnEnd::PlanLimit { window } if reports_plan_usage(dialect) => {
                let agy_samples = self.health.record_agy_usage(&self.ctx, &slot.app);
                let samples = if agy_samples.is_empty() {
                    &events.usage
                } else {
                    &agy_samples
                };
                if let Err(error) =
                    self.health
                        .plan_limit(&self.ctx, &slot.app, window.as_deref(), samples, now)
                {
                    eprintln!("ignite: the plan-limit sample was not stored: {error}");
                }
            }
            TurnEnd::PlanLimit { .. } => self.hold(
                &slot.app,
                HoldReason::PlanLimit,
                PLAN_LIMIT_MESSAGE,
                events.cli_version.as_deref(),
            ),
            _ => {
                self.health
                    .record_codex_usage(&self.ctx, &slot.app, events, slot.since);
                self.health.record_agy_usage(&self.ctx, &slot.app);
            }
        }
        if let Some(session) = &slot.session
            && let Err(error) = record_turn(
                &self.health.pool,
                &self.ctx,
                &slot.app,
                session,
                slot.ticket.as_ref(),
                slot.turn,
                events,
            )
        {
            eprintln!("ignite: the cost of {session}'s turn was not queued: {error}");
        }
    }

    fn handoff(&self, slot: &Slot, note: &str, resumed: bool) {
        use ignite_core::protocol::tickets::{BatchRequest, CULPRIT_HANDOFF_KEY, Operation};

        let (Some(ticket), Some(session)) = (&slot.ticket, &slot.session) else {
            return;
        };
        let request = BatchRequest {
            preconditions: Vec::new(),
            ops: vec![Operation::Update {
                id: ticket.to_string(),
                set: None,
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: std::collections::BTreeMap::from([(
                    CULPRIT_HANDOFF_KEY.to_owned(),
                    serde_json::json!({"session": session, "note": note, "resumed": resumed}),
                )]),
                unset_meta: Vec::new(),
            }],
        };
        if let Err(error) = self.ctx.hub.batch(&request) {
            eprintln!("ignite: the hand-off of {ticket} was not recorded: {error}");
        }
    }

    fn hold(&self, app: &str, reason: HoldReason, message: &str, cli_version: Option<&str>) {
        let now = self.ctx.clock.now();
        if let Err(error) = self
            .health
            .start_hold(app, reason, message, cli_version, now)
        {
            eprintln!("ignite: the {app} CLI hold was not stored: {error}");
        }
    }

    fn probed(&self, app: &str) -> bool {
        self.health.probed().contains(app)
    }

    fn claim_is_current(&self, session: &SessionId, ticket: &TicketId) -> Result<bool> {
        match self.ctx.hub.ticket(ticket) {
            Ok(detail) => Ok(detail.ticket.status == Status::InProgress
                && detail.ticket.assignee.as_deref() == Some(session.as_str())
                && detail.ticket.session.as_ref() == Some(session)
                && detail.ticket.machine.as_ref() == Some(&self.ctx.machine)),
            Err(Error::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }
}
