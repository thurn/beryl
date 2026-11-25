//! The daemon runs each worker as a headless CLI process and supervises one process per turn.
//!
//! Headless dispatch uses [`CliPool`] to hand tickets to CLI slots, send messages to sessions,
//! and check whether they are running. [`CliDaemon`] implements it, and its
//! [`CliDaemon::step`] is the body of the `ignite-cli` loop, which runs about once a second:
//!
//! - A slot whose turn is recorded as starting has its run looked up (`start::recover`), and a
//!   turn that was recorded and never started is started now, so a daemon that died between
//!   the two phases of a start starts the turn exactly once.
//! - A running turn's `output.log` is read from its start through the app's [`CliApp::read`]
//!   (see `read`). The transcript ID, model and CLI version go on the rows, and each line's
//!   effects are applied once, keyed by slot, turn and line number (`cli_turns.lines_applied`).
//!   A Claude turn that started another model than the one asked for is ended before its first
//!   request, its ticket is released and that model is hidden for 10 minutes.
//! - A turn that ended, or whose run says it was interrupted, has its end classified, `decide`
//!   chooses what follows, and the action is carried out (see `end`). A turn that hung, silent
//!   with no `ignite` verb running, is ended and its ticket released (see `read`).
//! - A slot waiting on an `ignite` verb, or backing off, starts its next turn when the verb has
//!   ended or the pause is over. No turn starts for an app on a CLI hold.
//! - A ticket that lands keeps its transcript in `cli_landed` for 24 hours, so that a regression
//!   it caused can resume the conversation in which the change was made (see `claim`).
//!
//! Nothing about a session lives in this struct's memory except the 10-minute hiding of a
//! refused model: every slot is a `cli_slots` row and every turn a `cli_turns` row, so a
//! restarted daemon continues where the last one stopped. What other parts of the daemon do
//! reaches this module through traits: [`Turns`] runs turns as supervised commands,
//! [`Effects`] takes usage samples, costs, holds and reports, and [`Host`] reports who
//! holds each ticket and which verbs are running, and releases tickets.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ignite_core::cli::{
    Action, CliApp, Facts, Hold, HoldReason, Slot, SlotState, TurnEnd, TurnEvents, TurnSpec,
    UsageSample,
};
use ignite_core::config::CliProgram;
use ignite_core::ids::{MachineId, SessionId, TicketId};
use ignite_core::protocol::Class;
use ignite_core::protocol::cli::TranscriptLine;
use ignite_core::protocol::hub::Reservation;
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, CommandSpec, Error, Result};
use ignite_store::local::cli_slots as store;
use ignite_store::local::rows::{CliMessage, CliSlot, CliTurn};
use rusqlite::Connection;

use crate::dispatchd::Host;
use crate::pool::Pool;

#[path = "clid_claim.rs"]
mod claim;
#[cfg(test)]
#[path = "clid_e2e_tests.rs"]
mod e2e;
#[path = "clid_end.rs"]
pub mod end;
#[path = "clid_fake.rs"]
mod fake;
#[path = "clid_health.rs"]
pub mod health;
#[path = "clid_messages.rs"]
pub mod messages;
#[path = "clid_read.rs"]
pub mod read;
#[path = "clid_start.rs"]
pub mod start;
#[cfg(test)]
#[path = "clid_tests.rs"]
mod tests;
#[path = "clid_transcript.rs"]
pub mod transcript;
#[path = "clid_turns.rs"]
pub mod turns;

pub use fake::{FakeCliPool, JusticiarStart, SentMessage};
pub use start::Recovery;
pub use turns::SupervisedTurns;

/// The name the `ignite-cli` loop registers under.
pub const LOOP: &str = "ignite-cli";
/// Seconds between two runs of [`CliDaemon::step`].
pub const STEP_S: f64 = 1.0;
/// Seconds a model the CLI refused to start is left out of what the app's slots offer.
pub const HIDE_MODEL_S: f64 = 600.0;
/// Seconds a ticket that landed from a CLI slot keeps its transcript for a regression it caused.
pub const LANDED_KEEP_S: f64 = 24.0 * 3600.0;

