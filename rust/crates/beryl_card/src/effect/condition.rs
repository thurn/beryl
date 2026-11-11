//! Conditions: whether an instruction is carried out, or an ability triggers.

use crate::ability::Custom;
use crate::cost::CastingKind;
use crate::effect::{Amount, Objects};
use crate::filter::Filter;

/// Something true or false of the game: the "if" of an [`crate::Effect::If`], or the
/// intervening "if" of a triggered ability
/// ([CR 603.4](https://yawgatog.com/resources/magic-rules/#R6034)): "When the trigger event
/// occurs, the ability checks whether the stated condition is true. The ability triggers only
/// if it is; otherwise it does nothing. If the ability triggers, it checks the stated
/// condition again as it resolves."
#[derive(Clone, Copy, Debug)]
pub enum Condition {
    /// "If you do", "if they do", "if a card was discarded this way": the instruction before
    /// did what it says (the [module](crate::effect) says which instruction that is).
    Did,
    /// "If it was kicked", "if its evoke cost was paid": the spell was cast with this kind of
    /// alternative or additional cost, or the permanent's spell was
    /// ([CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d)): "If a spell's
    /// controller declares the intention to pay any of that spell's kicker costs, that spell
    /// has been 'kicked.'"
    Paid(CastingKind),
    /// One amount against another: "if you control three or more artifacts", "if it has a
    /// time counter on it".
    Compare(Amount, Comparison, Amount),
    /// "If it's a [filter]": there is such an object and every one passes the filter, read
    /// for the ability's controller.
    Is(Objects, Filter),
    Not(&'static Condition),
    /// Every one of them: "if ... and ...".
    All(&'static [Condition]),
    /// At least one of them: "if ... or ...".
    Any(&'static [Condition]),
    /// The escape hatch: a Rust function only the engine can run and nothing can read.
    Custom(Custom),
}

/// How one [`Amount`] is held against another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Comparison {
    /// "N or more", "at least", "greater than or equal to".
    AtLeast,
    /// "N or fewer", "N or less", "at most".
    AtMost,
    /// "Exactly", "equal to".
    Equal,
    /// "More than", "greater than".
    MoreThan,
    /// "Fewer than", "less than".
    LessThan,
}
