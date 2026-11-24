//! What the show does for the pointer: the hit test on each frame, and handing a card the
//! pointer holds or let go over to a script that moves it (`crate::pointer`).

use super::Show;
use crate::pointer::{self, Pointer};
use beryl_protocol::protocol::{Side, ZoneName};
use game_table::table::Place;
use glam::Vec2;
use kit_frame::frame::Frame;

impl Show {
    /// The pointer on the table.
    #[must_use]
    pub fn pointer(&self) -> &Pointer {
        &self.pointer
    }

    /// Carries a card under the pointer at `now`. A card of the hand, or of a prompt's tray,
    /// keeps the size it had there when picked up, facing the camera, as a card dragged in the
    /// hand or in Arena's "Order Cards" screen does.
    pub fn carry_card(&mut self, slot: usize, at: Vec2, now: u64) {
        if self.pointer.held() != Some(slot)
            && self.table.in_tray(slot)
            && let Some(drawn) = self.drawn_pose(slot, now)
        {
            let camera = self.table.camera();
            let points = camera.data.card.height * camera.points_per_millimetre(drawn.position);
            let share = points * drawn.scale / camera.window.size.height;
            self.pointer.carry(slot, at, Some(share));
            return;
        }
        let table = &self.table;
        let hand = self.pointer.held() != Some(slot)
            && !table.in_tray(slot)
            && matches!(
                table.objects().card_place(slot),
                Some(Place::Card {
                    zone: ZoneName::Hand,
                    side: Side::You,
                    ..
                })
            );
        let hand_height = table.rest(slot).filter(|_| hand).map(|rest| {
            let camera = table.camera();
            let size = camera.window.size;
            let height =
                camera.data.card.height * rest.scale * camera.points_per_millimetre(rest.position);
            // On a narrow desktop window the web stage leaves room above and below it.
            // Its dragged card inherits that fit. The phone uses logical points directly.
            let fit = if table.profile().data().table.anchored.is_some() {
                let min_aspect = table.data().pointer.held_hand_stage_min_aspect;
                (size.width / (size.height * min_aspect)).min(1.0)
            } else {
                1.0
            };
            height / size.height * fit
        });
        self.pointer.carry(slot, at, hand_height);
    }

    /// Updates hover against the drawn frame, including cards that move under a still mouse.
    pub fn point(&mut self, frame: &mut Frame, now: u64) {
        let mut pointer = std::mem::replace(&mut self.pointer, Pointer::EMPTY);
        pointer::update(
            &mut pointer,
            frame,
            // An opening card rests in its screen box while the table's hand is below
            // the window. Pick the pose before the hover lift, with that box applied.
            (
                |slot| self.drawn_pose(slot, now),
                |slot| self.table.in_tray(slot),
            ),
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
