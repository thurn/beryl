//! What the client core asks of the session and of the platform, and the session's answers
//! (docs/beryl-3d.md, "Hosts"). The host drains these queues after each frame.

use std::path::PathBuf;

use beryl_session::ipc::ChooseRequest;
use beryl_session::prompt::{ClickReply, ClickRequest};
use beryl_session::{DecisionId, SessionError, SessionId, StartRequest, StopsSetting};

use crate::frame::ElementId;
use crate::input::Rect;

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
}

/// The session's answer to a `SessionCommand`. `Sessions::click` returns the prompt's next
/// state; every other function returns nothing or an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    Click(Result<ClickReply, SessionError>),
    Done(Result<(), SessionError>),
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
    /// Offer the file at `path` to the platform's share sheet.
    Share {
        path: PathBuf,
    },
    Haptic(Haptic),
    OpenUrl(String),
}

/// How strong a haptic tap is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haptic {
    Light,
    Medium,
}
