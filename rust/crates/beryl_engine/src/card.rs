//! Card definitions: the vocabulary of `beryl_card`, re-exported for the engine's callers,
//! and the result type of an ability's Rust body (architecture §6.1, §6.2, decision D1).
//!
//! Every definition type is `'static` table data shared by all threads. `State` never holds
//! one; it stores game-table indices (§6.4) and ability indices instead.

pub use beryl_card::{
    Ability, AbilityIx, AbilityKind, Affected, Behavior, CardDef, CastingIx, CastingKind,
    CastingOption, Castings, Colors, Condition, Cost, Custom, Delayed, FACE_DOWN, FACE_DOWN_COPY,
    Face, FaceAbility, FaceIx, GrantIx, Keywords, Layer, LayerSpec, ManaColor, ManaCost, ManaSet,
    Modification, Needs, ObjectAbility, Part, Place, Prompt, Purpose, Replacement, Rule, Shape,
    SpecIx, StaticCondition, Subtype, Subtypes, Supertypes, Takes, TargetKind, TargetSpec, Timing,
    Trigger, Types, mana,
};

/// The result of a body. `Err(Pause)` means the body asked a question its answer source could
/// not answer yet (§7.3). The engine keeps the question with the choice record rather than in
/// the `Err`, so `Flow` stays one byte wide.
pub type Flow = Result<(), Pause>;

/// A body stopped to wait for an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pause;
