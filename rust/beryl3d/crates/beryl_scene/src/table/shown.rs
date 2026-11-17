//! Cards shown in the flat layer's boxes (`Shown::Object`), such as the viewer's grid of a
//! pile's cards: the boxes the flat layer laid them out in, and each card's move into its box and
//! back to where the table rests it. A card in a box is still its one scene object, so it is the
//! same card on the table and in the viewer.
//!
//! The boxes do not change where a card rests: scripts are played from the cards' zones, as the
//! tray's cards are, and a card shown in a box is drawn there in its place, over the flat layer
//! (`Band::LayerCards`). While a box shows a card, the pile it lies in draws neither its side nor
//! its element, as the web interface hides a card the viewer lists (`useOwned` in
//! `ui/src/game/Piles.tsx`). While anything covers the table, such as the viewer's darkened
//! backdrop, no card of the table takes a press.

use beryl_protocol::protocol::ObjectId;

use super::scene::SceneObject;
use super::{Camera, Table};
use crate::data::space::shown::DEPTH;
use crate::input::Rect;
use crate::motion::between;
use crate::pose::{Curve, Pose};

/// A box that shows a card: where the card stands in it, and whether it is drawn there (false
/// for a box scrolled partly or wholly out of the element that clips it).
#[derive(Clone, Copy, Debug, PartialEq)]
struct InBox {
    pose: Pose,
    visible: bool,
}

/// A card's move into its box or out of it: where it was drawn as the move began, the move's
/// length and easing, and the times it runs over, once the frame after it was asked for has
/// given it a start.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Move {
    from: Pose,
    length: u64,
    curve: Curve,
    over: Option<(u64, u64)>,
}

impl Move {
    /// How far through the move is at `now`, eased, or `None` once it is over or before it has
    /// a start.
    fn share(&self, now: u64) -> Option<f32> {
        let (from, to) = self.over?;
        if now >= to {
            return None;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a move lasts well under a second of nanoseconds"
        )]
        let elapsed = now.saturating_sub(from) as f32 / (to - from).max(1) as f32;
        Some(self.curve.ease(elapsed))
    }

    /// Whether the move has not ended at `now`: it is under way, or waits for its start.
    fn running(&self, now: u64) -> bool {
        self.over.is_none_or(|(_, to)| now < to)
    }
}

/// The boxes cards are shown in and their moves, each by its card's slot. Both lists are empty
/// while no viewer is open, so that a copy of the table allocates nothing for them; they
/// change only as the client hands the table new boxes, never in a frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Shown {
    boxes: Vec<(usize, InBox)>,
    moves: Vec<(usize, Move)>,
    /// Whether something of the flat layer covers the table, so that no card takes a press.
    covered: bool,
    /// The `now` of the latest frame.
    now: u64,
}

impl Shown {
    /// Whether a box shows the card in `slot`.
    pub(crate) fn holds(&self, slot: usize) -> bool {
        self.in_box(slot).is_some()
    }

    fn in_box(&self, slot: usize) -> Option<InBox> {
        (self.boxes.iter()).find_map(|&(card, in_box)| (card == slot).then_some(in_box))
    }

    fn moving(&self, slot: usize) -> Option<Move> {
        (self.moves.iter()).find_map(|&(card, moving)| (card == slot).then_some(moving))
    }
}

/// How a card moves into its box and back out of it: each move's length in nanoseconds and its
/// easing. `relaid` says how a card glides when its box moves because the screen laid it out
/// again, as the opening moves a card between its hand and its slots; with none, a card whose box
/// moved stands at once in its new place, as one does when its box scrolls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moves {
    pub into: (u64, Curve),
    pub back: (u64, Curve),
    pub relaid: Option<Relaid>,
}

/// How a card glides to its box's new place: `cross` for a card whose box changed rows, by more
/// than half the box's height, and `shift` for one that moved along its row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Relaid {
    pub cross: (u64, Curve),
    pub shift: (u64, Curve),
}

impl Table {
    /// Shows each card `boxes` names in its box, a rectangle of the window in logical points
    /// that is drawn if its flag says so, and no other card. A card new to a box moves into it
    /// from where it is drawn now, and a card no box shows any more moves back to where it
    /// rests, each as `moves` says, from the next frame. A card whose box moved glides there
    /// from where it is drawn now as `moves.relaid` says, or, with none, as a scroll moves it,
    /// stands at once in its new place. `covered` says whether the flat layer covers the table,
    /// so that no card takes a press.
    pub fn show_in(&mut self, boxes: &[(ObjectId, Rect, bool)], moves: Moves, covered: bool) {
        self.shown.covered = covered;
        let now = self.shown.now;
        self.shown.moves.retain(|(_, moving)| moving.running(now));
        let gone: Vec<usize> = (self.shown.boxes.iter())
            .map(|&(slot, _)| slot)
            .filter(|&slot| !boxes.iter().any(|(card, ..)| usize::from(card.0) == slot))
            .collect();
        for slot in gone {
            let drawn = self.drawn_in_box(slot, now);
            self.shown.boxes.retain(|&(card, _)| card != slot);
            self.start_move(slot, drawn, moves.back);
        }
        for &(card, rect, visible) in boxes {
            let slot = usize::from(card.0);
            let in_box = InBox {
                pose: box_pose(self.camera(), rect),
                visible,
            };
            if let Some(index) = (self.shown.boxes.iter()).position(|(shown, _)| *shown == slot) {
                let was = self.shown.boxes[index].1;
                let glide = moves.relaid.filter(|_| was.pose != in_box.pose);
                let drawn = glide.map(|_| self.drawn_in_box(slot, now));
                self.shown.boxes[index].1 = in_box;
                if let (Some(relaid), Some(drawn)) = (glide, drawn) {
                    let camera = self.camera();
                    let rise = camera.to_screen(was.pose.position).y
                        - camera.to_screen(in_box.pose.position).y;
                    let crossing = rise.abs() > rect.height / 2.0;
                    let timing = if crossing { relaid.cross } else { relaid.shift };
                    self.start_move(slot, drawn, timing);
                }
                continue;
            }
            if let Some(drawn) = self.drawn_out_of_box(slot, now) {
                self.start_move(slot, drawn, moves.into);
            }
            self.shown.boxes.push((slot, in_box));
        }
    }

