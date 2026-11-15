//! The app's IPC surface: each command's name, request, result and error, the events, and
//! the messages a UI fixture records. `beryl_app` registers a Tauri command under each name
//! here (checked when it compiles), each taking its request as one argument named `request`;
//! `beryl_cli` records fixtures with them, and `beryl_session::typescript` writes the `Commands` and
//! `Events` maps `ui/src/ipc.ts` types its transport with.

use beryl_table::Refusal;
use beryl_table::deck::Deck;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::GameUpdate;
use crate::protocol::{CandidateIndex, DecisionId, SessionId, StopsSetting, TableAction};

pub mod decks;
mod table;
pub use table::*;

pub const APP_INFO: &str = "app_info";
/// The UI calls this once it has rendered.
pub const UI_READY: &str = "ui_ready";
pub const LIST_AGENTS: &str = "list_agents";
/// Starts a game ([`StartRequest`]), replacing any in progress; returns its [`SessionId`].
pub const START_GAME: &str = "start_game";
/// Answers the human's open decision with a candidate.
pub const CHOOSE: &str = "choose";
/// Takes a click on the human's open prompt ([`ClickRequest`]) and returns the prompt's state
/// after it, or that the click answered it ([`ClickReply`]).
pub const CLICK: &str = "click";
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
/// Resumes the newest unfinished game saved, unless a game is in progress
/// ([`crate::Sessions::resume_latest`]); returns the [`SessionId`] of the game in progress,
/// if any now. Its views arrive as a started game's do.
pub const RESUME_GAME: &str = "resume_game";
/// The timing of the steps the stage makes itself while a prompt is open
/// ([`crate::script::own_timings`]), which it asks for once.
pub const STAGE_TIMINGS: &str = "stage_timings";

/// Starts a solo manual table.
pub const TABLE_SOLO: &str = "table_solo";
/// Applies one manual action using references from the published view.
pub const TABLE_ACT: &str = "table_act";
/// Returns the manual table's latest update, if any.
pub const TABLE_CURRENT: &str = "table_current";
/// Closes the current manual table.
pub const TABLE_LEAVE: &str = "table_leave";
/// Gives the manual table its macOS shortcuts while the table screen is open.
pub const TABLE_KEYS: &str = "table_keys";
/// Asks the player before quitting the desktop app at a manual table.
pub const TABLE_QUIT: &str = "table_quit";

pub const DECK_IMPORT: &str = "deck_import";
pub const DECK_LIST: &str = "deck_list";
pub const DECK_DELETE: &str = "deck_delete";
pub const DECK_OPEN_FILE: &str = "deck_open_file";
pub const CARD_SEARCH: &str = "card_search";
pub const CARD_TOKENS: &str = "card_tokens";

/// Snapshots the window as it is drawn now, the bug report's frozen frame, and returns its
/// number: the app serves it at `beryl-art://localhost/snapshot/<n>` and turns it into the
/// report's images. Only the app has it.
pub const CAPTURE_WINDOW: &str = "capture_window";
/// Writes a bug report ([`crate::bugs::BugReport`]); returns where
/// ([`crate::bugs::BugSaved`]).
pub const REPORT_BUG: &str = "report_bug";
/// Saves the current bug reports as a ZIP. Returns the count, or None if canceled.
pub const EXPORT_BUGS: &str = "export_bugs";
/// Shares one saved bug report as a ZIP through the phone OS.
pub const SHARE_BUG: &str = "share_bug";
/// Journals the UI's warnings and errors ([`crate::bugs::UiLogRequest`]).
pub const UI_LOG: &str = "ui_log";
/// The card store's records for the names asked for ([`crate::card_records`]).
pub const CARD_RECORDS: &str = "card_records";
/// Says whether the stage is animating, so that a long search runs fewer threads meanwhile
/// and the frames play on time (`beryl_search::pace`).
pub const STAGE_ANIMATING: &str = "stage_animating";

/// The event every view arrives on, carrying a [`GameUpdate`].
pub const GAME_UPDATE: &str = "game-update";
/// The table's native Quit menu item was invoked.
pub const TABLE_QUIT_ASKED: &str = "table-quit-asked";

/// The [`CHOOSE`] command's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChooseRequest {
    /// The decision the UI received.
    pub decision: DecisionId,
    pub index: CandidateIndex,
    /// Joins a follow-up answer to the preceding choice for Undo.
    #[ts(optional)]
    pub continuation: Option<bool>,
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

/// The [`SHARE_BUG`] command's request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShareBugRequest {
    pub id: String,
}

/// The [`STAGE_ANIMATING`] command's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StageAnimatingRequest {
    pub animating: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TableSoloRequest {
    pub deck: Deck,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TableActRequest {
    pub action: TableAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TableKeysRequest {
    pub on: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TableRefusalReason {
    Permission,
    Stale,
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TableRefused {
    pub refused: TableRefusalReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub blocked_by: Option<Vec<u32>>,
}

impl From<Refusal> for TableRefused {
    fn from(refusal: Refusal) -> Self {
        let (refused, blocked_by) = match refusal {
            Refusal::Permission => (TableRefusalReason::Permission, None),
            Refusal::Stale => (TableRefusalReason::Stale, None),
            Refusal::Invalid { blocking } => (
                TableRefusalReason::Invalid,
                (!blocking.is_empty()).then_some(blocking),
            ),
        };
        Self {
            refused,
            blocked_by,
        }
    }
}

/// A rejected action is a normal reply the stage can use for gesture feedback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum TableActReply {
    Ok {
        #[ts(type = "true")]
        ok: bool,
    },
    Refused(TableRefused),
}

impl From<Result<(), Refusal>> for TableActReply {
    fn from(result: Result<(), Refusal>) -> Self {
        match result {
            Ok(()) => Self::Ok { ok: true },
            Err(refusal) => Self::Refused(refusal.into()),
        }
    }
}

/// The operating system hosting the app or live server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
    Ios,
    Android,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "ios") {
            Self::Ios
        } else if cfg!(target_os = "android") {
            Self::Android
        } else {
            Self::Linux
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Ios => "ios",
            Self::Android => "android",
        }
    }
}

/// What the app is, including its wire fingerprint for multiplayer compatibility.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    /// Running under `just smoke`.
    pub smoke: bool,
    pub platform: Platform,
    /// Running in the iOS Simulator or Android Emulator.
    pub simulator: bool,
    pub wire: String,
    pub profile: Option<String>,
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
        /// The request it answered, where a replay holds the UI to it: a click on a prompt,
        /// whose reply is the state after that click and no other.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional, type = "unknown")]
        request: Option<serde_json::Value>,
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
            request: None,
            value: serde_json::to_value(value).expect("a response serializes"),
        }
    }

    /// The response `value` to `command` asked with `request`, which a replay answers that
    /// request alone with.
    pub fn asked(
        command: &str,
        request: &impl Serialize,
        value: &impl Serialize,
    ) -> FixtureMessage {
        FixtureMessage::Response {
            command: command.to_owned(),
            request: Some(serde_json::to_value(request).expect("a request serializes")),
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
