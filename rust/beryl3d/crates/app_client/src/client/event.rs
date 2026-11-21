//! Events the host hands to the client.

use beryl_protocol::card_records::CardRecords;
use beryl_protocol::ipc::LobbyUpdate;
use beryl_protocol::{AgentView, GameUpdate};

use std::sync::Arc;

use crate::command::Reply;
use crate::settings::Settings;
use kit_base::data::Data;

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
    Device(kit_ui::input::Input),
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
