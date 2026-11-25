// Every reader must classify new vocabulary variants, and conversions must preserve the
// represented value. Cargo makes this crate inherit the workspace lint table as a unit.
#![deny(
    clippy::wildcard_enum_match_arm,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

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
pub mod phrases;
pub mod prompt;
pub mod render;
pub mod target;
pub mod trigger;
mod walk;
pub mod words;

pub use ability::{
    Ability, AbilityKind, Behavior, Custom, DeckCondition, Designation, Says, Timing,
};
pub use characteristics::{BasicLandTypes, Colors, Keywords, Subtype, Subtypes, Supertypes, Types};
pub use check::Malformed;
pub use continuous::{
    ChosenKind, Draws, Duration, Governed, Layer, LayerSpec, MODIFY_PARTS, Modification, Named,
    PlayerRules, Protection, Replaced, Replacement, Rewrite, RuleChange, SpellCost, When,
};
pub use cost::{
    CastingKind, CastingKinds, CastingOption, Castings, Chosen, Cost, CostAction, CostPart,
    ESCAPE_EXILES, Measure,
};
pub use counter::CounterKind;
pub use decision::DecisionKind;
pub use definition::{
    CardDef, FACE_DOWN, FACE_DOWN_COPY, Face, FaceAbility, FaceIx, ObjectAbility, card,
    card_with_shape,
};
pub use effect::{
    Amount, CastFace, CastTerms, Comparison, Condition, CopyChange, CopyException, Effect,
    ExileMatch, Found, LibraryOrder, LibraryPosition, Objects, Players, Prevention, PutOnto,
    Recipients, Regeneration, Taken,
};
pub use filter::Filter;
pub use ids::{Seat, TurnPhase, TurnStep, Zone, ZoneSet};
pub use index::{AbilityIx, CastingIx, ExceptIx, GrantIx, ModeIx, SpecIx};
pub use mana::{INTRINSIC_MANA, ManaColor, ManaCost, ManaRestriction, ManaSet, mana};
pub use prompt::{Part, Place, Prompt, Purpose, Shape};
pub use target::{Abilities, Takes, TargetKind, TargetSpec};
pub use trigger::{CastNumber, Subject, Trigger, TriggerEvent};
pub use walk::Node;
pub use words::{
    ColorChanges, ColorWords, LandTypeWords, SubtypeWord, SubtypeWords, TextChange, TextWord, Words,
};

/// Everything a card file uses: the vocabulary's types and its builders,
/// `use beryl_card::prelude::*;`.
pub mod prelude {
    pub use crate::effect::build::*;
    pub use crate::phrases::{
        creature_you_control, instant_or_sorcery, tap_and_sacrifice, unless_its_controller_pays,
        when_enters, whenever_attacks,
    };
    pub use crate::{
        Ability, Amount, BasicLandTypes, CardDef, CastFace, CastNumber, CastTerms, CastingKind,
        CastingOption, Castings, ChosenKind, Colors, Comparison, Condition, CopyChange,
        CopyException, Cost, CostAction, CostPart, CounterKind, DeckCondition, Duration, Effect,
        ExceptIx, ExileMatch, Face, FaceIx, Filter, Found, Governed, GrantIx, Keywords, LayerSpec,
        LibraryOrder, LibraryPosition, ManaColor, ManaCost, ManaRestriction, ManaSet, Modification,
        Named, Objects, Place, PlayerRules, Players, Prevention, Prompt, Protection, Purpose,
        PutOnto, Recipients, Replacement, RuleChange, SpellCost, Subtype, Subtypes, Supertypes,
        Taken, TargetKind, TargetSpec, TextWord, Trigger, TriggerEvent, TurnPhase, TurnStep, Types,
        When, Zone, ZoneSet, card, card_with_shape, mana,
    };
}
