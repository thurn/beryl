//! The Enhanced tier's shadow map, checked by single pixels of hand-built frames seen from
//! overhead (`render::overhead`: the point (x, y) is drawn at pixel (x + 100, 100 - y)).
//!
//! The key light points the way the Baseline tier's contact shadows fall: a card at height h
//! casts its shadow's centre at its own centre moved by `Shadow::fall() * h`, so with the light
//! of `data/beryl3d/lighting.json`, 20 mm up puts it 2.8 mm right of and 10 mm below the card's
//! centre.

use beryl_render::{BACK_LAYER, Limits, Renderer};
use glam::{Affine3A, Quat, Vec2, Vec3};
use kit_base::atlas::{FaceSize, Tier};
use kit_frame::frame::{Band, Draw};

use super::render::{Picture, card, draw_in, overhead, rectangle, renderer};

const GREY: [u8; 3] = [180, 180, 180];

/// A renderer with full limits, which must draw the Enhanced tier.
fn enhanced() -> Renderer {
    let renderer = renderer(Limits::Default);
    assert!(renderer.enhanced_available(), "no Enhanced support");
    renderer
}

/// `records` drawn in `tier`, seen from overhead into a 200 by 200 target.
fn draw(renderer: &mut Renderer, tier: Tier, records: &[(Band, Draw)]) -> Picture {
    draw_in(tier, renderer, overhead(), (200, 200), records).0
}

/// A grey table 400 mm square at height 0.
fn table() -> (Band, Draw) {
    (
        Band::Opaque,
        rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, GREY),
    )
}

/// A card 0.15 times the size of a board card, centred at `position`, so that its shadow 20 mm
/// below it falls mostly clear of it as seen from overhead.
fn small_card(position: Vec3) -> (Band, Draw) {
    small_card_looking(position, 255, 0.0)
}

/// `small_card` with an opacity of `alpha` out of 255, dissolved `dissolve` of the way, and in
/// the band the client puts a card that is not opaque.
fn small_card_looking(position: Vec3, alpha: u8, dissolve: f32) -> (Band, Draw) {
    let transform =
        Affine3A::from_scale_rotation_translation(Vec3::splat(0.15), Quat::IDENTITY, position);
    let draw = Draw::card(
        transform,
        [255, 255, 255, alpha],
        FaceSize::Board,
        2,
        BACK_LAYER,
        dissolve,
        0.0,
    );
    let band = if alpha == 255 && dissolve == 0.0 {
        Band::Opaque
    } else {
        Band::TableMarks
    };
    (band, draw)
}

/// Where a point at height `height` above a surface casts its shadow on it, from its own place
/// `at`, as the Baseline tier's `cast` puts it.
fn cast(at: Vec2, height: f32) -> Vec2 {
    at + crate::lighting().shadow.fall() * height
}

/// How much of what a shadow covers it darkens at its fullest, in both tiers.
fn darkness() -> f32 {
    crate::lighting().shadow.darkness
}

/// How much of `lit`'s brightness `shaded` lost, from 0 to 1, over all three channels.
fn darkening(shaded: [u8; 3], lit: [u8; 3]) -> f32 {
    let sum = |rgb: [u8; 3]| rgb.iter().map(|&c| f32::from(c)).sum::<f32>();
    1.0 - sum(shaded) / sum(lit)
}

// Measured on this Mac's Metal adapter, with the light shading only by the shadow: the table,
// [180, 180, 180] unshadowed, is [81, 81, 81] at the lifted card's shadow centre (a darkening
// of 0.55), and unchanged 40 mm the other side.
#[test]
fn a_lifted_card_shadows_the_table_where_cast_puts_its_shadow() {
    let mut renderer = enhanced();
    let lifted = small_card(Vec3::new(0.0, 0.0, 20.0));
    let with = draw(&mut renderer, Tier::Enhanced, &[lifted, table()]);
    let without = draw(&mut renderer, Tier::Enhanced, &[table()]);
    let centre = cast(Vec2::ZERO, 20.0);
    let found = darkening(
        with.overhead(centre.x, centre.y),
        without.overhead(centre.x, centre.y),
    );
    assert!(
        (found - darkness()).abs() <= 0.1 * darkness(),
        "the shadow's centre {centre} darkened by {found}, not {}",
        darkness()
    );
    let away = -centre.normalize() * 40.0;
    assert_eq!(
        with.overhead(away.x, away.y),
        without.overhead(away.x, away.y),
        "the table 40 mm from the card away from its shadow, at {away}, changed"
    );
}

