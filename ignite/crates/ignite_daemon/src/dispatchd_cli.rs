//! Starts CLI workers for hub reservations and delivers checks and messages to them.

use std::sync::Arc;

use ignite_core::capacity::{RoomInputs, start_room, worker_memory_limited};
use ignite_core::config::Config;
use ignite_core::ids::SessionId;
use ignite_core::protocol::hub::{ProviderOffer, Reservation};
use ignite_core::traits::Error;

use super::{Dispatcher, Reserved, State};
use crate::clid::{CULPRIT_BUSY, CliPool, Resume};
use crate::pool::Member;
use crate::server::Context;

/// The worker supervisor and configuration for this machine.
pub(super) struct CliMode {
    pub(super) pool: Arc<dyn CliPool>,
    config: Config,
}

impl CliMode {
    pub(super) fn can_start(&self, app: &str) -> bool {
        self.config.cli_program(app).is_some() && self.pool.held(app).is_none()
    }
}

/// Work to send to the CLI supervisor once the state is unlocked. A pending start is saved until it runs.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) enum Call {
    /// Start a session for `reserved` in the slot whose placeholder is `slot`, or, when `slot`
    /// is a CLI session (the next ticket of a `lane:` chain), in that session's slot. With
    /// `resume`, the session may resume the transcript of the worker whose ticket caused it.
    Begin {
        slot: SessionId,
        reserved: Reserved,
        resume: Option<Resume>,
        #[serde(default)]
        fresh: bool,
    },
    Refuse {
        reservation: Reservation,
        reason: String,
    },
    /// Queue `text` for the session's next turn. An urgent message (`ignite tell --now`, or an
    /// order from a justiciar) also ends the running turn.
    Message {
        session: SessionId,
        text: String,
        urgent: bool,
    },
    /// Check whether the worker still has a running turn.
    Check { session: SessionId },
}

/// The start of every CLI session and slot placeholder.
const PREFIX: &str = "cli-";
/// What follows the app in a slot's placeholder, `cli-<app>-slot-<k>`.
const SLOT_INFIX: &str = "-slot-";

/// The app and slot number of a CLI slot's placeholder, `cli-<app>-slot-<k>`.
pub(super) fn slot_of(session: &SessionId) -> Option<(&str, u32)> {
    let (app, index) = session
        .as_str()
        .strip_prefix(PREFIX)?
        .rsplit_once(SLOT_INFIX)?;
    Some((app, index.parse().ok()?))
}

/// Whether `session` is a CLI slot's placeholder.
pub(super) fn is_cli_slot(session: &SessionId) -> bool {
    slot_of(session).is_some()
}

/// Whether `session` is a session the daemon runs as CLI turns, `cli-<app>-<id>`, rather than a
/// desktop session or a slot's placeholder.
pub(super) fn is_cli_session(session: &SessionId) -> bool {
    session.as_str().starts_with(PREFIX) && !is_cli_slot(session)
}

/// The app of a CLI session, even after a finished worker leaves the pool.
pub(super) fn session_app(session: &SessionId) -> Option<&str> {
    if !is_cli_session(session) {
        return None;
    }
    let (app, _) = session.as_str().strip_prefix(PREFIX)?.rsplit_once('-')?;
    Some(app)
}

/// A hub-generated reservation ID has no local session yet.
pub(super) fn is_new_worker(session: &SessionId) -> bool {
    !is_cli_session(session) && !is_cli_slot(session)
}

pub(super) fn refuse(ctx: &Context, reservation: &Reservation, reason: &str) {
    eprintln!("ignite: {} not started: {reason}", reservation.ticket);
    if let Err(error) = ctx.hub.refuse_reservation(reservation) {
        eprintln!(
            "ignite: could not return the reservation of {}: {error}",
            reservation.ticket
        );
    }
}

/// The tiers the provider offers to the hub, from its model table.
fn tiers(config: &Config, provider: &str) -> Vec<String> {
    config
        .pool
        .models
        .get(provider)
        .map_or_else(Vec::new, |models| models.keys().cloned().collect())
}

impl Dispatcher {
    /// Connects the CLI supervisor to the worker pool.
    pub fn set_cli(&self, pool: Arc<dyn CliPool>, config: &Config) {
        let _ = self.cli.set(CliMode {
            pool,
            config: config.clone(),
        });
    }

    /// The providers with a configured CLI program.
    pub fn cli_apps(&self) -> impl Iterator<Item = &str> {
        self.cli
            .get()
            .into_iter()
            .flat_map(|cli| cli.config.pool.cli.keys().map(String::as_str))
    }