    /// Notes that the card in `slot` moves from `from`, as `(length, curve)` says, starting at
    /// the next frame.
    fn start_move(&mut self, slot: usize, from: Pose, (length, curve): (u64, Curve)) {
        self.shown.moves.retain(|&(card, _)| card != slot);
        let moving = Move {
            from,
            length,
            curve,
            over: None,
        };
        self.shown.moves.push((slot, moving));
    }

    /// Takes `now`, the time of the frame being drawn, and starts each move that waits for it.
    pub(crate) fn advance_shown(&mut self, now: u64) {
        self.shown.now = now;
        for (_, shown) in &mut self.shown.moves {
            if shown.over.is_none() {
                shown.over = Some((now, now + shown.length));
            }
        }
    }

    /// Where the card in `slot`, which a box shows, is drawn at `now`.
    fn drawn_in_box(&self, slot: usize, now: u64) -> Pose {
        let Some(Some(object)) = self.objects.cards.get(slot).copied() else {
            return Pose::default();
        };
        let mut object = object;
        self.shown_look(slot, &mut object, now);
        object.pose
    }

    /// Where the card in `slot`, which no box shows, is drawn at `now`: part of the way back
    /// from a box, or where it rests. None for a slot the table holds no card in.
    fn drawn_out_of_box(&self, slot: usize, now: u64) -> Option<Pose> {
        let mut object = self.objects.cards.get(slot).copied().flatten()?;
        self.shown_look(slot, &mut object, now);
        Some(object.pose)
    }

    /// Changes `object`, the card in `slot` as it would otherwise be drawn at `now`, for the box
    /// that shows it or its move out of one: it stands in its box, part of the way there, or
    /// part of the way back. Returns whether it moves.
    pub(super) fn shown_look(&self, slot: usize, object: &mut SceneObject, now: u64) -> bool {
        let moving = self.shown.moving(slot);
        let share = moving.and_then(|moving| moving.share(now));
        let waiting = moving.is_some_and(|moving| moving.over.is_none());
        match self.shown.in_box(slot) {
            Some(in_box) => match (moving, share) {
                (Some(moving), Some(share)) => {
                    object.pose = between(&moving.from, &in_box.pose, share);
                    object.visible = true;
                    true
                }
                (Some(moving), None) if waiting => {
                    object.pose = moving.from;
                    object.visible = true;
                    true
                }
                _ => {
                    object.pose = in_box.pose;
                    object.visible = in_box.visible;
                    false
                }
            },
            None => match (moving, share) {
                (Some(moving), Some(share)) => {
                    object.pose = between(&moving.from, &object.pose, share);
                    object.visible = true;
                    true
                }
                (Some(moving), None) if waiting => {
                    object.pose = moving.from;
                    object.visible = true;
                    true
                }
                _ => false,
            },
        }
    }

    /// Whether a box shows the card in `slot`.
    #[must_use]
    pub fn in_box(&self, slot: usize) -> bool {
        self.shown.holds(slot)
    }

    /// Whether a card moves into or out of a box at `now`.
    #[must_use]
    pub fn shown_moving(&self, now: u64) -> bool {
        (self.shown.moves.iter()).any(|(_, moving)| moving.running(now))
    }

    /// Whether the flat layer covers the table, so that no card of the table takes a press.
    pub(super) fn covered(&self) -> bool {
        self.shown.covered
    }
}

/// Where a card stands to fill `rect`, a box of the window in logical points, seen through
/// `camera`: centred in it, facing the camera, as large as fits it.
fn box_pose(camera: &Camera, rect: Rect) -> Pose {
    use super::{CARD_HEIGHT, CARD_WIDTH};
    let window = camera.window.size;
    let centre = glam::Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let ndc = glam::Vec2::new(
        centre.x / window.width * 2.0 - 1.0,
        1.0 - centre.y / window.height * 2.0,
    );
    let height = rect.height.min(rect.width * CARD_HEIGHT / CARD_WIDTH) / window.height;
    camera.screen_pose(ndc, DEPTH, glam::Quat::IDENTITY, height)
}
