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

mod bodies;
pub mod card;
mod cast;
mod combat;
pub mod coverage;
pub mod effect;
mod engine;
pub mod event;
mod face_down;
mod interpret;
mod layers;
mod mana;
mod reference;
mod resolve;
mod sba;
mod scratch;
mod start;
mod stops;
mod trigger;
mod turn;

// The game state and everything that writes it are the kernel's (`beryl_kernel`, plan K6).
// The engine stays the facade other crates import until the last caller has moved, so each
// moved module keeps its old path here.
pub use beryl_kernel::{
    bitboard, chance, decision, filter, fixed, ids, observation, query, relayout, rng, state, table,
};
pub(crate) use beryl_kernel::{invariants, zone};

/// The kernel's storage layouts, and the rules' per-layout function tables beside them.
pub mod layout {
    pub use crate::bodies::{Bodies, Body, Condition, Conditions};
    pub use beryl_kernel::layout::*;
}

/// The kernel's restriction profiles, and where they stop a player in a game.
pub mod profile {
    pub use crate::stops::{NextStop, can_declare_blockers, next_step_stop, next_stop};
    pub(crate) use crate::stops::{asks_lone, stops_for};
    pub use beryl_kernel::profile::*;
}

pub use bitboard::Bitboard;
pub use cast::{cost_being_cast, pointless, target_removes_permanent};
pub use combat::preferred_damage;
pub use decision::{
    Answers, Candidates, Choice, Decision, DecisionKind, Opt, Outcome, Refusal, Replay,
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
pub use reference::check_references;
pub use relayout::Relayout;
pub use state::{State, TurnStep, Zone, ZoneKey};
pub use table::{CardTable, Decklist, GameTable, TableError};

#[cfg(test)]
mod tests;
