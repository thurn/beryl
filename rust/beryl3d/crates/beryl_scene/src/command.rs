//! What the client core asks of the session and of the platform, and the session's answers
//! (docs/beryl-3d.md, "Hosts"). The host drains these queues after each frame.

use std::path::PathBuf;

use beryl_protocol::ipc::decks::{CardSearchHit, DeckImportReply, DeckImportRequest, SavedDeck};
use beryl_protocol::ipc::{
    ChooseRequest, LobbyChange, LobbyUpdate, SavedTable, TableActReply, TableRefused,
    TableSoloRequest,
};
use beryl_protocol::prompt::{ClickReply, ClickRequest};
use beryl_protocol::protocol::{Answer, Side, TableAction};
use beryl_protocol::{DecisionId, SessionError, SessionId, StartRequest, StopsSetting};

use crate::frame::ElementId;
use crate::input::{Rect, Size};
use crate::settings::Settings;
use crate::sound::Music;

/// One variant for each function of `beryl_session::Sessions` that a player's action calls,
/// carrying that function's own arguments. The host sends it to the session thread and passes
/// the answer back as `Input::Reply`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionCommand {
    Start(StartRequest),
    Resume,
    Choose(ChooseRequest),
    Click(ClickRequest),
    Cancel(DecisionId),
    Undo(DecisionId),
    PassOn(DecisionId),
    SetStops(SessionId, StopsSetting),
    Concede(SessionId),
    /// Start a solo manual table with this deck (`TableConnections::solo`).
    TableSolo(TableSoloRequest),
    /// Change the manual table (`TableConnections::act`).
    TableAct(TableAction),
    /// Answer the table's prompt or one of its assists: a proposal's line or Apply, a state
    /// nudge's label, an ability from a permanent's menu, the sorting tray, or the guided start
    /// (`TableConnections::answer`). The table's reply comes back as `Reply::TableAct`.
    TableAnswer(Answer),
    /// Close the manual table (`TableConnections::leave`).
    TableLeave,
    /// Host a linked table on this computer's network under the host's name
    /// (`TableConnections::host` with `TableLink::Local`), as the web's browser build hosts
    /// one. Beryl 3D's session is built without internet play.
    TableHost {
        name: String,
    },
    /// Join the table that `invite` names, sitting under `name` (`TableConnections::join`).
    TableJoin {
        invite: String,
        name: String,
    },
    /// Open the saved table with this id again: a hosted table, or the last table this
    /// profile joined (`TableConnections::resume`).
    TableResume(String),
    /// List the tables that can be resumed (`TableConnections::saved`).
    TableSaved,
    /// Open a solo table's lobby, or change the lobby of the table open now
    /// (`TableConnections::lobby`).
    TableLobby(LobbyChange),
    /// Find the card store's tokens whose name holds this text, for the manual table's token
    /// search, as the web's `card_search` does with `tokens` set.
    CardSearch(String),
    /// List the decks saved in the deck library (`beryl_table_session::decks::list`), which
    /// the web app keeps in the same folder.
    DeckList,
    /// Read a pasted decklist and save it in the deck library under its name
    /// (`beryl_table_session::decks::import`).
    DeckImport(DeckImportRequest),
}

/// The session's answer to a `SessionCommand`. `Sessions::click` returns the prompt's next
/// state; every other function returns nothing or an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Click(Result<ClickReply, SessionError>),
    Done(Result<(), SessionError>),
    /// The session a solo table started, or why it did not start.
    TableSolo(Result<SessionId, TableRefused>),
    /// Whether the table took an action, or why it refused it.
    TableAct(TableActReply),
    /// Whether the table closed, or the reason it gave for not closing.
    TableLeave(Result<(), String>),
    /// Whether a hosted, joined or resumed table opened, or why it did not.
    TableOpen(Result<(), String>),
    /// The tables that can be resumed, the joined table first, or why they could not be read.
    TableSaved(Result<Vec<SavedTable>, String>),
    /// The lobby asked for, which only `LobbyChange::View` answers with, or why the table
    /// refused the change. A change's new lobby comes back as `Input::Lobby`. Boxed because a
    /// lobby is far larger than every other reply.
    TableLobby(Result<Option<Box<LobbyUpdate>>, TableRefused>),
    /// The tokens a `CardSearch` for this text found, the best match first.
    CardSearch(String, Vec<CardSearchHit>),
    /// The decks saved in the deck library, or why they could not be read.
    DeckList(Result<Vec<SavedDeck>, SessionError>),
    /// The deck a decklist was read as, now saved, or why it could not be read or saved.
    DeckImport(Result<DeckImportReply, SessionError>),
}

/// What the client asks of the platform.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Show a native text field over `field`, at `rect` in logical points, holding `text`.
    BeginTextEntry {
        field: ElementId,
        rect: Rect,
        text: String,
    },
    EndTextEntry,
    /// Put this text on the platform's clipboard: the selection of a field, copied or cut.
    Copy(String),
    /// Read the platform's clipboard and hand its text back as `Input::Paste`.
    Paste,
    /// Save every bug report the app keeps to one file, as the web's `exportBugs` does.
    ExportBugs,
    /// The bug report has frozen the table: read back the next frame the host draws, which
    /// shows nothing of the report, as the report's picture of the window, and keep the element
    /// tree of that frame beside it. The client keeps drawing that frame unchanged until the
    /// picture has been taken (`screens::BugStage::Capturing`).
    FreezeFrame,
    /// Save a bug report saying `description` about `region` of the frozen window, in logical
    /// points of a window of `window`'s size, flagged as a questionable play of the AI's if
    /// `ai`, with the picture and tree `FreezeFrame` took. The host answers with
    /// `Input::BugSaved`.
    SaveBug {
        description: String,
        ai: bool,
        region: Rect,
        window: Size,
    },
    /// Offer the file at `path` to the platform's share sheet.
    Share {
        path: PathBuf,
    },
    Haptic(Haptic),
    OpenUrl(String),
    /// Save the player's settings, to be handed back as `Input::Settings` at the next start.
    SaveSettings(Settings),
    /// Play this music from now on, or none. Sent each time the music the screen calls for
    /// changes (`screens::music`).
    Music(Option<Music>),
    /// Play music and sound effects at these levels, from 0 to 100, or none at all if `muted`.
    Audio {
        music: u8,
        effects: u8,
        muted: bool,
    },
    /// Quit the app, as the player confirmed when Quit asked first on the manual table. The
    /// session shuts the table down as the app ends, so that it can be resumed.
    Quit,
    /// Replay the recording `name` from its start as `seat` sees it, in place of whatever
    /// feeds the client now, as the app's `--fixture` does. Sent from the testing screen, which
    /// only a tools build reaches.
    Replay {
        name: String,
        seat: Side,
    },
}

/// How strong a haptic tap is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haptic {
    Light,
    Medium,
}
