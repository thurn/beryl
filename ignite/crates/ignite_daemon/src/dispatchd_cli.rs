//! Starts CLI workers for hub reservations and delivers checks and messages to them.

use std::collections::BTreeSet;
use std::sync::{Arc, PoisonError};

use ignite_core::capacity::{ProviderCapacity, RoomInputs, start_room, worker_memory_room};
use ignite_core::config::Config;
use ignite_core::ids::SessionId;
use ignite_core::protocol::hub::{ProviderOffer, Reservation};
use ignite_core::time::Timestamp;
use ignite_core::traits::Error;

use super::{HeadlessDispatch, Reserved, State};
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

/// A queued start for a new worker, or for a lane successor in an existing session.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct Begin {
    pub(super) slot: SessionId,
    pub(super) reserved: Reserved,
    pub(super) resume: Option<Resume>,
    #[serde(default)]
    pub(super) fresh: bool,
}

/// Work to send to the CLI supervisor once the state is unlocked. A pending start is saved until it runs.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) enum Call {
    Begin(Begin),
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
    Check {
        session: SessionId,
    },
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

fn provider_offer(config: &Config, app: &str) -> ProviderOffer {
    let tiers = tiers(config, app);
    ProviderOffer {
        name: app.to_owned(),
        model_ids: tiers
            .iter()
            .filter_map(|tier| config.pool.model_id(app, tier).map(|id| (tier.clone(), id)))
            .collect(),
        tiers,
        default_model: crate::clid::health::default_model(config, app),
    }
}

impl HeadlessDispatch {
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

    /// Includes providers on hold and providers still waiting for their first successful probe.
    pub(super) fn provider_capacity(&self) -> Vec<ProviderCapacity> {
        let Some(cli) = self.cli.get() else {
            return Vec::new();
        };
        self.cli_apps()
            .map(|app| {
                let offer = provider_offer(&cli.config, app);
                ProviderCapacity {
                    name: offer.name,
                    tiers: offer.tiers,
                    probed: cli.pool.probed(app),
                    hold_reason: cli
                        .pool
                        .held(app)
                        .map(|hold| hold.reason.as_str().to_owned()),
                    model_ids: offer.model_ids,
                    default_model: offer.default_model,
                }
            })
            .collect()
    }

    pub(super) fn provider_offers(&self, state: &State) -> Vec<ProviderOffer> {
        let Some(cli) = self.cli.get() else {
            return Vec::new();
        };
        self.cli_apps()
            .filter(|app| !state.refused.contains_key(*app) && cli.pool.offers(app))
            .map(|app| provider_offer(&cli.config, app))
            .collect()
    }

    pub(super) fn room_inputs(&self, running: usize, starting: usize) -> RoomInputs {
        let (ceiling, memory_pressed, free_memory_mb) = self.host.worker_capacity();
        RoomInputs {
            ceiling,
            running: u32::try_from(running).unwrap_or(u32::MAX),
            starting: u32::try_from(starting).unwrap_or(u32::MAX),
            memory_pressed,
            free_memory_mb,
            worker_mb: self
                .cli
                .get()
                .map_or(2_000, |cli| cli.pool.worker_memory_mb()),
        }
    }

    fn current_room_inputs(&self) -> Option<RoomInputs> {
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
        Some(self.room_inputs(running, reserved))
    }

    pub(crate) fn worker_room(&self) -> u32 {
        self.current_room_inputs().as_ref().map_or(0, start_room)
    }

