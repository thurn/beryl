//! Where the cards a prompt holds up lie (docs/beryl-3d.md, "Objects and placement"): a port of
//! the web interface's `laidOut` (`ui/src/stage/tray.ts`) for a tray of one pile and of two.
//! One pile is laid out in rows about the middle of the space, as many as it takes. Two are
//! boxes side by side, each a row as long as the tray holds cards, so that a box keeps its place
//! whatever is moved between them.
//!
//! The web lays a tray out on its 1600 by 900 stage, in the space `desktopTray`
//! (`ui/src/stage/desktop.ts`) gives. Here the same space is measured in stage pixels of a stage
//! as tall as the window, `layout.desktop.tray` in `space.json`, and each card is placed relative
//! to the camera, so that the tray keeps its size and place on screen at any window shape. The
//! put-back and pick trays, which a prompt alone makes, are not ported here.

use beryl_protocol::protocol::{ObjectId, PlayerView};
use glam::{Quat, Vec2};

use super::scene::SceneObject;
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, Placement, Rest, Table};
use crate::data::space::layout::desktop::tray as data;
use crate::frame::OBJECTS;
use crate::motion::between;
use crate::pose::{Curve, Pose};

/// The space a layout gives a tray and the sizes it lays one out at, in stage pixels of a stage
/// `stage_height` tall and as wide as the window's shape makes it, as the web's `TraySpace`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraySpace {
    pub stage_height: f32,
    /// The top of the space, and how tall it is.
    pub y: f32,
    pub height: f32,
    /// The most the space reaches either side of the middle, and how far from the stage's right
    /// edge its own right edge must stay, clear of the stack: `desktopTray`'s
    /// `middle - fromMiddle(150)` and `STACK.x - 16 - middle`.
    pub half_most: f32,
    pub stack_side: f32,
    /// The widest and the narrowest a card is drawn.
    pub most: f32,
    pub least: f32,
    /// Between two cards, and between a box's edge and its cards.
    pub gap: f32,
    /// The strip over a box's cards that says what the pile is.
    pub heading: f32,
    /// How far in front of the eye the first card lies, in millimetres; each later card lies a
    /// millimetre nearer, so that it covers the one before it.
    pub depth: f32,
}

impl TraySpace {
    /// The desktop layout's, from `layout.desktop.tray`.
    pub const DESKTOP: Self = Self {
        stage_height: data::STAGE_HEIGHT,
        y: data::Y,
        height: data::HEIGHT,
        half_most: data::HALF_MOST,
        stack_side: data::STACK_SIDE,
        most: data::MOST,
        least: data::LEAST,
        gap: data::GAP,
        heading: data::HEADING,
        depth: data::DEPTH,
    };
}

/// The cards a prompt holds up: one or two piles, each its cards' slots in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub piles: Vec<Vec<ObjectId>>,
}

/// A card's place on the stage: its top left corner and its width.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    x: f32,
    y: f32,
    width: f32,
}

/// The space's left edge and width on a stage `stage_width` wide.
fn across(space: &TraySpace, stage_width: f32) -> (f32, f32) {
    let middle = stage_width / 2.0;
    let half = space.half_most.min(middle - space.stack_side).max(0.0);
    (middle - half, 2.0 * half)
}

/// The widths a card is tried at, the widest first: from `most` down by 8 pixels, and `least`.
fn widths(space: &TraySpace) -> impl Iterator<Item = f32> + '_ {
    let mut width = space.most;
    std::iter::from_fn(move || {
        if width > space.least {
            let tried = width;
            width -= 8.0;
            Some(tried)
        } else if width > f32::MIN {
            width = f32::MIN;
            Some(space.least)
        } else {
            None
        }
    })
}

/// How tall a card `width` wide is.
fn height_of(width: f32) -> f32 {
    width * CARD_HEIGHT / CARD_WIDTH
}

/// One pile of `count` cards in rows about the middle of the space, as the web's `rows`.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a tray holds a few dozen cards"
)]
fn rows(count: usize, space: &TraySpace, (left, wide): (f32, f32)) -> Vec<Spot> {
    let gap = space.gap;
    let total = count.max(1);
    let (mut width, mut across, mut step) = (space.least, total, 0.0);
    for tried in widths(space) {
        let fits = (((wide + gap) / (tried + gap)).floor() as usize).max(1);
        let deep = total.div_ceil(fits);
        width = tried;
        across = total.min(fits);
        step = tried + gap;
        if deep as f32 * (height_of(tried) + gap) - gap <= space.height {
            break;
        }
        if tried <= space.least {
            // Too many for whole cards: the rows the space holds, each closed up over itself.
            let held = (((space.height + gap) / (height_of(tried) + gap)).floor() as usize).max(1);
            across = total.div_ceil(held);
            step = f32::min(step, (wide - tried) / (across.max(2) - 1) as f32);
        }
    }
    let deep = total.div_ceil(across);
    let tall = deep as f32 * (height_of(width) + gap) - gap;
    let top = space.y + ((space.height - tall) / 2.0).max(0.0);
    (0..count)
        .map(|index| {
            let row = index / across;
            let in_row = across.min(count - row * across);
            let row_wide = width + (in_row as f32 - 1.0) * step;
            Spot {
                x: left + (wide - row_wide) / 2.0 + (index % across) as f32 * step,
                y: top + row as f32 * (height_of(width) + gap),
                width,
            }
        })
        .collect()
}

