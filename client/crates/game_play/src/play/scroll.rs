//! A battlefield row, the viewer's hand or the stack, scrolling under the pointer as the show
//! plays scripts over it (docs/beryl-3d.md, "Objects and placement"). A scroll changes where
//! the line's cards rest, so
//! the show works out every resting place again and compiles every script not yet ended again,
//! as it does when the window changes shape. A script that moves a card out of view then moves
//! it from the nearer end of the row, where placement rests it.

use super::Show;
use game_table::table::Scroller;

impl Show {
    /// Scrolls `row` so that the first group in view is `first`, or as near to it as the row's
    /// ends allow. Returns whether the row scrolled; a row that fits never does.
    pub fn scroll_row(&mut self, row: Scroller, first: isize) -> bool {
        if !self.table.scroll_row(row, first) {
            return false;
        }
        self.resize(self.table.camera().window);
        true
    }

    /// Takes the time of the frame about to be drawn, at which a slide that waits for it
    /// begins. The client calls it before `frame`.
    pub fn advance_slide(&mut self, now: u64) {
        self.table.advance_slide(now);
    }
}
