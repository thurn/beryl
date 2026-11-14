//! The facade other crates import: Beryl's rules (`beryl_rules`) and the game state they run
//! on (`beryl_kernel`), each under the path it had when the engine was one crate.
//! `docs/engine-architecture.md` is the design.
//!
//! The engine is a state machine that never calls the AI: [`next`] advances a [`State`] until
//! a player must decide or the game ends, and [`apply`] consumes one [`Choice`]. It is generic
//! over the state's storage [`Layout`]; in [`Small`], the one search runs on, the state is one
//! flat `Copy` value. The cards live in shared read-only tables ([`CardTable`],
//! [`GameTable`]). Players see the game only through an [`Observation`].
//!
//! Nothing is defined here. A caller stays on this crate until it moves to the two behind it
//!.

// The game state and everything that writes it are the kernel's.
pub use beryl_kernel::{
    bitboard, chance, decision, filter, fixed, ids, observation, query, relayout, rng, state,
    table, two_player,
};
// The rules' modules, among them `layout` and `profile`: the kernel's layouts and restriction
// profiles, with the rules' stops beside the profiles. A card file with a Rust body takes the
// rules library, `CustomBody` and `custom_body` from `beryl_rules` itself.
pub use beryl_rules::{card, coverage, event, layout, profile};

pub use beryl_kernel::Misfit;
pub use beryl_kernel::invariants::{At, Violation};
pub use beryl_rules::{
    Counts, Scratch, apply, apply_decided, check_references, concede, cost_being_cast,
    flashback_paid, interchangeable, mana_contribution, next, next_in_step, next_within, observe,
    paused_events, payable_responses, pointless, preferred_damage, refusal, target_takes, view,
    wins_instead_of_drawing,
};
pub use bitboard::Bitboard;
pub use decision::{
    Answers, Candidates, Choice, Decision, DecisionKind, Opt, Outcome, Refusal, Replay,
    StallReason, Step,
};
pub use ids::{CardId, CardIx, LargeCardIx, LargeSlot, Place, Seat, Slot, SlotId};
pub use layout::{Large, Layout, Small};
pub use observation::Observation;
pub use profile::{
    Breakpoint, Limits, NextStop, Payment, Profile, StepStops, Stops, can_declare_blockers,
    next_step_stop, next_stop,
};
pub use relayout::Relayout;
pub use state::{State, TurnStep, Zone, ZoneKey};
pub use table::{CardTable, Decklist, GameTable, TableError};
