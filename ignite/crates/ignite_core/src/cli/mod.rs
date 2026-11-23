//! CLI mode's pure logic: the types the daemon uses to run each worker as a headless CLI
//! process, one process per turn (see "CLI mode" in the design document).
//!
//! - [`CliApp`] is one CLI program: it builds a turn's command line, reads the events the
//!   turn printed, and says how the turn ended. [`claude`] and [`codex`] implement it.
//! - [`decide`] is the pure function that turns a turn's end and the facts around it into the
//!   [`Action`] the daemon carries out next.
//!
//! Nothing about a session lives in the daemon's memory: a [`Slot`] is a row of the daemon's
//! database, and a turn's events are read again from its `output.log` whenever the daemon looks.

use std::sync::{Arc, LazyLock};

use serde::{Deserialize, Serialize};

use crate::ids::{SessionId, TicketId};
use crate::time::Timestamp;

pub mod agy;
pub mod claude;
pub mod codex;
pub mod decide;
pub mod digest;

/// Where one CLI worker slot is between turns and during one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotState {
    /// A turn is recorded as starting, and its supervisor may not have started yet.
    Starting,
    /// A turn's process runs.
    Running,
    /// The turn ended while an `ignite` verb of the session runs; the next turn starts when
    /// the verb ends.
    WaitingVerb,
    /// The next turn starts after a pause, as after a single API error.
    BackingOff,
    /// No turn starts until the app's CLI hold ends or a person acts. The ticket stays held.
    Parked,
    /// The slot holds no ticket.
    #[default]
    Free,
}

impl SlotState {
    pub const ALL: [SlotState; 6] = [
        SlotState::Starting,
        SlotState::Running,
        SlotState::WaitingVerb,
        SlotState::BackingOff,
        SlotState::Parked,
        SlotState::Free,
    ];

    /// The name stored in `cli_slots.state`.
    pub fn as_str(self) -> &'static str {
        match self {
            SlotState::Starting => "starting",
            SlotState::Running => "running",
            SlotState::WaitingVerb => "waiting_verb",
            SlotState::BackingOff => "backing_off",
            SlotState::Parked => "parked",
            SlotState::Free => "free",
        }
    }

    /// The state stored as `name`; `None` for a name this build does not know, which a reader
    /// treats as parked so that it starts nothing.
    pub fn parse(name: &str) -> Option<SlotState> {
        SlotState::ALL.into_iter().find(|s| s.as_str() == name)
    }
}

impl std::fmt::Display for SlotState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One CLI worker slot of one app on this machine, as the daemon's `cli_slots` row holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    /// The app, such as `claude` or `codex`.
    pub app: String,
    /// The slot's number, from 1. The placeholder session offered to the hub while the slot is
    /// free is `cli-<app>-slot-<index>`.
    pub index: u32,
    /// The Ignite session of the ticket the slot works, `cli-<app>-<8 hex>`; `None` while free.
    pub session: Option<SessionId>,
    pub ticket: Option<TicketId>,
    pub state: SlotState,
    /// The number of the latest turn, from 1; 0 before the first.
    pub turn: u32,
    /// The CLI's own transcript ID (Claude's session UUID or Codex's thread ID), read from the
    /// latest turn's first line. `None` before any turn printed one.
    pub transcript: Option<String>,
    /// The model ID every turn asks for.
    pub model: Option<String>,
    /// The reasoning effort every turn asks for.
    pub effort: Option<String>,
    /// When the slot entered its state.
    pub since: Option<Timestamp>,
}

/// Everything one turn's command line is built from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnSpec {
    pub app: String,
    /// The program, from `[pool.cli.<app>]`.
    pub program: String,
    /// The arguments every turn of the app gets, from `[pool.cli.<app>]`.
    pub args: Vec<String>,
    pub model: String,
    pub effort: Option<String>,
    /// The transcript to resume; `None` starts a new one.
    pub transcript: Option<String>,
    /// For a new Claude transcript, the session UUID to give it; ignored when resuming and by
    /// apps that choose their own ID.
    pub new_transcript: Option<String>,
    pub prompt: String,
}

/// One window of the plan's usage, as one event of a turn reported it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageSample {
    /// The window's name as the CLI prints it, such as `five_hour` or `seven_day_opus`.
    pub window: String,
    /// The share of the window used, from 0 to 1.
    pub utilization: f64,
    /// When the window resets, as the CLI printed it.
    pub resets_at: Option<String>,
    /// The CLI's status for the window, such as `allowed` or `rejected`.
    pub status: Option<String>,
}