/// Several piles, a box each, side by side about the middle of the space, as the web's
/// `boxes`. The web gives each box room for what its pile can come to hold (`roomsOf`), which a
/// prompt says, and room for every card the tray holds when it says nothing. `Held` does not
/// say what a prompt can move yet, so each box has room for the cards it holds now.
#[expect(clippy::cast_precision_loss, reason = "a tray holds a few dozen cards")]
fn boxes(counts: &[usize], space: &TraySpace, (left, wide): (f32, f32)) -> Vec<Spot> {
    let (gap, heading) = (space.gap, space.heading);
    let piles = counts.len() as f32;
    let between: f32 = counts.iter().map(|&count| count.max(1) as f32 - 1.0).sum();
    // What is not a card's own width or the step to the next: the boxes' edges and the gaps
    // between the boxes.
    let edges = piles * 2.0 * gap + (piles - 1.0) * gap;
    let (mut width, mut step) = (space.least, 0.0);
    for tried in widths(space) {
        width = tried;
        step = tried + gap;
        let whole = piles * tried + between * step + edges;
        let tall = height_of(tried) + heading + 2.0 * gap;
        if whole <= wide && tall <= space.height {
            break;
        }
        if tried <= space.least && between > 0.0 {
            step = f32::min(step, (wide - edges - piles * tried) / between);
        }
    }
    let height = height_of(width) + heading + 2.0 * gap;
    let whole = piles * width + between * step + edges;
    let y = space.y + ((space.height - height) / 2.0).max(0.0);
    let mut x = left + (wide - whole) / 2.0;
    let mut spots = Vec::with_capacity(counts.iter().sum());
    for &count in counts {
        let box_wide = width + (count.max(1) as f32 - 1.0) * step + 2.0 * gap;
        let own = f32::min(
            step,
            (box_wide - 2.0 * gap - width) / (count.max(2) - 1) as f32,
        );
        for index in 0..count {
            spots.push(Spot {
                x: x + gap + index as f32 * own,
                y: y + heading + gap,
                width,
            });
        }
        x += box_wide + gap;
    }
    spots
}

/// Where each card of `piles`, one pile or two, each its cards in order, lies held up in
/// `space`, seen through `camera`: one pose for each card, in the order of the piles and then
/// of their cards. Each is face up and faces the camera; a later card lies nearer the eye.
#[must_use]
pub fn tray<T>(piles: &[&[T]], space: TraySpace, camera: &Camera) -> Vec<Pose> {
    let size = camera.window.size;
    // Stage pixels per logical point of the window.
    let per_point = space.stage_height / size.height;
    let stage_width = size.width * per_point;
    let span = across(&space, stage_width);
    let counts: Vec<usize> = piles.iter().map(|pile| pile.len()).collect();
    let spots = if counts.len() == 1 {
        rows(counts[0], &space, span)
    } else {
        boxes(&counts, &space, span)
    };
    #[expect(clippy::cast_precision_loss, reason = "a tray holds a few dozen cards")]
    let poses = spots
        .iter()
        .enumerate()
        .map(|(index, spot)| {
            let height = height_of(spot.width);
            let centre = Vec2::new(spot.x + spot.width / 2.0, spot.y + height / 2.0);
            let ndc = Vec2::new(
                centre.x / stage_width * 2.0 - 1.0,
                1.0 - centre.y / space.stage_height * 2.0,
            );
            camera.screen_pose(
                ndc,
                space.depth - index as f32,
                Quat::IDENTITY,
                height / space.stage_height,
            )
        })
        .collect();
    poses
}

impl Placement {
    /// Gives each card `held` names its place in the tray instead of its zone's, laid out in
    /// `space` and seen through `camera`. A card keeps its zone's `Place`, so that a step still
    /// finds it in its zone, and is drawn, wherever in its zone it lies.
    pub fn hold(&mut self, held: &Held, space: TraySpace, camera: &Camera) {
        let piles: Vec<&[ObjectId]> = held.piles.iter().map(Vec::as_slice).collect();
        let poses = tray(&piles, space, camera);
        for (&slot, pose) in held.piles.iter().flatten().zip(poses) {
            if let Some(rest) = self.get(slot).copied() {
                self.set(
                    slot,
                    Rest {
                        pose,
                        visible: true,
                        ..rest
                    },
                );
            }
        }
    }
}