#[test]
fn a_dragged_card_casts_a_shadow_from_the_flat_pass() {
    let mut renderer = enhanced();
    let height = 20.0;
    let held_at = Vec2::new(25.0, 0.0);
    let preview_at = Vec2::new(-25.0, 0.0);
    let overlay_at = Vec2::new(0.0, 25.0);
    // All three cards share one run in the flat pass. Only the held card shades the table.
    let held = (Band::Dragged, small_card(held_at.extend(height)).1);
    let preview = (Band::LayerCards, small_card(preview_at.extend(height)).1);
    let overlay = (Band::Foreground, small_card(overlay_at.extend(height)).1);
    let with = draw(
        &mut renderer,
        Tier::Enhanced,
        &[table(), preview, held, overlay],
    );
    let without = draw(&mut renderer, Tier::Enhanced, &[table()]);
    let centre = cast(held_at, height);
    assert!(
        darkening(
            with.overhead(centre.x, centre.y),
            without.overhead(centre.x, centre.y),
        ) > 0.5 * darkness(),
        "the table at the held card's shadow centre {centre}"
    );
    for at in [preview_at, overlay_at] {
        let centre = cast(at, height);
        assert_eq!(
            with.overhead(centre.x, centre.y),
            without.overhead(centre.x, centre.y),
            "the table under the overlay card at {centre}"
        );
    }
}

// Measured: the lower card's face at the lifted card's shadow centre loses 0.55 of its
// brightness.
#[test]
fn a_lifted_card_shadows_the_card_below_it() {
    let mut renderer = enhanced();
    let lower = (Band::Opaque, card(Vec3::new(0.0, 0.0, 1.0), 1, [255; 4]));
    let lifted = small_card(Vec3::new(0.0, 0.0, 21.0));
    let with = draw(&mut renderer, Tier::Enhanced, &[lifted, lower, table()]);
    let without = draw(&mut renderer, Tier::Enhanced, &[lower, table()]);
    let centre = cast(Vec2::ZERO, 20.0);
    let (shaded, lit) = (
        with.overhead(centre.x, centre.y),
        without.overhead(centre.x, centre.y),
    );
    let found = darkening(shaded, lit);
    assert!(
        found > 0.5 * darkness(),
        "the lower card's face at {centre} went from {lit:?} to {shaded:?}, a darkening of {found}"
    );
}

/// A card lying on the table casts onto nothing it can see but its own face, which the depth
/// biases keep it from shadowing: no pixel of its face is darker than in the Baseline tier. The
/// Enhanced lighting draws a card lying flat and unshadowed in its Baseline color but for the
/// sheen, which may add up to its strength in white.
#[test]
fn a_card_lying_flat_does_not_shadow_its_own_face() {
    let mut renderer = enhanced();
    let lying = (Band::Opaque, card(Vec3::new(0.0, 0.0, 1.0), 1, [255; 4]));
    let records = [lying, table()];
    let enhanced = draw(&mut renderer, Tier::Enhanced, &records);
    let baseline = draw(&mut renderer, Tier::Baseline, &records);
    let most_sheen = (crate::lighting().enhanced.light.sheen * 255.0).ceil() as i16 + 3;
    for (x, y) in [(0.0, 0.0), (-20.0, 30.0), (20.0, -30.0), (25.0, 35.0)] {
        let (found, expected) = (enhanced.overhead(x, y), baseline.overhead(x, y));
        assert!(
            found
                .iter()
                .zip(expected)
                .all(|(&a, b)| (-3..=most_sheen).contains(&(i16::from(a) - i16::from(b)))),
            "the card's face at ({x}, {y}) is {found:?} in Enhanced and {expected:?} in Baseline"
        );
    }
}

