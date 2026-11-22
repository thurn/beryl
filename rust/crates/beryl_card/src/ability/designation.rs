//! Class level bars and the two Case keywords.

use super::{Ability, AbilityKind};
use crate::cost::Cost;
use crate::effect::{Condition, Players};
use crate::ids::TurnStep;
use crate::trigger::{Subject, Trigger, TriggerEvent};

/// A level bar, a Class ability below a bar, or a Case keyword.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Designation {
    #[default]
    None,
    LevelUp(u8),
    AtLevel(u8),
    ToSolve,
    Solved,
}

const YOUR_END_STEP: Trigger = Trigger::step(TurnStep::End, Players::You);

struct ClassLevel<const LEVEL: u8>;

impl<const LEVEL: u8> ClassLevel<LEVEL> {
    const TRIGGER: Trigger = Trigger::of(TriggerEvent::ClassLevel(LEVEL), Subject::This);
}

impl Ability {
    /// A Class level bar's activated ability (see CR 717.2a).
    ///
    /// # Panics
    ///
    /// If the destination level is less than two.
    pub const fn level(level: u8, cost: Cost) -> Self {
        assert!(level >= 2);
        Self {
            designation: Designation::LevelUp(level),
            ..Self::activated(cost).as_a_sorcery()
        }
    }

    /// An ability printed below a Class level bar, gained at that level and retained at
    /// higher levels (see CR 717.2a).
    ///
    /// # Panics
    ///
    /// If `level` is less than two or this is a spell ability or another designation ability.
    #[must_use]
    pub const fn at_class_level(self, level: u8) -> Self {
        assert!(level >= 2);
        assert!(!matches!(self.kind, AbilityKind::Spell));
        assert!(matches!(self.designation, Designation::None));
        Self {
            designation: Designation::AtLevel(level),
            ..self
        }
    }

    /// "When this Class becomes level N". The ability is gained at that level.
    ///
    /// # Panics
    ///
    /// If the destination level is less than two.
    pub const fn becomes_class_level<const LEVEL: u8>() -> Self {
        Self::triggered(&ClassLevel::<LEVEL>::TRIGGER).at_class_level(LEVEL)
    }

    /// "To solve — [condition]" (see CR 720.3a). The Case solves at your end step if
    /// it is still unsolved and the condition holds both at trigger and at resolution.
    pub const fn to_solve(condition: &'static Condition) -> Self {
        Self {
            designation: Designation::ToSolve,
            ..Self::triggered(&YOUR_END_STEP).only_if(condition)
        }
    }

    /// "Solved — [ability]" (see CR 720.3c and CR 702.169).
    ///
    /// # Panics
    ///
    /// If this is a spell ability or another designation ability.
    #[must_use]
    pub const fn solved(self) -> Self {
        assert!(!matches!(self.kind, AbilityKind::Spell));
        assert!(matches!(self.designation, Designation::None));
        Self {
            designation: Designation::Solved,
            ..self
        }
    }

    /// The Class or Case rule attached to this ability, if any.
    #[inline]
    pub const fn designation(&self) -> Designation {
        self.designation
    }
}