/// A hand-off that should resume the conversation of the worker whose ticket `culprit` caused
/// it: a regression, or a ticket whose landing was reverted (then `culprit` is the ticket
/// itself). The first turn resumes that worker's transcript with `prompt` when `culprit` landed
/// from a slot of the same app in the last 24 hours, and starts fresh otherwise.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Resume {
    pub culprit: Option<TicketId>,
    pub prompt: String,
    #[serde(default)]
    pub fresh_reason: Option<String>,
}

/// A hand-off should be retried once the transcript's current worker finishes.
pub(crate) const CULPRIT_BUSY: &str = "the culprit's transcript is still in use";

/// Whether a CLI session has a slot and a running turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionCheck {
    /// A turn's process runs: its turn lock is held.
    Running,
    /// No turn runs now.
    Idle,
    /// The next turn will start when the session's `ignite` work finishes.
    WaitingForVerb,
    /// The session has no slot.
    Gone,
}

/// Starts and monitors CLI worker sessions.
pub trait CliPool: Send + Sync {
    /// Takes a reservation the hub made for one of `app`'s CLI slots (its placeholder session
    /// is `cli-<app>-slot-<k>`), or for a CLI session whose ticket ended (the next ticket of a
    /// `lane:` chain): creates the slot's row with a new Ignite session, or the same one, and
    /// starts its first turn with the hand-off prompt. With `fresh`, a lane's next ticket starts
    /// a fresh transcript in the same Ignite session. With `resume`, a new session's first
    /// turn may resume the culprit's transcript instead (see [`Resume`]). Returns the session,
    /// which the daemon adds to the pool as a single-use worker. A reservation (its ticket
    /// and code) that a slot already took returns that slot's session and starts nothing.
    /// The admission callback runs only for a new start, just before it is recorded. A refusal
    /// leaves no slot and returns `Ok(None)`.
    fn begin_if(
        &self,
        reservation: &Reservation,
        model: &str,
        effort: Option<&str>,
        resume: Option<&Resume>,
        fresh: bool,
        admit: &mut dyn FnMut() -> bool,
    ) -> Result<Option<SessionId>>;
    #[cfg(test)]
    fn begin(
        &self,
        reservation: &Reservation,
        model: &str,
        effort: Option<&str>,
        resume: Option<&Resume>,
        fresh: bool,
    ) -> Result<SessionId> {
        self.begin_if(reservation, model, effort, resume, fresh, &mut || true)
            .map(|session| session.expect("the start was admitted"))
    }
    /// Queues `text` for the session's next turn. An urgent message (`tell --now`, or an order
    /// from a justiciar) ends the running turn and resumes with it.
    fn message(&self, session: &SessionId, text: &str, urgent: bool) -> Result<()>;
    /// Whether the session runs, waits on a verb, is otherwise idle, or has no slot.
    fn check(&self, session: &SessionId) -> SessionCheck;
    /// Current worker rows of `app`, ordered by number.
    fn slots(&self, app: &str) -> Vec<Slot>;
    /// Ends the session's running turn, if any, and frees its slot, for a session whose ticket
    /// was lost while this machine was away (see `ignite_core::protocol::returning`). Returns
    /// whether the session had a slot.
    fn retire(&self, session: &SessionId) -> Result<bool>;
    /// How this worker started a regression or reverted ticket.
    fn handoff_note(&self, _session: &SessionId) -> Option<String> {
        None
    }

