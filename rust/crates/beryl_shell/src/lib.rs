//! What both app shells share, the Tauri app (`beryl_app`) and Beryl 3D (`beryl_3d`), so that
//! they keep one set of data: the profile an instance runs as ([`Profile`]), the folders it
//! keeps its data, logs and bug reports in ([`Dirs`]), the game sessions that save replays and
//! the games ledger there ([`sessions`]), resuming the newest unfinished game
//! ([`resume_latest`]), and finding and zipping a saved bug report ([`reports`]). A game saved
//! by either app resumes in the other. Nothing here names a Tauri or windowing type.

mod dirs;
mod profile;
pub mod reports;

pub use dirs::{Dirs, desktop_base, desktop_logs};
pub use profile::{InvalidName, Profile};

use beryl_engine::CardTable;
use beryl_session::{GameUpdate, SessionId, Sessions};

/// The sessions, playing with `cards`, writing replays into `<data>/replays` and the games ledger into
/// `<data>/games.jsonl`, and handing every update to `emit`.
pub fn sessions(
    cards: &'static CardTable,
    dirs: &Dirs,
    emit: impl Fn(GameUpdate) + Send + Sync + 'static,
) -> Sessions {
    Sessions::new(cards, Some(dirs.data.join("replays")), emit)
        .with_ledger(dirs.data.join("games.jsonl"))
}

/// Resumes the newest unfinished game saved in the sessions' replay folder, if there is one
/// and no game is being played, and returns its session. While a game is being played, returns
/// that game's session.
pub fn resume_latest(sessions: &Sessions) -> Option<SessionId> {
    sessions.resume_latest()
}
