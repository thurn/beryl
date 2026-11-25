//! Merged permanents (see [CR 731.2]): "That permanent becomes a merged permanent represented by
//! the card or copy that represented that object in addition to any other components that were
//! representing it."
//!
//! A merged permanent stays the object it was, in its slot: its incarnation, counters,
//! attachments, timestamp and status do not change, and nothing enters (see [CR 731.2c]): "Because
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
//! A melded permanent is kept the same way (see CR 713.6): one of its two cards is the
//! permanent, with a copy effect giving it the combined back face ([`Copiable::melded`]), and
//! the other waits in [`Zone::Merged`] ([`State::meld_under`]).
//!
//! Implements: none. The mutator stores a resolved merge and its component links.
//!
//! [CR 731.2]: https://yawgatog.com/resources/magic-rules/#R7312
//! [CR 731.2c]: https://yawgatog.com/resources/magic-rules/#R7312c

use beryl_card::FaceIx;

use crate::event::Sink;
use crate::fixed::Full;
use crate::layout::{Layout, Table};
use crate::memos::Memo;
use crate::query::Copiable;
use crate::reference::{Permanent, Spell};
use crate::state::{State, Zone};
use crate::zone::Place;

impl<L: Layout> State<L> {
    /// Merges the card `card`, a mutating creature spell with its face `face` up, with the
    /// permanent `onto`, on top of it (`top`) or under it (see [CR 702.140c]): "As a mutating
    /// creature spell resolves, if its target is legal, it doesn't enter the battlefield.
    /// Rather, it merges with the target creature and becomes one object represented by more
    /// than one card or token". The card leaves the stack for [`Zone::Merged`], and a copy
    /// effect of the card gives the permanent its new values ([`State::become_copy`]). Over,
    /// the permanent's definition becomes the card's, and its slot remembers the card it
    /// really is. [`Full`] if the stores or the values had no room.
    ///
    /// [CR 702.140c]: https://yawgatog.com/resources/magic-rules/#R702140c
    /// [CR 731.2e]: https://yawgatog.com/resources/magic-rules/#R7312e
    pub fn merge(
        &mut self,
        sink: &mut Sink<L>,
        card: Spell<L>,
        face: FaceIx,
        onto: Permanent<L>,
        top: bool,
    ) -> Result<(), Full> {
        let definition = self.object(card.slot()).card;
        let values: Copiable<L> = self
            .copiable(onto.slot())
            .merged_with(definition, face, top)?;
        let values = if top || !self.has_copied_face(onto.slot()) {
            values.with_component_face()
        } else {
            values
        };
        self.move_object(sink, card.slot(), Zone::Merged, Place::Top)?;
        self.remember(card.slot(), Memo::PartOf(onto.slot()))?;
        // Over a face-down permanent, the merged permanent is face up and every player sees
        // it, but its face-down card stays hidden from those who didn't know it (see [CR 731.2e]):
        // "If a merged permanent contains face-up and face-down components, the permanent's
        // status is determined by its topmost component."
        let hidden = top && self.object(onto.slot()).face == FaceIx::DOWN;
        if hidden {
            let knew = self.who_knows(onto.slot());
            self.remember(onto.slot(), Memo::FaceDownUnder(knew))?;
        }
        self.set_copy(definition, onto.into(), values)?;
        if hidden {
            for k in self.knowledge.as_mut_slice() {
                k.insert(onto.slot());
            }
        }
        Ok(())
    }

    /// Makes the card in `card`, in exile, the other card of the melded permanent `onto`,
    /// which its partner became as it entered with the pair's combined back face up
    /// (see [CR 713.6]): "If a meld card is being melded with its counterpart, those cards
    /// enter the battlefield as a single permanent with their back faces up." The card leaves
    /// exile for [`Zone::Merged`] and names the permanent ([`Memo::PartOf`]), so it leaves the
    /// battlefield with it ([`State::move_object`]). [`Full`] if the store had no room.
    ///
    /// [CR 713.6]: https://yawgatog.com/resources/magic-rules/#R7136
    pub fn meld_under(
        &mut self,
        sink: &mut Sink<L>,
        onto: Permanent<L>,
        card: L::Slot,
    ) -> Result<(), Full> {
        debug_assert_eq!(self.object(card).zone, Zone::Exile);
        self.move_object(sink, card, Zone::Merged, Place::Top)?;
        self.remember(card, Memo::PartOf(onto.slot()))
    }
}