    /// The median peak memory of a recent worker turn, in megabytes.
    fn worker_memory_mb(&self) -> u64 {
        2_000
    }
    /// `app`'s CLI hold, if it is on one.
    fn held(&self, app: &str) -> Option<Hold>;
    /// Whether the provider has passed a probe since the daemon started.
    fn probed(&self, app: &str) -> bool;
    /// Whether the provider has passed a probe and is clear of CLI holds.
    fn offers(&self, app: &str) -> bool {
        self.held(app).is_none() && self.probed(app)
    }
    /// Starts a session of `app` that holds no ticket, whose first turn runs `prompt` on `model`
    /// at `effort`: the justiciar the daemon starts when the hub cannot be reached. Returns the
    /// new session. The default refuses, for a pool that cannot start one.
    fn begin_justiciar(
        &self,
        app: &str,
        prompt: &str,
        model: &str,
        effort: &str,
    ) -> Result<SessionId> {
        let _ = (prompt, model, effort);
        Err(Error::Invalid(format!(
            "this CLI pool cannot start a justiciar for {app}"
        )))
    }
    /// The digest of every turn of the session `target` names (a session ID, or the ticket it
    /// holds or last held), for `ignite pool watch`. `None` when no slot has it. The default is
    /// for a pool that keeps no transcripts.
    fn digest(&self, target: &str) -> Result<Option<(SessionId, Vec<TranscriptLine>)>> {
        let _ = target;
        Ok(None)
    }
    /// The model IDs of `app` that its slots must not offer now, because a turn asked for one
    /// of them started another model in the last 10 minutes.
    fn hidden_models(&self, app: &str) -> Vec<String> {
        let _ = app;
        Vec::new()
    }
}

/// The supervised runs of CLI turns. The real one starts `ignite internal cli-turn` under a
/// supervisor without the scheduler and finds runs by the `IGNITE_CLI_TURN` variable in their
/// `spec.json` (see `start`).
pub trait Turns: Send + Sync {
    /// Starts turn `turn` of `session`, whose program and arguments are `command.args`. It
    /// returns once the supervisor has started, not when the turn ends.
    fn start(&self, session: &SessionId, turn: u32, command: &CommandSpec) -> Result<()>;
    /// What the runs say about the turn.
    fn recover(&self, session: &SessionId, turn: u32) -> Recovery;
    /// The text of the turn's `output.log` so far; `None` when it has no run.
    fn output(&self, session: &SessionId, turn: u32) -> Option<String>;
    /// When the turn's `output.log` last grew, or when the run started if it is still empty.
    fn grew_at(&self, session: &SessionId, turn: u32) -> Option<Timestamp>;
    /// The process ID of the turn's CLI program, whose children [`Host::running_under`] lists.
    fn process(&self, session: &SessionId, turn: u32) -> Option<u32>;
    fn record_worker_memory(&self, _session: &SessionId, _turn: u32) {}
    fn worker_memory_mb(&self) -> u64 {
        2_000
    }
    /// Ends every process of the turn. Its run then records an exit with no result line.
    fn end(&self, session: &SessionId, turn: u32) -> Result<()>;
}

/// What the loop hands to other parts of the daemon. Each is called at least once for what it
/// describes; a daemon that dies at the wrong moment can call one twice.
pub trait Effects: Send + Sync {
    /// New usage samples a running turn printed, in order.
    fn usage(&self, slot: &Slot, samples: &[UsageSample]) {
        let _ = (slot, samples);
    }
    /// A turn ended as `end`, with these events, before what follows it is carried out.
    fn ended(&self, slot: &Slot, end: &TurnEnd, events: &TurnEvents) {
        let _ = (slot, end, events);
    }
    /// Puts `app` on a CLI hold.
    fn hold(&self, app: &str, reason: HoldReason, message: &str, cli_version: Option<&str>) {
        let _ = (app, reason, message, cli_version);
    }
    /// Reports how a regression or reverted ticket was handed to a worker.
    fn handoff(&self, slot: &Slot, note: &str, resumed: bool) {
        let _ = (slot, note, resumed);
    }
    /// Reports a parked slot, or a release that needs a person's attention.
    fn report(&self, slot: &Slot, report: &str) {
        let _ = (slot, report);
    }
    /// Whether a probe turn has passed for `app` since the daemon started, so that its slots
    /// may be offered (see `health`).
    fn probed(&self, app: &str) -> bool {
        let _ = app;
        true
    }
    /// Whether the hub still assigns this ticket to the session. An error defers recovery.
    fn claim_is_current(&self, session: &SessionId, ticket: &TicketId) -> Result<bool> {
        let _ = (session, ticket);
        Ok(true)
    }
}

/// [`Effects`] that do nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoEffects;

impl Effects for NoEffects {}

