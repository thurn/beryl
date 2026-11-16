//! The app's IPC surface ([`beryl_protocol::ipc`]), and the `Commands` and `Events` maps
//! `ui/src/ipc.ts` types its transport with, written by [`crate::typescript`].

use std::fmt::Write;

use ts_rs::{Config, TS};

pub use beryl_protocol::ipc::*;

use crate::bugs::{BugReport, BugSaved, UiLogRequest};
use crate::card_records::{CardRecords, CardRecordsRequest};
use crate::prompt::{ClickReply, ClickRequest};
use crate::protocol::SessionId;
use crate::script::OwnTimings;
use crate::{AgentView, GameUpdate, SessionError, StartRequest};

use decks::{
    CardSearchHit, CardSearchRequest, CardTokenLink, CardTokensRequest, DeckDeleteRequest,
    DeckImportReply, DeckImportRequest, DeckOpenReply, SavedDeck,
};

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
    let table_error = name::<TableConnectionError>(cfg);
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
            CLICK,
            request(name::<ClickRequest>(cfg)),
            name::<ClickReply>(cfg),
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
            RESUME_GAME,
            "Record<string, never>".to_owned(),
            name::<Option<SessionId>>(cfg),
            never.clone(),
        ),
        (
            STAGE_TIMINGS,
            "Record<string, never>".to_owned(),
            name::<OwnTimings>(cfg),
            never.clone(),
        ),
        (
            TABLE_SOLO,
            request(name::<TableSoloRequest>(cfg)),
            name::<SessionId>(cfg),
            name::<TableRefused>(cfg),
        ),
        (
            TABLE_HOST,
            request(name::<TableHostRequest>(cfg)),
            name::<TableHostReply>(cfg),
            table_error.clone(),
        ),
        (
            TABLE_JOIN,
            request(name::<TableJoinRequest>(cfg)),
            name::<SessionId>(cfg),
            table_error.clone(),
        ),
        (
            TABLE_RESUME,
            request(name::<TableResumeRequest>(cfg)),
            name::<TableHostReply>(cfg),
            table_error.clone(),
        ),
        (
            TABLE_SAVED,
            "Record<string, never>".to_owned(),
            name::<Vec<SavedTable>>(cfg),
            table_error.clone(),
        ),
        (
            TABLE_SETTINGS_GET,
            "Record<string, never>".to_owned(),
            name::<TableSettings>(cfg),
            table_error.clone(),
        ),
        (
            TABLE_SETTINGS_SET,
            request(name::<TableSettings>(cfg)),
            name::<()>(cfg),
            table_error.clone(),
        ),
        (
            TABLE_ACT,
            request(name::<TableActRequest>(cfg)),
            name::<TableActReply>(cfg),
            never.clone(),
        ),
        (
            TABLE_ANSWER,
            request(name::<TableAnswerRequest>(cfg)),
            name::<TableActReply>(cfg),
            never.clone(),
        ),
        (
            TABLE_LOBBY_COMMAND,
            request(name::<TableLobbyRequest>(cfg)),
            name::<Option<LobbyUpdate>>(cfg),
            name::<TableRefused>(cfg),
        ),
        (
            TABLE_CURRENT,
            "Record<string, never>".to_owned(),
            name::<Option<GameUpdate>>(cfg),
            never.clone(),
        ),
        (
            TABLE_LEAVE,
            "Record<string, never>".to_owned(),
            name::<()>(cfg),
            table_error,
        ),
        (
            TABLE_KEYS,
            request(name::<TableKeysRequest>(cfg)),
            name::<()>(cfg),
            error.clone(),
        ),
        (
            TABLE_QUIT,
            "Record<string, never>".to_owned(),
            name::<()>(cfg),
            error.clone(),
        ),
        (
            DECK_IMPORT,
            request(name::<DeckImportRequest>(cfg)),
            name::<DeckImportReply>(cfg),
            error.clone(),
        ),
        (
            DECK_LIST,
            "Record<string, never>".to_owned(),
            name::<Vec<SavedDeck>>(cfg),
            error.clone(),
        ),
        (
            DECK_DELETE,
            request(name::<DeckDeleteRequest>(cfg)),
            name::<()>(cfg),
            error.clone(),
        ),
        (
            DECK_OPEN_FILE,
            "Record<string, never>".to_owned(),
            name::<DeckOpenReply>(cfg),
            error.clone(),
        ),
        (
            CARD_SEARCH,
            request(name::<CardSearchRequest>(cfg)),
            name::<Vec<CardSearchHit>>(cfg),
            never.clone(),
        ),
        (
            CARD_TOKENS,
            request(name::<CardTokensRequest>(cfg)),
            name::<Vec<CardTokenLink>>(cfg),
            never.clone(),
        ),
        (
            CAPTURE_WINDOW,
            "Record<string, never>".to_owned(),
            name::<u32>(cfg),
            error.clone(),
        ),
        (
            REPORT_BUG,
            request(name::<BugReport>(cfg)),
            name::<BugSaved>(cfg),
            error.clone(),
        ),
        (
            EXPORT_BUGS,
            "Record<string, never>".to_owned(),
            name::<Option<u32>>(cfg),
            error.clone(),
        ),
        (
            SHARE_BUG,
            request(name::<ShareBugRequest>(cfg)),
            name::<()>(cfg),
            error.clone(),
        ),
        (
            UI_LOG,
            request(name::<UiLogRequest>(cfg)),
            name::<()>(cfg),
            never.clone(),
        ),
        (
            CARD_RECORDS,
            request(name::<CardRecordsRequest>(cfg)),
            name::<CardRecords>(cfg),
            never.clone(),
        ),
        (
            STAGE_ANIMATING,
            request(name::<StageAnimatingRequest>(cfg)),
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
        "  \"{GAME_UPDATE}\": {};\n  \"{TABLE_QUIT_ASKED}\": {};\n  \"{TABLE_LINK}\": LinkState | ({{ kind: \"error\" }} & TableConnectionError);\n  \"{TABLE_LOBBY}\": {};\n}};",
        name::<GameUpdate>(cfg),
        name::<()>(cfg),
        name::<LobbyUpdate>(cfg)
    )
    .expect("a String takes writes");
    out
}
