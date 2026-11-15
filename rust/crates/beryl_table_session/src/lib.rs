//! Projects manual Magic tables onto the game session's shared UI protocol.

pub mod decks;
mod host;
pub mod publication;
pub mod refusal;
mod saves;
mod script;
pub mod sessions;
mod view;

pub use saves::{HostSettings, SavedTable};
pub use sessions::TableSessions;
