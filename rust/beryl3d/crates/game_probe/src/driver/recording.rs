//! The events that control recording playback.

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::Status;

/// The event that a recording made with no engine, such as a screen's, holds to put the client
/// on a screen or open a sheet over it before its first view. Its payload is the name of the
/// screen or the sheet (`Screen::name`, `Sheet::name`).
pub const OPEN: &str = "beryl3d-open";

/// The event that a recording made with no engine, such as the testing screen's, holds to give
/// the client the names of the recordings a tools build lists (`Input::Recordings`). Its
/// payload is an array of the names.
pub const RECORDINGS: &str = "beryl3d-recordings";

/// Whether the session follows `update` with its next view unasked, because the game plays on
/// without the viewer, as the fixture transport's `playsOn` says.
#[must_use]
pub fn plays_on(update: &GameUpdate) -> bool {
    matches!(update.view.status, Status::Thinking | Status::Playing)
}
