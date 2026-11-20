//! The part of a dispatcher step that asks the hub: reading the tickets of finished sessions,
//! retrying hand-offs whose ticket could not be read, and asking for new tickets for idle
//! workers.
//!
//! The dispatcher's state is locked only for the decisions. Every hub request runs between two
//! short locks, because a slow hub would otherwise hold up whatever else needs the state, such
//! as the sender's heartbeat (see [`Dispatcher::view`]).

use ignite_core::config::Config;
use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::hub::{IdleWorker, MachineFree, NextReply, Reservation};
use ignite_core::protocol::tickets::TicketDetail;
use ignite_core::time::Timestamp;

use super::{
    DEFAULT_EFFORT, Dispatcher, Purpose, Reserved, State, cli, feed, label, shared, slots, suits,
    tier_for,
};
use crate::clid::Resume;
use crate::pool::Member;
use crate::server::Context;

/// A ticket the hub reserved for a worker, with the ticket's details if they could be read.
type Offer = (Reservation, String, Option<TicketDetail>);

/// What a step needs to ask the hub for tickets.
struct Asking {
    idle: Vec<Member>,
    /// The idle CLI slots that take only tickets that hold `master` back (see `slots`).
    extra: Vec<Member>,
    trees: u32,
    /// The pool's workers that are not down and its Codex slots, idle or not.
    workers: usize,
    /// The pool is paused, so the hub may offer only a ticket that holds `master` back.
    master_only: bool,
    /// Seconds the oldest request of ticket work has waited for cores, when that is at least
    /// [`BACKLOG_SECONDS`]: the hub is then asked only for tickets that hold `master` back.
    backlog: Option<f64>,
    /// Why only tickets that hold `master` back start while a shared failure lasts (see
    /// `shared`).
    shared: Option<String>,
}

