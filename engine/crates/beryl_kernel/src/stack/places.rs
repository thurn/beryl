//! The places of a stack entry's targets in the order they were chosen.
//!
//! Implements: none. Numbers the targets a stack entry holds.

use crate::layout::Layout;

use super::{TARGETS, Target};

/// The place of a chosen target in a stack entry's list of them
/// ([`StackEntry::targets`](crate::state::StackEntry::targets)), counting from 0 in the order they were chosen
/// (see [CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): "The player announces
/// their choice of an appropriate object or player for each target the spell requires."
///
/// It is not the target's [`SpecIx`](beryl_card::SpecIx): one instance of the word "target" may choose several
/// targets or none, so the two count differently, and neither is passed for the other.
///
/// ```compile_fail
/// use beryl_card::SpecIx;
/// use beryl_kernel::state::{ObjectRef, Target, TargetPlace};
///
/// // The first place is not the first instance of the word "target".
/// let target: Target = Target::object(TargetPlace::FIRST, ObjectRef::default());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TargetPlace(u8);

impl TargetPlace {
    /// The place of the first target chosen.
    pub const FIRST: TargetPlace = TargetPlace(0);

    /// The place `place`, counting from 0.
    #[inline]
    pub const fn new(place: u8) -> TargetPlace {
        assert!(
            (place as usize) < TARGETS,
            "a stack entry has TARGETS places"
        );
        TargetPlace(place)
    }

    /// The position in the list.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Each of `targets`, a stack entry's, with its place, in the order they were chosen
/// (see CR 601.2c).
#[inline]
pub fn placed<L: Layout>(
    targets: &[Target<L>],
) -> impl Iterator<Item = (TargetPlace, Target<L>)> + '_ {
    debug_assert!(targets.len() <= TARGETS);
    (targets.iter().enumerate()).map(|(i, &target)| {
        (
            TargetPlace(u8::try_from(i).expect("a target place fits in u8")),
            target,
        )
    })
}

/// A set of the places of one stack entry's targets: a byte, a bit for each place. It has no
/// order of its own; [`placed`] gives the targets in the order they were chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TargetPlaces(u8);

const _: () = assert!(TARGETS <= 8, "a bit for each place");

impl TargetPlaces {
    pub const NONE: TargetPlaces = TargetPlaces(0);

    #[inline]
    pub fn insert(&mut self, place: TargetPlace) {
        self.0 |= 1 << place.0;
    }

    #[inline]
    pub fn contains(self, place: TargetPlace) -> bool {
        self.0 & (1 << place.0) != 0
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}
