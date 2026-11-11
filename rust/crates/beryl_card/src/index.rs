//! Indices: which entry of one of a definition's lists is meant.
//!
//! Each list has an index type of its own, so the index of a target cannot be passed where
//! the index of an ability is expected, though both are a byte. A card file writes the
//! number and a builder wraps it ([`crate::effect::build::target`],
//! [`crate::Modification::grant`], [`crate::CastingOption::overload`],
//! [`crate::Castings::only`]). [`crate::FaceIx`], which face of a card, is the sixth.
//!
//! ```compile_fail
//! use beryl_card::{AbilityIx, SpecIx};
//!
//! // The first target is not the first ability.
//! let ability: AbilityIx = SpecIx::new(0);
//! ```
//!
//! The place of a chosen target in a stack entry's list of them is not one of these: no
//! definition names it, and the kernel defines it.

/// Defines one index type: a byte that only its own list reads.
macro_rules! index {
    ($(#[$attribute:meta])* $name:ident) => {
        $(#[$attribute])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        pub struct $name(u8);

        impl $name {
            /// The index `index`, counting from 0.
            #[inline]
            pub const fn new(index: u8) -> $name {
                $name(index)
            }

            /// The number, as wire formats write it.
            #[inline]
            pub const fn get(self) -> u8 {
                self.0
            }

            /// The position in the list.
            #[inline]
            pub const fn index(self) -> usize {
                self.0 as usize
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

index! {
    /// Which instance of the word "target" in a spell or ability: an index into
    /// [`crate::Ability::targets`]
    /// ([CR 115.1](https://yawgatog.com/resources/magic-rules/#R1151)): "The targets are
    /// object(s) and/or player(s) the spell or ability will affect." One instance may choose
    /// several targets ([`crate::TargetSpec::count`]), and every one of them carries its index.
    SpecIx
}

impl SpecIx {
    /// The instance of the word "target" after this one, as targets are announced in order
    /// ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): "The player
    /// announces their choice of an appropriate object or player for each target the spell
    /// requires."
    #[inline]
    #[must_use]
    pub const fn next(self) -> SpecIx {
        SpecIx(self.0 + 1)
    }
}

index! {
    /// Which of a face's own abilities: an index into [`crate::Face::abilities`]
    /// ([CR 113.1a](https://yawgatog.com/resources/magic-rules/#R1131a)): "An object's
    /// abilities are defined by its rules text or by the effect that created it."
    AbilityIx
}

index! {
    /// Which of the abilities a face grants to other objects: an index into
    /// [`crate::Face::grants`], as [`crate::Modification::Grant`] names one
    /// ([CR 613.1f](https://yawgatog.com/resources/magic-rules/#R6131f)): "Layer 6:
    /// Ability-adding effects, keyword counters, ability-removing effects, and effects that
    /// say an object can't have an ability are applied."
    GrantIx
}

index! {
    /// Which of a face's casting options: an index into [`crate::Face::casting`], and the
    /// bit of a [`crate::Castings`] set
    /// ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b)): "If the spell has
    /// alternative or additional costs that will be paid as it's being cast such as buyback
    /// or kicker costs (see rules 118.8 and 118.9), the player announces their intentions to
    /// pay any or all of those costs".
    CastingIx
}

index! {
    /// Which mode of a modal spell or ability
    /// ([CR 700.2](https://yawgatog.com/resources/magic-rules/#R7002)): "A spell or ability
    /// is modal if it has two or more options in a bulleted list preceded by instructions for
    /// a player to choose a number of those options, such as 'Choose one --.' Each of those
    /// options is a mode." No card of the pool is modal yet, so nothing carries one.
    ModeIx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An index is the byte it wraps, so the records of the state that hold one keep their
    /// sizes and their hashes.
    #[test]
    fn an_index_is_a_byte() {
        assert_eq!(size_of::<SpecIx>(), 1);
        assert_eq!(size_of::<Option<(AbilityIx, GrantIx)>>(), 3);
        assert_eq!(CastingIx::new(3).get(), 3);
        assert_eq!(ModeIx::new(3).index(), 3);
        assert_eq!(SpecIx::new(1).next(), SpecIx::new(2));
    }
}
