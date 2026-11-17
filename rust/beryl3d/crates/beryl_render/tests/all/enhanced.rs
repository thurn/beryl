//! The Enhanced tier's renderer drawing known scenes offscreen, against the same scenes drawn in
//! Baseline. A test fails when the adapter has no Enhanced support, rather than passing without
//! drawing; this Mac's Metal adapter has it. wgpu's default handler for an uncaptured error
//! panics, so a validation error fails the test.

use beryl_render::{Limits, Renderer};
use beryl_scene::data::space::enhanced::light;
use beryl_scene::frame::{Band, Draw, sprites};
use beryl_scene::input::Tier;
use beryl_scene::table::{CARD_HEIGHT, CARD_WIDTH};
use glam::{Affine3A, Quat, Vec2, Vec3};

use super::render::{Picture, TwentyCards, draw_in, overhead, rectangle, renderer, twenty_cards};

/// How far a channel of an Enhanced pixel may be from Baseline's where the tiers agree, out of
/// 255.
const TOLERANCE: u8 = 4;

pub(super) fn within(found: [u8; 3], expected: [u8; 3], tolerance: u8) -> bool {
    found
        .iter()
        .zip(expected)
        .all(|(&a, b)| a.abs_diff(b) <= tolerance)
}

/// A table-material rectangle `size` millimetres large at height `z`, centred on the origin and
/// turned 30 degrees: one solid color, on the card mesh with its rounded corners.
fn turned(size: Vec2, z: f32, rgb: [u8; 3]) -> Draw {
    let transform = Affine3A::from_scale_rotation_translation(
        Vec3::new(size.x / CARD_WIDTH, size.y / CARD_HEIGHT, 1.0),
        Quat::from_rotation_z(30_f32.to_radians()),
        Vec3::new(0.0, 0.0, z),
    );
    Draw::table(transform, [rgb[0], rgb[1], rgb[2], 255])
}

/// A renderer with full limits, which must draw the Enhanced tier.
pub(super) fn enhanced() -> Renderer {
    let renderer = renderer(Limits::Default);
    assert!(
        renderer.enhanced_available(),
        "the adapter {:?} has no Enhanced support",
        renderer.adapter()
    );
    renderer
}

/// `records` drawn in each tier, seen from overhead into a 200 by 200 target.
pub(super) fn both_tiers(renderer: &mut Renderer, records: &[(Band, Draw)]) -> [Picture; 2] {
    [Tier::Baseline, Tier::Enhanced]
        .map(|tier| draw_in(tier, renderer, overhead(), (200, 200), records).0)
}

#[test]
fn twenty_cards_draw_in_enhanced_as_in_baseline_away_from_edges() {
    let TwentyCards {
        camera,
        width,
        height,
        records,
        cards,
        cloth: (cloth_point, _),
        ..
    } = twenty_cards();
    // The Enhanced tier's draw lists hold no contact shadows.
    let lit: Vec<(Band, Draw)> = records
        .iter()
        .copied()
        .filter(|&(band, _)| band != Band::ContactShadows)
        .collect();
    let mut renderer = enhanced();
    let size = (width, height);
    let (baseline, _) = draw_in(
        Tier::Baseline,
        &mut renderer,
        camera.view_projection,
        size,
        &records,
    );
    let (enhanced, _) = draw_in(
        Tier::Enhanced,
        &mut renderer,
        camera.view_projection,
        size,
        &lit,
    );
    let pixels = |point: Vec3| {
        let p = camera.to_screen(point);
        let (x, y) = (p.x as u32, p.y as u32);
        ((x, y), enhanced.at(x, y), baseline.at(x, y))
    };
    // The table, lying flat in the open, is lit to exactly its Baseline color.
    for point in [cloth_point, Vec3::new(300.0, 200.0, -0.5)] {
        let ((x, y), found, expected) = pixels(point);
        assert!(
            within(found, expected, TOLERANCE),
            "at {point} (pixel {x}, {y}) Enhanced drew {found:?} and Baseline {expected:?}"
        );
    }
    // So is a card lying flat, but for the sheen: the same white added to every channel, no
    // more than the sheen's strength. The desktop camera is near where a flat card mirrors the
    // key light, so the sheen there is near its strongest.
    let most_sheen = (light::SHEEN * 255.0).ceil() as i16 + i16::from(TOLERANCE);
    for point in [0, 3, 7, 12, 19].map(|i| cards[i]) {
        let ((x, y), found, expected) = pixels(point);
        let added: [i16; 3] = std::array::from_fn(|i| i16::from(found[i]) - i16::from(expected[i]));
        let even = added.iter().max().zip(added.iter().min());
        assert!(
            added
                .iter()
                .all(|&channel| (-i16::from(TOLERANCE)..=most_sheen).contains(&channel))
                && even.is_some_and(|(most, least)| most - least <= i16::from(TOLERANCE)),
            "at {point} (pixel {x}, {y}) Enhanced drew {found:?} and Baseline {expected:?}"
        );
    }
}

