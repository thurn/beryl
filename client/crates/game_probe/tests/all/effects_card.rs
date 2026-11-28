//! The effects on a card's own record (docs/beryl-3d.md, "Effects"): `settle`, `dissolve`,
//! `dim`, `blanch` and `cover` change how the card is drawn, and `shake` moves the whole
//! picture and nothing that the checks or the pointer see.

use beryl_protocol::script::{Edge, StepEffect};
use game_table::table::Look;
use glam::Vec2;
use kit_base::pose::{Curve, Pose};
use kit_frame::frame::{EffectTarget, ElementId, Frame, Material};
use kit_fx::effects::{EffectRun, card, shake};
use kit_ui::input::Tier;

use crate::common::{MS, empty_frame, frame};
use crate::effects::{effect, started};

const ON: EffectTarget = EffectTarget::Key(ElementId(7));
/// The faces of the record before and after a covered card changes.
const EARLIER: Option<u16> = Some(3);
const LATER: Option<u16> = Some(4);

/// A run of `effect` on `ON` from 1 s for 1 s, linear.
fn run(effect: &str) -> EffectRun {
    EffectRun {
        definition: crate::test_data::data()
            .effects
            .get(effect)
            .unwrap()
            .clone(),
        on: ON,
        on_point: false,
        particle_seed: Some(7),
        colors: None,
        start: 1_000 * MS,
        duration: 1_000 * MS,
        curve: Curve::LINEAR,
        step: 0,
    }
}

/// How a plain card showing `LATER` looks with one run of `effect` at `share` of the way through.
fn look_at(effect: &str, share: f32) -> Look {
    let mut look = Look {
        pose: Pose::default(),
        tint: [255, 255, 255],
        dissolve: 0.0,
        whiten: 0.0,
        face: LATER,
        full: None,
        state: 0,
        interactive: false,
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a share from 0 to 1 of a second"
    )]
    let now = 1_000 * MS + (f64::from(share) * 1e9).round() as u64;
    card::look(Tier::Baseline, &[run(effect)], ON, now, EARLIER, &mut look);
    look
}

/// Whether `found` is within a rounding error of `expected`.
fn near(found: f32, expected: f32) -> bool {
    (found - expected).abs() < 0.01
}

/// The card's `params` (dissolve, whiten), tint and opacity with `effect` at each of `shares`.
fn check(effect: &str, expected: &[(f32, f32, f32, u8, f32)]) {
    for &(share, dissolve, whiten, tint, opacity) in expected {
        let look = look_at(effect, share);
        assert!(
            near(look.dissolve, dissolve)
                && near(look.whiten, whiten)
                && look.tint[0].abs_diff(tint) <= 1
                && near(look.pose.opacity, opacity),
            "{effect:?} at {share}: dissolve {}, whiten {}, tint {:?}, opacity {}; expected \
             {dissolve}, {whiten}, {tint}, {opacity}",
            look.dissolve,
            look.whiten,
            look.tint,
            look.pose.opacity
        );
    }
}

/// 255 times `share`, as a tint channel.
fn channel(share: f32) -> u8 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a channel from 0 to 255"
    )]
    let channel = (255.0 * share).round() as u8;
    channel
}

#[test]
fn settle_is_still_then_washes_white_and_back() {
    check(
        "settle",
        &[
            (0.0, 0.0, 0.0, 255, 1.0),
            (0.071, 0.0, 0.0, 255, 1.0),
            (0.3, 0.0, 1.0, 255, 1.0),
            (0.999, 0.0, 0.0, 255, 1.0),
        ],
    );
}

#[test]
fn dissolve_darkens_holds_and_breaks_up() {
    const DARKENED: f32 = 0.3;
    let dark = channel(0.35);
    check(
        "dissolve",
        &[
            (0.0, 0.0, 0.0, 255, 1.0),
            (DARKENED, 0.0, 0.0, dark, 1.0),
            (f32::midpoint(DARKENED, 1.0), 0.5, 0.0, dark, 1.0),
            (0.999, 1.0, 0.0, dark, 1.0),
        ],
    );
}

#[test]
fn dim_darkens_and_fades() {
    let dark = channel(0.35);
    check(
        "dim",
        &[
            (0.0, 0.0, 0.0, 255, 1.0),
            (0.3, 0.0, 0.0, dark, 1.0 - 0.3),
            (0.999, 0.0, 0.0, dark, 0.0),
        ],
    );
}

#[test]
fn blanch_washes_white_then_fades() {
    check(
        "blanch",
        &[
            (0.0, 0.0, 0.0, 255, 1.0),
            (0.5, 0.0, 1.0, 255, 1.0),
            (0.999, 0.0, 1.0, 255, 0.0),
        ],
    );
}