/// How long ticket work may wait for cores before the dispatcher stops starting ordinary
/// tickets. On 2026-11-20 jarvis kept starting workers while 16 to 18 commands already waited
/// for its 15 cores. Each new worker's commands joined the queue, so every command waited
/// longer, and a one-card change's `ignite check` ran 80 minutes without finishing. Admission by
/// the tickets' expected cores did not stop it: those figures are history and say nothing about
/// the queue now. Two minutes is the longest wait for cores a short change should see; past it
/// the machine has more work than cores, and a new ticket only makes every ticket slower.
pub const BACKLOG_SECONDS: f64 = 120.0;

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
    /// idle and trees are free, asks the hub for tickets. Then starts the CLI sessions and sends
    /// the CLI messages and checks this decided on (see `cli`).
    pub(super) fn refresh(&self, ctx: &Context, members: &[Member]) {
        self.update_cli_layout();
        self.refresh_state(ctx, members);
        self.run_cli_calls(ctx);
    }

    fn refresh_state(&self, ctx: &Context, members: &[Member]) {
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
                    Self::queue_handoff(
                        &ctx.config,
                        &mut state,
                        &reservation,
                        &app,
                        detail,
                        members,
                        now,
                    );
                }
            }
            Self::expire_given(&mut state, now);
            self.watch_silence(&mut state, now);
            self.watch_shared(&mut state, members, now);
            Self::drop_claimed_handoffs(&mut state);
            Self::retry_orphans(&mut state, members, now);
            Self::drop_expired_handoffs(&mut state, now);
            self.plan_asking(&mut state, members, now)
        };
        let Some(asking) = asking else {
            return;
        };
        let _asking = AskingGuard(self);
        let (offers, idle_reason) = self.ask_for_work(ctx, asking);
        let mut state = self.state();
        state.idle_reason = idle_reason;
        for (reservation, app, detail) in offers {
            Self::queue_handoff(
                &ctx.config,
                &mut state,
                &reservation,
                &app,
                detail,
                members,
                now,
            );
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
                || cli::is_cli_slot(&session)
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

    /// Removes the hand-offs waiting for a worker (the orphans, and the queued hand-offs not yet
    /// given to the dispatcher) whose ticket a session of this machine already holds, and the
    /// reservations they held.
    ///
    /// A hand-off whose acknowledgement failed can still reach its worker: on 2026-11-20 the
    /// desktop app delivered hand-off prompts five to eight minutes after they were sent. That
    /// worker then claims the ticket, while the reservation waits as an orphan for another idle
    /// worker. Handing it on gave a second worker a start that was refused because another
    /// session held the ticket, and that worker stopped unfinished.
    pub(super) fn drop_claimed_handoffs(state: &mut State) {
        let claimed: std::collections::BTreeSet<TicketId> = state.held.values().cloned().collect();
        if claimed.is_empty() {
            return;
        }
        state.orphans.retain(|r| !claimed.contains(&r.ticket));
        let outdated: Vec<(Option<SessionId>, TicketId)> = state
            .pending
            .iter()
            .filter_map(|queued| match &queued.purpose {
                Purpose::Handoff(reserved) if claimed.contains(&reserved.ticket) => Some((
                    slots::handoff_session(state, queued),
                    reserved.ticket.clone(),
                )),
                _ => None,
            })
            .collect();
        state.pending.retain(|queued| {
            !matches!(&queued.purpose, Purpose::Handoff(reserved) if claimed.contains(&reserved.ticket))
        });
        for (session, ticket) in outdated {
            // The holder's own reservation was already removed when its claim was noticed.
            if let Some(session) = session
                && !state.held.contains_key(&session)
            {
                Self::drop_placeholder(state, &session, &ticket);
            }
        }
    }

    /// Decides whether to ask the hub for tickets, and for which workers. Only one step asks
    /// at a time.
    fn plan_asking(&self, state: &mut State, members: &[Member], now: Timestamp) -> Option<Asking> {
        // A paused pool still asks, for the tickets that hold `master` back: a regression or
        // justiciar ticket left until `ignite pool resume` would keep `master` stuck while
        // nobody watches. The hub offers such tickets to one worker at a time, and nothing at
        // all while every machine is paused (`ignite pool pause --all`).
        let master_only = self.pool.paused().unwrap_or(true);
        if state.asking_hub {
            return None;
        }
        if state.draining || master_only && self.pool.paused_everywhere() {
            state.idle_reason = None;
            return None;
        }
        let workers = slots::with_slots(state, members);
        let mut idle: Vec<Member> = Self::idle_workers(state, &workers, now)
            .into_iter()
            // An app in CLI mode is offered only its CLI slots; its desktop workers finish.
            .filter(|m| cli::is_cli_slot(&m.session) || !cli::in_cli_mode(state, &m.app))
            .cloned()
            .collect();
        // A dispatcher that stopped answering stops only the hand-offs it would carry out.
        if state.down {
            idle.retain(|m| cli::is_cli_slot(&m.session));
            if idle.is_empty() {
                state.idle_reason = None;
                return None;
            }
        }
        let (extra, idle): (Vec<Member>, Vec<Member>) = idle
            .into_iter()
            .partition(|m| cli::is_extra_slot(state, &m.session));
        let reserved = u32::try_from(state.reserved.len()).unwrap_or(u32::MAX);
        let trees = self.host.free_ticket_trees().saturating_sub(reserved);
        if idle.is_empty() && extra.is_empty() || trees == 0 {
            let waiting = idle.len() + extra.len();
            state.idle_reason =
                (waiting > 0).then(|| (waiting, "no ticket tree is free".to_owned()));
            return None;
        }
        let waited = self.host.ticket_work_wait();
        state.asking_hub = true;
        Some(Asking {
            idle,
            extra,
            trees,
            workers: slots::capacity(state, members),
            master_only,
            backlog: (waited >= BACKLOG_SECONDS).then_some(waited),
            shared: state
                .shared
                .current
                .as_ref()
                .map(shared::SharedFailure::reason),
        })
    }

    /// How many workers were left idle after the last request for tickets, and why: the hub's
    /// reason when it offered nothing, for `ignite status`. `None` when no worker was idle or the
    /// pool asked for nothing (down, updating or paused everywhere).
    pub fn idle_reason(&self) -> Option<(usize, String)> {
        self.state().idle_reason.clone()
    }

    /// Asks the hub for tickets while workers are idle and ticket trees are free, and reads
    /// each reserved ticket. Runs with the state unlocked. Also returns how many workers are
    /// left idle and why, or `None` when every one got a ticket.
    ///
    /// While the machine is on a usage hold (see `ignite_core::plan_usage`), a Claude worker is
    /// not offered the tiers whose model a held window counts. A Claude worker left with no tier
    /// is asked for separately, only for the tickets that hold `master` back, which the hub gives
    /// a machine one at a time, as it does for a paused machine.
    fn ask_for_work(&self, ctx: &Context, asking: Asking) -> (Vec<Offer>, Option<(usize, String)>) {
        let Asking {
            idle,
            extra,
            mut trees,
            workers,
            master_only,
            backlog,
            shared,
        } = asking;
        // While ticket work waits long for cores, only a ticket that holds `master` back starts:
        // the pool asks as a paused pool does.
        let master_only = master_only || backlog.is_some() || shared.is_some();
        let holds = super::usage_holds(
            &self.pool,
            ctx.config.pool.usage_hold_percent,
            ctx.clock.now(),
        );
        let mut free = Vec::new();
        let mut held = Vec::new();
        for member in idle {
            let models = offered_models(&ctx.config.pool, &member.app, &member.models);
            if member.app != "claude" || holds.is_empty() {
                free.push((member, models));
                continue;
            }
            let open: Vec<String> = models
                .iter()
                .filter(|tier| {
                    let id = ctx
                        .config
                        .pool
                        .model_id(&member.app, tier)
                        .unwrap_or_else(|| (*tier).clone());
                    !holds.iter().any(|hold| hold.holds_model(&id))
                })
                .cloned()
                .collect();
            if open.is_empty() {
                held.push((member, models));
            } else {
                free.push((member, open));
            }
        }
        let mut offers = Vec::new();
        let mut why = self.ask_group(
            ctx,
            &mut free,
            &mut trees,
            workers,
            master_only,
            &mut offers,
        );
        if let Some(waited) = backlog
            && !free.is_empty()
        {
            why = format!(
                "commands have waited {waited:.0} s for cores, so only tickets that hold master \
                 back start ({why})"
            );
        }
        if let Some(reason) = &shared
            && !free.is_empty()
        {
            why = format!("{reason} ({why})");
        }
        if !held.is_empty() {
            let held_why = self.ask_group(ctx, &mut held, &mut trees, workers, true, &mut offers);
            if !held.is_empty() {
                let windows: Vec<String> = holds
                    .iter()
                    .map(ignite_core::plan_usage::Hold::describe)
                    .collect();
                why = format!(
                    "the machine is on a usage hold ({}), so only tickets that hold master back \
                     start ({held_why})",
                    windows.join("; ")
                );
            }
        }
        // The extra CLI slots are asked last, so that a ticket that holds `master` back goes to
        // an ordinary slot while one is free.
        let mut extra: Vec<(Member, Vec<String>)> = extra
            .into_iter()
            .map(|member| {
                let models = offered_models(&ctx.config.pool, &member.app, &member.models);
                (member, models)
            })
            .collect();
        if !extra.is_empty() {
            let extra_why = self.ask_group(ctx, &mut extra, &mut trees, workers, true, &mut offers);
            if free.is_empty() && held.is_empty() && !extra.is_empty() {
                why = format!(
                    "only tickets that hold master back go to the extra CLI slot ({extra_why})"
                );
            }
        }
        let idle_left = free.len() + held.len() + extra.len();
        let left = (idle_left > 0).then_some((idle_left, why));
        (offers, left)
    }

    /// Asks the hub for tickets for `group`, each idle worker with the tiers it may be offered,
    /// while it has workers and ticket trees are free. A worker that gets a ticket leaves the
    /// group. Returns why the rest got none.
    fn ask_group(
        &self,
        ctx: &Context,
        group: &mut Vec<(Member, Vec<String>)>,
        trees: &mut u32,
        workers: usize,
        master_only: bool,
        offers: &mut Vec<Offer>,
    ) -> String {
        let mut why = "no ticket tree is free".to_owned();
        while !group.is_empty() && *trees > 0 {
            let free = MachineFree {
                machine: ctx.machine.clone(),
                os: self.setup.os,
                idle_workers: group
                    .iter()
                    .map(|(m, models)| IdleWorker {
                        session: m.session.clone(),
                        app: m.app.clone(),
                        models: models.clone(),
                    })
                    .collect(),
                free_ticket_trees: *trees,
                free_cores: self.host.free_cores(),
                admit_cores: Some(self.host.admit_cores()),
                families: self.setup.families.clone(),
                name: None,
                // The last idle worker of a pool of several is kept for justiciar tickets.
                // A paused pool asks for justiciar and regression tickets alike.
                // An app in CLI mode keeps its extra slot for them instead (see `slots`).
                justiciar_only: !master_only
                    && group.len() == 1
                    && workers > 1
                    && !cli::is_cli_slot(&group[0].0.session),
                master_only,
                // `next_explained` asks for the reason itself.
                explain: false,
            };
            let reservation = match ctx.hub.next_explained(&free) {
                Ok(NextReply::Reserved(reservation)) => reservation,
                Ok(NextReply::Nothing(reason)) => {
                    why = reason.unwrap_or_else(|| "the hub gave no reason".to_owned());
                    break;
                }
                Err(error) => {
                    why = format!("the hub did not answer: {error}");
                    break;
                }
            };
            let Some(index) = group
                .iter()
                .position(|(m, _)| m.session == reservation.session)
            else {
                "the hub reserved a ticket for a worker that is not idle".clone_into(&mut why);
                break;
            };
            let (worker, _) = group.remove(index);
            *trees -= 1;
            let detail = ctx.hub.ticket(&reservation.ticket).ok();
            offers.push((reservation, worker.app, detail));
        }
        why
    }

    /// Turns a reservation into a hand-off. `detail` is the ticket as the hub has it. The model
    /// comes from its labels, so until they can be read the worker stays reserved and nothing is
    /// sent, so a ticket never runs on a model it does not name; the hand-off is decided again
    /// when they arrive.
    ///
    /// A reservation's code may be used by any session of the machine it was made for, so the
    /// worker it goes to is decided here:
    ///
    /// - A regression goes to the idle worker whose last ticket its `caused-by` line names, which
    ///   still has the conversation in which it made that change. The hub decides which machine
    ///   works a regression; this only picks among this machine's workers.
    /// - A reservation for a worker that was meanwhile given a regression in its place goes to
    ///   another idle worker, or waits for one with the refused hand-offs.
    /// - Every other reservation goes to the worker the hub named.
    pub(super) fn queue_handoff(
        config: &Config,
        state: &mut State,
        reservation: &Reservation,
        app: &str,
        detail: Option<TicketDetail>,
        members: &[Member],
        now: Timestamp,
    ) {
        let named = &reservation.session;
        let ticket = &reservation.ticket;
        let expires_at = reservation.expires_at;
        let code = Some(reservation.code.clone());
        // The named worker holds another reservation when an earlier hand-off of this step gave
        // it a regression it caused.
        let taken = state
            .reserved
            .get(named)
            .is_some_and(|reserved| reserved.ticket != *ticket);
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
            if !taken {
                state.reserved.insert(named.clone(), placeholder);
            }
            state.unread.push((reservation.clone(), app.to_owned()));
            return;
        };
        let labels = &detail.ticket.labels;
        let tier = tier_for(labels, app);
        // The hub is only told about tiers the app has a model ID for (see `ask_for_work`), so
        // a tier without one means `ignite.toml` changed after the hub was asked. Reserving the
        // worker anyway would hand the dispatcher a model it cannot set, so the worker is left
        // idle and the reservation lapses at the hub.
        let model = match &tier {
            Some(tier) => {
                let Some(id) = config.pool.model_id(app, tier) else {
                    Self::drop_placeholder(state, named, ticket);
                    return;
                };
                Some(id)
            }
            None => None,
        };
        let reserved = Reserved {
            ticket: ticket.clone(),
            expires_at,
            code,
            tier,
            model,
            effort: label(labels, "effort:")
                .unwrap_or(DEFAULT_EFFORT)
                .to_owned(),
            app: app.to_owned(),
        };
        if let Some(culprit) = Self::culprit_worker(state, &detail, &reserved, members, now) {
            Self::drop_placeholder(state, named, ticket);
            if detail.ticket.metadata.contains_key("reverted") {
                Self::queue_reverted_handoff(state, &culprit, reserved, &detail);
            } else {
                Self::queue_regression_handoff(state, &culprit, reserved, &detail.ticket.title);
            }
            return;
        }
        // A CLI worker's session ended with its ticket, so no idle worker holds the culprit's
        // conversation; the slot resumes its transcript instead, when it was kept.
        if !taken
            && cli::is_cli_slot(named)
            && let Some(culprit) = feed::cause(&detail)
        {
            let prompt = feed::culprit_prompt(&detail, &reserved);
            state.reserved.insert(named.clone(), reserved.clone());
            state.cli_calls.push(cli::Call::Begin {
                slot: named.clone(),
                reserved,
                resume: Some(Resume { culprit, prompt }),
            });
            return;
        }
        if !taken {
            Self::queue_handoff_reserved(state, named, reserved);
            return;
        }
        let workers = slots::with_slots(state, members);
        let other = Self::idle_workers(state, &workers, now)
            .into_iter()
            .find(|m| suits(m, reserved.tier.as_ref(), &reserved.app))
            .map(|m| m.session.clone());
        match other {
            Some(other) => Self::queue_handoff_reserved(state, &other, reserved),
            None => state.orphans.push(reserved),
        }
    }

    /// The idle worker here whose last ticket caused `detail`, a regression, or whose last ticket
    /// `detail` is, a ticket that was reverted, if one can run it.
    fn culprit_worker(
        state: &State,
        detail: &TicketDetail,
        reserved: &Reserved,
        members: &[Member],
        now: Timestamp,
    ) -> Option<SessionId> {
        let cause = feed::cause(detail)?;
        let workers = slots::with_slots(state, members);
        Self::idle_workers(state, &workers, now)
            .into_iter()
            .filter(|m| suits(m, reserved.tier.as_ref(), &reserved.app))
            .find(|m| state.last_ticket.get(&m.session) == Some(&cause))
            .map(|m| m.session.clone())
    }

    /// Offers a reopened ticket whose landing a tip round reverted to the worker that landed
    /// it. It keeps its conversation, as the culprit of a regression does.
    fn queue_reverted_handoff(
        state: &mut State,
        session: &SessionId,
        reserved: Reserved,
        detail: &TicketDetail,
    ) {
        let prompt = feed::reverted_prompt(detail, &reserved);
        Self::push_handoff(state, session, reserved, prompt, false);
    }

    /// Removes the placeholder reservation of `ticket` for `session`, left while the ticket's
    /// details could not be read, so that the worker is idle again.
    fn drop_placeholder(state: &mut State, session: &SessionId, ticket: &TicketId) {
        if state
            .reserved
            .get(session)
            .is_some_and(|reserved| reserved.ticket == *ticket)
        {
            state.reserved.remove(session);
        }
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
            usage_hold_percent: 99,
            cli: BTreeMap::new(),
        };
        assert_eq!(offered_models(&pool, "codex", &[]), ["opus", "sonnet"]);
        let listed = ["opus".to_owned(), "light".to_owned()];
        assert_eq!(offered_models(&pool, "claude", &listed), ["opus"]);
        assert!(offered_models(&pool, "other", &[]).is_empty());
    }
}
