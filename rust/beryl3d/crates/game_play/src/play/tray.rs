//! What the show does for the cards a prompt holds up in its tray (`game_table::table::tray`): lifts
//! them into it, lets them go, and hands a card it let go over to the script that moves it, so
//! that the card flies from where it was held up rather than from its zone, as the web
//! interface's `Hold.keeps` (`ui/src/stage/table.ts`) has it.

use beryl_protocol::protocol::{ObjectId, PlayerView};
use beryl_protocol::script::Script;

use super::Show;
use super::compile::{nanoseconds, subject_of};
use crate::beat::Laid;
use crate::light::Lit;
use game_table::table::{CARD_WIDTH, Held, Moves, TrayPoses};
use kit_base::pose::{Curve, Pose};
use kit_ui::input::Rect;

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
    pub fn lift_tray(&mut self, held: Held, poses: TrayPoses, over: (u64, u64), curve: Curve) {
        self.table.lift(held, poses, over, curve);
    }

    /// Shows each card `boxes` names in its box of the flat layer, and no other
    /// (`Table::show_in`).
    pub fn show_in(&mut self, boxes: &[(ObjectId, Rect, bool)], moves: Moves, covered: bool) {
        self.table.show_in(boxes, moves, covered);
    }

    /// Covers the table for the open prompt or uncovers it (`Table::cover_for_prompt`).
    pub fn cover_table(&mut self, covers: bool) {
        self.table.cover_for_prompt(covers);
    }

    /// Where each card held up rests in its zone in the view the table shows (`Table::zone_rests`).
    #[must_use]
    pub fn zone_rests(&self) -> Vec<(ObjectId, Pose, bool)> {
        match self.shown.as_deref() {
            Some(update) => self.table.zone_rests(&update.view),
            None => Vec::new(),
        }
    }

    /// Lets the cards held up go back to `rests` over `length` nanoseconds: from the next frame,
    /// or, where `script` names the script that answers a closed prompt, from the beat of that
    /// script that `place_let_go` finds (`Table::let_go`).
    pub fn let_go_tray(
        &mut self,
        rests: &[(ObjectId, Pose, bool)],
        (length, script): (u64, Option<usize>),
        curve: Curve,
    ) {
        self.table.let_go(rests, (length, script), curve);
    }

    /// Begins the let-go of the tray's cards that waits for the script answering a closed
    /// prompt, once that script plays. The web interface puts the end of the tray first in the
    /// answer's first beat with a step about a card the tray held, or in its first beat where
    /// none has one (`lowered` in `ui/src/stage/prompt.ts`), so the cards go back as that
    /// beat's turn comes on the script's clock. A script later than the one awaited, which
    /// should not happen, lets them go at once.
    pub(super) fn place_let_go(&mut self, now: u64) {
        let (Some((number, letting)), Some(playing)) =
            (self.table.let_go_waits_for(), self.playing.as_ref())
        else {
            return;
        };
        if playing.number < number {
            return;
        }
        let (beat, from) = if playing.number == number {
            let update = playing.update.as_deref().or(self.shown.as_deref());
            let script = update.and_then(|update| update.view.script.as_ref());
            first_about(script, &playing.compiled.laid, letting)
        } else {
            (0, playing.clock(now))
        };
        let view = playing.number + 1;
        let start = playing.real(from);
        let length = self.table.start_let_go(start);
        self.placed_let_go = Some(PlacedLetGo {
            view,
            beat,
            from,
            length,
            start,
        });
    }

    /// Where the let-go that waited for its script was placed since this was last called.
    pub fn take_placed_let_go(&mut self) -> Option<PlacedLetGo> {
        self.placed_let_go.take()
    }

    /// Hands each card the tray let go over to the script that just started, if the script
    /// carries it somewhere else, more than half a card's width away: each of its first segments
    /// that starts from where it rested before the tray held it up (`hand_over`) starts from
    /// where the tray let it go instead, and one that holds it there holds it there, so that it
    /// waits where it was held up until its own step carries it on.
    /// Writes the script's record again if any card was handed over.
    pub(super) fn hand_over_tray(&mut self) {
        let Some(playing) = &mut self.playing else {
            return;
        };
        if !self.table.tray_leaving() {
            return;
        }
        let mut handed = Vec::new();
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
            handed.push(slot);
            for segment in track.segments_mut() {
                if !hand_over(segment, &resting, from) {
                    break;
                }
            }
            for record in
                (playing.compiled.segments.iter_mut()).filter(|record| record.slot == slot)
            {
                if !hand_over(&mut record.segment, &resting, from) {
                    break;
                }
            }
        }
        // Only the first script to start after the tray let its cards go takes them over.
        self.table.handed_over_tray(&handed);
        if !handed.is_empty()
            && let Some(playing) = self.playing.take()
        {
            self.write_record(playing.number, &playing.compiled);
            self.playing = Some(playing);
        }
    }
}

/// How far from where a card rested before the tray held it up a segment may start, in
/// millimetres, and still start from there for `hand_over`. A pile on the desktop stands so that
/// its top card stays where the web interface draws it (`table::FittedPiles`), so each card that
/// leaves it moves the cards left in it a fraction of a millimetre, and a step that takes a card
/// from the pile slides each card still in it that far before its own step carries it off.
const HANDED_NEAR: f32 = 2.0;

/// Starts `segment` from `from`, where the tray let its card go, if it starts from `resting`,
/// where the card rested before the tray held it up (within `HANDED_NEAR`), and holds it there
/// too if it also ends there. Says whether the next segment may start from `resting` as well.
fn hand_over(segment: &mut kit_base::pose::Segment, resting: &Pose, from: Pose) -> bool {
    let near = |pose: &Pose| {
        pose.position.distance(resting.position) <= HANDED_NEAR
            && pose.rotation == resting.rotation
            && (pose.turn - resting.turn).abs() < 1e-3
    };
    if !near(&segment.from) {
        return false;
    }
    segment.from = from;
    if !near(&segment.to) {
        return false;
    }
    segment.to = from;
    true
}

/// Where a let-go of the tray's cards that waited for the script answering a closed prompt was
/// placed, for the client's record of the steps it makes itself (`prompts::OwnRecord`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedLetGo {
    /// The view the script leads to, counted from 1, and the beat of it the let-go is first in.
    pub view: usize,
    pub beat: usize,
    /// When it begins, in nanoseconds on the script's clock, and how long it takes.
    pub from: u64,
    pub length: u64,
    /// When it begins, in nanoseconds since the client started.
    pub start: u64,
}

/// The first beat of `script`, laid out as `laid`, with a step about one of the cards in
/// `letting` (by slot), and when that beat's turn comes, in nanoseconds on the script's clock;
/// its first beat, at its start, where no beat has one or its steps were not laid out.
fn first_about(script: Option<&Script>, laid: &[Laid], letting: &[usize]) -> (usize, u64) {
    let Some((script, beat)) = script.and_then(|script| Some((script, about(script, letting)?)))
    else {
        return (0, 0);
    };
    let first: usize = script.beats[..beat].iter().map(Vec::len).sum();
    let turn = laid.get(first).map_or(0, |laid| nanoseconds(laid.turn));
    (beat, turn)
}

/// The first beat of `script` with a step about one of the cards in `letting` (by slot), as the
/// web's `lowered` finds the beat a closed prompt's tray lets go in.
pub(super) fn about(script: &Script, letting: &[usize]) -> Option<usize> {
    (script.beats.iter()).position(|steps| {
        (steps.iter()).any(|step| subject_of(step).is_some_and(|slot| letting.contains(&slot)))
    })
}
