//! Tests for timed `"kind": "vfx"` layers, `"subject_colors"` palette resolution, and the
//! state-driven looping VFX hook (`write_state_loop`).

use std::sync::Arc;

use glam::{Affine3A, Vec2, Vec3};
use kit_base::atlas::Tier;
use kit_base::data::effect::{EffectPalette, Effects, SubjectColors, Vfx};
use kit_base::pose::{Curve, Pose};
use kit_frame::frame::{Band, Draw, EffectTarget, ElementId, Frame, FrameBuilder};
use kit_fx::effects::vfx::{StateLoop, unit_hash, write_state_loop};
use kit_fx::effects::{self, EffectRun, Reach};

const PALETTE_JSON: &str = include_str!("../../../../../data/client/effects/palette.json");

const PROBE_EFFECT: &str = r#"{
  "name": "probe-ring",
  "target": "subject",
  "preview": true,
  "layers": [
    {
      "kind": "vfx",
      "vfx": "probe",
      "size": { "from": 0.8, "by": 0.6 },
      "tint": "subject_colors",
      "fade": { "from": 0.0, "by": 1.0 },
      "offset": [0.1, -0.05]
    }
  ]
}"#;

fn load_probe_effects() -> (Effects, EffectPalette) {
    let palette = EffectPalette::parse(PALETTE_JSON).expect("palette.json");
    let effects = Effects::parse([("probe-ring", PROBE_EFFECT)])
        .expect("probe effect parses")
        .with_lift(1.0)
        .with_palette(palette);
    (effects, palette)
}

fn run_probe(effects: &Effects, tier: Tier, colors: SubjectColors, now: u64) -> Frame {
    let definition = Arc::clone(effects.get("probe-ring").expect("probe-ring"));
    let target = EffectTarget::Key(ElementId(7));
    let run = EffectRun {
        definition,
        on: target,
        on_point: false,
        particle_seed: Some(7),
        start: 1_000,
        duration: 10_000,
        curve: Curve::LINEAR,
        step: 0,
    };
    let reach = Reach {
        pose: Pose::default(),
        half: Vec2::new(31.5, 44.0),
        colors,
        settle: kit_base::data::effect::SettleKind::Small,
    };
    let mut frame = Frame::with_capacity(16, 32, 256);
    let mut builder = FrameBuilder::new(&mut frame, tier, Vec2::new(63.0, 88.0));
    effects::write(
        effects,
        &[run],
        now,
        |_, _| Some(reach),
        (Vec3::Z, 1.0),
        &mut builder,
    );
    builder.finish();
    frame
}

fn glows(frame: &Frame) -> &[Draw] {
    let start = frame.bands[Band::Glows as usize] as usize;
    let end = frame.bands[Band::Glows as usize + 1] as usize;
    &frame.draws[start..end]
}

#[test]
fn subject_colors_resolves_mono_red_two_color_and_colorless_from_palette() {
    let (effects, palette) = load_probe_effects();
    let mid = 6_000;

    let red = run_probe(&effects, Tier::Enhanced, SubjectColors::RED, mid);
    assert_eq!(glows(&red).len(), 1);
    let red_draw = &glows(&red)[0];
    assert!(red_draw.is_vfx());
    assert_eq!(red_draw.uv[0], Vfx::Probe as u16);
    assert_eq!(red_draw.tint, palette.red);
    assert!(
        (red_draw.params[0] - 0.5).abs() < 1e-5,
        "normalized progress"
    );
    assert!((red_draw.params[1] - 0.5).abs() < 1e-5, "fade opacity");
    assert!((red_draw.params[3] - 1.0).abs() < 1e-5, "square aspect");

    let two_color = run_probe(
        &effects,
        Tier::Enhanced,
        SubjectColors::WHITE.union(SubjectColors::BLUE),
        mid,
    );
    assert_eq!(glows(&two_color).len(), 1);
    assert_eq!(glows(&two_color)[0].tint, palette.multicolor);

    let colorless = run_probe(&effects, Tier::Enhanced, SubjectColors::NONE, mid);
    assert_eq!(glows(&colorless).len(), 1);
    assert_eq!(glows(&colorless)[0].tint, palette.colorless);

    // `"kind": "vfx"` is Enhanced-only: Baseline draws nothing.
    let baseline = run_probe(&effects, Tier::Baseline, SubjectColors::RED, mid);
    assert!(baseline.draws.is_empty());
    assert!(!baseline.ambient_motion);
}

