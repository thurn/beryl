//! Building the `PlayerView` protocol ([`beryl_protocol::protocol`]) from one player's
//! [`Observation`](beryl_engine::Observation), so a view carries nothing that player does not
//! know.

mod build;
mod candidates;
mod decide;
mod event;
mod outcome;
mod rules_text;

pub use beryl_protocol::protocol::*;
pub use build::*;
pub(crate) use candidates::*;
pub(crate) use decide::*;
pub use event::*;
use outcome::*;

#[cfg(test)]
mod tests;
