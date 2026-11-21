//! Starts CLI workers for hub reservations and delivers checks and messages to them.

use std::sync::Arc;

use ignite_core::config::Config;
use ignite_core::ids::SessionId;
use ignite_core::protocol::hub::Reservation;

use super::{Dispatcher, Reserved, State};
use crate::clid::{CliPool, Resume};
use crate::pool::Member;
use crate::server::Context;

/// The worker supervisor and configuration for this machine.
pub(super) struct CliMode {
    pub(super) pool: Arc<dyn CliPool>,
    /// The machine's hostname, which names its `[machine.<host>]` table.
    hostname: String,
    config: Config,
}

/// One app in CLI mode as a step sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub(super) app: String,
    /// The slots that take any ticket; slot `slots + 1` takes only tickets that hold `master`
    /// back.
    pub(super) slots: u32,
    /// The tiers the app runs, matched against a ticket's `tier:` label.
    pub(super) models: Vec<String>,
    /// Whether the slots are offered to the hub: a probe turn has passed since the daemon
    /// started and the app is on no CLI hold.
    pub(super) offered: bool,
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

/// The placeholder session of slot `index` of `app`.
pub(super) fn placeholder(app: &str, index: u32) -> Option<SessionId> {
    SessionId::new(format!("{PREFIX}{app}{SLOT_INFIX}{index}")).ok()
}

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

/// Whether `session` is the slot of an app in CLI mode that takes only tickets that hold
/// `master` back.
pub(super) fn is_extra_slot(state: &State, session: &SessionId) -> bool {
    slot_of(session).is_some_and(|(app, index)| {
        state
            .cli_layout
            .iter()
            .any(|layout| layout.app == app && index == layout.slots + 1)
    })
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
    pub fn set_cli(&self, pool: Arc<dyn CliPool>, hostname: &str, config: &Config) {
        let _ = self.cli.set(CliMode {
            pool,
            hostname: hostname.to_owned(),
            config: config.clone(),
        });
    }

    /// Whether the configured app has worker slots on this machine.
    pub fn cli_enabled(&self, app: &str) -> bool {
        self.cli.get().is_some_and(|cli| {
            cli.config.cli_program(app).is_some()
                && cli.config.machine_pool(&cli.hostname, app).slots > 0
        })
    }

    /// The configured providers that can take work on this machine.
    pub fn cli_apps(&self) -> impl Iterator<Item = &str> {
        self.cli
            .get()
            .into_iter()
            .flat_map(|cli| cli.config.pool.cli.keys().map(String::as_str))
            .filter(|app| self.cli_enabled(app))
    }

    /// The slots each app in CLI mode has: `slots` from `ignite.toml`, and one more for tickets
    /// that hold `master` back. Empty without a CLI pool.
    pub(super) fn cli_layouts(&self) -> Vec<Layout> {
        let Some(cli) = self.cli.get() else {
            return Vec::new();
        };
        self.cli_apps()
            .map(|app| Layout {
                app: app.to_owned(),
                slots: cli.config.machine_pool(&cli.hostname, app).slots,
                models: tiers(&cli.config, app),
                offered: cli.pool.offers(app),
            })
            .collect()
    }

    /// Updates the slots offered based on the latest probe results and holds.
    pub(super) fn update_cli_layout(&self) {
        let layouts = self.cli_layouts();
        self.state().cli_layout = layouts;
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
        // A hand-off saved before a restart may run after its reservation lapsed at the hub,
        // which would refuse the `ignite start` of its prompt.
        let stopped = if reserved.expires_at <= now {
            Some("its reservation lapsed")
        } else if crate::fleet_notice::is_off(&self.pool) {
            Some("this machine is off")
        } else {
            None
        };
        if let Some(reason) = stopped {
            eprintln!(
                "ignite: {} not started in {slot}: {reason}",
                reserved.ticket
            );
            let mut state = self.state();
            if state
                .reserved
                .get(slot)
                .is_some_and(|r| r.ticket == reserved.ticket)
            {
                state.reserved.remove(slot);
            }
            return;
        }
        // A hold starts no turn, so a reservation made just before it began lapses instead.
        if let Some(hold) = cli.pool.held(&reserved.app) {
            eprintln!(
                "ignite: {} not started in {slot}: {} is on a CLI hold ({})",
                reserved.ticket,
                reserved.app,
                hold.reason.as_str()
            );
            let mut state = self.state();
            if state
                .reserved
                .get(slot)
                .is_some_and(|r| r.ticket == reserved.ticket)
            {
                state.reserved.remove(slot);
            }
            state.refused.insert(slot.clone(), now);
            return;
        }
        let reservation = Reservation {
            provider: None,
            ticket: reserved.ticket.clone(),
            machine: ctx.machine.clone(),
            session: slot.clone(),
            expires_at: reserved.expires_at,
            code: reserved.code.clone().unwrap_or_default(),
        };
        // The CLI pool resolves a hand-off without a model to the app's configured default.
        let model = reserved.model.clone().unwrap_or_default();
        let started = cli
            .pool
            .begin(&reservation, &model, Some(&reserved.effort), resume, fresh);
        let session = match started {
            Ok(session) => session,
            Err(error) => {
                eprintln!(
                    "ignite: could not start {} in {slot}: {error}",
                    reserved.ticket
                );
                let mut state = self.state();
                if state
                    .reserved
                    .get(slot)
                    .is_some_and(|r| r.ticket == reserved.ticket)
                {
                    state.reserved.remove(slot);
                }
                state.refused.insert(slot.clone(), now);
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
        let (app, model) = self.cli_layouts().into_iter().find_map(|layout| {
            let id = ctx.config.pool.model_id(&layout.app, TIER)?;
            Some((layout.app, id))
        })?;
        match cli.pool.begin_justiciar(&app, prompt, &model, EFFORT) {
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