#[test]
fn state_loop_marks_ambient_motion_in_enhanced_and_is_silent_under_reduced_motion_or_baseline() {
    let seed = unit_hash(3, 0);
    let draw_loop = |tier: Tier, now: u64, reduced_motion: bool| {
        let mut frame = Frame::with_capacity(8, 16, 128);
        let mut builder = FrameBuilder::new(&mut frame, tier, Vec2::new(63.0, 88.0));
        write_state_loop(
            &mut builder,
            Affine3A::IDENTITY,
            &StateLoop {
                kind: Vfx::Probe,
                band: Band::Glows,
                seed,
                now,
                aspect: 1.0,
                tint: [255, 200, 120, 255],
                opacity: 0.85,
                reduced_motion,
            },
        );
        builder.finish();
        frame
    };

    let t1 = draw_loop(Tier::Enhanced, 250_000_000, false);
    let t2 = draw_loop(Tier::Enhanced, 750_000_000, false);
    assert!(t1.ambient_motion, "live host wakes for ambient state loop");
    assert_eq!(glows(&t1).len(), 1);
    assert_eq!(glows(&t1)[0].uv[0], Vfx::Probe as u16);
    assert!(
        glows(&t2)[0].params[0] > glows(&t1)[0].params[0],
        "loop time advances with frame clock"
    );

    let reduced = draw_loop(Tier::Enhanced, 250_000_000, true);
    assert!(
        reduced.draws.is_empty(),
        "reduced motion emits no state loop"
    );
    assert!(
        !reduced.ambient_motion,
        "reduced motion does not request ambient ticks"
    );

    let baseline = draw_loop(Tier::Baseline, 250_000_000, false);
    assert!(baseline.draws.is_empty(), "Baseline emits no state loop");
    assert!(!baseline.ambient_motion);
}

#[test]
fn settle_effect_encodes_mana_tier_and_flying_in_enhanced_and_sprites_in_baseline() {
    use kit_base::data::effect::SettleKind;

    let palette = EffectPalette::parse(PALETTE_JSON).expect("palette.json");
    let settle_json = include_str!("../../../../../data/client/effects/settle.json");
    let effects = Effects::parse([("settle", settle_json)])
        .expect("settle.json")
        .with_lift(1.0)
        .with_palette(palette);
    let definition = Arc::clone(effects.get("settle").expect("settle"));
    let target = EffectTarget::Key(ElementId(7));
    let run = EffectRun {
        definition,
        on: target,
        on_point: false,
        particle_seed: Some(7),
        start: 1_000,
        duration: 10_000,
        curve: Curve::LINEAR,
        step: 0,
    };
    let draw_settle = |tier: Tier, settle: SettleKind| {
        let reach = Reach {
            pose: Pose::default(),
            half: Vec2::new(31.5, 44.0),
            colors: SubjectColors::NONE,
            settle,
        };
        let mut frame = Frame::with_capacity(16, 32, 256);
        let mut builder = FrameBuilder::new(&mut frame, tier, Vec2::new(63.0, 88.0));
        effects::write(
            &effects,
            std::slice::from_ref(&run),
            3_500,
            |_, _| Some(reach),
            (Vec3::new(0.0, 0.0, 400.0), 1.0),
            &mut builder,
        );
        builder.finish();
        frame
    };

    let sm = draw_settle(Tier::Enhanced, SettleKind::Small);
    let lg = draw_settle(Tier::Enhanced, SettleKind::Large);
    let fly = draw_settle(Tier::Enhanced, SettleKind::FlyingMedium);
    assert_eq!(glows(&sm).len(), 1);
    assert_eq!(glows(&sm)[0].uv[0], Vfx::Settle as u16);
    assert_eq!(glows(&sm)[0].uv[3], SettleKind::Small as u16);
    assert_eq!(glows(&lg)[0].uv[3], SettleKind::Large as u16);
    assert_eq!(glows(&fly)[0].uv[3], SettleKind::FlyingMedium as u16);

    let sm_w = Vec3::from_slice(&glows(&sm)[0].model[0..3]).length();
    let lg_w = Vec3::from_slice(&glows(&lg)[0].model[0..3]).length();
    assert!(
        lg_w > sm_w * 1.3,
        "large tier scales quad: {lg_w} vs {sm_w}"
    );

    let base = draw_settle(Tier::Baseline, SettleKind::Small);
    assert!(
        glows(&base).len() >= 2 && glows(&base).iter().all(|d| d.uv[0] != Vfx::Settle as u16),
        "Baseline draws cloud and ring sprites without procedural Vfx"
    );
}

