//! Values chosen as permanents enter (see CR 614.12a).
//!
//! Implements: none. Reads the values from the memo store.

use super::{Kinds, Layout, Memo, State};
use crate::table::GameTable;

impl<L: Layout> State<L> {
    /// The permanents on the battlefield that chose a card name this game can match, with
    /// their chosen names. A phased-out permanent still remembers its choice, but it is
    /// "treated as though it's not on the battlefield" (see [CR 702.26d]).
    ///
    /// [CR 702.26d]: https://yawgatog.com/resources/magic-rules/#R70226d
    #[inline]
    pub fn chosen_names(&self, t: &GameTable) -> impl Iterator<Item = (L::Slot, &'static str)> {
        self.memos
            .of(Kinds::CHOSEN)
            .iter()
            .filter_map(move |entry| {
                let Memo::Chosen(chosen) = entry.memo else {
                    return None;
                };
                let name = chosen.card_name(t).filter(|name| !name.is_empty())?;
                self.battlefield_position(entry.slot)
                    .map(|_| (entry.slot, name))
            })
    }
}
