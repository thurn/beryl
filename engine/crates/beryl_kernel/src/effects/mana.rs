//! Mana-changing continuous effects in the Effects store (see [CR 605.1b]): triggered mana
//! abilities created by resolving spells or abilities that add mana when a matching permanent
//! is tapped for mana.
//!
//! [CR 605.1b]: https://yawgatog.com/resources/magic-rules/#R6051b
//!
//! Implements: none. Stores and queries resolved mana-changing effects.

use beryl_card::{AbilityIx, ColorChanges, FaceIx, LandChanges};

use crate::ids::Seat;
use crate::layout::Layout;
use crate::observation::Observation;
use crate::state::State;

use super::{ContinuousEffect, Kinds, Payload, Permitted};

/// A resolved mana-changing continuous effect (see
/// [CR 605.1b](https://yawgatog.com/resources/magic-rules/#R6051b)): names the
/// [`beryl_card::Effect::ChangeMana`] instruction by the `face`, `ability` and pre-order `part`
/// index on its source card, its `controller` as it resolved, and the text changes (`colors` and
/// `lands`) the resolving spell or ability had.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ManaEffect {
    pub face: FaceIx,
    pub ability: AbilityIx,
    pub part: u8,
    pub controller: Seat,
    pub colors: ColorChanges,
    pub lands: LandChanges,
}

impl<L: Layout> State<L> {
    /// Whether any mana-changing continuous effect is live or any permanent on the battlefield
    /// has a mana-changing ability it has not lost.
    #[inline]
    pub fn has_mana_changes(&self) -> bool {
        self.effects.kinds.0 & Kinds::MANA != 0 || self.replacing.has_mana_change()
    }

    /// The live mana-changing continuous effects, each with the card that created it, in the
    /// order they began. With none live, one AND.
    #[inline]
    pub fn mana_effects(&self) -> impl Iterator<Item = (L::Card, ManaEffect)> + '_ {
        self.effects.of(Kinds::MANA).iter().filter_map(|e| {
            if let Payload::Mana(effect) = e.payload {
                Some((e.source, effect))
            } else {
                None
            }
        })
    }
}

impl<'a, L: Layout> Observation<'a, L> {
    /// The continuous effects, in the order they began. Every one is public: a spell or
    /// ability resolves in the open, and what it affects is an object every player sees or
    /// a slot, which is a public handle.
    pub fn effects(&self) -> &'a [ContinuousEffect<L>] {
        self.shown().effects()
    }

    /// Who may cast which card from a zone it could not otherwise be cast from, and on what
    /// terms: public, as the effects that grant them are. A card exiled until someone casts
    /// it is face up (see [CR 406.3](https://yawgatog.com/resources/magic-rules/#R4063): "Exiled
    /// cards are, by default, kept face up and may be examined by any player at any time").
    pub fn cast_permissions(&self) -> impl Iterator<Item = Permitted<L>> + 'a {
        self.shown().cast_permissions()
    }
}
