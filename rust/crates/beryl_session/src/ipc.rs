//! The app's IPC surface: each command's name, arguments, result and error, the events, and
//! the messages a UI fixture records. `beryl_app` registers a Tauri command under each name
//! here, `beryl_cli` records fixtures with them, and [`crate::typescript`] writes the `Commands`
//! and `Events` maps `ui/src/ipc.ts` types its transport with.

use std::fmt::Write;

use serde::{Deserialize, Serialize};
use ts_rs::{Config, TS};

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

/// The event every view arrives on, carrying a [`GameUpdate`].
pub const GAME_UPDATE: &str = "game-update";

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
pub(crate) fn typescript(cfg: &Config) -> String {
    fn name<T: TS>(cfg: &Config) -> String {
        T::name(cfg)
    }
    let none = "Record<string, never>".to_owned();
    let args = |fields: &[(&str, String)]| {
        let fields: Vec<String> = fields.iter().map(|(n, ty)| format!("{n}: {ty}")).collect();
        format!("{{ {} }}", fields.join("; "))
    };
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
            args(&[("request", name::<StartRequest>(cfg))]),
            name::<SessionId>(cfg),
            error.clone(),
        ),
        (
            CHOOSE,
            args(&[
                ("decision", name::<DecisionId>(cfg)),
                ("index", name::<CandidateIndex>(cfg)),
            ]),
            name::<()>(cfg),
            error.clone(),
        ),
        (
            SET_STOPS,
            args(&[
                ("session", name::<SessionId>(cfg)),
                ("stops", name::<StopsSetting>(cfg)),
            ]),
            name::<()>(cfg),
            error,
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
