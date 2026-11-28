//! Projects manual Magic tables onto the game session's shared UI protocol.

mod assists;
pub mod connections;
pub mod decks;
mod guest;
mod host;
pub mod lobby;
pub mod prompt;
pub mod publication;
pub mod refusal;
mod saves;
mod script;
pub mod sessions;
mod view;

pub use beryl_protocol::ipc::SavedTable;
pub use saves::HostSettings;
pub use sessions::{TableDeckSnapshot, TableEvent, TableReportSnapshot, TableSessions};
