//! A manual Magic table records cards, their zones and the players' actions. It never
//! interprets a card name. Host-side conveniences get printed facts from the card store.
//! A card entering a hand and a shuffle are the only operations that remove knowledge.
//! The host keeps this state. Views sent to players must redact cards they do not know.

pub mod action;
pub mod apply;
pub mod chance;
pub mod change;
pub mod deck;
pub mod helpers;
pub mod log;
pub mod permissions;
pub mod redact;
pub mod save;
pub mod state;

mod undo;

pub use action::Action;
pub use apply::{Entry, Refusal, apply};
pub use chance::Chance;
pub use change::Change;
pub use log::Log;
pub use state::{Seat, Table};
