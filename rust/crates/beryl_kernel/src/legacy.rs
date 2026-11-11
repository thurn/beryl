//! The legacy accessor: raw writes of what milestone 4 (by-w8gv.7.1) deletes, which
//! therefore gets no typed mutator (the crate's contract, "The legacy accessor"). A search
//! for `legacy_mut` finds every caller milestone 4 has to move.

use crate::layout::Layout;
use crate::state::State;

/// Lent by [`State::legacy_mut`]. Nothing is left behind it: a store replaced the last of
/// what it wrote, the counters no object keeps in a field ([`crate::counters`]), and
/// by-w8gv.7.10 deletes the accessor.
pub struct Legacy<'a, L: Layout>(
    #[expect(
        dead_code,
        reason = "nothing is left to write; by-w8gv.7.10 deletes the accessor"
    )]
    &'a mut State<L>,
);

impl<L: Layout> State<L> {
    /// Lends the raw writes of what milestone 4 (by-w8gv.7.1) moves to memos and stores:
    /// [`Legacy`] lists them.
    #[inline]
    pub fn legacy_mut(&mut self) -> Legacy<'_, L> {
        Legacy(self)
    }
}
