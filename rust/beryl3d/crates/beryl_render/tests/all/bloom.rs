//! The Enhanced tier's bloom on glows and streaks, checked by sampling single pixels of
//! hand-built frames drawn in both tiers. The overhead camera shows one millimetre of the table
//! per pixel of the 200 by 200 target.

use beryl_scene::frame::{Band, Draw, sprites};
use beryl_scene::input::Tier;
use glam::{Affine3A, Quat, Vec2, Vec3};

use super::enhanced::{both_tiers, enhanced, within};
use super::render::{Picture, draw_in, overhead, rectangle};

/// A black table under the whole target.
fn table() -> Draw {
    rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, [0, 0, 0])
}

/// A light record on the table, `size` millimetres large and centred on the origin.
fn placed(size: Vec2) -> Affine3A {
    Affine3A::from_scale_rotation_translation(
        size.extend(1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
    )
}

fn brightness(pixel: [u8; 3]) -> u32 {
    pixel.iter().map(|&channel| u32::from(channel)).sum()
}

/// The first pixel to the right of `(x, y)`, along its row, that `picture` leaves black.
fn first_dark(picture: &Picture, (x, y): (u32, u32)) -> u32 {
    (x..200)
        .find(|&x| picture.at(x, y) == [0, 0, 0])
        .expect("the light ends inside the target")
}

#[test]
fn a_glow_blooms_past_its_edge_and_not_far_away() {
    let glow = Draw::glow(placed(Vec2::splat(60.0)), [255; 4], sprites::DISC, 0.0);
    let records = [(Band::Opaque, table()), (Band::Glows, glow)];
    let mut renderer = enhanced();
    let [baseline, enhanced] = both_tiers(&mut renderer, &records);
    // The first black pixel right of the centre in Baseline lies just outside the disc.
    let outside = first_dark(&baseline, (100, 100));
    let lit = enhanced.at(outside, 100);
    assert!(
        brightness(lit) > 0,
        "at {outside} pixels from the left, outside the disc, Enhanced drew {lit:?}"
    );
    // 60 pixels past the disc's edge no light reaches.
    let far = outside + 60;
    assert_eq!(enhanced.at(far, 100), baseline.at(far, 100), "at {far}");
}

#[test]
fn a_streak_blooms_along_its_length_and_not_past_its_head() {
    // 160 millimetres long from x = -80 to 80, so a point `t` of the way along is at
    // x = 160 t - 80. Its head is half way along and its tail a quarter of the way long.
    let streak = Draw::streak(
        placed(Vec2::new(160.0, 20.0)),
        [255; 4],
        sprites::RIBBON,
        0.5,
        0.25,
    );
    let records = [(Band::Opaque, table()), (Band::Glows, streak)];
    let mut renderer = enhanced();
    let [baseline, enhanced] = both_tiers(&mut renderer, &records);
    let along = |t: f32| 160.0 * t - 80.0;
    let [lit, ..] = enhanced.overhead(along(0.4), 0.0);
    assert!(lit > 64, "0.4 of the way along is {lit}");
    // Beside the ribbon, 0.4 of the way along, the light spreads where Baseline drew none.
    let column = (along(0.4) + 100.0) as u32;
    let beside = (100..200)
        .find(|&y| baseline.at(column, y) == [0, 0, 0])
        .expect("the ribbon ends inside the target");
    let spread = enhanced.at(column, beside);
    assert!(
        brightness(spread) > 0,
        "beside the ribbon, at row {beside}, Enhanced drew {spread:?}"
    );
    // The bloom spreads a few pixels, so the dark points are 0.15 of the length (24 pixels) or
    // more beyond the head and the tail.
    for t in [0.1, 0.65, 0.8] {
        assert_eq!(
            enhanced.overhead(along(t), 0.0),
            [0, 0, 0],
            "{t} of the way along"
        );
    }
}

/// A frame with nothing in the Glows band costs bloom no draw call and draws every pixel as
/// Baseline does. The table covers the whole target, so no edge of it is multisampled.
#[test]
fn a_frame_without_glows_has_no_bloom() {
    let panel = rectangle(Vec2::ZERO, Vec2::splat(40.0), 500.0, [40, 180, 60]);
    let records = [(Band::Opaque, table()), (Band::Layer, panel)];
    let mut renderer = enhanced();
    let [(baseline, baseline_calls), (enhanced, enhanced_calls)] = [Tier::Baseline, Tier::Enhanced]
        .map(|tier| draw_in(tier, &mut renderer, overhead(), (200, 200), &records));
    assert_eq!(enhanced_calls, baseline_calls, "draw calls");
    for y in 0..200 {
        for x in 0..200 {
            let (found, expected) = (enhanced.at(x, y), baseline.at(x, y));
            assert!(
                within(found, expected, 4),
                "at ({x}, {y}) Enhanced drew {found:?} and Baseline {expected:?}"
            );
        }
    }
}

/// The flat layer is drawn after bloom, so a panel over a glow is not lit by it.
#[test]
fn a_panel_over_a_glow_does_not_bloom() {
    let glow = Draw::glow(placed(Vec2::splat(60.0)), [255; 4], sprites::DISC, 0.0);
    let panel = rectangle(Vec2::ZERO, Vec2::splat(50.0), 500.0, [40, 180, 60]);
    let records = [
        (Band::Opaque, table()),
        (Band::Glows, glow),
        (Band::Layer, panel),
    ];
    let mut renderer = enhanced();
    let [baseline, enhanced] = both_tiers(&mut renderer, &records);
    // The panel reaches 25 pixels from the centre; its edge pixels are left out.
    for y in 77..=123 {
        for x in 77..=123 {
            let (found, expected) = (enhanced.at(x, y), baseline.at(x, y));
            assert!(
                within(found, expected, 2),
                "at ({x}, {y}) Enhanced drew {found:?} and Baseline {expected:?}"
            );
        }
    }
}
