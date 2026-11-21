//! Events the host hands to the client.

use beryl_protocol::card_records::CardRecords;
use beryl_protocol::ipc::LobbyUpdate;
use beryl_protocol::{AgentView, GameUpdate};

use std::sync::Arc;

use crate::command::Reply;
use crate::settings::Settings;
use kit_base::data::Data;
use kit_ui::input::{AssetReady, Insets, Key, Lifecycle, Pointer, Size, TextChange, Tier};

/// One thing that has happened, passed to `Client::handle`.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// Validated tuning data from the host.
    Data(Arc<Data>),
    /// The host could not validate an edit. The last valid tuning remains active.
    DataError(String),
    /// A view from the session, boxed because it is far larger than every other input.
    Update(Box<GameUpdate>),
    /// The card store's records of every card and token the match can show, from which the
    /// client makes the match's list of faces (`Client::faces`) when the match loads. Boxed
    /// because it is far larger than every other input.
    Cards(Box<CardRecords>),
    /// The drawing surface's size in logical points, how many physical pixels make one point,
    /// and the part of it that system bars and rounded corners leave clear.
    Surface {
        size: Size,
        scale: f32,
        safe_area: Insets,
    },
    /// A mouse, touch or pen event.
    Pointer(Pointer),
    Key(Key),
    /// Text typed into the focused field, or its native field's new contents on a phone.
    Text(TextChange),
    /// The platform's clipboard held this text when the client asked for it
    /// (`Request::Paste`): it is inserted at the focused field's caret.
    Paste(String),
    /// A face, or an atlas page of glyphs or icons, is ready on the GPU.
    Assets(AssetReady),
    /// The app was suspended or resumed, or the system is low on memory.
    Lifecycle(Lifecycle),
    /// Which platform the host runs on, sent once at startup: whether `Cmd` in a shortcut
    /// means the Command key, as on the Mac, or Control. Until it arrives the client takes the
    /// Mac's.
    Platform {
        mac: bool,
    },
    /// The visual tier to draw in.
    Tier(Tier),
    /// The session's answer to a `SessionCommand`.
    Reply(Reply),
    /// The manual table's lobby changed (`TableEvent::Lobby`): its new state, or no lobby once
    /// the table's game has started. Boxed because it is far larger than most inputs.
    Lobby(Box<LobbyUpdate>),
    /// The linked table reported a failure (`TableEvent::Error`): the other player has a
    /// different version of Beryl, or the connection ended with a reason.
    TableError(String),
    /// The player's saved settings, which the host hands over at startup.
    Settings(Settings),
    /// The agents a person may play, in the order they are offered (`beryl_session::agents`).
    /// The client shows the start screen when they arrive before any view of a game.
    Agents(Vec<AgentView>),
    /// The names of the recordings the testing screen lists, sorted. Only a tools build of the
    /// app sends it, so a release build's client has no testing screen to go to.
    Recordings(Vec<String>),
    /// The bug report asked for with `Request::SaveBug` was written, or why it was not.
    BugSaved(Result<(), String>),
    /// Nothing changes. A thread other than the host's sends it to wake a host that sleeps until
    /// an event, so that the host looks at what the thread left for it (`Feed::wake`).
    Nudge,
}
