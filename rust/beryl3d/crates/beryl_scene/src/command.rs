//! What the client core asks of the session and of the platform, and the session's answers
//! (docs/beryl-3d.md, "Hosts"). The host drains these queues after each frame.

use std::path::PathBuf;

use beryl_protocol::ipc::decks::CardSearchHit;
use beryl_protocol::ipc::{
    ChooseRequest, LobbyChange, LobbyUpdate, TableActReply, TableRefused, TableSoloRequest,
};
use beryl_protocol::prompt::{ClickReply, ClickRequest};
use beryl_protocol::protocol::{Side, TableAction};
use beryl_protocol::{DecisionId, SessionError, SessionId, StartRequest, StopsSetting};

use crate::frame::ElementId;
use crate::input::{Rect, Size};
use crate::screens::Music;
use crate::settings::Settings;

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
    /// Close the manual table (`TableConnections::leave`).
    TableLeave,
    /// Open a solo table's lobby, or change the lobby of the table open now
    /// (`TableConnections::lobby`).
    TableLobby(LobbyChange),
    /// Find the card store's tokens whose name holds this text, for the manual table's token
    /// search, as the web's `card_search` does with `tokens` set.
    CardSearch(String),
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
    /// The lobby asked for, which only `LobbyChange::View` answers with, or why the table
    /// refused the change. A change's new lobby comes back as `Input::Lobby`. Boxed because a
    /// lobby is far larger than every other reply.
    TableLobby(Result<Option<Box<LobbyUpdate>>, TableRefused>),
    /// The tokens a `CardSearch` for this text found, the best match first.
    CardSearch(String, Vec<CardSearchHit>),
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
