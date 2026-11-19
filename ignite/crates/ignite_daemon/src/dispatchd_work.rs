//! The part of a dispatcher step that asks the hub: reading the tickets of finished sessions,
//! retrying hand-offs whose ticket could not be read, and asking for new tickets for idle
//! workers.
//!
//! The dispatcher's state is locked only for the decisions. Every hub request runs between two
//! short locks, because a slow hub would otherwise hold up whatever else needs the state, such
//! as the sender's heartbeat (see [`Dispatcher::view`]).

use ignite_core::config::Config;
use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::hub::{IdleWorker, MachineFree, Reservation};
use ignite_core::protocol::tickets::TicketDetail;
use ignite_core::time::Timestamp;

use super::{DEFAULT_EFFORT, Dispatcher, Purpose, Reserved, State, label, slots, tier_for};
use crate::pool::Member;
use crate::server::Context;

/// A ticket the hub reserved for a worker, with the ticket's details if they could be read.
type Offer = (Reservation, String, Option<TicketDetail>);

/// What a step needs to ask the hub for tickets.
struct Asking {
    idle: Vec<Member>,
    trees: u32,
    /// The pool's workers that are not down and its Codex slots, idle or not.
    workers: usize,
}

/// Clears the flag that says a step is asking the hub, however the step ends.
struct AskingGuard<'a>(&'a Dispatcher);

impl Drop for AskingGuard<'_> {
    fn drop(&mut self) {
        self.0.state().asking_hub = false;
    }
}

impl Dispatcher {
    /// Brings the state up to date: notices claims, finished tickets and lapsed reservations,
    /// retries what could not be read, gives overdue instructions again and, while workers are
    /// idle and trees are free, asks the hub for tickets.
    pub(super) fn refresh(&self, ctx: &Context, members: &[Member]) {
        let now = ctx.clock.now();
        let (finished, unread) = {
            let mut state = self.state();
            let finished = self.notice_holders(&mut state, now);
            let unread = std::mem::take(&mut state.unread)
                .into_iter()
                .filter(|(reservation, _)| {
                    reservation.expires_at > now
                        && state.reserved.contains_key(&reservation.session)
                })
                .collect::<Vec<_>>();
            (finished, unread)
        };
        for (session, ticket) in finished {
            if let Ok(detail) = ctx.hub.ticket(&ticket)
                && let Some(landed) = detail.ticket.landed
            {
                let _ = self.pool.update(&session, |m| m.files = landed.files);
            }
        }
        // A chat that left frees its slot in this step, so the members are read again.
        let fresh;
        let members = if self.end_chats() {
            fresh = self.pool.members().unwrap_or_default();
            &fresh
        } else {
            members
        };
        let unread: Vec<Offer> = unread
            .into_iter()
            .map(|(reservation, app)| {
                let detail = ctx.hub.ticket(&reservation.ticket).ok();
                (reservation, app, detail)
            })
            .collect();
        let asking = {
            let mut state = self.state();
            for (reservation, app, detail) in unread {
                if state.reserved.contains_key(&reservation.session) {
                    Self::queue_handoff(&ctx.config, &mut state, &reservation, &app, detail);
                }
            }
            Self::expire_given(&mut state, now);
            self.watch_silence(&mut state, now);
            Self::retry_orphans(&mut state, members, now);
            Self::drop_expired_handoffs(&mut state, now);
            self.plan_asking(&mut state, members, now)
        };
        let Some(asking) = asking else {
            return;
        };
        let _asking = AskingGuard(self);
        let offers = self.ask_for_work(ctx, asking);
        let mut state = self.state();
        for (reservation, app, detail) in offers {
            Self::queue_handoff(&ctx.config, &mut state, &reservation, &app, detail);
        }
    }

