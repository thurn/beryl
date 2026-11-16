//! What the show does for the cards a prompt holds up in its tray (`crate::table::tray`): lifts
//! them into it, lets them go, and hands a card it let go over to the script that moves it, so
//! that the card flies from where it was held up rather than from its zone, as the web
//! interface's `Hold.keeps` (`ui/src/stage/table.ts`) has it.

use beryl_protocol::protocol::{ObjectId, PlayerView};

use super::Show;
use crate::input::Rect;
use crate::light::Lit;
use crate::pose::{Curve, Pose};
use crate::table::{CARD_WIDTH, Held, Moves};

impl Show {
    /// The latest view, which says where each object lies, and the spans the table shows, for
    /// the steps a prompt makes itself.
    pub fn latest_and_lit_mut(&mut self) -> (Option<&PlayerView>, &mut Lit) {
        let latest = (self.waiting.back())
            .map(|waiting| &*waiting.update)
            .or(self
                .playing
                .as_ref()
                .and_then(|playing| playing.update.as_deref()))
            .or(self.shown.as_deref());
        (latest.map(|update| &update.view), &mut self.lit)
    }

    /// Holds up the cards of `held`, each going to its place in `poses` over `over`, in
    /// nanoseconds since the client started (`Table::lift`).
    pub fn lift_tray(
        &mut self,
        held: Held,
        poses: Vec<(ObjectId, Pose)>,
        over: (u64, u64),
        curve: Curve,
    ) {
        self.table.lift(held, poses, over, curve);
    }

    /// Shows each card `boxes` names in its box of the flat layer, and no other
    /// (`Table::show_in`).
    pub fn show_in(&mut self, boxes: &[(ObjectId, Rect, bool)], moves: Moves, covered: bool) {
        self.table.show_in(boxes, moves, covered);
    }

    /// Where each card held up rests in its zone in the view the table shows (`Table::zone_rests`).
    #[must_use]
    pub fn zone_rests(&self) -> Vec<(ObjectId, Pose, bool)> {
        match self.shown.as_deref() {
            Some(update) => self.table.zone_rests(&update.view),
            None => Vec::new(),
        }
    }

    /// Lets the cards held up go back to `rests`, over `length` nanoseconds from the next frame
    /// (`Table::let_go`).
    pub fn let_go_tray(&mut self, rests: &[(ObjectId, Pose, bool)], length: u64, curve: Curve) {
        self.table.let_go(rests, length, curve);
    }

    /// Hands each card the tray let go over to the script that just started, if the script
    /// carries it somewhere else, more than half a card's width away: each of its first segments that starts from where it rested before the tray
    /// held it up starts from where the tray let it go instead, and one that holds it there
    /// holds it there, so that it waits where it was held up until its own step carries it on.
    /// Writes the script's record again if any card was handed over.
    pub(super) fn hand_over_tray(&mut self) {
        let Some(playing) = &mut self.playing else {
            return;
        };
        if !self.table.tray_leaving() {
            return;
        }
        let mut handed = false;
        for (slot, track) in playing.compiled.tracks.iter_mut().enumerate() {
            let (Some(from), Some(resting), Some(last)) = (
                self.table.letting_go(slot),
                track.segments().first().map(|first| first.from),
                track.segments().last().map(|last| last.to),
            ) else {
                continue;
            };
            // A card the script only turns over where it lies, or moves within its pile as it
            // reorders it, goes back with the rest.
            if last.position.distance(resting.position) <= CARD_WIDTH / 2.0 {
                continue;
            }
            handed = true;
            for segment in track.segments_mut() {
                if segment.from != resting {
                    break;
                }
                segment.from = from;
                if segment.to != resting {
                    break;
                }
                segment.to = from;
            }
            for record in
                (playing.compiled.segments.iter_mut()).filter(|record| record.slot == slot)
            {
                let segment = &mut record.segment;
                if segment.from != resting {
                    break;
                }
                segment.from = from;
                if segment.to != resting {
                    break;
                }
                segment.to = from;
            }
        }
        // Only the first script to start after the tray let its cards go takes them over.
        self.table.handed_over_tray();
        if handed && let Some(playing) = self.playing.take() {
            self.write_record(playing.number, &playing.compiled);
            self.playing = Some(playing);
        }
    }
}