/// The cards of a tray on their way into it or back out of it: for each slot, where the card
/// was drawn as the move began, which it goes from to where it now rests over `over` (in
/// nanoseconds since the client started), eased by `curve`. A move that is waiting for its
/// first frame has no `over` yet, only its length.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TrayMotion {
    /// Where each card held up lies in the tray, and, by slot, where it rests in its zone and
    /// whether it is drawn there, which scripts are played from (`Table::rests`).
    poses: Vec<(ObjectId, Pose)>,
    zone: Vec<Option<(Pose, bool)>>,
    from: Vec<Option<Pose>>,
    over: Option<(u64, u64)>,
    /// How long a move waiting for its first frame takes, in nanoseconds.
    waiting: Option<u64>,
    curve: Curve,
    /// Whether the cards are going back out of the tray.
    leaving: bool,
    /// The `now` of the latest frame.
    now: u64,
}

impl TrayMotion {
    /// Room for every slot, so that no move allocates.
    pub(crate) fn new() -> Self {
        Self {
            poses: Vec::new(),
            zone: vec![None; OBJECTS],
            from: vec![None; OBJECTS],
            over: None,
            waiting: None,
            curve: Curve::EASE_OUT,
            leaving: false,
            now: 0,
        }
    }

    /// How far through the move is at `now`, eased, or `None` once it is over.
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

    /// Notes that the card in `slot` moves from `pose`.
    fn starts(&mut self, slot: usize, pose: Pose) {
        if let Some(from) = self.from.get_mut(slot) {
            *from = Some(pose);
        }
    }
}

impl Table {
    /// Holds up the cards `held` names, or none: from the next placement on, they rest in the
    /// tray rather than in their zones.
    pub fn hold(&mut self, held: Option<Held>) {
        self.held = held;
    }

    /// Whether the card in `slot` is held up in the tray.
    #[must_use]
    pub fn in_tray(&self, slot: usize) -> bool {
        self.held.as_ref().is_some_and(|held| {
            (held.piles.iter().flatten()).any(|card| usize::from(card.0) == slot)
        })
    }

    /// Whether a press may reach the card in `slot`, resting at `place`: one where the pointer
    /// reaches cards, while no tray is up; and while one is, only a card in it, as the shade the
    /// web interface lays over the table under a tray takes the pointer (`coversTable` in
    /// `ui/src/stage/prompt.ts`). Scripts are played from where the cards rest in their zones:
    /// only the table's own resting places put them in the tray.
    pub(super) fn pressable(&self, slot: usize, place: crate::frame::Place) -> bool {
        match self.held {
            Some(_) => self.in_tray(slot),
            None => crate::pointer::takes_pointer(place),
        }
    }

    /// The cards held up in the tray, if any.
    #[must_use]
    pub fn held(&self) -> Option<&Held> {
        self.held.as_ref()
    }

    /// Where each card of `held` lies in the tray for this window, by slot.
    #[must_use]
    pub fn tray_poses(&self, held: &Held) -> Vec<(ObjectId, Pose)> {
        let piles: Vec<&[ObjectId]> = held.piles.iter().map(Vec::as_slice).collect();
        let poses = tray(&piles, self.layout().tray, self.camera());
        held.piles.iter().flatten().copied().zip(poses).collect()
    }

    /// Holds up the cards of `held`, each going from where it is drawn now to its place in
    /// `poses` (from `tray_poses`) over `over`, in nanoseconds since the client started, eased
    /// by `curve`. Allocates nothing.
    pub fn lift(
        &mut self,
        held: Held,
        poses: Vec<(ObjectId, Pose)>,
        over: (u64, u64),
        curve: Curve,
    ) {
        for &(slot, pose) in &poses {
            let slot = usize::from(slot.0);
            let Some(Some(mut object)) = self.objects.cards.get(slot).copied() else {
                continue;
            };
            let drawn = self.tray_drawn(slot, object);
            self.tray.starts(slot, drawn);
            if let Some(zone) = self.tray.zone.get_mut(slot) {
                *zone = Some((object.pose, object.visible));
            }
            object.pose = pose;
            object.visible = true;
            self.objects.cards[slot] = Some(object);
        }
        self.held = Some(held);
        self.tray.poses = poses;
        self.tray.over = Some(over);
        self.tray.waiting = None;
        self.tray.curve = curve;
        self.tray.leaving = false;
    }

    /// Where each card held up rests in its zone in `view`, the view the table shows, for
    /// `let_go`: its pose and whether it is drawn there.
    #[must_use]
    pub fn zone_rests(&self, view: &PlayerView) -> Vec<(ObjectId, Pose, bool)> {
        let Some(held) = &self.held else {
            return Vec::new();
        };
        let placement = self.placement(view, &self.objects.numbers);
        (held.piles.iter().flatten())
            .filter_map(|&slot| {
                let rest = placement.get(slot)?;
                Some((slot, rest.pose, rest.visible))
            })
            .collect()
    }

