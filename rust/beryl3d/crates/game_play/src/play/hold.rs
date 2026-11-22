//! What the show does for the pointer: the hit test on each frame, and handing a card the
//! pointer holds or let go over to a script that moves it (`crate::pointer`).

use super::Show;
use crate::pointer::{self, Pointer};
use glam::Vec2;
use kit_frame::frame::Frame;

impl Show {
    /// The pointer on the table.
    #[must_use]
    pub fn pointer(&self) -> &Pointer {
        &self.pointer
    }

    /// Updates hover against the drawn frame, including cards that move under a still mouse.
    pub fn point(&mut self, frame: &mut Frame, now: u64) {
        let mut pointer = std::mem::replace(&mut self.pointer, Pointer::EMPTY);
        pointer::update(
            &mut pointer,
            frame,
            |slot| self.pose(slot, now),
            (now, self.table.camera()),
            &self.table.data().pointer,
        );
        self.pointer = pointer;
    }

    /// Releases the card's drawn pose before the owning feature sends a command.
    pub fn release_card(&mut self, at: Option<Vec2>) -> Option<usize> {
        self.pointer
            .release(at, self.table.camera(), &self.table.data().pointer)
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
            &table.data().pointer,
        ) {
            return;
        }
        if let Some(playing) = self.playing.take() {
            self.write_record(playing.number, &playing.compiled);
            self.playing = Some(playing);
        }
    }
}
