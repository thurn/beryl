//! Beryl's card vocabulary: the types a card definition is written in
//!.
//!
//! A card is a [`CardDef`] of [`Face`]s, each with its printed characteristics and its
//! [`Ability`]s. What an ability does is its [`Behavior`]: [`Effect`] data, which the engine's
//! interpreter runs and which tools read, or a [`Custom`] Rust function for what the
//! vocabulary cannot say yet. Everything here is `'static` data built in `const` context, so
//! a definition is a `static` with no startup cost.
//!
//! A question a card or the rules ask a player is a [`Prompt`]: a [`Purpose`] from a closed
//! list, answered in that purpose's [`Shape`] ([`prompt`]).
//!
//! This crate knows nothing of the game state. It depends on no other crate, and
//! holds the identifiers definitions share with the state ([`ids`]); the kernel crate depends
//! on this one.
//!
//! Each entry cites the Comprehensive Rules it says, at
//! <https://yawgatog.com/resources/magic-rules/>.

pub mod ability;
pub mod characteristics;
pub mod check;
pub mod continuous;
pub mod cost;
pub mod counter;
pub mod decision;
pub mod definition;
pub mod effect;
mod facts;
pub mod filter;
pub mod ids;
pub mod index;
pub mod mana;
pub mod mana_notation;
pub mod prompt;
pub mod render;
pub mod target;
pub mod trigger;
mod walk;

pub use ability::{Ability, AbilityKind, Behavior, Custom, Says, Timing};
pub use characteristics::{Colors, Keywords, Subtype, Subtypes, Supertypes, Types};
pub use check::{MODIFY_PARTS, Malformed};
pub use continuous::{
    Duration, Governed, Layer, LayerSpec, Modification, Replacement, RuleChange, When,
};
pub use cost::{
    CastingKind, CastingKinds, CastingOption, Castings, Chosen, Cost, CostAction, CostPart, Measure,
};
pub use counter::CounterKind;
pub use decision::DecisionKind;
pub use definition::{
    CardDef, FACE_DOWN, FACE_DOWN_COPY, Face, FaceAbility, FaceIx, ObjectAbility, card,
};
pub use effect::{
    Amount, CastTerms, Comparison, Condition, CopyException, Effect, Found, LibraryOrder,
    LibraryPosition, Objects, Players, Recipients, Regeneration, Taken,
};
pub use filter::Filter;
pub use ids::{Seat, TurnStep, Zone, ZoneSet};
pub use index::{AbilityIx, CastingIx, GrantIx, ModeIx, SpecIx};
pub use mana::{ManaColor, ManaCost, ManaSet, mana};
pub use prompt::{Part, Place, Prompt, Purpose, Shape};
pub use target::{Takes, TargetKind, TargetSpec};
pub use trigger::{Subject, Trigger, TriggerEvent};
pub use walk::Node;

/// Everything a card file uses: the vocabulary's types and its builders,
/// `use beryl_card::prelude::*;`.
pub mod prelude {
    pub use crate::effect::build::*;
    pub use crate::{
        Ability, Amount, CardDef, CastTerms, CastingKind, CastingOption, Castings, Colors,
        Comparison, Condition, Cost, CostAction, CostPart, CounterKind, Duration, Effect, Face,
        FaceIx, Filter, Found, Governed, Keywords, LayerSpec, LibraryOrder, LibraryPosition,
        ManaColor, ManaCost, ManaSet, Modification, Objects, Place, Players, Prompt, Purpose,
        Recipients, Replacement, RuleChange, Subtype, Subtypes, Supertypes, Taken, TargetKind,
        TargetSpec, Trigger, TriggerEvent, TurnStep, Types, When, Zone, card, mana,
    };
}