    /// Lets the cards held up go back to their places in `rests` (from `zone_rests`), over
    /// `length` nanoseconds from the next frame, eased by `curve`: each rests there from now on
    /// and is drawn on its way. A card a script moves meanwhile starts from the tray instead
    /// (`Show::hand_over_tray`). Allocates nothing.
    pub fn let_go(&mut self, rests: &[(ObjectId, Pose, bool)], length: u64, curve: Curve) {
        if self.held.take().is_none() {
            return;
        }
        self.tray.poses.clear();
        for &(slot, pose, visible) in rests {
            let index = usize::from(slot.0);
            let Some(Some(mut object)) = self.objects.cards.get(index).copied() else {
                continue;
            };
            let drawn = self.tray_drawn(index, object);
            self.tray.starts(index, drawn);
            object.pose = pose;
            object.visible = visible;
            self.objects.cards[index] = Some(object);
        }
        self.tray.over = None;
        self.tray.waiting = Some(length);
        self.tray.curve = curve;
        self.tray.leaving = true;
    }

    /// Takes `now`, the time of the frame being drawn, and begins a move of the tray's cards
    /// that waits for it.
    pub(crate) fn advance_tray(&mut self, now: u64) {
        self.tray.now = now;
        if let Some(length) = self.tray.waiting.take() {
            self.tray.over = Some((now, now + length));
        }
    }

    /// Where the card in `slot`, resting as `object` says, is drawn now: part of the way through
    /// a move of the tray's, or where it rests.
    fn tray_drawn(&self, slot: usize, mut object: SceneObject) -> Pose {
        self.tray_look(slot, &mut object, self.tray.now);
        object.pose
    }

    /// Changes `object`, the card in `slot` as it rests, for the move of the tray's cards under
    /// way at `now`: it is drawn part of the way from where it was as the move began. Returns
    /// whether it moves.
    pub(super) fn tray_look(&self, slot: usize, object: &mut SceneObject, now: u64) -> bool {
        let (Some(share), Some(Some(from))) =
            (self.tray.share(now), self.tray.from.get(slot).copied())
        else {
            return false;
        };
        object.pose = between(&from, &object.pose, share);
        true
    }

    /// Whether a card of the tray moves at `now`.
    #[must_use]
    pub fn tray_moving(&self, now: u64) -> bool {
        self.tray.share(now).is_some() || self.tray.waiting.is_some()
    }

    /// Whether the tray has let its cards go and no script has started since: the script that
    /// starts next moves each card it moves from where the tray let it go
    /// (`Show::hand_over_tray`).
    pub(crate) fn tray_leaving(&self) -> bool {
        self.tray.leaving
    }

    /// A script has taken over the cards the tray let go.
    pub(crate) fn handed_over_tray(&mut self) {
        self.tray.leaving = false;
    }

    /// Where the card in `slot` was drawn as the tray began to let it go, while no script has
    /// taken the tray's cards over yet.
    pub(crate) fn letting_go(&self, slot: usize) -> Option<Pose> {
        if !self.tray.leaving {
            return None;
        }
        self.tray.from.get(slot).copied().flatten()
    }

    /// `placement` with the cards held up in the tray, where this window lays it out.
    pub(super) fn with_held(&mut self, mut placement: Placement) -> Placement {
        if let Some(held) = &self.held {
            placement.hold(held, self.layout().tray, self.camera());
            self.tray.poses = self.tray_poses(held);
        }
        placement
    }

    /// Puts each card held up back at its place in the tray, after the scene objects were
    /// exchanged for a script's, which are played from the cards' zones. Allocates nothing.
    pub(super) fn rehold(&mut self) {
        for &(slot, pose) in &self.tray.poses {
            let index = usize::from(slot.0);
            if let Some(Some(object)) = self.objects.cards.get_mut(index) {
                if let Some(zone) = self.tray.zone.get_mut(index) {
                    *zone = Some((object.pose, object.visible));
                }
                object.pose = pose;
                object.visible = true;
            }
        }
    }

    /// Gives each card held up its place in its zone in `placement`, the table's resting
    /// places, for the scripts played from them: the tray is the table's to draw, not theirs.
    pub(super) fn unhold(&self, placement: &mut Placement) {
        for &(slot, _) in &self.tray.poses {
            let index = usize::from(slot.0);
            let (Some(Some(rest)), Some(Some((pose, visible)))) = (
                placement.rests.get_mut(index),
                self.tray.zone.get(index).copied(),
            ) else {
                continue;
            };
            rest.pose = pose;
            rest.visible = visible;
        }
    }
}