/// The tokens one turn used, from its `result` line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// What a turn's output said, as far as it has been written.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TurnEvents {
    /// The transcript ID the first line named.
    pub transcript: Option<String>,
    /// The model the first line named; Codex names none.
    pub model: Option<String>,
    /// The CLI's version, as the first line named it.
    pub cli_version: Option<String>,
    /// Every usage sample, in order.
    pub usage: Vec<UsageSample>,
    /// The turn's cost in US dollars, from its result.
    pub cost_usd: Option<f64>,
    pub tokens: Option<TokenCounts>,
    /// The text of the last assistant message.
    pub last_text: Option<String>,
    /// Whether the stream ended with a result line (Claude's `result`, Codex's
    /// `turn.completed` or `turn.failed`). A turn without one was interrupted.
    pub result_seen: bool,
    /// Whether the result says the turn failed.
    pub result_error: bool,
    /// The error the stream reported, such as a result's subtype and message or a
    /// `turn.failed` message, for `classify` and for reports.
    pub error: Option<String>,
    /// The number of lines read, JSON or not, so a reader can tell a turn that printed nothing.
    pub lines: u32,
    /// A sanitized diagnostic from before the first event, for holds on startup failures.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup_error: Option<String>,
}

/// Why a turn's failure is not the session's own, which puts the app on a CLI hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldReason {
    NotLoggedIn,
    ModelRefused,
    /// The worker reached the plan's limit.
    PlanLimit,
    /// The program could not be started.
    ProgramMissing,
    /// The program exited before printing its first line.
    NoFirstLine,
    /// An API error such as a 5xx answer, an overloaded service or a network failure. A stored
    /// hold that names no reason reads as this one, the mildest: it ends when a probe succeeds.
    #[default]
    ApiError,
}

impl HoldReason {
    /// The name stored in `cli_holds.reason`.
    pub fn as_str(self) -> &'static str {
        match self {
            HoldReason::NotLoggedIn => "not_logged_in",
            HoldReason::ModelRefused => "model_refused",
            HoldReason::PlanLimit => "plan_limit",
            HoldReason::ProgramMissing => "program_missing",
            HoldReason::NoFirstLine => "no_first_line",
            HoldReason::ApiError => "api_error",
        }
    }

    /// The reason stored as `name`; `None` for a name this build does not know.
    pub fn parse(name: &str) -> Option<HoldReason> {
        [
            HoldReason::NotLoggedIn,
            HoldReason::ModelRefused,
            HoldReason::PlanLimit,
            HoldReason::ProgramMissing,
            HoldReason::NoFirstLine,
            HoldReason::ApiError,
        ]
        .into_iter()
        .find(|reason| reason.as_str() == name)
    }
}

/// How a turn ended, as [`CliApp::classify`] reads its events and exit. These are the kinds of
/// "How a turn ends" in the design document, except that a clean end is one kind here: whether
/// the session still holds its ticket comes from the hub, which [`decide`] is told in
/// [`Facts`], so [`decide`] splits it into the first two.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnEnd {
    /// The turn printed its result and the process exited.
    Clean,
    /// No result line: a crash, a kill, a reboot, or the daemon ended a hung turn.
    Interrupted,
    /// The plan's limit was reached in `window`, when the stream named one.
    PlanLimit { window: Option<String> },
    /// The failure is not the session's: the slot is parked and the app goes on a CLI hold.
    Hold {
        #[serde(default)]
        reason: HoldReason,
        #[serde(default)]
        message: String,
    },
    /// An exit or event the adapter cannot place.
    Unknown {
        #[serde(default)]
        why: String,
    },
}

/// What the daemon does after a turn's end, as [`decide`] chooses it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Action {
    /// The ticket is no longer held: the slot is free.
    Free,
    /// Start the next turn now with `prompt`.
    Resume { prompt: String },
    /// Start the next turn with `prompt` when the session's `ignite` verb `handle` ends.
    WaitForVerb { handle: String, prompt: String },
    /// Start the next turn with `prompt` at `at`.
    BackOff { at: Timestamp, prompt: String },
    /// Give back the ticket, keeping its work, with `note`; the slot is then free.
    Release { note: String },
    /// Park the slot with its ticket held and put the app on a CLI hold.
    Hold { reason: HoldReason, message: String },
    /// Park the slot with its ticket held and report `report`. Never releases a ticket and
    /// never starts a turn.
    Park { report: String },
}

/// An app's CLI hold: while it lasts, no turn starts for the app on this machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hold {
    pub app: String,
    pub reason: HoldReason,
    pub since: Timestamp,
    /// The CLI's version when the hold began, when a turn had named it.
    pub cli_version: Option<String>,
    /// The first line of the error.
    pub message: String,
}

/// An `ignite` verb of the session that still runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verb {
    /// The verb and its first argument, such as `land`.
    pub command: String,
    /// The handle to wait on, such as `L-5521`.
    pub handle: String,
}