/// `decide`'s signature: what follows a turn's end.
pub type Decide = dyn Fn(&Slot, &TurnEnd, &Facts) -> Action + Send + Sync;
/// The signature of the function that builds the prompt delivering messages.
pub type MessagesPrompt = dyn Fn(&[String]) -> Option<String> + Send + Sync;

/// The pure rules the loop follows, as functions so tests can choose an action.
pub struct Rules {
    /// What follows a turn's end ([`ignite_core::cli::decide::decide`]).
    pub decide: Box<Decide>,
    /// The prompt that delivers messages (`messages::prompt`).
    pub prompt: Box<MessagesPrompt>,
}

impl Rules {
    /// The rules the daemon runs with.
    pub fn standard() -> Rules {
        Rules {
            decide: Box::new(ignite_core::cli::decide::decide),
            prompt: Box::new(messages::prompt),
        }
    }
}

/// One app in CLI mode on this machine.
pub struct CliAppSetup {
    pub app: Arc<dyn CliApp>,
    /// `[pool.cli.<app>]`.
    pub program: CliProgram,
    /// The configured model for tickets without a tier.
    pub default_model: Option<String>,
}

/// What [`CliDaemon`] is built from.
pub struct CliSetup {
    /// The main checkout, where every turn runs.
    pub checkout: PathBuf,
    /// The apps in CLI mode, by name.
    pub apps: BTreeMap<String, CliAppSetup>,
}

/// The CLI pool: the slots in the local database and the loop that follows their turns.
pub struct CliDaemon {
    setup: CliSetup,
    pool: Arc<Pool>,
    host: Arc<dyn Host>,
    clock: Arc<dyn Clock>,
    turns: Arc<dyn Turns>,
    effects: Arc<dyn Effects>,
    rules: Rules,
    /// Models a turn refused to start, by app and model, until when they are hidden.
    hidden: Mutex<BTreeMap<(String, String), Timestamp>>,
    /// The machine digest lines are sent as; without one, no digest is sent.
    machine: Option<MachineId>,
    /// How many lines of each turn's digest (by session and turn) are in the outbox, as far as
    /// this daemon knows. A restart forgets it, and the hub drops the repeats.
    sent: Mutex<BTreeMap<(SessionId, u32), usize>>,
    /// Held while a hand-off chooses a slot and claims it, so threads handing off tickets take
    /// slots one at a time (see `claim::open_slot`).
    opening: Mutex<()>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn store_error(error: &ignite_store::StoreError) -> Error {
    Error::Io(error.to_string())
}

/// The slot a stored row describes. A state this build does not know reads as parked, so that
/// nothing starts for it.
pub fn slot_of(row: &CliSlot) -> Slot {
    Slot {
        app: row.app.clone(),
        index: u32::try_from(row.slot).unwrap_or(0),
        session: row.session.as_deref().and_then(|s| SessionId::new(s).ok()),
        ticket: row.ticket.as_deref().and_then(|t| TicketId::new(t).ok()),
        state: SlotState::parse(&row.state).unwrap_or(SlotState::Parked),
        turn: u32::try_from(row.turn).unwrap_or(0),
        transcript: row.transcript.clone(),
        model: row.model.clone(),
        effort: row.effort.clone(),
        since: row.since.as_deref().and_then(|t| Timestamp::parse(t).ok()),
    }
}

/// The app and number of a placeholder session `cli-<app>-slot-<k>`.
fn placeholder(session: &SessionId) -> Option<(String, u32)> {
    let (app, index) = session
        .as_str()
        .strip_prefix("cli-")?
        .rsplit_once("-slot-")?;
    Some((app.to_owned(), index.parse().ok()?))
}

fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0_u8; N];
    getrandom::getrandom(&mut bytes).map_err(|e| Error::Io(e.to_string()))?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// A random version 4 UUID, the session ID a new Claude transcript is given.
fn new_uuid() -> Result<String> {
    let mut bytes = random_bytes::<16>()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let text = hex(&bytes);
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &text[0..8],
        &text[8..12],
        &text[12..16],
        &text[16..20],
        &text[20..]
    ))
}