#[test]
fn cover_washes_white_while_the_face_changes_at_half_its_run() {
    check(
        "cover",
        &[
            (0.0, 0.0, 0.0, 255, 1.0),
            (0.4, 0.0, 1.0, 255, 1.0),
            (0.6, 0.0, 1.0, 255, 1.0),
            (0.999, 0.0, 0.0, 255, 1.0),
        ],
    );
    assert_eq!(look_at("cover", 0.49).face, EARLIER);
    assert_eq!(look_at("cover", 0.51).face, LATER);
    assert_eq!(look_at("cover", 1.0).face, LATER, "after the run");
}

#[test]
fn a_run_on_another_card_or_outside_its_time_changes_nothing() {
    let plain = look_at("settle", 2.0);
    assert!(plain.whiten <= 0.0, "after its run");
    let mut look = look_at("settle", 2.0);
    let elsewhere = EffectRun {
        on: EffectTarget::Key(ElementId(8)),
        particle_seed: Some(8),
        ..run("blanch")
    };
    card::look(
        Tier::Baseline,
        &[elsewhere],
        ON,
        1_500 * MS,
        EARLIER,
        &mut look,
    );
    assert_eq!(look, plain);
}

/// The card records in `frame` that are breaking up, as whether each is among the opaque ones.
fn dissolving(frame: &Frame) -> Vec<bool> {
    let card = Material::Card as u8;
    frame
        .draws
        .iter()
        .enumerate()
        .filter(|(_, draw)| draw.material == card && draw.params[0] > 0.0)
        .map(|(index, _)| index < frame.opaque as usize)
        .collect()
}

#[test]
fn a_dissolving_card_is_drawn_with_the_blended_records() {
    // The discard runs from 100 to 500 ms.
    let (mut show, _) = started(vec![effect("dissolve", Edge::Start)]);
    let mut buffer = empty_frame();
    let card = Material::Card as u8;
    let before = frame(&mut show, &mut buffer, 50 * MS);
    assert!(
        before.draws[before.opaque as usize..]
            .iter()
            .all(|draw| draw.material != card || draw.is_edge_only()),
        "before the run every card is opaque, with only its soft edge among the blended records"
    );
    let during = frame(&mut show, &mut buffer, 400 * MS);
    assert_eq!(
        dissolving(&during),
        vec![false],
        "one card, after the opaque ones"
    );
}

#[test]
fn a_settling_card_is_drawn_under_a_burst() {
    let (mut show, _) = started(vec![StepEffect {
        duration: Some(1000.0),
        ..effect("settle", Edge::Start)
    }]);
    let glow = Material::Glow as u8;
    let mut buffer = empty_frame();
    let glows = |frame: &Frame| {
        frame
            .draws
            .iter()
            .filter(|draw| draw.material == glow)
            .count()
    };
    let still = frame(&mut show, &mut buffer, 50 * MS);
    // A third of the way through the run, past `settle.white`, the card is white
    // and Baseline draws its dust cloud, ring, and burst layers.
    let white = frame(&mut show, &mut buffer, 433 * MS);
    assert_eq!(glows(&white), glows(&still) + 3);
}

#[test]
fn a_shake_moves_the_picture_only_while_it_plays_and_moves_no_element() {
    // The shake runs from 100 to 500 ms, as long as its step.
    let (mut shaken, _) = started(vec![effect("shake", Edge::Start)]);
    let (mut plain, _) = started(Vec::new());
    let mut buffer = empty_frame();
    for ms in [50, 100, 220, 300, 430, 500, 600] {
        let with = frame(&mut shaken, &mut buffer, ms * MS);
        let without = frame(&mut plain, &mut buffer, ms * MS);
        assert_eq!(without.shake, Vec2::ZERO);
        if (100..500).contains(&ms) && ms != 100 && ms != 300 {
            assert_ne!(with.shake, Vec2::ZERO, "shaken at {ms} ms");
        } else {
            assert!(with.shake.length() < 1e-3, "{:?} at {ms} ms", with.shake);
        }
        assert_eq!(
            with.resolved
                .iter()
                .map(|element| element.corners)
                .collect::<Vec<_>>(),
            without
                .resolved
                .iter()
                .map(|element| element.corners)
                .collect::<Vec<_>>(),
            "the elements at {ms} ms"
        );
    }
}

#[test]
fn a_shake_swings_twice_and_ends_still() {
    let runs = [run("shake")];
    let at = |share: f64| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a time"
        )]
        let now = 1_000 * MS + (share * 1e9) as u64;
        shake::offset(&runs, now, 2.0, Tier::Baseline)
    };
    assert_eq!(at(0.0), Vec2::ZERO);
    assert_eq!(at(1.0), Vec2::ZERO, "the run is over");
    // The first swing to the right peaks near an eighth of the way, the second smaller.
    let first = at(0.125).x;
    let second = at(0.625).x;
    assert!(
        first > 0.0 && second > 0.0 && second < first,
        "{first}, {second}"
    );
    assert!(at(0.375).x < 0.0, "it swings back to the left between");
}
