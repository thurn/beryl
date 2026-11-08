//! The app's IPC surface: each command's name, request, result and error, the events, and
//! the messages a UI fixture records. `beryl_app` registers a Tauri command under each name
//! here (checked when it compiles), each taking its request as one argument named `request`;
//! `beryl_cli` records fixtures with them, and [`crate::typescript`] writes the `Commands` and
//! `Events` maps `ui/src/ipc.ts` types its transport with.

use std::fmt::Write;

use serde::{Deserialize, Serialize};
use ts_rs::{Config, TS};

use crate::bugs::{BugReport, BugSaved, UiLogRequest};
use crate::protocol::{CandidateIndex, DecisionId, SessionId, StopsSetting};
use crate::{AgentView, GameUpdate, SessionError, StartRequest};

pub const APP_INFO: &str = "app_info";
/// The UI calls this once it has rendered.
pub const UI_READY: &str = "ui_ready";
pub const LIST_AGENTS: &str = "list_agents";
/// Starts a game ([`StartRequest`]), replacing any in progress; returns its [`SessionId`].
pub const START_GAME: &str = "start_game";
/// Answers the human's open decision with a candidate.
pub const CHOOSE: &str = "choose";
/// Switches the human's priority stops.
pub const SET_STOPS: &str = "set_stops";
/// Takes back the spell the human is casting, or the ability they are activating, before its
/// costs are paid ([`crate::protocol::DecisionView::cancellable`]).
pub const CANCEL: &str = "cancel";
/// Takes back the human's latest answer ([`crate::protocol::DecisionView::undo`]).
pub const UNDO: &str = "undo";
/// Passes the human's priority on to their next stop
/// ([`crate::protocol::DecisionView::pass_on`]).
pub const PASS_ON: &str = "pass_on";
/// The human concedes the game in progress.
pub const CONCEDE: &str = "concede";
/// The latest view of the game in progress ([`crate::Sessions::current`]), which a reloaded UI
/// takes up the game from.
pub const CURRENT_GAME: &str = "current_game";

/// The window as it is drawn now, a PNG in base64: the bug report's frozen frame. Only the app
/// has it.
pub const CAPTURE_WINDOW: &str = "capture_window";
/// Writes a bug report ([`crate::bugs::BugReport`]); returns where
/// ([`crate::bugs::BugSaved`]).
pub const REPORT_BUG: &str = "report_bug";
/// Journals the UI's warnings and errors ([`crate::bugs::UiLogRequest`]).
pub const UI_LOG: &str = "ui_log";

/// The event every view arrives on, carrying a [`GameUpdate`].
pub const GAME_UPDATE: &str = "game-update";

/// The [`CHOOSE`] command's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChooseRequest {
    /// The decision the UI received.
    pub decision: DecisionId,
    pub index: CandidateIndex,
}

/// The [`SET_STOPS`] command's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetStopsRequest {
    pub session: SessionId,
    pub stops: StopsSetting,
}

/// The [`CANCEL`] command's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CancelRequest {
    /// The open decision the UI received, part of the cast.
    pub decision: DecisionId,
}

/// The [`UNDO`] command's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UndoRequest {
    /// The open decision the UI received.
    pub decision: DecisionId,
}

/// The [`PASS_ON`] command's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PassOnRequest {
    /// The open decision the UI received.
    pub decision: DecisionId,
}

/// The [`CONCEDE`] command's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConcedeRequest {
    pub session: SessionId,
}

/// What the app is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    /// Running under `just smoke`.
    pub smoke: bool,
}

/// A recorded stream of the app's messages, which the UI's fixture transport replays.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Fixture {
    pub messages: Vec<FixtureMessage>,
}

/// One recorded message: a command's response, or an event the app emitted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FixtureMessage {
    Response {
        #[ts(type = "keyof Commands")]
        command: String,
        #[ts(type = "unknown")]
        value: serde_json::Value,
    },
    Event {
        #[ts(type = "keyof Events")]
        event: String,
        #[ts(type = "unknown")]
        payload: serde_json::Value,
    },
}

impl FixtureMessage {
    pub fn response(command: &str, value: &impl Serialize) -> FixtureMessage {
        FixtureMessage::Response {
            command: command.to_owned(),
            value: serde_json::to_value(value).expect("a response serializes"),
        }
    }

    pub fn game_update(update: &GameUpdate) -> FixtureMessage {
        FixtureMessage::Event {
            event: GAME_UPDATE.to_owned(),
            payload: serde_json::to_value(update).expect("a view serializes"),
        }
    }
}

/// The `Commands` and `Events` maps, as TypeScript.
#[allow(clippy::too_many_lines)] // A line or five per command.
pub(crate) fn typescript(cfg: &Config) -> String {
    fn name<T: TS>(cfg: &Config) -> String {
        T::name(cfg)
    }
    let none = "Record<string, never>".to_owned();
    let request = |ty: String| format!("{{ request: {ty} }}");
    let error = name::<SessionError>(cfg);
    let never = "never".to_owned();
    let commands = [
        (APP_INFO, none.clone(), name::<AppInfo>(cfg), never.clone()),
        (UI_READY, none.clone(), name::<()>(cfg), never.clone()),
        (
            LIST_AGENTS,
            none,
            name::<Vec<AgentView>>(cfg),
            never.clone(),
        ),
        (
            START_GAME,
            request(name::<StartRequest>(cfg)),
            name::<SessionId>(cfg),
            error.clone(),
        ),
        (
            CHOOSE,
            request(name::<ChooseRequest>(cfg)),
            name::<()>(cfg),
            error.clone(),
        ),
        (
            SET_STOPS,
            request(name::<SetStopsRequest>(cfg)),
            name::<()>(cfg),
            error.clone(),
        ),
        (
            CANCEL,
            request(name::<CancelRequest>(cfg)),
            name::<()>(cfg),
            error.clone(),
        ),
        (
            UNDO,
            request(name::<UndoRequest>(cfg)),
            name::<()>(cfg),
            error.clone(),
        ),
        (
            PASS_ON,
            request(name::<PassOnRequest>(cfg)),
            name::<()>(cfg),
            error.clone(),
        ),
        (
            CONCEDE,
            request(name::<ConcedeRequest>(cfg)),
            name::<()>(cfg),
            error.clone(),
        ),
        (
            CURRENT_GAME,
            "Record<string, never>".to_owned(),
            name::<Option<GameUpdate>>(cfg),
            never.clone(),
        ),
        (
            CAPTURE_WINDOW,
            "Record<string, never>".to_owned(),
            name::<String>(cfg),
            error.clone(),
        ),
        (
            REPORT_BUG,
            request(name::<BugReport>(cfg)),
            name::<BugSaved>(cfg),
            error.clone(),
        ),
        (
            UI_LOG,
            request(name::<UiLogRequest>(cfg)),
            name::<()>(cfg),
            never.clone(),
        ),
    ];
    let mut out = String::from(
        "/** Each command: its arguments, its result, and the error it rejects with. */\n\
         export type Commands = {\n",
    );
    for (command, args, result, error) in commands {
        writeln!(
            out,
            "  {command}: {{ args: {args}; result: {result}; error: {error} }};"
        )
        .expect("a String takes writes");
    }
    out.push_str("};\n\n/** Each event's payload. */\nexport type Events = {\n");
    writeln!(
        out,
        "  \"{GAME_UPDATE}\": {};\n}};",
        name::<GameUpdate>(cfg)
    )
    .expect("a String takes writes");
    out
}
