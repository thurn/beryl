//! The `PlayerView` protocol (architecture §19.2, decision D14): what the UI draws, built from
//! one player's [`Observation`](beryl_engine::Observation) so it carries nothing that player
//! does not know. Every type here has a generated TypeScript twin in
//! `ui/src/generated/protocol.ts` ([`crate::typescript`]).

mod ask;
mod build;
mod candidates;
mod decide;
mod decision;
mod event;
mod names;
mod outcome;
mod shown;
mod stops;
mod view;

pub use build::*;
pub(crate) use candidates::*;
pub(crate) use decide::*;
pub use decision::*;
pub use event::*;
pub use names::*;
pub use outcome::*;
pub use stops::*;
pub use view::*;

#[cfg(test)]
mod tests;