/// How the daemon answered a session's previous turn end, where the rules answer a second such
/// end differently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviousAnswer {
    /// Neither of the others.
    #[default]
    Other,
    /// A clean end was answered by resuming with the turn-ended block text; a second one
    /// releases the ticket.
    BlockSent,
    /// A single API error was answered with a back-off; a second one puts the app on a hold.
    BackedOff,
}

/// What [`decide`] needs besides the slot and the turn's end.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facts {
    pub now: Timestamp,
    /// Whether the session still holds its ticket.
    pub ticket_held: bool,
    /// The session's `ignite` verbs that still run.
    pub running_verbs: Vec<Verb>,
    /// Whether a `land` of the session has begun its push.
    pub land_pushing: bool,
    /// The undelivered messages, in order, each starting `Message about <ticket>:`.
    pub messages: Vec<String>,
    /// Interrupted ends in a row with no `ignite` command between them, this one included.
    pub interrupted_in_row: u32,
    /// How the daemon answered the session's previous end, when that answer changes this one.
    pub previous: PreviousAnswer,
    /// The app's CLI hold, if it is on one.
    pub hold: Option<Hold>,
    /// The worker's last assistant text, for a release's note.
    pub last_text: Option<String>,
}

static ADAPTERS: LazyLock<Vec<Arc<dyn CliApp>>> = LazyLock::new(|| {
    vec![
        Arc::new(claude::Claude),
        Arc::new(codex::Codex),
        Arc::new(agy::Agy),
    ]
});

/// The adapter for a CLI dialect supported by this build.
pub fn adapter(dialect: &str) -> Option<Arc<dyn CliApp>> {
    ADAPTERS.iter().find(|app| app.name() == dialect).cloned()
}

/// The dialects a provider can select in `[pool.cli.<provider>]`.
pub fn dialects() -> impl Iterator<Item = &'static str> {
    ADAPTERS.iter().map(|app| app.name())
}

/// One CLI's command-line and event format. Multiple providers can use the same adapter.
pub trait CliApp: Send + Sync {
    /// The dialect name, used to select this adapter.
    fn name(&self) -> &'static str;
    /// The program and every argument of one turn.
    fn command(&self, spec: &TurnSpec) -> Vec<String>;
    /// What the text of a turn's `output.log`, as far as it has been written, says. Adapters
    /// read JSON events they recognize and may also extract sanitized startup diagnostics.
    fn read(&self, output: &str) -> TurnEvents;
    /// How a turn ended, from its events and its exit code (`None` when a signal ended it or no
    /// exit was recorded).
    fn classify(&self, events: &TurnEvents, exit: Option<i32>) -> TurnEnd;
    /// Human-readable transcript lines for the pool watch and dashboard.
    fn digest(&self, _output: &str) -> Vec<digest::Entry> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_states_round_trip_through_their_stored_names() {
        for state in SlotState::ALL {
            assert_eq!(SlotState::parse(state.as_str()), Some(state));
            assert_eq!(
                serde_json::to_value(state).unwrap(),
                serde_json::Value::from(state.as_str())
            );
        }
        assert_eq!(SlotState::parse("dreaming"), None);
    }

    /// `cli_turns.end_kind` keeps a `TurnEnd` as JSON, and an older build's rows must stay
    /// readable when a later build adds a field. So every field of every variant takes a
    /// default when it is missing, and a variant with no other field reads from its tag alone.
    #[test]
    fn every_field_of_a_stored_turn_end_has_a_default() {
        for kind in ["clean", "interrupted", "plan_limit", "hold", "unknown"] {
            let json = format!(r#"{{"kind":"{kind}"}}"#);
            serde_json::from_str::<TurnEnd>(&json)
                .unwrap_or_else(|e| panic!("{json} does not read as a TurnEnd: {e}"));
        }
        // A tag alone must name each variant, so that a variant added without a default for
        // one of its fields fails above rather than in a daemon's database.
        let named = [
            TurnEnd::Clean,
            TurnEnd::Interrupted,
            TurnEnd::PlanLimit { window: None },
            TurnEnd::Hold {
                reason: HoldReason::default(),
                message: String::new(),
            },
            TurnEnd::Unknown { why: String::new() },
        ];
        for end in named {
            let json = serde_json::to_value(&end).unwrap();
            let kind = json["kind"].as_str().unwrap().to_owned();
            let tag_only = serde_json::json!({ "kind": kind });
            serde_json::from_value::<TurnEnd>(tag_only).unwrap();
        }
    }

    #[test]
    fn hold_reasons_round_trip_through_their_stored_names() {
        for reason in [
            HoldReason::NotLoggedIn,
            HoldReason::ModelRefused,
            HoldReason::PlanLimit,
            HoldReason::ProgramMissing,
            HoldReason::NoFirstLine,
            HoldReason::ApiError,
        ] {
            assert_eq!(HoldReason::parse(reason.as_str()), Some(reason));
        }
        assert_eq!(HoldReason::parse("sulking"), None);
    }
}
