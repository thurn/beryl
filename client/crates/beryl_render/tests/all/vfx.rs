//! Procedural Enhanced-tier combat VFX rendered offscreen: attacker chevrons (unblocked crimson
//! and blocked gold), blocker golden shield dome, time-varying plasma, clean edge fade within
//! `kit_frame::frame::vfx` quad padding, and additive bloom spill.

use glam::{Affine3A, Quat, Vec2, Vec3};
use kit_base::atlas::Tier;
use kit_frame::frame::{Band, Draw, LineStyle, Vfx, vfx};

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

fn render_layer_vfx(renderer: &mut beryl_render::Renderer, draw: Draw) -> Picture {
    let records = [(Band::Opaque, black_table()), (Band::LayerCover, draw)];
    draw_in(Tier::Enhanced, renderer, overhead(), (200, 200), &records).0
}

#[test]
fn enhanced_line_ribbons_and_spearheads_render_distinct_arena_palettes_in_layer_cover() {
    let mut renderer = enhanced();
    let ribbon_tf = Affine3A::from_scale_rotation_translation(
        Vec3::new(140.0, 44.0, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
    );
    let mut ribbon = |style: LineStyle, time_s: f32, opacity: f32| {
        render_layer_vfx(
            &mut renderer,
            Draw::vfx_line_ribbon(
                ribbon_tf,
                opacity,
                style,
                [1.0, 0.85],
                [0.0, 0.0],
                [time_s, 0.15, 0.85, 8.0],
            ),
        )
    };

    let aim = channel_sums(&ribbon(LineStyle::Aim, 0.5, 1.0));
    let friendly = channel_sums(&ribbon(LineStyle::FriendlyTarget, 0.5, 1.0));
    let attack = channel_sums(&ribbon(LineStyle::Attack, 0.5, 1.0));
    let block = channel_sums(&ribbon(LineStyle::Block, 0.5, 1.0));
    let source = channel_sums(&ribbon(LineStyle::Source, 0.5, 1.0));

    assert!(
        aim[2] > aim[0] * 2 && aim[1] > aim[0],
        "aim is cyan: {aim:?}"
    );
    assert!(
        friendly[1] > friendly[0] * 2 && friendly[1] > friendly[2],
        "friendly target is green: {friendly:?}"
    );
    assert!(
        attack[0] > attack[1] * 3 / 2 && attack[0] > attack[2] && attack[2] > attack[1],
        "attack beam is rose-crimson: {attack:?}"
    );
    assert!(
        block[0] > block[1] && block[1] > block[2] * 3 / 2,
        "block beam is solar gold: {block:?}"
    );
    assert!(
        source[1] > source[0] && source[1] < aim[1],
        "source line is delicate pale teal-silver: source={source:?}, aim={aim:?}"
    );

    // Scrolling wisps change over time, and zero opacity draws nothing.
    let t0 = ribbon(LineStyle::Aim, 0.0, 1.0);
    let t1 = ribbon(LineStyle::Aim, 0.75, 1.0);
    let diff = t0
        .pixels
        .iter()
        .zip(&t1.pixels)
        .filter(|(a, b)| a != b)
        .count();
    assert!(diff > 400, "line plasma animates over time: {diff}");
    assert_eq!(
        channel_sums(&ribbon(LineStyle::Aim, 0.5, 0.0)),
        [0, 0, 0],
        "zero opacity draws nothing"
    );

    // Faceted spearhead lights up in the matching palette and fades before quad corners.
    let head_tf = Affine3A::from_scale_rotation_translation(
        Vec3::new(90.0, 50.0, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
    );
    let head = render_layer_vfx(
        &mut renderer,
        Draw::vfx_line_head(
            head_tf,
            1.0,
            LineStyle::Aim,
            [0.5, 1.0, 0.2, vfx::LINE_HEAD_ASPECT],
        ),
    );
    let head_sums = channel_sums(&head);
    assert!(
        head_sums[2] > 50_000 && head_sums[2] > head_sums[0] * 2,
        "cyan spearhead renders strongly in layer cover: {head_sums:?}"
    );
    for (x, y) in [(0, 0), (199, 0), (0, 199), (199, 199)] {
        assert_eq!(
            head.at(x, y),
            [0, 0, 0],
            "head corner ({x}, {y}) stays dark"
        );
    }
}

fn mean_radius(picture: &Picture) -> f64 {
    let width = picture.width;
    let height = u32::try_from(picture.pixels.len() / 4).expect("pixel count") / width;
    let cx = (f64::from(width) - 1.0) * 0.5;
    let cy = (f64::from(height) - 1.0) * 0.5;
    let mut weighted = 0.0_f64;
    let mut total = 0.0_f64;
    for y in 0..height {
        for x in 0..width {
            let [r, g, b] = picture.at(x, y);
            let lum = f64::from(r) + f64::from(g) + f64::from(b);
            if lum > 0.0 {
                let dx = f64::from(x) - cx;
                let dy = f64::from(y) - cy;
                weighted += lum * dx.hypot(dy);
                total += lum;
            }
        }
    }
    if total == 0.0 { 0.0 } else { weighted / total }
}

#[test]
fn probe_vfx_expands_ring_with_progress_and_respects_tint_and_fade() {
    let mut renderer = enhanced();
    let tf = Affine3A::from_scale_rotation_translation(
        Vec3::new(140.0, 140.0, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
    );
    let tint = [255, 88, 46, 255];
    let early = render_vfx(
        &mut renderer,
        Draw::vfx(tf, tint, Vfx::Probe, [0.1, 1.0, 0.2, 1.0]),
    );
    let mid = render_vfx(
        &mut renderer,
        Draw::vfx(tf, tint, Vfx::Probe, [0.5, 1.0, 0.2, 1.0]),
    );
    let late = render_vfx(
        &mut renderer,
        Draw::vfx(tf, tint, Vfx::Probe, [0.9, 1.0, 0.2, 1.0]),
    );

    for (label, pic) in [("early", &early), ("mid", &mid), ("late", &late)] {
        let [r, g, b] = channel_sums(pic);
        assert!(
            r > 20_000 && r > g && g > b,
            "{label} probe lights up in warm red-orange tint: {:?}",
            [r, g, b]
        );
        for (x, y) in [(0, 0), (199, 0), (0, 199), (199, 199)] {
            assert_eq!(
                pic.at(x, y),
                [0, 0, 0],
                "{label} corner ({x}, {y}) stays dark"
            );
        }
    }

    let r_early = mean_radius(&early);
    let r_mid = mean_radius(&mid);
    let r_late = mean_radius(&late);
    assert!(
        r_early + 8.0 < r_mid && r_mid + 8.0 < r_late,
        "ring radius expands with progress: early={r_early:.1}, mid={r_mid:.1}, late={r_late:.1}"
    );

    let faded = render_vfx(
        &mut renderer,
        Draw::vfx(tf, tint, Vfx::Probe, [0.5, 0.0, 0.2, 1.0]),
    );
    assert_eq!(channel_sums(&faded), [0, 0, 0], "zero fade draws nothing");
}

#[test]
fn settle_vfx_renders_mana_tiers_and_flying_wind() {
    let mut renderer = enhanced();
    let tf = Affine3A::from_scale_rotation_translation(
        Vec3::new(160.0, 216.0, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
    );
    let tint = [255, 198, 127, 255];
    let sm = render_vfx(
        &mut renderer,
        Draw::vfx_styled(tf, tint, Vfx::Settle, 0, [0.22, 1.0, 0.3, 1.35]),
    );
    let lg = render_vfx(
        &mut renderer,
        Draw::vfx_styled(tf, tint, Vfx::Settle, 2, [0.22, 1.0, 0.3, 1.35]),
    );
    let fly = render_vfx(
        &mut renderer,
        Draw::vfx_styled(tf, tint, Vfx::Settle, 3, [0.22, 1.0, 0.3, 1.35]),
    );

    let [sr, sg, sb] = channel_sums(&sm);
    let [lr, lg_c, _] = channel_sums(&lg);
    let [_, _, fb] = channel_sums(&fly);
    assert!(
        sr > 20_000 && sr > sg && sg > sb,
        "warm dust: {sr},{sg},{sb}"
    );
    assert!(lr > sr && lg_c > sg, "large tier throws brighter dust");
    assert!(
        fb > sb,
        "flying variant adds cooler wind wisps: {fb} > {sb}"
    );
}

#[test]
fn highlight_shimmer_and_pop_render_cyan_outlines_with_clean_edges() {
    let mut renderer = enhanced();
    let shimmer_tf = Affine3A::from_scale_rotation_translation(
        Vec3::new(100.0, 132.0, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
    );
    let pop_tf = Affine3A::from_scale_rotation_translation(
        Vec3::new(112.0, 144.0, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
    );
    let cyan = [51, 220, 254, 255];
    let t0 = render_vfx(
        &mut renderer,
        Draw::vfx_styled(
            shimmer_tf,
            cyan,
            Vfx::HighlightShimmer,
            0,
            [0.0, 1.0, 0.2, 1.32],
        ),
    );
    let t1 = render_vfx(
        &mut renderer,
        Draw::vfx_styled(
            shimmer_tf,
            cyan,
            Vfx::HighlightShimmer,
            0,
            [1.4, 1.0, 0.2, 1.32],
        ),
    );
    let pop = render_vfx(
        &mut renderer,
        Draw::vfx_styled(pop_tf, cyan, Vfx::HighlightPop, 0, [0.22, 1.0, 0.2, 1.29]),
    );

    for (label, pic) in [("shimmer", &t0), ("pop", &pop)] {
        let [r, g, b] = channel_sums(pic);
        assert!(
            g > 40_000 && b > 40_000 && g > r && b > r,
            "{label} renders saturated cyan-mint: {:?}",
            [r, g, b]
        );
        assert_eq!(
            pic.at(100, 100),
            [0, 0, 0],
            "{label} keeps card center clear"
        );
        for (x, y) in [(0, 0), (199, 0), (0, 199), (199, 199)] {
            assert_eq!(
                pic.at(x, y),
                [0, 0, 0],
                "{label} corner ({x},{y}) stays dark"
            );
        }
    }
    assert_ne!(t0.pixels, t1.pixels, "shimmer animates smoothly over time");
}

#[test]
fn mythic_arrival_vfx_renders_tinted_glow_flare_and_smolder() {
    let mut renderer = enhanced();
    let tf = Affine3A::from_scale_rotation_translation(
        Vec3::new(220.0, 297.0, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
    );
    let blue = render_vfx(
        &mut renderer,
        Draw::vfx(
            tf,
            [64, 168, 255, 255],
            Vfx::MythicArrival,
            [0.22, 1.0, 0.3, 1.35],
        ),
    );
    let red = render_vfx(
        &mut renderer,
        Draw::vfx(
            tf,
            [255, 88, 46, 255],
            Vfx::MythicArrival,
            [0.22, 1.0, 0.3, 1.35],
        ),
    );
    let late = render_vfx(
        &mut renderer,
        Draw::vfx(
            tf,
            [64, 168, 255, 255],
            Vfx::MythicArrival,
            [0.72, 1.0, 0.3, 1.35],
        ),
    );

    let [br, _, bb] = channel_sums(&blue);
    let [rr, _, rb] = channel_sums(&red);
    let [_, _, lb] = channel_sums(&late);
    assert!(bb > br && bb > 25_000, "blue mythic glow: {br},{bb}");
    assert!(rr > rb && rr > 25_000, "red mythic glow: {rr},{rb}");
    assert!(
        lb > 1_000 && lb < bb,
        "smoldering residue lingers and fades: {lb} < {bb}"
    );
}

#[test]
fn mana_birth_and_hit_take_the_mana_color_and_stay_inside_their_quads() {
    let mut renderer = enhanced();
    let tf = Affine3A::from_scale_rotation_translation(
        Vec3::new(180.0, 180.0, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
    );
    let blue = [64, 168, 255, 255];
    let red = [255, 88, 46, 255];
    let shade = |renderer: &mut beryl_render::Renderer, kind, tint, t| {
        render_vfx(renderer, Draw::vfx(tf, tint, kind, [t, 1.0, 0.4, 1.0]))
    };
    for kind in [Vfx::ManaBirth, Vfx::ManaHit] {
        let [br, _, bb] = channel_sums(&shade(&mut renderer, kind, blue, 0.2));
        let [rr, _, rb] = channel_sums(&shade(&mut renderer, kind, red, 0.2));
        assert!(bb > br && rr > rb, "{kind:?} takes the mana's color");
        let pic = shade(&mut renderer, kind, blue, 0.2);
        for (x, y) in [(0, 0), (199, 0), (0, 199), (199, 199)] {
            assert_eq!(
                pic.at(x, y),
                [0, 0, 0],
                "{kind:?} corner ({x},{y}) stays dark"
            );
        }
        let late = channel_sums(&shade(&mut renderer, kind, blue, 0.98));
        let early = channel_sums(&shade(&mut renderer, kind, blue, 0.2));
        assert!(
            late.iter().sum::<u64>() * 10 < early.iter().sum::<u64>(),
            "{kind:?} is nearly gone by its end: {late:?} against {early:?}"
        );
    }
    let hit = shade(&mut renderer, Vfx::ManaHit, red, 0.08);
    let [r, g, b] = hit.at(100, 100);
    assert!(
        r > 200 && g > 200 && b > 150,
        "the hit's flare is white in the middle: {:?}",
        [r, g, b]
    );
}

#[test]
fn stack_flares_stay_inside_their_quads_flash_gold_and_are_gone_by_their_end() {
    let mut renderer = enhanced();
    // A quad 1.7 card widths across and 1.4 times as tall as wide, as the effect files draw it.
    let tf = Affine3A::from_scale_rotation_translation(
        Vec3::new(180.0, 180.0, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 1.0),
    );
    let gold = [255, 205, 100, 255];
    let grey = [190, 192, 200, 255];
    let shade = |renderer: &mut beryl_render::Renderer, kind, tint, t| {
        render_vfx(renderer, Draw::vfx(tf, tint, kind, [t, 1.0, 1.4, 1.0]))
    };
    for (kind, tint) in [(Vfx::StackEnter, gold), (Vfx::StackExit, grey)] {
        let pic = shade(&mut renderer, kind, tint, 0.2);
        for (x, y) in [(0, 0), (199, 0), (0, 199), (199, 199)] {
            assert_eq!(
                pic.at(x, y),
                [0, 0, 0],
                "{kind:?} corner ({x},{y}) stays dark"
            );
        }
        let late = channel_sums(&shade(&mut renderer, kind, tint, 0.98));
        let early = channel_sums(&pic);
        assert!(
            late.iter().sum::<u64>() * 10 < early.iter().sum::<u64>(),
            "{kind:?} is nearly gone by its end: {late:?} against {early:?}"
        );
    }
    // The arrival's flash is gold, though its rim runs white: red leads green, green leads blue.
    let [r, g, b] = channel_sums(&shade(&mut renderer, Vfx::StackEnter, gold, 0.05));
    assert!(
        r > g && g * 10 > b * 12,
        "the flash is gold: {:?}",
        [r, g, b]
    );
    // The flash is up at once and gone by the half, while the exit's puff is dimmer than it.
    let flash = channel_sums(&shade(&mut renderer, Vfx::StackEnter, gold, 0.05));
    let spent = channel_sums(&shade(&mut renderer, Vfx::StackEnter, gold, 0.6));
    assert!(spent.iter().sum::<u64>() * 3 < flash.iter().sum::<u64>());
    let puff = channel_sums(&shade(&mut renderer, Vfx::StackExit, grey, 0.05));
    assert!(puff.iter().sum::<u64>() < flash.iter().sum::<u64>());
}

#[test]
fn mythic_stack_aura_hugs_the_card_in_its_colors_and_its_burst_fades() {
    let mut renderer = enhanced();
    // A card 120 wide padded by 0.22 of its width on each side, and 0.55 for the burst.
    let aura_size = Vec3::new(120.0 * 1.44, 120.0 * 1.397 + 120.0 * 0.44, 1.0);
    let burst_size = Vec3::new(120.0 * 2.1, 120.0 * 1.397 + 120.0 * 1.1, 1.0);
    let place = |size: Vec3| {
        Affine3A::from_scale_rotation_translation(size, Quat::IDENTITY, Vec3::new(0.0, 0.0, 1.0))
    };
    let aspect = |size: Vec3| size.y / size.x;
    let blue = [64, 168, 255, 255];
    let violet = [154, 92, 214, 255];
    let aura = |renderer: &mut beryl_render::Renderer, tint, t| {
        let draw = Draw::vfx(
            place(aura_size),
            tint,
            Vfx::MythicStack,
            [t, 1.0, 0.3, aspect(aura_size)],
        );
        render_vfx(renderer, draw)
    };
    let [br, _, bb] = channel_sums(&aura(&mut renderer, blue, 12.0));
    let [vr, vg, _] = channel_sums(&aura(&mut renderer, violet, 12.0));
    assert!(
        bb > br && bb > 20_000,
        "a blue spell's aura is blue: {br},{bb}"
    );
    assert!(vr > vg, "a black spell's aura is violet: {vr},{vg}");
    let early = aura(&mut renderer, blue, 12.0);
    // The card's face stays readable: its middle takes almost no light.
    let [r, g, b] = early.at(100, 100);
    assert!(
        r + g + b < 60,
        "the card's middle stays clear: {:?}",
        [r, g, b]
    );
    assert_ne!(
        early.pixels,
        aura(&mut renderer, blue, 13.3).pixels,
        "the aura moves as it loops"
    );
    let burst = |renderer: &mut beryl_render::Renderer, t| {
        let draw = Draw::vfx(
            place(burst_size),
            blue,
            Vfx::MythicStackOutro,
            [t, 1.0, 0.3, aspect(burst_size)],
        );
        channel_sums(&render_vfx(renderer, draw))
            .iter()
            .sum::<u64>()
    };
    let flash = burst(&mut renderer, 0.1);
    let end = burst(&mut renderer, 0.97);
    assert!(flash > 30_000, "the burst flares: {flash}");
    assert!(
        end * 10 < flash,
        "the burst is nearly gone by its end: {end} against {flash}"
    );
}
