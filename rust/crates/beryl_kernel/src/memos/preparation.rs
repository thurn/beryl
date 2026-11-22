//! Prepared designations and their spell copies.
//!
//! A prepared permanent remembers the designation and the slot of its exiled prepare spell
//! copy ([`Memo::Prepared`]). A copy of that permanent inherits its prepare spell face, and
//! its own entry replacements can prepare it independently (see CR 723).
//!
//! Implements: none. Stores the designation and the copy it created.

use beryl_card::{FaceIx, definition::Shape};

use super::{Bitboard, Full, GameTable, Kinds, Layout, Memo, Queue, Seat, SlotId, State, Zone};
use crate::event::Sink;
use crate::reference::Spell;
use crate::zone::Place;

impl<L: Layout> State<L> {
    /// Whether any permanent has the prepared designation (see CR 723.3a).
    #[inline]
    pub fn has_preparations(&self) -> bool {
        self.memos.kinds.0 & Kinds::PREPARED != 0
    }

    /// Whether the permanent has the prepared designation, even while phased out.
    pub fn is_prepared(&self, slot: L::Slot) -> bool {
        self.memos
            .of(Kinds::PREPARED)
            .iter()
            .any(|row| row.slot == slot && matches!(row.memo, Memo::Prepared(_)))
    }

    /// The prepared permanent whose designation created `copy`, if any.
    pub fn prepare_source(&self, copy: L::Slot) -> Option<L::Slot> {
        self.memos
            .of(Kinds::PREPARED)
            .iter()
            .find_map(|row| (row.memo == Memo::Prepared(Some(copy))).then_some(row.slot))
    }

    /// The copies in exile that the current controller of the corresponding permanents can
    /// cast (see CR 723.3c).
    pub fn prepare_spells_for(&self, player: Seat) -> Bitboard<L::Slot> {
        self.memos
            .of(Kinds::PREPARED)
            .iter()
            .filter_map(|row| {
                let Memo::Prepared(Some(copy)) = row.memo else {
                    return None;
                };
                (self.object(copy).zone() == Zone::Exile
                    && self.object(row.slot).on_battlefield()
                    && self.controller(row.slot) == player)
                    .then_some(copy)
            })
            .collect()
    }

    /// Creates the copy that accompanies a new designation or a prepared permanent phasing
    /// in. The inset keeps its printed characteristics (see [CR 723.3c]): "that copy has only
    /// the characteristics of that permanent’s prepare spell, ignoring other exceptions to
    /// the copying process that apply to that permanent."
    ///
    /// [CR 723.3c]: https://yawgatog.com/resources/magic-rules/#R7233c
    pub fn create_prepare_copy(
        &mut self,
        t: &GameTable,
        sink: &mut Sink<L>,
        source: L::Slot,
    ) -> Result<bool, Full> {
        let values = crate::Copiable::of_object(self.object(source));
        if values.face_up() != FaceIx::FRONT || values.shape(t) != Shape::Prepared {
            return Ok(false);
        }
        let owner = self.controller(source);
        let copy = L::Slot::pool()
            .find(|&slot| self.object(slot).zone() == Zone::Nowhere)
            .ok_or(Full)?;
        // The copy starts to exist in exile. It did not enter exile from another zone.
        let face = self.make_token(t, copy, owner, values.turned(t, FaceIx::BACK))?;
        self.move_object(sink, copy, Zone::Exile, Place::Top)?;
        self.object_mut(copy).face = face;
        if let Some(memo) = self.preparation_mut(source) {
            debug_assert_eq!(*memo, None);
            *memo = Some(copy);
        } else {
            self.remember(source, Memo::Prepared(Some(copy)))?;
        }
        Ok(true)
    }

    /// Sets the owner of a prepare spell that has been put on the stack. See [CR 112.2]:
    /// "the owner of the spell is the player under whose control it was put on the stack."
    ///
    /// [CR 112.2]: https://yawgatog.com/resources/magic-rules/#R1122
    pub fn set_prepare_spell_owner(&mut self, spell: Spell<L>, owner: Seat) {
        self.object_mut(spell.slot()).owner = owner;
    }

    /// Removes the designation and any copy still in exile. A copy already on the stack
    /// remains there (see CR 723.3b–c).
    pub fn unprepare(&mut self, source: L::Slot) -> Result<bool, Full> {
        if !self.is_prepared(source) {
            return Ok(false);
        }
        self.remove_prepare_copy(source)?;
        let ended = self
            .memos
            .end(|row| row.slot != source || !matches!(row.memo, Memo::Prepared(_)));
        self.settle_ended(ended);
        Ok(true)
    }

    /// Detaches the copy while keeping the prepared designation, as when a permanent
    /// phases out. A copy still in exile ceases to exist (see CR 723.3c).
    pub(crate) fn remove_prepare_copy(&mut self, source: L::Slot) -> Result<(), Full> {
        if let Some(copy) = self.preparation_mut(source).and_then(Option::take)
            && self.object(copy).zone() == Zone::Exile
        {
            self.cease_to_exist(copy)?;
        }
        Ok(())
    }

    /// Ends the old object's relation to its copy before a zone change. The one exception
    /// is a copy moving to the stack, which retains the relation until the cast completes.
    #[cold]
    #[inline(never)]
    pub(crate) fn leaving_preparations(
        &mut self,
        slot: L::Slot,
        from: Zone,
        to: Zone,
    ) -> Result<(), Full> {
        if from == Zone::Battlefield {
            self.remove_prepare_copy(slot)?;
        } else if from == Zone::Exile
            && to != Zone::Stack
            && let Some(source) = self.prepare_source(slot)
            && let Some(memo) = self.preparation_mut(source)
        {
            *memo = None;
        }
        Ok(())
    }

    fn preparation_mut(&mut self, source: L::Slot) -> Option<&mut Option<L::Slot>> {
        self.memos
            .entries
            .as_mut_slice()
            .iter_mut()
            .find_map(|row| {
                if row.slot == source
                    && let Memo::Prepared(copy) = &mut row.memo
                {
                    return Some(copy);
                }
                None
            })
    }
}
