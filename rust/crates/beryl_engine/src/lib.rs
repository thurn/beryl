//! Beryl's rules engine. `docs/engine-architecture.md` is the design; section numbers in
//! these docs refer to it.
//!
//! The engine is a state machine that never calls the AI: [`next`] advances a [`State`] until
//! a player must decide or the game ends, and [`apply`] consumes one [`Choice`]. The engine is
//! generic over the state's storage [`Layout`]; in [`Small`], the one search runs on, the
//! state is one flat `Copy` value. The cards live in shared read-only tables ([`CardTable`],
//! [`GameTable`]). Players see the game only through an [`Observation`].
//!
//! Rules code cites the Comprehensive Rules at <https://yawgatog.com/resources/magic-rules/>.

pub mod bitboard;
pub mod card;
mod cast;
pub mod chance;
mod combat;
pub mod coverage;
pub mod decision;
pub mod effect;
mod engine;
pub mod event;
mod face_down;
pub mod filter;
pub mod fixed;
pub mod ids;
mod invariants;
mod layers;
pub mod layout;
mod mana;
pub mod observation;
pub mod profile;
mod reference;
pub mod relayout;
mod resolve;
pub mod rng;
mod sba;
mod setup;
mod start;
pub mod state;
pub mod table;
mod trigger;
mod turn;
mod zone;

pub use bitboard::Bitboard;
pub use cast::{cost_being_cast, pointless, target_removes_permanent};
pub use decision::{
    Answers, Ask, Candidates, Choice, Decision, DecisionKind, Opt, Outcome, Refusal, Replay,
    StallReason, Step,
};
pub use engine::{
    Counts, Scratch, apply, apply_decided, concede, interchangeable, next, next_in_step, observe,
    paused_events, refusal, view,
};
pub use ids::{CardIx, LargeSlot, Seat, Slot, SlotId};
pub use invariants::{At, Violation};
pub use layout::{Large, Layout, Small};
pub use mana::mana_contribution;
pub use observation::Observation;
pub use profile::{
    Breakpoint, Limits, NextStop, Payment, Profile, StepStops, Stops, can_declare_blockers,
    next_step_stop, next_stop,
};
pub use relayout::Relayout;
pub use state::{State, TurnStep, Zone};
pub use table::{CardTable, Decklist, GameTable, TableError};

#[cfg(test)]
mod tests;