/// The first prompt of a ticket's first turn.
pub fn handoff_prompt(ticket: &TicketId, code: &str) -> String {
    format!("Run `ignite start {ticket} --reservation {code}` and follow what it prints.")
}

impl CliDaemon {
    pub fn new(
        setup: CliSetup,
        pool: Arc<Pool>,
        host: Arc<dyn Host>,
        clock: Arc<dyn Clock>,
        turns: Arc<dyn Turns>,
    ) -> CliDaemon {
        CliDaemon {
            setup,
            pool,
            host,
            clock,
            turns,
            effects: Arc::new(NoEffects),
            rules: Rules::standard(),
            hidden: Mutex::new(BTreeMap::new()),
            machine: None,
            sent: Mutex::new(BTreeMap::new()),
            opening: Mutex::new(()),
        }
    }

    #[must_use]
    pub fn with_effects(mut self, effects: Arc<dyn Effects>) -> CliDaemon {
        self.effects = effects;
        self
    }

    /// Sends the digest of each turn's transcript to the hub as `machine`.
    #[must_use]
    pub fn with_machine(mut self, machine: MachineId) -> CliDaemon {
        self.machine = Some(machine);
        self
    }

    #[must_use]
    pub fn with_rules(mut self, rules: Rules) -> CliDaemon {
        self.rules = rules;
        self
    }

    fn database<T>(&self, f: impl FnOnce(&mut Connection) -> ignite_store::Result<T>) -> Result<T> {
        self.pool.with_database(f).map_err(|e| store_error(&e))
    }

    fn app(&self, name: &str) -> Result<&CliAppSetup> {
        self.setup
            .apps
            .get(name)
            .ok_or_else(|| Error::Invalid(format!("{name} does not run in CLI mode here")))
    }

    fn rows(&self) -> Result<Vec<CliSlot>> {
        self.database(|conn| store::list(conn))
    }

    fn row(&self, session: &SessionId) -> Result<Option<CliSlot>> {
        self.database(|conn| store::by_session(conn, session.as_str()))
    }

    fn turn_row(&self, session: &SessionId, turn: u32) -> Result<Option<CliTurn>> {
        let turns = self.database(|conn| store::turns(conn, session.as_str()))?;
        Ok(turns.into_iter().find(|t| t.turn == i64::from(turn)))
    }

    fn now(&self) -> Timestamp {
        self.clock.now()
    }

    /// Hides `model` of `app` from the slots' offers for [`HIDE_MODEL_S`].
    fn hide(&self, app: &str, model: &str) {
        let until = self.now().plus_seconds(HIDE_MODEL_S);
        lock(&self.hidden).insert((app.to_owned(), model.to_owned()), until);
    }