    /// Why the machine can or cannot start more workers.
    pub(crate) fn worker_room_line(&self) -> Option<String> {
        let inputs = self.current_room_inputs()?;
        let start = start_room(&inputs);
        let memory_room = worker_memory_room(&inputs);
        let ceiling_room = inputs
            .ceiling
            .saturating_sub(inputs.running.saturating_add(inputs.starting));
        Some(if memory_room == 0 {
            "worker room limited by memory".to_owned()
        } else if memory_room <= ceiling_room && memory_room < ignite_core::capacity::WORKER_BATCH {
            format!("room to start {start} (limited by memory)")
        } else if ceiling_room <= ignite_core::capacity::WORKER_BATCH {
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
    /// restart, and [`CliPool::begin_if`] returns the session it already started, if it did.
    ///
    /// A restored start waits for the CLI pool to be attached or its reservation to lapse.
    pub(super) fn run_cli_calls(&self, ctx: &Context) {
        let _running = self
            .cli_calls_running
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut started = BTreeSet::new();
        loop {
            let calls = {
                let mut state = self.state();
                let mut calls = std::mem::take(&mut state.cli_calls);
                if self.cli.get().is_none() {
                    let now = ctx.clock.now();
                    let (begins, rest): (Vec<Call>, Vec<Call>) =
                        calls.into_iter().partition(|c| matches!(c, Call::Begin(_)));
                    state.cli_calls = begins
                        .into_iter()
                        .filter(
                            |c| matches!(c, Call::Begin(begin) if begin.reserved.expires_at > now),
                        )
                        .collect();
                    calls = rest;
                }
                let begins = calls.iter().filter(|c| matches!(c, Call::Begin(_)));
                state.begins_running.extend(begins.cloned());
                calls
            };
            if calls.is_empty() {
                break;
            }
            self.save();
            self.run_cli_batch(ctx, calls, &mut started);
        }
        self.save();
    }

    fn run_cli_batch(&self, ctx: &Context, calls: Vec<Call>, started: &mut BTreeSet<SessionId>) {
        let Some(cli) = self.cli.get() else {
            eprintln!(
                "ignite: {} calls for CLI sessions were dropped: this daemon runs no CLI turns",
                calls.len()
            );
            return;
        };
        for call in calls {
            match call {
                Call::Begin(ref begin) => {
                    if let Some(session) = self.begin(ctx, cli, begin, started) {
                        started.insert(session);
                    }
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
                        eprintln!("ignite: {}", self.release(ctx, cli.pool.as_ref(), &due));
                    }
                }
            }
        }
    }

    /// Starts a reserved session or recovers a recorded one, then adds it to the pool
    /// as a single-use worker so stall checks, messages and tells reach it.
    fn begin(
        &self,
        ctx: &Context,
        cli: &CliMode,
        begin: &Begin,
        started: &BTreeSet<SessionId>,
    ) -> Option<SessionId> {
        let Begin {
            slot,
            reserved,
            resume,
            fresh,
        } = begin;
        let reservation = Reservation {
            provider: is_new_worker(slot).then(|| reserved.app.clone()),
            ticket: reserved.ticket.clone(),
            machine: ctx.machine.clone(),
            session: slot.clone(),
            expires_at: reserved.expires_at,
            code: reserved.code.clone().unwrap_or_default(),
            master_blocker: reserved.master_blocker,
        };
        let mut refusal = None;
        let result = cli.pool.begin_if(
            &reservation,
            reserved.model.as_deref().unwrap_or_default(),
            Some(&reserved.effort),
            resume.as_ref(),
            *fresh,
            &mut || {
                refusal = self.start_refusal(ctx.clock.now(), reserved, started);
                refusal.is_none()
            },
        );
        let now = ctx.clock.now();
        let session = match result {
            Ok(Some(session)) => session,
            Ok(None) => {
                refuse(ctx, &reservation, refusal.expect("the start was refused"));
                Self::drop_placeholder(&mut self.state(), slot, &reserved.ticket);
                return None;
            }
            Err(error) => {
                refuse(ctx, &reservation, &error.to_string());
                let mut state = self.state();
                Self::drop_placeholder(&mut state, slot, &reserved.ticket);
                let transcript_busy = matches!(error, Error::Conflict(ref message)
                    if message == CULPRIT_BUSY);
                if !transcript_busy && cli.pool.held(&reserved.app).is_none() {
                    state.refused.insert(reserved.app.clone(), now);
                }
                return None;
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
        state.reserved.insert(session.clone(), reserved.clone());
        Some(session)
    }

    fn start_refusal(
        &self,
        now: Timestamp,
        reserved: &Reserved,
        started: &BTreeSet<SessionId>,
    ) -> Option<&'static str> {
        if reserved.expires_at <= now {
            return Some("its reservation lapsed");
        }
        if crate::fleet_notice::is_off(&self.pool) {
            return Some("this machine is off");
        }
        let holders = self.host.holders();
        let running = holders
            .keys()
            .filter(|session| is_cli_session(session) && !started.contains(*session))
            .count();
        let state = self.state();
        let pending = state
            .reserved
            .iter()
            .filter(|(session, reservation)| {
                is_cli_session(session)
                    && reservation.expires_at > now
                    && !holders.contains_key(*session)
                    && !started.contains(*session)
                    // A lane continuation can be queued under a real session ID.
                    && !state.cli_calls.iter().chain(&state.begins_running).any(|call| {
                        matches!(call, Call::Begin(begin) if begin.slot == **session
                            && begin.reserved.code == reservation.code)
                    })
            })
            .count();
        drop(state);
        // Keep counting this batch's launches even if a worker has already claimed its ticket.
        let inputs = self.room_inputs(running, pending + started.len());
        if inputs.memory_pressed {
            Some("memory pressure prevents another worker from starting")
        } else if reserved.master_blocker {
            None
        } else if worker_memory_room(&inputs) == 0 {
            Some("there is not enough available memory to start another worker")
        } else if start_room(&inputs) == 0 {
            Some("the machine is at its worker ceiling")
        } else {
            None
        }
    }

    pub(crate) fn handoff_note(&self, session: &SessionId) -> Option<String> {
        self.cli.get()?.pool.handoff_note(session)
    }

    /// Starts a local justiciar when the hub is unreachable and one is not already on duty.
    pub fn hand_off_justiciar(&self, ctx: &Context, prompt: &str) -> Option<SessionId> {
        const HOLD_S: f64 = 2.0 * 3_600.0;
        const TIER: &str = "high";
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
