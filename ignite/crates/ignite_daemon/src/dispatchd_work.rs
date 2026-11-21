//! Asks the hub for tickets when CLI slots and trees are free.
//! Hub requests run outside the state lock so heartbeat reporting can continue.

use ignite_core::config::Config;
use ignite_core::dispatch_pressure::{BACKLOG_SECONDS, DispatchPressure};
use ignite_core::fleet::MachineState;
use ignite_core::ids::{SessionId, TicketId};
use ignite_core::protocol::hub::{IdleWorker, MachineFree, NextReply, Reservation};
use ignite_core::protocol::tickets::TicketDetail;
use ignite_core::time::Timestamp;

use super::{Dispatcher, REFUSED_S, Reserved, State, cli, feed, label, shared, workers};
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
    /// The pool is paused, so the hub may offer only a ticket that holds `master` back.
    master_only: bool,
    /// Seconds the oldest request of ticket work has waited for cores, when that is at least
    /// [`BACKLOG_SECONDS`]: the hub is then asked only for tickets that hold `master` back.
    backlog: Option<f64>,
    /// Why only tickets that hold `master` back start while a shared failure lasts (see
    /// `shared`).
    shared: Option<String>,
}

/// Clears the flag that says a step is asking the hub, however the step ends.
struct AskingGuard<'a>(&'a Dispatcher);

impl Drop for AskingGuard<'_> {
    fn drop(&mut self) {
        self.0.state().asking_hub = false;
    }
}

impl Dispatcher {
    pub(crate) fn pressure(&self, now: Timestamp) -> Option<DispatchPressure> {
        self.host.pressure(now)
    }

    /// Queues a lane's successor for the session that landed its predecessor. The sender
    /// offers the same reservation again until it is claimed or expires.
    pub fn chain(
        &self,
        ctx: &Context,
        predecessor: &TicketId,
        reservation: &Reservation,
        fresh: bool,
    ) {
        let session = &reservation.session;
        if reservation.expires_at <= ctx.clock.now() || self.host.holders().contains_key(session) {
            return;
        }
        let Some(app) = cli::session_app(session) else {
            return;
        };
        let Some(cli) = self.cli.get() else { return };
        // The predecessor's turn must free the slot before the successor can start.
        if cli.pool.check(session) != crate::clid::SessionCheck::Gone {
            return;
        }
        let Ok(detail) = ctx.hub.ticket(&reservation.ticket) else {
            return;
        };
        let Some(reserved) = handoff_details(&ctx.config, reservation, app, &detail) else {
            return;
        };
        let mut state = self.state();
        if state
            .reserved
            .get(session)
            .is_some_and(|queued| queued.ticket != *predecessor)
            || state
                .reserved
                .values()
                .any(|queued| queued.ticket == reserved.ticket && queued.code == reserved.code)
        {
            return;
        }
        Self::begin_in_slot(&mut state, session, reserved, None, fresh);
        drop(state);
        self.save();
        self.run_cli_calls(ctx);
    }

    /// Updates claims and reservations, retries unread tickets and asks the hub for work.
    /// Starts reserved workers and delivers pending checks and messages.
    pub fn tick(&self, ctx: &Context) {
        self.host.observe_queue(ctx.clock.now());
        self.update_cli_layout();
        let mut members = self.pool.members().unwrap_or_default();
        self.refresh_state(ctx, &mut members);
        self.run_cli_calls(ctx);
    }