    /// One run of the `ignite-cli` loop: follows every slot that holds a session. A slot that
    /// fails is left as it is for the next run, and the first failure is returned once every
    /// slot has been looked at.
    pub fn step(&self) -> Result<()> {
        let mut first = None;
        for row in self.rows()? {
            if row.session.is_none() {
                continue;
            }
            if let Err(error) = self.follow(&row) {
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(()), Err)
    }

    fn follow(&self, row: &CliSlot) -> Result<()> {
        let slot = slot_of(row);
        let Some(session) = slot.session.clone() else {
            return Ok(());
        };
        match slot.state {
            SlotState::Starting => {
                if self.turns.recover(&session, slot.turn) == Recovery::NotStarted {
                    if self.held(&row.app).is_some() || self.retire_if_claim_lost(row)? {
                        return Ok(());
                    }
                    self.launch(row)
                } else {
                    self.mark_running(row)?;
                    self.watch(row)
                }
            }
            SlotState::Running => self.watch(row),
            SlotState::WaitingVerb => {
                if !self.verbs_of(&session).is_empty() {
                    return Ok(());
                }
                if slot
                    .ticket
                    .as_ref()
                    .is_some_and(|ticket| self.host.holders().get(&session) != Some(ticket))
                {
                    return self.retire_slot(row, &session);
                }
                self.start_next(row)
            }
            SlotState::BackingOff => {
                let due = row
                    .resume_at
                    .as_deref()
                    .and_then(|t| Timestamp::parse(t).ok())
                    .is_none_or(|at| at <= self.now());
                if due { self.start_next(row) } else { Ok(()) }
            }
            SlotState::Parked => self.free_if_released(row),
            SlotState::Free => Ok(()),
        }
    }

    fn mark_running(&self, row: &CliSlot) -> Result<()> {
        let session = row.session.clone().unwrap_or_default();
        let turn = row.turn;
        self.database(|conn| {
            conn.execute(
                "UPDATE cli_slots SET state = 'running', park_reason = NULL \
                 WHERE session = ?1 AND turn = ?2 \
                 AND state = 'starting'",
                rusqlite::params![session, turn],
            )?;
            conn.execute(
                "UPDATE cli_turns SET state = 'running' WHERE session = ?1 AND turn = ?2 \
                 AND state = 'starting'",
                rusqlite::params![session, turn],
            )?;
            Ok(())
        })
    }

    /// The slot's next turn, not yet recorded, and the IDs of the messages it delivers: its
    /// prompt is the one the slot waits to start with followed by every undelivered message.
    /// `None` when there is nothing to say.
    fn next_turn(&self, row: &CliSlot) -> Result<Option<(CliTurn, Vec<i64>)>> {
        let session = row.session.clone().unwrap_or_default();
        let waiting = self.database(|conn| store::undelivered(conn, &session))?;
        let mut prompt = row.next_prompt.clone().unwrap_or_default();
        // A prompt `decide` built already holds the messages it was told of; any that came
        // later are added after it. Every waiting message is delivered by this turn.
        let later: Vec<String> = waiting
            .iter()
            .filter(|m| !prompt.contains(&m.text))
            .map(|m| m.text.clone())
            .collect();
        if let Some(text) = (self.rules.prompt)(&later) {
            if !prompt.is_empty() {
                prompt.push_str("\n\n");
            }
            prompt.push_str(&text);
        }
        if prompt.is_empty() {
            return Ok(None);
        }
        let turn = CliTurn {
            session,
            turn: row.turn + 1,
            app: row.app.clone(),
            slot: row.slot,
            ticket: row.ticket.clone(),
            prompt,
            state: "starting".to_owned(),
            run: None,
            started_at: Some(self.now().to_string()),
            ended_at: None,
            exit: None,
            end_kind: None,
            transcript: None,
            model: None,
            cli_version: None,
            cost_usd: None,
            lines_applied: 0,
        };
        let deliver = waiting.iter().map(|m: &CliMessage| m.id).collect();
        Ok(Some((turn, deliver)))
    }

    /// Starts the slot's recorded turn, whose row says it is starting.
    fn launch(&self, row: &CliSlot) -> Result<()> {
        if self.held(&row.app).is_some() {
            return Ok(());
        }
        let slot = slot_of(row);
        let session = slot.session.clone().ok_or_else(|| {
            Error::Invalid(format!("slot {} of {} has no session", row.slot, row.app))
        })?;
        let turn = self
            .turn_row(&session, slot.turn)?
            .ok_or_else(|| Error::NotFound(format!("turn {} of {session}", slot.turn)))?;
        let setup = self.app(&row.app)?;
        let spec = TurnSpec {
            app: row.app.clone(),
            program: setup.program.program.clone(),
            args: setup.program.args.clone(),
            model: setup.model_for(&row.app, row.model.as_deref())?.to_owned(),
            effort: row.effort.clone(),
            transcript: row.transcript.clone(),
            new_transcript: if row.transcript.is_none() {
                Some(new_uuid()?)
            } else {
                None
            },
            prompt: turn.prompt.clone(),
        };
        let command = CommandSpec {
            args: setup.app.command(&spec),
            cwd: self.setup.checkout.clone(),
            env: vec![
                ("IGNITE_SESSION".to_owned(), session.to_string()),
                (
                    start::TURN_VARIABLE.to_owned(),
                    format!("{session}:{}", slot.turn),
                ),
            ],
            class: Class::Agent,
            cores: 0,
            resources: Vec::new(),
            grows: false,
            tree: None,
            ticket: slot.ticket.clone(),
            hang_limit_s: None,
            reserve: None,
        };
        self.turns.start(&session, slot.turn, &command)?;
        self.mark_running(row)
    }

    /// The `ignite` verbs of the session that still run, as `decide` names them.
    fn verbs_of(&self, session: &SessionId) -> Vec<ignite_core::cli::Verb> {
        let mut verbs: Vec<ignite_core::cli::Verb> = self
            .host
            .running_verbs(session)
            .into_iter()
            .map(|verb| ignite_core::cli::Verb {
                command: verb.verb,
                handle: verb.handle,
            })
            .collect();
        if verbs.is_empty()
            && let Some(command) = self.host.command(session).filter(|c| c.running)
        {
            verbs.push(ignite_core::cli::Verb {
                command: command.command,
                handle: None,
            });
        }
        verbs
    }
}

impl CliPool for CliDaemon {
    fn begin_if(
        &self,
        reservation: &Reservation,
        model: &str,
        effort: Option<&str>,
        resume: Option<&Resume>,
        fresh: bool,
        admit: &mut dyn FnMut() -> bool,
    ) -> Result<Option<SessionId>> {
        self.begin_ticket(reservation, model, effort, resume, fresh, admit)
    }

