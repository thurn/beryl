//! What the show does for the pointer: the hit test on each frame, and handing a card the
//! pointer holds or let go over to a script that moves it (`crate::pointer`).

use super::Show;
use crate::pointer::{self, LetGo, Pointer};
use kit_frame::frame::Frame;

impl Show {
    /// The pointer on the table.
    #[must_use]
    pub fn pointer(&self) -> &Pointer {
        &self.pointer
    }

    /// Finds and marks what the pointer is over in `frame`, just built at `now`, and returns what
    /// a press on a card that ended since the last frame meant (`pointer::update`).
    pub fn point(&mut self, frame: &mut Frame, now: u64) -> Option<LetGo> {
        let mut pointer = std::mem::replace(&mut self.pointer, Pointer::EMPTY);
        let camera = self.table.camera();
        let rest = |slot| self.pose(slot, now);
        let in_tray = |slot| self.table.in_tray(slot);
        let clicked = pointer::update(&mut pointer, frame, (rest, in_tray), (now, camera));
        self.pointer = pointer;
        clicked
    }

    /// Hands the card the pointer holds or let go over to the script that just started, at
    /// `now`, if the script moves it (`pointer::hand_over`), and writes the script's record again
    /// with the segment that now starts where the pointer had the card.
    pub(super) fn hand_over(&mut self, now: u64) {
        let Some(playing) = &mut self.playing else {
            return;
        };
        let table = &self.table;
        let rest = |slot| table.rest(slot);
        let camera = table.camera();
        if !pointer::hand_over(
            &mut self.pointer,
            &mut playing.compiled,
            rest,
            (now, camera),
        ) {
            return;
        }
        if let Some(playing) = self.playing.take() {
            self.write_record(playing.number, &playing.compiled);
            self.playing = Some(playing);
        }
    }
}
