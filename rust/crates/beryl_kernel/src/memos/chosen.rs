//! Values chosen as permanents enter (see CR 614.12a).
//!
//! Implements: none. Reads the values from the memo store.

use super::{ChosenValue, Kinds, Layout, Memo, State};

impl<L: Layout> State<L> {
    /// The permanents on the battlefield that chose a card name this game can match, with
    /// the chosen cards. A phased-out permanent still remembers its choice, but it is
    /// "treated as though it's not on the battlefield" (see [CR 702.26d]).
    ///
    /// [CR 702.26d]: https://yawgatog.com/resources/magic-rules/#R70226d
    #[inline]
    pub fn chosen_names(&self) -> impl Iterator<Item = (L::Slot, L::Card)> + '_ {
        self.memos
            .of(Kinds::CHOSEN)
            .iter()
            .filter_map(move |entry| {
                let Memo::Chosen(ChosenValue::Name(Some(card))) = entry.memo else {
                    return None;
                };
                self.battlefield_position(entry.slot)
                    .map(|_| (entry.slot, card))
            })
    }
}
