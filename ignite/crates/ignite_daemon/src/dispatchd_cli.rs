//! The dispatcher's side of CLI mode (see "CLI mode" in the design document): an app in CLI mode
//! on this machine runs its workers as headless CLI processes that the daemon starts itself
//! through [`CliPool`], so it needs no dispatcher session.
//!
//! - The app's mode is read on every step: the override `ignite pool mode` stored in the local
//!   database, or else `[machine.<host>.pool.<app>]` in `ignite.toml`. An app is in CLI mode only
//!   while the daemon has a [`CliPool`] and `[pool.cli.<app>]` names its program. A switch never
//!   touches a ticket in progress: workers of the old mode finish, and only new hand-offs follow
//!   the new mode.
//! - An app in CLI mode offers the hub `slots` idle workers, and one more that takes only tickets
//!   that hold `master` back (see `slots`). "Dispatcher down" never stops its hand-offs. Its
//!   slots are offered only once a probe turn has passed since the daemon started, and never
//!   while the app is on a CLI hold ([`CliPool::offers`]); a hold also starts no session.
//! - A hand-off to a CLI slot, and a message or a `check` for a CLI session, never enters the
//!   instruction queue. Each is recorded as a [`Call`] while the state is locked, and carried out
//!   through [`CliPool`] once it is unlocked, since starting a turn writes to the database and
//!   starts a process. The settle time, refusals, clears, read-backs and rotation of the desktop
//!   dispatcher do not apply.
//! - A `check` of a CLI session is answered at once by [`CliPool::check`], and the answer goes
//!   through the same stall rules as a desktop dispatcher's (see `stall`).
//! - With the hub unreachable for 15 minutes, the daemon starts a CLI session with the justiciar
//!   prompt and no ticket, before it tries a desktop worker (see `justiciar`).

use std::sync::Arc;

use ignite_core::config::{Config, PoolMode};
use ignite_core::ids::SessionId;
use ignite_core::protocol::cli::{PoolModeReply, PoolModeSetting};
use ignite_core::protocol::hub::Reservation;
use ignite_core::traits::{Error, Result};
use ignite_store::local::cli_slots;

use super::{Dispatcher, Reserved, State, justiciar};
use crate::clid::{CliPool, Resume};
use crate::pool::{CLAUDE_MODELS, Member};
use crate::server::Context;

/// The apps a machine may run.
pub const APPS: [&str; 2] = ["claude", "codex"];

/// What the dispatcher needs to run apps in CLI mode.
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
    /// started and the app is on no CLI hold. A held app stays in CLI mode, so its desktop
    /// workers are offered nothing either.
    pub(super) offered: bool,
}

/// Something to ask the [`CliPool`] once the state is unlocked.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Call {
    /// Start a session for `reserved` in the slot whose placeholder is `slot`, or, when `slot`
    /// is a CLI session (the next ticket of a `lane:` chain), in that session's slot. With
    /// `resume`, the session may resume the transcript of the worker whose ticket caused it.
    Begin {
        slot: SessionId,
        reserved: Reserved,
        resume: Option<Resume>,
    },
    /// Queue `text` for the session's next turn. An urgent message (`ignite tell --now`, or an
    /// order from a justiciar) also ends the running turn.
    Message {
        session: SessionId,
        text: String,
        urgent: bool,
    },
    /// Ask whether the session is running, and apply the answer as a `check` acknowledgement.
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
        .split_once(SLOT_INFIX)?;
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

/// Whether `app` runs in CLI mode at the last step.
pub(super) fn in_cli_mode(state: &State, app: &str) -> bool {
    state.cli_layout.iter().any(|layout| layout.app == app)
}

/// The tiers `app` runs: its `[pool.models.<app>]` table's, or for Claude without one, every
/// Claude tier. An empty list leaves the choice to `offered_models` (see `work`).
fn tiers(config: &Config, app: &str) -> Vec<String> {
    match config.pool.models.get(app) {
        Some(table) => table.keys().cloned().collect(),
        None if app == "claude" => CLAUDE_MODELS.iter().map(|m| (*m).to_owned()).collect(),
        None => Vec::new(),
    }
}

impl Dispatcher {
    /// Lets the dispatcher run apps in CLI mode through `pool`, on the machine named `hostname`
    /// in `config`. Without it, every app runs with its desktop dispatcher.
    pub fn set_cli(&self, pool: Arc<dyn CliPool>, hostname: &str, config: &Config) {
        let _ = self.cli.set(CliMode {
            pool,
            hostname: hostname.to_owned(),
            config: config.clone(),
        });
    }

    /// The mode `app` runs in now, and whether `ignite pool mode` forced it.
    pub fn mode(&self, app: &str) -> (PoolMode, bool) {
        let stored = self
            .pool
            .with_database(|conn| cli_slots::mode_override(conn, app))
            .ok()
            .flatten()
            .and_then(|word| PoolModeSetting::parse(&word))
            .and_then(PoolModeSetting::mode);
        let Some(cli) = self.cli.get() else {
            return (PoolMode::Desktop, stored.is_some());
        };
        let wanted = stored.unwrap_or_else(|| cli.config.machine_pool(&cli.hostname, app).mode);
        // A program is what a CLI turn runs, so an app without one stays with its dispatcher.
        let runnable = cli.config.cli_program(app).is_some();
        let mode = if wanted == PoolMode::Cli && runnable {
            PoolMode::Cli
        } else {
            PoolMode::Desktop
        };
        (mode, stored.is_some())
    }