#[test]
fn highlight_emits_shimmer_and_400ms_pop_in_enhanced_and_animates_tint_in_baseline() {
    use glam::Affine3A;
    use kit_frame::frame::vfx::{HIGHLIGHT_POP_NS, HighlightStyle};
    use kit_fx::effects::vfx::{HighlightVfx, baseline_highlight_tint, write_highlight};

    let draw_hl = |tier: Tier, now: u64, age_ns: u64, reduced_motion: bool| {
        let mut frame = Frame::with_capacity(8, 16, 64);
        let mut builder = FrameBuilder::new(&mut frame, tier, Vec2::new(63.0, 88.0));
        let drawn = write_highlight(
            &mut builder,
            &HighlightVfx {
                style: HighlightStyle::Eligible,
                band: Band::TableMarks,
                pop_band: Band::Glows,
                clip: 0,
                seed: 0.25,
                now,
                age_ns,
                base_size: Vec2::new(63.0, 88.0),
                tint: [51, 220, 254, 255],
                reduced_motion,
            },
            |size| Affine3A::from_scale(size.extend(1.0)),
        );
        builder.finish();
        (drawn, frame)
    };

    let (drawn_pop, pop_frame) = draw_hl(Tier::Enhanced, 500_000_000, 100_000_000, false);
    assert!(drawn_pop);
    assert!(pop_frame.ambient_motion);
    assert_eq!(pop_frame.draws.len(), 2, "shimmer + pop during first 400ms");
    assert!(
        pop_frame
            .draws
            .iter()
            .any(|d| d.uv[0] == Vfx::HighlightShimmer as u16)
    );
    assert!(
        pop_frame
            .draws
            .iter()
            .any(|d| d.uv[0] == Vfx::HighlightPop as u16)
    );

    let (drawn_steady, steady_frame) =
        draw_hl(Tier::Enhanced, 1_500_000_000, HIGHLIGHT_POP_NS, false);
    assert!(drawn_steady);
    assert!(steady_frame.ambient_motion);
    assert_eq!(steady_frame.draws.len(), 1, "only shimmer after 400ms pop");
    assert_eq!(steady_frame.draws[0].uv[0], Vfx::HighlightShimmer as u16);

    let (drawn_reduced, reduced_frame) = draw_hl(Tier::Enhanced, 500_000_000, 100_000_000, true);
    assert!(!drawn_reduced);
    assert!(reduced_frame.draws.is_empty());
    assert!(!reduced_frame.ambient_motion);

    let (drawn_base, base_frame) = draw_hl(Tier::Baseline, 500_000_000, 100_000_000, false);
    assert!(!drawn_base);
    assert!(base_frame.draws.is_empty());
    assert!(!base_frame.ambient_motion);

    let base_tint = [51, 220, 254, 255];
    let pop_tint = baseline_highlight_tint(base_tint, 0, 20_000_000, 0.0);
    let steady_tint = baseline_highlight_tint(base_tint, 0, HIGHLIGHT_POP_NS, 0.0);
    assert!(
        pop_tint[0] > steady_tint[0],
        "Baseline tint lifts brighter during the 400ms appear pop: {pop_tint:?} vs {steady_tint:?}"
    );
}
