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

use beryl_protocol::protocol::ObjectId;
use glam::{Quat, Vec2};

use super::{CARD_HEIGHT, CARD_WIDTH, Camera, Placement, Rest, Table};
use crate::data::space::layout::desktop::tray as data;
use crate::pose::Pose;

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

impl Table {
    /// Holds up the cards `held` names, or none: from the next placement on, they rest in the
    /// tray rather than in their zones.
    pub fn hold(&mut self, held: Option<Held>) {
        self.held = held;
    }

    /// `placement` with the cards held up in the tray.
    pub(super) fn with_held(&self, mut placement: Placement) -> Placement {
        self.hold_in(&mut placement);
        placement
    }

    /// Gives the cards held up their places in the tray in `placement`.
    pub(super) fn hold_in(&self, placement: &mut Placement) {
        if let Some(held) = &self.held {
            placement.hold(held, self.layout().tray, self.camera());
        }
    }
}