    /// Notices claims, finished tickets and lapsed reservations. Returns the sessions that
    /// finished a ticket and the ticket each had, for the caller to read from the hub.
    fn notice_holders(&self, state: &mut State, now: Timestamp) -> Vec<(SessionId, TicketId)> {
        let holders = self.host.holders();
        let mut finished = Vec::new();
        for (session, ticket) in &state.held {
            if !holders.contains_key(session) {
                state.last_ticket.insert(session.clone(), ticket.clone());
                state.stalls.remove(session);
                state.finished_at.insert(session.clone(), now);
                finished.push((session.clone(), ticket.clone()));
            }
        }
        let lapsed: Vec<SessionId> = state
            .reserved
            .iter()
            .filter(|(session, r)| !holders.contains_key(*session) && r.expires_at <= now)
            .map(|(session, _)| session.clone())
            .collect();
        state
            .reserved
            .retain(|session, _| !holders.contains_key(session));
        for session in lapsed {
            state.reserved.remove(&session);
            // A slot or a chat opened for this ticket is simply free again (see `slots`).
            let chat = slots::is_slot(&session)
                || self
                    .pool
                    .member(&session)
                    .ok()
                    .flatten()
                    .is_some_and(|m| m.single_use);
            if !chat {
                // A reservation nobody claimed: the worker is treated as stalled.
                Self::queue_check(state, &session);
            }
        }
        state.orphans.retain(|r| r.expires_at > now);
        state.held = holders;
        finished
    }

    /// Removes the queued hand-offs whose reservation has lapsed, and the reservations they
    /// held. Giving a worker a ticket the hub no longer holds for it would leave the worker
    /// waiting for a start that is refused.
    pub(super) fn drop_expired_handoffs(state: &mut State, now: Timestamp) {
        let mut dropped: Vec<(Option<SessionId>, TicketId)> = Vec::new();
        state.pending.retain(|queued| match &queued.purpose {
            Purpose::Handoff(reserved) if reserved.expires_at <= now => {
                dropped.push((queued.body.session.clone(), reserved.ticket.clone()));
                false
            }
            _ => true,
        });
        for (session, ticket) in dropped {
            if let Some(session) = session
                && state
                    .reserved
                    .get(&session)
                    .is_some_and(|reserved| reserved.ticket == ticket)
            {
                state.reserved.remove(&session);
            }
        }
    }

    /// Decides whether to ask the hub for tickets, and for which workers. Only one step asks
    /// at a time.
    fn plan_asking(&self, state: &mut State, members: &[Member], now: Timestamp) -> Option<Asking> {
        if state.down || state.draining || state.asking_hub || self.pool.paused().unwrap_or(true) {
            return None;
        }
        let workers = slots::with_slots(state, members);
        let idle: Vec<Member> = Self::idle_workers(state, &workers, now)
            .into_iter()
            .cloned()
            .collect();
        let reserved = u32::try_from(state.reserved.len()).unwrap_or(u32::MAX);
        let trees = self.host.free_ticket_trees().saturating_sub(reserved);
        if idle.is_empty() || trees == 0 {
            return None;
        }
        state.asking_hub = true;
        Some(Asking {
            idle,
            trees,
            workers: slots::capacity(members),
        })
    }

    /// Asks the hub for tickets while workers are idle and ticket trees are free, and reads
    /// each reserved ticket. Runs with the state unlocked.
    fn ask_for_work(&self, ctx: &Context, asking: Asking) -> Vec<Offer> {
        let Asking {
            mut idle,
            mut trees,
            workers,
        } = asking;
        let mut offers = Vec::new();
        while !idle.is_empty() && trees > 0 {
            let free = MachineFree {
                machine: ctx.machine.clone(),
                os: self.setup.os,
                idle_workers: idle
                    .iter()
                    .map(|m| IdleWorker {
                        session: m.session.clone(),
                        app: m.app.clone(),
                        models: offered_models(&ctx.config.pool, &m.app, &m.models),
                    })
                    .collect(),
                free_ticket_trees: trees,
                free_cores: self.host.free_cores(),
                admit_cores: Some(self.host.admit_cores()),
                families: self.setup.families.clone(),
                name: None,
                // The last idle worker of a pool of several is kept for justiciar tickets.
                justiciar_only: idle.len() == 1 && workers > 1,
            };
            let Ok(Some(reservation)) = ctx.hub.next(&free) else {
                break;
            };
            let Some(index) = idle.iter().position(|m| m.session == reservation.session) else {
                break;
            };
            let worker = idle.remove(index);
            trees -= 1;
            let detail = ctx.hub.ticket(&reservation.ticket).ok();
            offers.push((reservation, worker.app, detail));
        }
        offers
    }