    /// `ignite pool mode <app> <setting>`: stores the override, or removes it for `config`.
    pub fn set_mode(&self, app: &str, setting: PoolModeSetting) -> Result<PoolModeReply> {
        if !APPS.contains(&app) {
            return Err(Error::Invalid(format!("unknown app {app}")));
        }
        let word = setting.mode().map(PoolMode::as_str);
        self.pool
            .with_database(|conn| cli_slots::set_mode_override(conn, app, word))
            .map_err(|e| Error::Io(e.to_string()))?;
        let (mode, overridden) = self.mode(app);
        self.update_cli_layout();
        Ok(PoolModeReply {
            app: app.to_owned(),
            mode,
            overridden,
        })
    }

    /// Why `app` runs with its dispatcher although CLI mode was asked for, if it does.
    pub fn cli_unavailable(&self, app: &str) -> Option<String> {
        let Some(cli) = self.cli.get() else {
            return Some("this daemon runs no CLI turns".to_owned());
        };
        if cli.config.cli_program(app).is_none() {
            return Some(format!("ignite.toml has no [pool.cli.{app}] table"));
        }
        None
    }

    /// The slots each app in CLI mode has: `slots` from `ignite.toml`, and one more for tickets
    /// that hold `master` back. Empty without a CLI pool.
    pub(super) fn cli_layouts(&self) -> Vec<Layout> {
        let Some(cli) = self.cli.get() else {
            return Vec::new();
        };
        APPS.iter()
            .filter(|app| self.mode(app).0 == PoolMode::Cli)
            .map(|app| Layout {
                app: (*app).to_owned(),
                slots: cli.config.machine_pool(&cli.hostname, app).slots,
                models: tiers(&cli.config, app),
                offered: cli.pool.offers(app),
            })
            .collect()
    }

    /// Reads each app's mode again, so that a switch takes effect at the next step.
    pub(super) fn update_cli_layout(&self) {
        let layouts = self.cli_layouts();
        self.state().cli_layout = layouts;
    }

    /// Brings the state up to date, as a dispatcher's step does, without giving an instruction:
    /// hands tickets to free CLI slots, and checks silent CLI sessions. The CLI loop calls it,
    /// since a machine whose apps all run in CLI mode has no dispatcher to ask for steps.
    pub fn tick(&self, ctx: &Context) {
        let members = self.pool.members().unwrap_or_default();
        self.refresh(ctx, &members);
        self.save();
    }

    /// Carries out the calls queued while the state was locked, and those that carrying them
    /// out queues: the answer to a check can queue a reminder for the same session.
    pub(super) fn run_cli_calls(&self, ctx: &Context) {
        loop {
            let calls = std::mem::take(&mut self.state().cli_calls);
            if calls.is_empty() {
                break;
            }
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
                } => self.begin(ctx, cli, &slot, reserved, resume.as_ref()),
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
                    let (_, release) = self.checked(&mut state, ctx, &session, answer.as_str());
                    drop(state);
                    if let Some(due) = release {
                        eprintln!("ignite: {}", self.release(&due).trim_start_matches("; "));
                    }
                }
            }
        }
    }

    /// Starts a session for `reserved` in `slot`. The session joins the pool as a single-use
    /// worker and takes the reservation, so stall checks, messages and tells reach it. A start
    /// that fails frees the slot, which is left alone for a minute, as a desktop worker that
    /// refused a hand-off is, and the reservation lapses at the hub.
    fn begin(
        &self,
        ctx: &Context,
        cli: &CliMode,
        slot: &SessionId,
        reserved: Reserved,
        resume: Option<&Resume>,
    ) {
        let now = ctx.clock.now();
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
            ticket: reserved.ticket.clone(),
            machine: ctx.machine.clone(),
            session: slot.clone(),
            expires_at: reserved.expires_at,
            code: reserved.code.clone().unwrap_or_default(),
        };
        // A ticket with no tier on an app with no default runs on the app's own default model.
        let model = reserved.model.clone().unwrap_or_default();
        let started = cli
            .pool
            .begin(&reservation, &model, Some(reserved.effort.as_str()), resume);
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
                role: ignite_core::protocol::hub::WorkerRole::Worker,
                down: false,
                models: tiers(&cli.config, &reserved.app),
                parent: None,
                files: Vec::new(),
                added_at: now,
                slots: 0,
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

    /// Starts a justiciar CLI session with `prompt` and no ticket, in the first app in CLI mode
    /// that has an ID for the justiciar's tier. Returns the session, or `None` when no app runs
    /// in CLI mode or the start failed, so that a desktop worker is tried instead. The caller
    /// has checked that no justiciar is within its hold.
    pub(super) fn cli_justiciar(&self, ctx: &Context, prompt: &str) -> Option<SessionId> {
        let cli = self.cli.get()?;
        let (app, model) = self.cli_layouts().into_iter().find_map(|layout| {
            let id = ctx.config.pool.model_id(&layout.app, justiciar::TIER)?;
            Some((layout.app, id))
        })?;
        match cli
            .pool
            .begin_justiciar(&app, prompt, &model, justiciar::EFFORT)
        {
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
