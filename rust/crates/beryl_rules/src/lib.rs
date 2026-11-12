//! Beryl's rules: turns, priority, casting, mana, combat, layers, triggers and the interpreter
//! of effect data, over the kernel's state (`beryl_kernel`) and the card vocabulary
//! (`beryl_card`). `docs/engine-architecture.md` is the design; section numbers in these docs
//! refer to it.
//!
//! The rules are a state machine that never calls the AI: [`next`] advances a
//! [`State`](state::State) until a player must decide or the game ends, and [`apply`] consumes
//! one [`Choice`](decision::Choice). They are generic over the state's storage
//! [`Layout`](layout::Layout); in [`Small`](layout::Small), the one search runs on, the state
//! is one flat `Copy` value. The cards live in shared read-only tables
//! ([`CardTable`](table::CardTable), [`GameTable`](table::GameTable)). Players see the game
//! only through an [`Observation`](observation::Observation).
//!
//! Other crates import the rules through `beryl_engine`, the facade that re-exports this
//! crate and the kernel under the paths they had as one crate, until the last caller has
//! moved (`docs/project-50k.md`, Crates).
//!
//! Rules code cites the Comprehensive Rules at <https://yawgatog.com/resources/magic-rules/>.

mod announced;
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
mod permission;
mod reference;
mod resolve;
mod rule_change;
mod sba;
mod scratch;
mod start;
mod stops;
mod trigger;
mod turn;

// The game state and everything that writes it are the kernel's (`beryl_kernel`, plan K6).
// The rules name its modules as their own and export none of them whole: a caller reads the
// kernel from the kernel, or from the facade. What the rules do export of it stands beside
// items of their own, in `layout`, `profile` and `event`.
pub(crate) use beryl_kernel::{
    PlayerSet, bitboard, chance, decision, filter, fixed, ids, invariants, observation, query,
    state, table, zone,
};

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

pub use announced::target_takes;
pub use cast::{cost_being_cast, pointless};
pub use combat::preferred_damage;
pub use engine::{
    Counts, Scratch, apply, apply_decided, concede, interchangeable, next, next_in_step, observe,
    paused_events, refusal, view,
};
pub use mana::mana_contribution;
pub use permission::flashback_paid;
pub use reference::check_references;

// The tests set games up as a caller of the facade does, with the kernel's names at the root.
#[cfg(test)]
pub(crate) use beryl_kernel::{
    bitboard::Bitboard,
    decision::{
        Answers, Candidates, Choice, Decision, DecisionKind, Opt, Outcome, Replay, StallReason,
        Step,
    },
    ids::{Seat, Slot},
    layout::{Large, Layout},
    observation::Observation,
    profile::{Payment, Profile},
    rng,
    state::{State, TurnStep},
    table::{CardTable, Decklist, GameTable},
};

#[cfg(test)]
mod tests;