    /// Turns a reservation into a hand-off for its worker. `detail` is the ticket as the hub
    /// has it. The model comes from its labels, so until they can be read the worker stays
    /// reserved and nothing is sent, so a ticket never runs on a model it does not name.
    pub(super) fn queue_handoff(
        config: &Config,
        state: &mut State,
        reservation: &Reservation,
        app: &str,
        detail: Option<TicketDetail>,
    ) {
        let session = &reservation.session;
        let ticket = &reservation.ticket;
        let expires_at = reservation.expires_at;
        let code = Some(reservation.code.clone());
        let Some(detail) = detail else {
            let placeholder = Reserved {
                ticket: ticket.clone(),
                expires_at,
                code,
                tier: None,
                model: None,
                effort: String::new(),
                app: app.to_owned(),
            };
            state.reserved.insert(session.clone(), placeholder);
            state.unread.push((reservation.clone(), app.to_owned()));
            return;
        };
        let labels = detail.ticket.labels;
        let tier = tier_for(&labels, app);
        // The hub is only told about tiers the app has a model ID for (see `ask_for_work`), so
        // a tier without one means `ignite.toml` changed after the hub was asked. Reserving the
        // worker anyway would hand the dispatcher a model it cannot set, so the worker is left
        // idle and the reservation lapses at the hub.
        let model = match &tier {
            Some(tier) => match config.pool.model_id(app, tier) {
                Some(id) => Some(id),
                None => return,
            },
            None => None,
        };
        let reserved = Reserved {
            ticket: ticket.clone(),
            expires_at,
            code,
            tier,
            model,
            effort: label(&labels, "effort:")
                .unwrap_or(DEFAULT_EFFORT)
                .to_owned(),
            app: app.to_owned(),
        };
        Self::queue_handoff_reserved(state, session, reserved);
    }
}

/// The tiers the hub is told a worker of `app` runs. A model the app has no ID for cannot be
/// set, so the hub is not told the worker runs it and offers no ticket that needs it. A member
/// added without a list, as a Codex worker is, runs every tier its app's `[pool.models.<app>]`
/// table names, so that a ticket routed to that app is not held back by its `tier:` label.
fn offered_models(
    pool: &ignite_core::config::PoolConfig,
    app: &str,
    listed: &[String],
) -> Vec<String> {
    if listed.is_empty() {
        return pool
            .models
            .get(app)
            .map(|table| table.keys().cloned().collect())
            .unwrap_or_default();
    }
    listed
        .iter()
        .filter(|tier| pool.model_id(app, tier).is_some())
        .cloned()
        .collect()
}

#[cfg(test)]
mod offered_models_tests {
    use super::offered_models;
    use ignite_core::config::PoolConfig;
    use std::collections::BTreeMap;

    #[test]
    fn a_worker_without_a_list_runs_every_tier_of_its_app() {
        let table = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect()
        };
        let pool = PoolConfig {
            models: BTreeMap::from([
                (
                    "claude".to_owned(),
                    table(&[("opus", "o"), ("sonnet", "s")]),
                ),
                (
                    "codex".to_owned(),
                    table(&[("opus", "sol"), ("sonnet", "sol")]),
                ),
            ]),
            default_owner: None,
        };
        assert_eq!(offered_models(&pool, "codex", &[]), ["opus", "sonnet"]);
        let listed = ["opus".to_owned(), "light".to_owned()];
        assert_eq!(offered_models(&pool, "claude", &listed), ["opus"]);
        assert!(offered_models(&pool, "other", &[]).is_empty());
    }
}