    pub(super) fn provider_offers(&self, state: &State) -> Vec<ProviderOffer> {
        let Some(cli) = self.cli.get() else {
            return Vec::new();
        };
        self.cli_apps()
            .filter(|app| !state.refused.contains_key(*app) && cli.pool.offers(app))
            .map(|app| {
                let models = tiers(&cli.config, app);
                ProviderOffer {
                    name: app.to_owned(),
                    model_ids: models
                        .iter()
                        .filter_map(|tier| {
                            cli.config
                                .pool
                                .model_id(app, tier)
                                .map(|id| (tier.clone(), id))
                        })
                        .collect(),
                    tiers: models,
                    default_model: crate::clid::health::default_model(&cli.config, app),
                }
            })
            .collect()
    }

    pub(super) fn room_inputs(&self, running: usize) -> RoomInputs {
        let (ceiling, memory_pressed, free_memory_mb) = self.host.worker_capacity();
        RoomInputs {
            ceiling,
            running: u32::try_from(running).unwrap_or(u32::MAX),
            memory_pressed,
            free_memory_mb,
            worker_mb: self
                .cli
                .get()
                .map_or(2_000, |cli| cli.pool.worker_memory_mb()),
        }
    }

    /// Why the machine can or cannot start more workers.
    pub(crate) fn worker_room_line(&self) -> Option<String> {
        self.cli.get()?;
        let holders = self.host.holders();
        let state = self.state();
        let running = holders
            .keys()
            .filter(|session| is_cli_session(session))
            .count();
        let reserved = state
            .reserved
            .keys()
            .filter(|session| !holders.contains_key(*session))
            .count();
        let inputs = self.room_inputs(running + reserved);
        let start = start_room(&inputs);
        Some(if worker_memory_limited(&inputs) {
            "worker room limited by memory".to_owned()
        } else if inputs.ceiling.saturating_sub(inputs.running)
            <= ignite_core::capacity::WORKER_BATCH
        {
            format!("room to start {start} (worker ceiling {})", inputs.ceiling)
        } else {
            format!("room to start {start}; memory and worker ceiling allow more")
        })
    }

    /// Carries out the calls queued while the state was locked, and those that carrying them
    /// out queues: the answer to a check can queue a reminder for the same session.
    ///
    /// A `Begin` stays in `begins_running`, which is saved, until it has run and moved its
    /// reservation to the new session. A daemon that dies before then runs it again after its
    /// restart, and [`CliPool::begin`] returns the session it already started, if it did.
    ///
    /// A restored start waits for the CLI pool to be attached or its reservation to lapse.
    pub(super) fn run_cli_calls(&self, ctx: &Context) {
        loop {
            let calls = {
                let mut state = self.state();
                let mut calls = std::mem::take(&mut state.cli_calls);
                if self.cli.get().is_none() {
                    let now = ctx.clock.now();
                    let (begins, rest): (Vec<Call>, Vec<Call>) = calls
                        .into_iter()
                        .partition(|c| matches!(c, Call::Begin { .. }));
                    state.cli_calls = begins
                        .into_iter()
                        .filter(|c| {
                            matches!(c, Call::Begin { reserved, .. } if reserved.expires_at > now)
                        })
                        .collect();
                    calls = rest;
                }
                let begins = calls.iter().filter(|c| matches!(c, Call::Begin { .. }));
                state.begins_running.extend(begins.cloned());
                calls
            };
            if calls.is_empty() {
                break;
            }
            self.save();
            self.run_cli_batch(ctx, calls);
        }
        self.save();
    }

    fn run_cli_batch(&self, ctx: &Context, calls: Vec<Call>) {
        let Some(cli) = self.cli.get() else {
            eprintln!(
                "ignite: {} calls for CLI sessions were dropped: this daemon runs no CLI turns",
                calls.len()
            );
            return;
        };
        for call in calls {
            match call {
                Call::Begin {
                    slot,
                    reserved,
                    resume,
                    fresh,
                } => {
                    let call = Call::Begin {
                        slot: slot.clone(),
                        reserved: reserved.clone(),
                        resume: resume.clone(),
                        fresh,
                    };
                    self.begin(ctx, cli, &slot, reserved, resume.as_ref(), fresh);
                    let mut state = self.state();
                    if let Some(index) = state.begins_running.iter().position(|c| *c == call) {
                        state.begins_running.remove(index);
                    }
                }
                Call::Refuse {
                    reservation,
                    reason,
                } => refuse(ctx, &reservation, &reason),
                Call::Message {
                    session,
                    text,
                    urgent,
                } => {
                    if let Err(error) = cli.pool.message(&session, &text, urgent) {
                        eprintln!("ignite: could not queue a message for {session}: {error}");
                    }
                }
                Call::Check { session } => {
                    let answer = cli.pool.check(&session);
                    let mut state = self.state();
                    let release = self.checked(&mut state, ctx, &session, answer);
                    drop(state);
                    if let Some(due) = release {
                        eprintln!("ignite: {}", self.release(&due));
                    }
                }
            }
        }
    }

