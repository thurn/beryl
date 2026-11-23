//! Procedural Enhanced-tier combat VFX rendered offscreen: attacker chevrons (unblocked crimson
//! and blocked gold), blocker golden shield dome, time-varying plasma, clean edge fade within
//! `kit_frame::frame::vfx` quad padding, and additive bloom spill.

use glam::{Affine3A, Quat, Vec2, Vec3};
use kit_base::atlas::Tier;
use kit_frame::frame::{Band, Draw, Vfx, vfx};

use super::enhanced::enhanced;
use super::render::{Picture, draw_in, overhead, rectangle};

fn black_table() -> Draw {
    rectangle(Vec2::ZERO, Vec2::splat(400.0), 0.0, [0, 0, 0])
}

fn placed_vfx(kind: Vfx, base_width: f32, height: f32, pad: [f32; 3], params: [f32; 4]) -> Draw {
    let (size, _) = vfx::padded_quad(base_width, height, pad);
    let transform = Affine3A::from_scale_rotation_translation(
        size.extend(1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
    );
    Draw::vfx(transform, [255; 4], kind, params)
}

fn render_vfx(renderer: &mut beryl_render::Renderer, draw: Draw) -> Picture {
    let records = [(Band::Opaque, black_table()), (Band::Glows, draw)];
    draw_in(Tier::Enhanced, renderer, overhead(), (200, 200), &records).0
}

fn channel_sums(picture: &Picture) -> [u64; 3] {
    let mut sums = [0_u64; 3];
    for pixel in picture.pixels.as_chunks::<4>().0 {
        sums[0] += u64::from(pixel[0]);
        sums[1] += u64::from(pixel[1]);
        sums[2] += u64::from(pixel[2]);
    }
    sums
}

#[test]
fn unblocked_and_blocked_chevrons_and_blocker_shield_render_distinct_palettes() {
    let mut renderer = enhanced();
    let aspect = 0.25;
    let unblocked = render_vfx(
        &mut renderer,
        placed_vfx(
            Vfx::Chevron,
            100.0,
            25.0,
            vfx::CHEVRON_PAD,
            [0.5, 1.0, 0.2, aspect],
        ),
    );
    let blocked = render_vfx(
        &mut renderer,
        placed_vfx(
            Vfx::ChevronBlocked,
            100.0,
            25.0,
            vfx::CHEVRON_PAD,
            [0.5, 1.0, 0.2, aspect],
        ),
    );
    let shield = render_vfx(
        &mut renderer,
        placed_vfx(
            Vfx::ShieldDome,
            100.0,
            22.0,
            vfx::SHIELD_PAD,
            [0.5, 1.0, 0.2, 0.22],
        ),
    );

    let [ur, ug, ub] = channel_sums(&unblocked);
    let [br, bg, bb] = channel_sums(&blocked);
    let [sr, sg, sb] = channel_sums(&shield);

    assert!(ur > 100_000, "unblocked chevron lights up strongly: {ur}");
    assert!(
        ur > ug * 2 && ug > ub,
        "unblocked is crimson-orange: {:?}",
        [ur, ug, ub]
    );

    assert!(br > 80_000, "blocked chevron lights up strongly: {br}");
    assert!(
        bg * ur > ug * br,
        "blocked chevron shifts warmer/gold than unblocked: blocked={:?}, unblocked={:?}",
        [br, bg, bb],
        [ur, ug, ub]
    );
    assert!(
        br > bg && bg > bb,
        "blocked chevron is golden: {:?}",
        [br, bg, bb]
    );

    assert!(sr > 80_000, "blocker shield lights up strongly: {sr}");
    assert!(
        sr > sg && sg > sb,
        "blocker shield is golden: {:?}",
        [sr, sg, sb]
    );
}

#[test]
fn combat_vfx_animates_over_time_and_respects_fade_and_quad_edges() {
    let mut renderer = enhanced();
    let t0 = render_vfx(
        &mut renderer,
        placed_vfx(
            Vfx::Chevron,
            100.0,
            25.0,
            vfx::CHEVRON_PAD,
            [0.0, 1.0, 0.3, 0.25],
        ),
    );
    let t1 = render_vfx(
        &mut renderer,
        placed_vfx(
            Vfx::Chevron,
            100.0,
            25.0,
            vfx::CHEVRON_PAD,
            [0.75, 1.0, 0.3, 0.25],
        ),
    );
    let diff = t0
        .pixels
        .iter()
        .zip(&t1.pixels)
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        diff > 500,
        "plasma changes visibly over time: {diff} channel diffs"
    );

    let faded = render_vfx(
        &mut renderer,
        placed_vfx(
            Vfx::ShieldDome,
            100.0,
            22.0,
            vfx::SHIELD_PAD,
            [0.5, 0.0, 0.3, 0.22],
        ),
    );
    assert_eq!(channel_sums(&faded), [0, 0, 0], "zero fade draws nothing");

    // Far corners of the 200x200 target stay dark, confirming no unclipped edge artifacts.
    for (x, y) in [(0, 0), (199, 0), (0, 199), (199, 199)] {
        assert_eq!(t0.at(x, y), [0, 0, 0], "corner ({x}, {y}) stays dark");
    }
}
