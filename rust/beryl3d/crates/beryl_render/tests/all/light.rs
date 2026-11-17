//! The Enhanced tier's per-pixel lighting, read from single pixels of hand-built frames. Each
//! card is drawn in one uniform grey (a black tint whitened halfway), so that a pixel's color
//! changes only with the light, not with the placeholder face's shapes, and the cards lie on a
//! black table.
//!
//! The cards are seen through a perspective camera on the key light's side of the table, 45
//! degrees above them, so that a card lying flat mirrors the key light far from the camera and
//! shows no sheen. The camera is close, so that the direction toward it changes enough across a
//! card for the sheen's highlight to move along the face as the card tilts.

use beryl_render::{BACK_LAYER, Limits};
use beryl_scene::data::space::shadow;
use beryl_scene::frame::{Band, Draw};
use beryl_scene::input::{FaceSize, Tier};
use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Affine3A, Mat4, Quat, Vec2, Vec3};

use super::render::{Picture, draw_in, rectangle, renderer};

/// The target's size, in pixels.
const SIZE: u32 = 400;
/// How far the camera is from the cards' centre, in millimetres.
const DISTANCE: f32 = 110.0;
/// Where the cards' centre is: high above the table, so that a tilted card stays above it.
const CENTRE: Vec3 = Vec3::new(0.0, 0.0, 50.0);

/// The horizontal direction toward the key light: against the way the Baseline tier's contact
/// shadows fall, which is `(light.x, -light.y)` per millimetre of height.
fn toward_key() -> Vec3 {
    Vec3::new(-shadow::LIGHT.x, shadow::LIGHT.y, 0.0).normalize()
}

/// A camera on the key light's side, 45 degrees above the table, looking at its centre.
fn camera() -> Mat4 {
    let eye = CENTRE + (toward_key() + Vec3::Z).normalize() * DISTANCE;
    directx::perspective(70_f32.to_radians(), 1.0, 10.0, 2000.0)
        * look_at_mat4(eye, CENTRE, Vec3::Z)
}

/// The turn that tilts a card lying face up by `degrees` toward the key light (away from it
/// when negative), about a horizontal axis through its centre.
fn tilt(degrees: f32) -> Quat {
    Quat::from_axis_angle(Vec3::Z.cross(toward_key()), degrees.to_radians())
}

/// A uniformly grey card at `CENTRE`, turned by `rotation`.
fn grey_card(rotation: Quat) -> Draw {
    Draw::card(
        Affine3A::from_rotation_translation(rotation, CENTRE),
        [0, 0, 0, 255],
        FaceSize::Board,
        1,
        BACK_LAYER,
        0.0,
        0.5,
    )
}

/// `card` on a black table, drawn in `tier`.
fn draw(tier: Tier, card: Draw) -> Picture {
    let table = rectangle(Vec2::ZERO, Vec2::splat(2000.0), 0.0, [0, 0, 0]);
    let records = [(Band::Opaque, card), (Band::Opaque, table)];
    let mut renderer = renderer(Limits::Default);
    assert!(renderer.enhanced_available(), "no Enhanced support");
    draw_in(tier, &mut renderer, camera(), (SIZE, SIZE), &records).0
}

/// The pixel showing `point`.
fn pixel(picture: &Picture, point: Vec3) -> [u8; 3] {
    let clip = camera() * point.extend(1.0);
    let ndc = clip.truncate() / clip.w;
    let size = SIZE as f32;
    picture.at(
        (f32::midpoint(ndc.x, 1.0) * size) as u32,
        ((1.0 - ndc.y) * 0.5 * size) as u32,
    )
}

fn brightness(rgb: [u8; 3]) -> u32 {
    rgb.iter().map(|&channel| u32::from(channel)).sum()
}

/// The centre's pixel of a grey card turned by `rotation`, lit in Enhanced.
fn lit_centre(rotation: Quat) -> [u8; 3] {
    pixel(&draw(Tier::Enhanced, grey_card(rotation)), CENTRE)
}

#[test]
fn a_card_lying_flat_in_the_open_keeps_its_baseline_color() {
    let baseline = pixel(&draw(Tier::Baseline, grey_card(Quat::IDENTITY)), CENTRE);
    let lit = lit_centre(Quat::IDENTITY);
    assert!(
        lit.iter()
            .zip(baseline)
            .all(|(&found, expected)| found.abs_diff(expected) <= 3),
        "lying flat, Enhanced drew {lit:?} and Baseline {baseline:?}"
    );
}

#[test]
fn a_card_tilted_toward_the_key_light_is_brighter_than_one_tilted_away() {
    let toward = lit_centre(tilt(25.0));
    let away = lit_centre(tilt(-25.0));
    assert!(
        brightness(toward) > brightness(away) + 15,
        "tilted toward the key {toward:?}, away {away:?}"
    );
}

/// Nine pixels down the middle of a card tilted toward the camera and the key light, from its
/// near end to its far end: the sheen's highlight is brightest at one of them, and at another
/// once the card tilts 10 degrees further, since its face then mirrors the key light toward the
/// camera from another place.
#[test]
fn the_sheen_moves_across_the_face_as_the_card_tilts() {
    let brightest = |degrees: f32| {
        let rotation = tilt(degrees);
        let picture = draw(Tier::Enhanced, grey_card(rotation));
        let samples: Vec<u32> = (0..9)
            .map(|i| {
                let along = (i as f32 - 4.0) * 7.0;
                let point = rotation * (toward_key() * along) + CENTRE;
                brightness(pixel(&picture, point))
            })
            .collect();
        let (index, _) = samples
            .iter()
            .enumerate()
            .max_by_key(|&(_, &value)| value)
            .expect("nine samples");
        (index, samples)
    };
    let (before, before_samples) = brightest(SHEEN_TILT);
    let (after, after_samples) = brightest(SHEEN_TILT + 10.0);
    assert!(
        before_samples.iter().max() > before_samples.iter().min(),
        "no highlight on the face: {before_samples:?}"
    );
    assert_ne!(
        before, after,
        "the highlight stayed at sample {before}: {before_samples:?} then {after_samples:?}"
    );
}

/// The tilt at which the sheen's highlight lies on the face for `the_sheen_moves_...`.
const SHEEN_TILT: f32 = 40.0;

/// A card turned face down is lit as its face would be in the same place: its back, facing up,
/// is drawn as its face is when the face faces up, tilted either way.
#[test]
fn a_cards_back_is_lit_by_the_same_rule_as_its_face() {
    let axis = Vec3::Z.cross(toward_key());
    for degrees in [-25.0_f32, 0.0, 25.0] {
        let face = lit_centre(tilt(degrees));
        let back = lit_centre(Quat::from_axis_angle(axis, (180.0 + degrees).to_radians()));
        assert!(
            face.iter()
                .zip(back)
                .all(|(&found, expected)| found.abs_diff(expected) <= 2),
            "tilted {degrees} degrees, the face drew {face:?} and the back {back:?}"
        );
    }
}