    /// Starts a session for `reserved` in `slot`. The session joins the pool as a single-use
    /// worker and takes the reservation, so stall checks, messages and tells reach it. A start
    /// that fails frees the slot for a retry after one minute. Its reservation lapses at the hub.
    fn begin(
        &self,
        ctx: &Context,
        cli: &CliMode,
        slot: &SessionId,
        reserved: Reserved,
        resume: Option<&Resume>,
        fresh: bool,
    ) {
        let now = ctx.clock.now();
        let reservation = Reservation {
            provider: is_new_worker(slot).then(|| reserved.app.clone()),
            ticket: reserved.ticket.clone(),
            machine: ctx.machine.clone(),
            session: slot.clone(),
            expires_at: reserved.expires_at,
            code: reserved.code.clone().unwrap_or_default(),
        };
        let stopped = if reserved.expires_at <= now {
            Some("its reservation lapsed")
        } else if crate::fleet_notice::is_off(&self.pool) {
            Some("this machine is off")
        } else {
            None
        };
        if let Some(reason) = stopped {
            refuse(ctx, &reservation, reason);
            Self::drop_placeholder(&mut self.state(), slot, &reserved.ticket);
            return;
        }
        // The CLI pool resolves a hand-off without a model to the app's configured default.
        let model = reserved.model.clone().unwrap_or_default();
        let started = cli
            .pool
            .begin(&reservation, &model, Some(&reserved.effort), resume, fresh);
        let session = match started {
            Ok(session) => session,
            Err(error) => {
                refuse(ctx, &reservation, &error.to_string());
                let mut state = self.state();
                Self::drop_placeholder(&mut state, slot, &reserved.ticket);
                let transcript_busy = matches!(error, Error::Conflict(ref message)
                    if message == CULPRIT_BUSY);
                if !transcript_busy && cli.pool.held(&reserved.app).is_none() {
                    state.refused.insert(reserved.app.clone(), now);
                }
                return;
            }
        };
        if !matches!(self.pool.member(&session), Ok(Some(_))) {
            let member = Member {
                session: session.clone(),
                app: reserved.app.clone(),
                down: false,
                models: tiers(&cli.config, &reserved.app),
                files: Vec::new(),
                added_at: now,
                single_use: true,
            };
            if let Err(error) = self.pool.put(&member) {
                eprintln!("ignite: could not add {session} to the pool: {error}");
            }
        }
        let mut state = self.state();
        if state
            .reserved
            .get(slot)
            .is_some_and(|r| r.ticket == reserved.ticket)
        {
            state.reserved.remove(slot);
        }
        state.reserved.insert(session, reserved);
    }

    pub(crate) fn handoff_note(&self, session: &SessionId) -> Option<String> {
        self.cli.get()?.pool.handoff_note(session)
    }

    /// Starts a local justiciar when the hub is unreachable and one is not already on duty.
    pub fn hand_off_justiciar(&self, ctx: &Context, prompt: &str) -> Option<SessionId> {
        const HOLD_S: f64 = 2.0 * 3_600.0;
        const TIER: &str = "opus";
        const EFFORT: &str = "high";
        let now = ctx.clock.now();
        if self
            .state()
            .justiciar
            .as_ref()
            .is_some_and(|(_, at)| now.seconds_since(*at) < HOLD_S)
        {
            return None;
        }
        let cli = self.cli.get()?;
        let (app, model) = self
            .cli_apps()
            .find_map(|app| Some((app, ctx.config.pool.model_id(app, TIER)?)))?;
        match cli.pool.begin_justiciar(app, prompt, &model, EFFORT) {
            Ok(session) => {
                self.state().justiciar = Some((session.clone(), ctx.clock.now()));
                self.save();
                Some(session)
            }
            Err(error) => {
                eprintln!("ignite: could not start a {app} justiciar: {error}");
                None
            }
        }
    }
}