    fn refresh_state(&self, ctx: &Context, members: &mut Vec<Member>) {
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
        self.retire_workers(members);
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
            self.watch_silence(&mut state, now);
            self.watch_shared(&mut state, now);
            self.plan_asking(&mut state, members)
        };
        let Some(asking) = asking else {
            return;
        };
        let _asking = AskingGuard(self);
        let (offers, idle_reason) = self.ask_for_work(ctx, asking);
        let mut state = self.state();
        state.idle_reason = idle_reason;
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
                state.stalls.remove(session);
                finished.push((session.clone(), ticket.clone()));
            }
        }
        state.reserved.retain(|session, reserved| {
            !holders.contains_key(session) && reserved.expires_at > now
        });
        state
            .refused
            .retain(|_, at| now.seconds_since(*at) < REFUSED_S);
        state.held = holders;
        finished
    }

    /// Decides whether to ask the hub for tickets, and for which workers. Only one step asks
    /// at a time.
    fn plan_asking(&self, state: &mut State, members: &[Member]) -> Option<Asking> {
        if state.asking_hub {
            return None;
        }
        let machine = crate::fleet_notice::current(&self.pool).map(|notice| notice.state);
        if state.draining || self.pool.paused_everywhere() || machine == Some(MachineState::Off) {
            state.idle_reason = None;
            return None;
        }
        let master_only = machine.map_or_else(
            || self.pool.paused().unwrap_or(true),
            |state| state == MachineState::ChecksOnly,
        );
        let (extra, idle): (Vec<Member>, Vec<Member>) = workers::cli_placeholders(state, members)
            .into_iter()
            .filter(|m| !state.reserved.contains_key(&m.session))
            .filter(|m| !state.refused.contains_key(&m.session))
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
            master_only,
            backlog: (waited >= f64::from(BACKLOG_SECONDS)).then_some(waited),
            shared: state
                .shared
                .current
                .as_ref()
                .map(shared::SharedFailure::reason),
        })
    }

    /// How many workers were left idle after the last request for tickets, and why: the hub's
    /// reason when it offered nothing, for `ignite status`. `None` when no worker was idle or the
    /// pool asked for nothing (updating or paused everywhere).
    pub fn idle_reason(&self) -> Option<(usize, String)> {
        self.state().idle_reason.clone()
    }

    /// Asks the hub for tickets while workers are idle and ticket trees are free, and reads
    /// each reserved ticket. Runs with the state unlocked. Also returns how many workers are
    /// left idle and why, or `None` when every one got a ticket.
    fn ask_for_work(&self, ctx: &Context, asking: Asking) -> (Vec<Offer>, Option<(usize, String)>) {
        let Asking {
            idle: mut free,
            mut extra,
            mut trees,
            master_only,
            backlog,
            shared,
        } = asking;
        // While ticket work waits long for cores, only a ticket that holds `master` back starts:
        // the pool asks as a paused pool does.
        let master_only = master_only || backlog.is_some() || shared.is_some();
        let mut offers = Vec::new();
        let mut why = self.ask_group(ctx, &mut free, &mut trees, master_only, &mut offers);
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
        // The extra CLI slots are asked last, so that a ticket that holds `master` back goes to
        // an ordinary slot while one is free.
        if !extra.is_empty() {
            let extra_why = self.ask_group(ctx, &mut extra, &mut trees, true, &mut offers);
            if free.is_empty() && !extra.is_empty() {
                why = format!(
                    "only tickets that hold master back go to the extra CLI slot ({extra_why})"
                );
            }
        }
        let idle_left = free.len() + extra.len();
        let left = (idle_left > 0).then_some((idle_left, why));
        (offers, left)
    }

    /// Asks the hub for tickets for `group`, each idle worker with the tiers it may be offered,
    /// while it has workers and ticket trees are free. A worker that gets a ticket leaves the
    /// group. Returns why the rest got none.
    fn ask_group(
        &self,
        ctx: &Context,
        group: &mut Vec<Member>,
        trees: &mut u32,
        master_only: bool,
        offers: &mut Vec<Offer>,
    ) -> String {
        let mut why = "no ticket tree is free".to_owned();
        while !group.is_empty() && *trees > 0 {
            if crate::fleet_notice::is_off(&self.pool) {
                return "this machine is off".to_owned();
            }
            let free = MachineFree {
                room: None,
                machine: ctx.machine.clone(),
                os: self.setup.os,
                idle_workers: group
                    .iter()
                    .map(|m| IdleWorker {
                        session: m.session.clone(),
                        app: m.app.clone(),
                        models: m.models.clone(),
                        model_ids: m
                            .models
                            .iter()
                            .filter_map(|tier| {
                                ctx.config
                                    .pool
                                    .model_id(&m.app, tier)
                                    .map(|id| (tier.clone(), id))
                            })
                            .collect(),
                        default_model: crate::clid::health::default_model(&ctx.config, &m.app),
                    })
                    .collect(),
                free_ticket_trees: *trees,
                free_cores: self.host.free_cores(),
                admit_cores: Some(self.host.admit_cores()),
                pressure: self.pressure(ctx.clock.now()),
                families: self.setup.families.clone(),
                name: None,
                justiciar_only: false,
                master_only,
                // `next_explained` asks for the reason itself.
                explain: false,
            };
            let reservation = match ctx.hub.next_explained(&free) {
                Ok(NextReply::Reserved(reservation)) => reservation,
                Ok(NextReply::ReclaimDisk { reason, target_gb }) => {
                    self.host.reclaim_for_handoff(target_gb);
                    why = reason;
                    break;
                }
                Ok(NextReply::Nothing(reason)) => {
                    why = reason.unwrap_or_else(|| "the hub gave no reason".to_owned());
                    break;
                }
                Err(error) => {
                    why = format!("the hub did not answer: {error}");
                    break;
                }
            };
            let Some(index) = group.iter().position(|m| m.session == reservation.session) else {
                "the hub reserved a ticket for a worker that is not idle".clone_into(&mut why);
                break;
            };
            let worker = group.remove(index);
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
    pub(super) fn queue_handoff(
        config: &Config,
        state: &mut State,
        reservation: &Reservation,
        app: &str,
        detail: Option<TicketDetail>,
    ) {
        let slot = &reservation.session;
        let ticket = &reservation.ticket;
        let Some(detail) = detail else {
            state.reserved.insert(
                slot.clone(),
                Reserved {
                    ticket: ticket.clone(),
                    expires_at: reservation.expires_at,
                    code: Some(reservation.code.clone()),
                    model: None,
                    effort: String::new(),
                    app: app.to_owned(),
                },
            );
            state.unread.push((reservation.clone(), app.to_owned()));
            return;
        };
        let Some(reserved) = handoff_details(config, reservation, app, &detail) else {
            Self::drop_placeholder(state, slot, ticket);
            return;
        };
        let resume = feed::cause(&detail).map(|culprit| Resume {
            culprit,
            prompt: feed::culprit_prompt(&detail, &reserved),
        });
        Self::begin_in_slot(state, slot, reserved, resume, false);
    }

    pub(super) fn begin_in_slot(
        state: &mut State,
        slot: &SessionId,
        reserved: Reserved,
        resume: Option<Resume>,
        fresh: bool,
    ) {
        state.reserved.insert(slot.clone(), reserved.clone());
        state.cli_calls.push(cli::Call::Begin {
            slot: slot.clone(),
            reserved,
            resume,
            fresh,
        });
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

/// The successor uses its own ticket's model and effort even when it resumes a transcript.
fn handoff_details(
    config: &Config,
    reservation: &Reservation,
    app: &str,
    detail: &TicketDetail,
) -> Option<Reserved> {
    let labels = &detail.ticket.labels;
    // Untiered tickets use the provider's default. A requested tier needs a model ID.
    let model = match label(labels, "tier:") {
        Some(tier) => Some(config.pool.model_id(app, tier)?),
        None => None,
    };
    Some(Reserved {
        ticket: reservation.ticket.clone(),
        expires_at: reservation.expires_at,
        code: Some(reservation.code.clone()),
        model,
        effort: label(labels, "effort:")
            .unwrap_or_else(|| config.pool.default_effort(app))
            .to_owned(),
        app: app.to_owned(),
    })
}