    fn handoff_note(&self, session: &SessionId) -> Option<String> {
        self.row(session).ok().flatten()?.handoff_note
    }

    fn begin_justiciar(
        &self,
        app: &str,
        prompt: &str,
        model: &str,
        effort: &str,
    ) -> Result<SessionId> {
        self.app(app)?;
        let _opening = lock(&self.opening);
        let session = Self::new_session(app)?;
        let row = self.claim_slot(
            app,
            0,
            &session,
            model,
            Some(effort),
            prompt.to_owned(),
            None,
        )?;
        self.open_slot(row)?;
        Ok(session)
    }

    /// Queues the message (see `messages`). An urgent one ends the running turn, unless a
    /// `land` of the session is under way, so the loop finds the turn interrupted and resumes
    /// it with the message; any other ends the session's `ignite wait`.
    fn message(&self, session: &SessionId, text: &str, urgent: bool) -> Result<()> {
        self.queue_message(session, text, urgent)
    }

    fn check(&self, session: &SessionId) -> SessionCheck {
        match self.row(session) {
            Ok(Some(row)) => {
                let slot = slot_of(&row);
                let running = match slot.state {
                    SlotState::Starting => true,
                    SlotState::Running => {
                        self.turns.recover(session, slot.turn) == Recovery::Running
                    }
                    _ => false,
                };
                if running {
                    SessionCheck::Running
                } else if slot.state == SlotState::WaitingVerb {
                    SessionCheck::WaitingForVerb
                } else {
                    SessionCheck::Idle
                }
            }
            Ok(None) => SessionCheck::Gone,
            // The database cannot be read: say it runs, so nothing releases its ticket.
            Err(_) => SessionCheck::Running,
        }
    }

    fn slots(&self, app: &str) -> Vec<Slot> {
        self.rows()
            .unwrap_or_default()
            .iter()
            .filter(|r| r.app == app)
            .map(slot_of)
            .collect()
    }

    fn retire(&self, session: &SessionId) -> Result<bool> {
        self.retire_lost(session)
    }

    fn worker_memory_mb(&self) -> u64 {
        self.turns.worker_memory_mb()
    }

    fn held(&self, app: &str) -> Option<Hold> {
        let holds = self.database(|conn| store::holds(conn)).ok()?;
        let row = holds.into_iter().find(|h| h.app == app)?;
        Some(health::hold_of(&row, self.now()))
    }

    fn probed(&self, app: &str) -> bool {
        self.effects.probed(app)
    }

    fn hidden_models(&self, app: &str) -> Vec<String> {
        let now = self.now();
        let mut hidden = lock(&self.hidden);
        hidden.retain(|_, until| *until > now);
        hidden
            .keys()
            .filter(|(a, _)| a == app)
            .map(|(_, model)| model.clone())
            .collect()
    }

    fn digest(&self, target: &str) -> Result<Option<(SessionId, Vec<TranscriptLine>)>> {
        self.session_digest(target)
    }
}
