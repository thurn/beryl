//! Card definitions: the vocabulary of `beryl_card`, re-exported for the engine's callers,
//! and the result type of an ability's Rust body (architecture §6.1, §6.2, decision D1).
//!
//! Every definition type is `'static` table data shared by all threads. `State` never holds
//! one; it stores game-table indices (§6.4) and ability indices instead.
//!
//! Implements: none. Re-exports card vocabulary and defines resolution results.

use crate::fixed::Full;

pub use beryl_card::definition::Shape as CardShape;
pub use beryl_card::{
    Abilities, Ability, AbilityIx, AbilityKind, Amount, BasicLandTypes, Behavior, CardDef,
    CastingIx, CastingKind, CastingKinds, CastingOption, Castings, Chosen, ChosenKind, Colors,
    Condition, Cost, CostAction, CostPart, Custom, Effect, ExceptIx, FACE_DOWN, FACE_DOWN_COPY,
    Face, FaceAbility, FaceIx, GrantIx, INTRINSIC_MANA, Keywords, Layer, LayerSpec, ManaColor,
    ManaCost, ManaSet, Measure, Modification, ObjectAbility, Objects, Part, Place, Players, Prompt,
    Purpose, Recipients, Replaced, Replacement, Rewrite, RuleChange, Says, Shape, SpecIx, Subject,
    Subtype, SubtypeWords, Subtypes, Supertypes, Taken, Takes, TargetKind, TargetSpec, Timing,
    Trigger, TriggerEvent, Types, Words, counter_kinds, mana,
};

/// The result of a body, and of any step of a resolution that can stop it early. The engine
/// keeps a question with the choice record rather than in the `Err`, so `Flow` stays one byte
/// wide.
pub type Flow = Result<(), Stop>;

/// Why a resolution stopped before its end (the crate's contract, "How a resolution stops").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// It asked a question its answer source could not answer yet (§7.3): it is run again
    /// from the top once the answer is on the record.
    Pause,
    /// A fixed capacity ran out (§12.3): the game stalls, and goes on in the layout whose
    /// lists grow.
    Full,
}

impl From<Full> for Stop {
    #[inline]
    fn from(Full: Full) -> Stop {
        Stop::Full
    }
}