/// A card drawn less than half opaque, as a phased-out card is, casts no shadow, as the
/// Baseline tier's contact shadow under it fades with its opacity.
#[test]
fn a_card_less_than_half_opaque_casts_no_shadow() {
    let mut renderer = enhanced();
    let faded = small_card_looking(Vec3::new(0.0, 0.0, 20.0), 100, 0.0);
    let with = draw(&mut renderer, Tier::Enhanced, &[faded, table()]);
    let without = draw(&mut renderer, Tier::Enhanced, &[table()]);
    let centre = cast(Vec2::ZERO, 20.0);
    assert_eq!(
        with.overhead(centre.x, centre.y),
        without.overhead(centre.x, centre.y),
        "the table at the faded card's shadow centre {centre}"
    );
}

/// A card half dissolved has broken up from its top down, so only its lower half casts: 40 mm
/// up, the table under the shadow of its top quarter is lit, and under its bottom quarter it is
/// shaded as a whole card shades it.
#[test]
fn a_half_dissolved_card_casts_only_its_lower_half() {
    let mut renderer = enhanced();
    let height = 40.0;
    let at = Vec3::new(0.0, 0.0, height);
    let without = draw(&mut renderer, Tier::Enhanced, &[table()]);
    let whole = draw(&mut renderer, Tier::Enhanced, &[small_card(at), table()]);
    let half = small_card_looking(at, 255, 0.5);
    let dissolved = draw(&mut renderer, Tier::Enhanced, &[half, table()]);
    // The small card is 13.2 mm tall, so its quarters are 3.3 mm from its centre.
    let (top, bottom) = (
        cast(Vec2::new(0.0, 3.3), height),
        cast(Vec2::new(0.0, -3.3), height),
    );
    assert!(
        darkening(whole.overhead(top.x, top.y), without.overhead(top.x, top.y)) > 0.4,
        "a whole card shades the table at {top}"
    );
    assert_eq!(
        dissolved.overhead(top.x, top.y),
        without.overhead(top.x, top.y),
        "the shadow of the dissolved top quarter, at {top}"
    );
    assert_eq!(
        dissolved.overhead(bottom.x, bottom.y),
        whole.overhead(bottom.x, bottom.y),
        "the shadow of the bottom quarter, at {bottom}"
    );
}

#[test]
fn the_same_frame_draws_the_same_shadows_twice() {
    let mut renderer = enhanced();
    let records = [small_card(Vec3::new(0.0, 0.0, 20.0)), table()];
    let [first, second] = [0, 1].map(|_| draw(&mut renderer, Tier::Enhanced, &records));
    for y in (-30..=10).step_by(2) {
        for x in (-20..=20).step_by(2) {
            let (x, y) = (x as f32, y as f32);
            assert_eq!(first.overhead(x, y), second.overhead(x, y), "at ({x}, {y})");
        }
    }
}

/// The shadow map's darkness and size come from the tuning the renderer is handed, and a
/// renderer handed new tuning draws with it: after `set_lighting` with a lighter shadow and a
/// smaller map, as a watched edit of `lighting.json` gives, the table at the shadow's centre
/// darkens by the new amount.
#[test]
fn a_renderer_handed_new_lighting_shades_with_it() {
    let mut renderer = enhanced();
    let mut lighting = crate::lighting();
    lighting.shadow.darkness = 0.3;
    lighting.enhanced.shadow_map.size = 1024;
    renderer.set_lighting(lighting);
    let lifted = small_card(Vec3::new(0.0, 0.0, 20.0));
    let with = draw(&mut renderer, Tier::Enhanced, &[lifted, table()]);
    let without = draw(&mut renderer, Tier::Enhanced, &[table()]);
    let centre = cast(Vec2::ZERO, 20.0);
    let found = darkening(
        with.overhead(centre.x, centre.y),
        without.overhead(centre.x, centre.y),
    );
    assert!(
        (found - 0.3).abs() <= 0.03,
        "the shadow's centre {centre} darkened by {found}, not 0.3"
    );
}
