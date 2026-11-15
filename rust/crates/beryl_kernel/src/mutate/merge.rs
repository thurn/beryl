//! Merged permanents ([CR 731.2]): "That permanent becomes a merged permanent represented by
//! the card or copy that represented that object in addition to any other components that were
//! representing it."
//!
//! A merged permanent stays the object it was, in its slot: its incarnation, counters,
//! attachments, timestamp and status do not change, and nothing enters ([CR 731.2c]): "Because
//! a merged permanent is the same object that it was before, it hasn't just come under a
//! player's control, any continuous effects that affected it continue to do so, and so on."
//! Each card that merges
//! with it waits in [`Zone::Merged`] with a memo naming the permanent ([`Memo::PartOf`]), in no
//! battlefield list or bitboard, so nothing that walks the battlefield sees it. The
//! permanent's copiable values are its top card's face with an addition for every ability of
//! each other card ([`Copiable::merged_with`]), kept by a copy effect on it, so the layers, the
//! listeners and a copy of it read them as they read any copy's. When the permanent leaves the
//! battlefield, its cards go with it ([`State::move_object`]).
//!
//! [CR 731.2]: https://yawgatog.com/resources/magic-rules/#R7312
//! [CR 731.2c]: https://yawgatog.com/resources/magic-rules/#R7312c

use beryl_card::FaceIx;

use crate::event::Sink;
use crate::fixed::Full;
use crate::layout::Layout;
use crate::memos::Memo;
use crate::object::ObjectRef;
use crate::query::Copiable;
use crate::state::{State, Zone};
use crate::zone::Place;

impl<L: Layout> State<L> {
    /// Merges the card `card`, a mutating creature spell with its face `face` up, with the
    /// permanent `onto`, on top of it (`top`) or under it ([CR 702.140c]): "As a mutating
    /// creature spell resolves, if its target is legal, it doesn't enter the battlefield.
    /// Rather, it merges with the target creature and becomes one object represented by more
    /// than one card or token". The card leaves the stack for [`Zone::Merged`], and a copy
    /// effect of the card gives the permanent its new values ([`State::become_copy`]). Over,
    /// the permanent's definition becomes the card's, and its slot remembers the card it
    /// really is. [`Full`] if the stores or the values had no room.
    ///
    /// [CR 702.140c]: https://yawgatog.com/resources/magic-rules/#R702140c
    pub fn merge(
        &mut self,
        sink: &mut Sink<L>,
        card: ObjectRef<L>,
        face: FaceIx,
        onto: ObjectRef<L>,
        top: bool,
    ) -> Result<(), Full> {
        debug_assert!(self.is_current(card) && self.is_current(onto));
        debug_assert_eq!(self.object(onto.slot).zone, Zone::Battlefield);
        let definition = self.object(card.slot).card;
        let values: Copiable<L> = self
            .copiable(onto.slot)
            .merged_with(definition, face, top)?;
        self.move_object(sink, card.slot, Zone::Merged, Place::Top)?;
        self.remember(card.slot, Memo::PartOf(onto.slot))?;
        self.become_copy(definition, onto, values)
    }
}