/// A card-shaped record of one color turned 30 degrees on a black table: Baseline draws every
/// pixel in one of the two colors, and Enhanced's four samples blend them along the edge. The
/// record is of the table material, whose color is its tint, so that a pixel of neither color
/// can only come from the edge.
#[test]
fn enhanced_blends_the_edge_of_a_turned_card() {
    let red = [200, 40, 40];
    let black = [0, 0, 0];
    let turned = turned(Vec2::new(63.0, 88.0), 1.0, red);
    let table = rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, black);
    let records = [(Band::Opaque, turned), (Band::Opaque, table)];
    let mut renderer = enhanced();
    let [baseline, enhanced] = both_tiers(&mut renderer, &records);
    let neither = |picture: &Picture| {
        (0..200)
            .flat_map(|y| (0..200).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let pixel = picture.at(x, y);
                !within(pixel, red, 2) && !within(pixel, black, 2)
            })
            .count()
    };
    assert_eq!(
        neither(&baseline),
        0,
        "Baseline drew a pixel of neither color"
    );
    let blended = neither(&enhanced);
    // The rectangle's outline is about 300 pixels long.
    assert!(
        blended > 100,
        "Enhanced blended {blended} pixels along the edge"
    );
}

/// A renderer on the floor's limits has no Enhanced tier, and draws a frame in it exactly as
/// it draws the frame in Baseline.
#[test]
fn a_renderer_without_enhanced_draws_an_enhanced_frame_in_baseline() {
    let TwentyCards {
        camera,
        width,
        height,
        records,
        ..
    } = twenty_cards();
    let mut renderer = renderer(Limits::Downlevel);
    assert!(!renderer.enhanced_available());
    let [baseline, enhanced] = [Tier::Baseline, Tier::Enhanced].map(|tier| {
        draw_in(
            tier,
            &mut renderer,
            camera.view_projection,
            (width, height),
            &records,
        )
        .0
    });
    let different = baseline
        .pixels
        .iter()
        .zip(&enhanced.pixels)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(different, 0, "bytes differ between the tiers");
}

/// A flat-layer panel turned 30 degrees over a card-sized rectangle is drawn after the resolve,
/// with one sample: its pixels, edges included, are Baseline's. The rectangle is of the table
/// material, which a flat surface in the open lights to exactly its Baseline color, where a card
/// would take the sheen.
#[test]
fn the_flat_layer_is_drawn_after_the_resolve() {
    let lying = rectangle(Vec2::ZERO, Vec2::new(63.0, 88.0), 1.0, [230, 200, 120]);
    let table = rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, [0, 0, 0]);
    let panel = turned(Vec2::splat(30.0), 500.0, [40, 180, 60]);
    let records = [
        (Band::Opaque, lying),
        (Band::Opaque, table),
        (Band::Layer, panel),
    ];
    let mut renderer = enhanced();
    let [baseline, enhanced] = both_tiers(&mut renderer, &records);
    // The turned panel reaches about 20.5 pixels from the centre, and the rectangle 31.5 and 44,
    // so the rectangle lies under every pixel compared.
    for y in 79..=121 {
        for x in 79..=121 {
            let (found, expected) = (enhanced.at(x, y), baseline.at(x, y));
            assert!(
                within(found, expected, 2),
                "at ({x}, {y}) Enhanced drew {found:?} and Baseline {expected:?}"
            );
        }
    }
}

#[test]
fn a_particle_lights_its_centre_while_young_and_nothing_when_old() {
    let table = rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, [0, 0, 0]);
    let particle = |age: f32| {
        Draw::particle(
            Affine3A::from_scale_rotation_translation(
                Vec3::new(160.0, 160.0, 1.0),
                Quat::IDENTITY,
                Vec3::new(0.0, 0.0, 1.0),
            ),
            [255; 4],
            sprites::DISC,
            age,
            0.0,
        )
    };
    let mut renderer = enhanced();
    let mut centre = |age: f32| {
        let records = [(Band::Opaque, table), (Band::Particles, particle(age))];
        draw_in(
            Tier::Enhanced,
            &mut renderer,
            overhead(),
            (200, 200),
            &records,
        )
        .0
        .overhead(0.0, 0.0)
    };
    let [young, ..] = centre(0.0);
    assert!(young >= 255 - TOLERANCE, "at age 0 the centre is {young}");
    assert_eq!(centre(1.0), [0, 0, 0], "at age 1 the centre is lit");
}
