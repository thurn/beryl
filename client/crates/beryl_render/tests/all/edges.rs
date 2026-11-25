//! A tilted card's outline is antialiased: the pixels where it meets what lies behind it are
//! blends of the two, not a staircase of pixels that are wholly card or wholly background.
//! The player saw that staircase on the tilted cards of the opening hand, which the flat layer
//! draws with one sample in both tiers.

use beryl_render::{BACK_LAYER, Target};
use glam::{Affine3A, Quat, Vec2, Vec3};
use kit_base::atlas::{FaceSize, Tier};
use kit_base::pose::Pose;
use kit_frame::frame::{
    Band, Draw, ElementId, Frame, FrameBuilder, NameRef, Place, Resolved, Role, StateBits,
};

use crate::render::{BOTH, Picture, overhead, rectangle, renderer};

/// How far the card is turned, in degrees: about as far as the outer cards of a hand's fan.
const TILT: f32 = 5.0;

/// A black card turned `TILT` degrees, its middle at the target's, drawn with
/// `FrameBuilder::draw_card` as the client draws a card in `state` (on the table for none, in
/// the flat layer's band for `StateBits::RAISED`) whose band at rest is `resting` (`Opaque`, or
/// a blended band for a fading card), over a white background. Returns the picture, 200 by 200
/// pixels, one millimetre per pixel.
fn tilted_card(
    tier: Tier,
    renderer: &mut beryl_render::Renderer,
    state: u32,
    resting: Band,
) -> Picture {
    let mut frame = Frame::with_capacity(2, 4, 0);
    let mut builder = FrameBuilder::new(&mut frame, tier, crate::card());
    builder.camera(overhead());
    builder.draw(
        Band::Opaque,
        rectangle(Vec2::ZERO, Vec2::splat(200.0), 0.0, [255, 255, 255]),
    );
    let position = Vec3::new(0.0, 0.0, 10.0);
    let at = builder.resolved(
        "tilted card",
        Resolved {
            id: ElementId(1),
            parent: u32::MAX,
            role: Role::Card,
            name: NameRef::default(),
            place: Place::Layer,
            pose: Some(Pose {
                position,
                ..Default::default()
            }),
            corners: [Vec2::ZERO; 4],
            depth: 0.0,
            opacity: 1.0,
            clip: None,
            interactive: false,
            state: StateBits(state),
            face: Some(1),
            count: 1,
            annotations: Default::default(),
        },
    );
    let transform =
        Affine3A::from_rotation_translation(Quat::from_rotation_z(TILT.to_radians()), position);
    // A black tint makes the card black whatever its placeholder face shows.
    let card = Draw::card(
        transform,
        [0, 0, 0, 255],
        FaceSize::Board,
        1,
        BACK_LAYER,
        0.0,
        0.0,
    );
    builder.draw_card(at, resting, card);
    let frame = builder.finish();
    let mut target = Target::offscreen(renderer, 200, 200);
    renderer.render(frame, &mut target);
    Picture {
        pixels: renderer.read(&target),
        width: 200,
    }
}

/// Of the rows across the middle of the card's left edge, the share whose first pixel darker
/// than the background, scanning in from the left, is a blend: neither the background's
/// brightness nor the black card's. An outline that is not antialiased has none.
fn blended_share(picture: &Picture) -> f32 {
    let bright = |x, y| {
        let [r, g, b] = picture.at(x, y);
        (u32::from(r) + u32::from(g) + u32::from(b)) / 3
    };
    let rows = 70..130;
    let mut blended = 0;
    for y in rows.clone() {
        // The background beside the card, which the Enhanced tier's light shades a little.
        let background = bright(5, y);
        let edge = (0..100)
            .find(|&x| bright(x, y) + 12 < background)
            .expect("the card's left edge is in the left half");
        let value = bright(edge, y);
        if value > 12 && value + 12 < background {
            blended += 1;
        }
    }
    blended as f32 / rows.len() as f32
}

#[test]
fn a_tilted_cards_outline_blends_with_what_lies_behind_it() {
    for limits in BOTH {
        let mut renderer = renderer(limits);
        for tier in [Tier::Baseline, Tier::Enhanced] {
            for (place, state, resting) in [
                ("on the table", 0, Band::Opaque),
                ("raised in the flat layer", StateBits::RAISED, Band::Opaque),
                ("blended on the table", 0, Band::TableMarks),
                (
                    "blended in the flat layer",
                    StateBits::RAISED,
                    Band::TableMarks,
                ),
            ] {
                let picture = tilted_card(tier, &mut renderer, state, resting);
                let share = blended_share(&picture);
                assert!(
                    share >= 0.6,
                    "{limits:?} {tier:?}, a card {place}: only {:.0}% of the rows along its \
                     left edge have a blended pixel",
                    share * 100.0
                );
            }
        }
    }
}
