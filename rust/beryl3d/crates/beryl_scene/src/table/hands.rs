//! The hands: each fanned along an edge of the screen, placed relative to the camera, as the web
//! interface's `handPlacements` and `heldPlacements` (`ui/src/game/handLayout.ts`) lay them out
//! (docs/beryl-3d.md, "Objects and placement").

use beryl_protocol::protocol::{ObjectView, Side, ZoneName};
use glam::{Quat, Vec2};

use super::place::{Placement, Rest, turn};
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, HandData};
use crate::frame::Place;

/// A hand, fanned along an edge of the screen and placed relative to the camera so that it
/// keeps its size and place at any window shape. The middle of each card's inner edge (the
/// viewer's cards' top edges, the opponent's bottom edges) lies on one circle of
/// `HandData::fan_radius` card widths, and the card turns to lie along it. Each card faces the
/// camera a little nearer the eye than the card before it, so later cards cover earlier ones.
/// The viewer's hand runs along the bottom edge, as the web interface's `handPlacements` has
/// it; the opponent's along the top, upside down to the viewer as the opponent holds it and
/// mostly out of view, as `heldPlacements` has it.
pub(super) fn hand(
    placement: &mut Placement,
    objects: &[&ObjectView],
    side: Side,
    (data, strip): (HandData, f32),
    camera: &Camera,
) {
    let count = objects.len();
    let window = camera.window.size;
    let aspect = window.width / window.height;
    // Distances on screen are in half the window's height, the vertical unit of normalized
    // device coordinates, so that a turn on screen is a true turn.
    let tall = data.height * 2.0;
    let wide = tall * CARD_WIDTH / CARD_HEIGHT;
    // The viewer presses their own cards, so each keeps a strip to press; the opponent's are
    // not theirs to press, and close up within `data.room`, as the web interface's do.
    let least = match side {
        Side::You => strip * 2.0 / window.height,
        Side::Opponent => 0.0,
    };
    let spacing = hand_spacing(count, &data, (wide, least), aspect);
    let radius = data.fan_radius * wide;
    let bias = data.bias_degrees.to_radians();
    // The viewer's cards hang down from their edge on the circle, and the opponent's stand up
    // from it, turned half a turn.
    let (down, half_turn) = match side {
        Side::You => (1.0, 0.0),
        Side::Opponent => (-1.0, std::f32::consts::PI),
    };
    #[expect(
        clippy::cast_precision_loss,
        reason = "a hand holds far fewer than 2^24 cards"
    )]
    let middle = (count as f32 - 1.0) / 2.0;
    for (index, object) in objects.iter().enumerate() {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a hand holds far fewer than 2^24 cards"
        )]
        let index = index as f32;
        let dx = (index - middle) * spacing;
        let along = dx.clamp(-radius, radius);
        let sink = radius - (radius * radius - along * along).sqrt();
        // Turned clockwise right of the middle on the viewer's side, and anticlockwise on the
        // opponent's, measured anticlockwise on screen.
        let angle = -down * ((along / radius).asin() + bias);
        let edge = Vec2::new(dx, data.edge - down * sink);
        let centre = edge + Vec2::from_angle(angle).rotate(Vec2::new(0.0, -down * tall / 2.0));
        let ndc = Vec2::new(centre.x / aspect, centre.y);
        let rotation = Quat::from_rotation_z(angle + half_turn);
        let depth = data.depth - index * data.step_depth;
        let mut pose = camera.screen_pose(ndc, depth, rotation, data.height);
        pose.turn = turn(object);
        let place = Place::Card {
            zone: ZoneName::Hand,
            side,
            row: None,
        };
        placement.set(
            object.slot,
            Rest {
                pose,
                place,
                visible: true,
                copies: 1,
            },
        );
    }
}

/// How far apart the centres of a hand of `count` cards `wide` stand, in half the window's
/// height, in a window of `aspect`: `data.spacing` while the hand has room, and closer once the
/// outer cards' centres would stand more than `data.room` apart. They never close up so far that
/// a card's uncovered strip is narrower than `least`, unless the hand would then run past the
/// window's sides: the outer cards' inner corners, which reach farthest out once a card turns
/// along the circle, stay inside it.
fn hand_spacing(count: usize, data: &HandData, (wide, least): (f32, f32), aspect: f32) -> f32 {
    if count < 2 {
        return 0.0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a hand holds far fewer than 2^24 cards"
    )]
    let gaps = (count - 1) as f32;
    let crowded = (data.spacing * wide).min(data.room * wide / gaps);
    let widest = 2.0 * (aspect - wide / 2.0) / gaps;
    crowded.max(least).min(widest)
}
