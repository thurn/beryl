//! Beryl's rules engine. `docs/engine-architecture.md` is the design; section numbers in
//! these docs refer to it.
//!
//! The engine is a state machine that never calls the AI: [`next`] advances a [`State`] until
//! a player must decide or the game ends, and [`apply`] consumes one [`Choice`]. The state is
//! one flat `Copy` value; the cards live in shared read-only tables ([`CardTable`],
//! [`GameTable`]). Players see the game only through an [`Observation`].
//!
//! Rules code cites the Comprehensive Rules at <https://yawgatog.com/resources/magic-rules/>.

pub mod bitboard;
pub mod card;
pub mod decision;
pub mod effect;
mod engine;
pub mod filter;
pub mod fixed;
mod invariants;
pub mod observation;
pub mod profile;
pub mod rng;
mod sba;
mod setup;
mod start;
pub mod state;
pub mod table;
mod turn;
mod zone;

pub use bitboard::Bitboard;
pub use decision::{
    Answers, Candidates, Choice, Decision, DecisionKind, Opt, Outcome, Replay, StallReason, Step,
};
pub use engine::{Counts, Scratch, apply, next};
pub use invariants::Violation;
pub use observation::{Observation, UNKNOWN};
pub use profile::{Breakpoint, Profile, Stops};
pub use state::{State, TurnStep, Zone};
pub use table::{CardTable, Decklist, GameTable, TableError};

#[cfg(test)]
mod tests;
