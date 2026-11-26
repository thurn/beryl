//! Building the `PlayerView` protocol ([`beryl_protocol::protocol`]) from one player's
//! [`Observation`](beryl_engine::Observation), so a view carries nothing that player does not
//! know.

mod build;
mod candidates;
mod decide;
mod event;
mod handling;
mod outcome;
mod rules_text;

pub use beryl_protocol::protocol::*;
pub use build::*;
pub(crate) use candidates::*;
pub(crate) use decide::*;
pub use event::*;
pub(crate) use handling::shown;
use outcome::outcome_view;

#[cfg(test)]
mod tests;
