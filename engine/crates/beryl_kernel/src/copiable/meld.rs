//! The copiable values of a melded permanent.
//!
//! Implements: none. Builds the values the meld instruction gives the permanent.

use beryl_card::FaceIx;

use super::Copiable;
use crate::layout::Layout;

impl<L: Layout> Copiable<L> {
    /// The copiable values of a melded permanent: its meld pair's combined back face `card`
    /// (see [CR 713.5c]): "While the two cards of a meld pair are on the battlefield as a melded
    /// permanent, the object represented by those cards has only the characteristics of the
    /// combined back face". The cards that represent it supply that face, as a merged
    /// permanent's top card supplies its own, so its mana value is theirs
    /// ([`crate::State::mana_value`]), and a copy of it copies the face alone.
    ///
    /// [CR 713.5c]: https://yawgatog.com/resources/magic-rules/#R7135c
    #[must_use]
    pub fn melded(card: L::Card) -> Copiable<L> {
        Self::of_face(card, FaceIx::FRONT).with_component_face()
    }
}
