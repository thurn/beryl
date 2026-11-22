//! Draw records and surface values of the original effect implementations at ten times in each tier.

use std::fmt::Write;

use beryl_protocol::protocol::{ObjectId, Side};
use beryl_protocol::script::Subject;
use glam::{Quat, Vec2, Vec3};
use kit_base::data::effect::Effects;
use kit_base::pose::{Curve, Pose};
use kit_frame::frame::{Frame, FrameBuilder};
use kit_fx::effects::{self, EffectRun, Reach, card, light, particles};
use kit_ui::input::Tier;

fn records(effect: &str, data: &Effects) -> String {
    let on = match effect {
        "loss" => Subject::Player { player: Side::You },
        _ => Subject::Object { slot: ObjectId(7) },
    };
    let pose = Pose {
        position: Vec3::new(12.0, -20.0, 2.0),
        rotation: Quat::from_rotation_z(0.5),
        ..Pose::default()
    };
    let reach = Reach {
        pose,
        half: if effect == "loss" {
            Vec2::splat(19.0)
        } else {
            Vec2::new(32.0, 44.5)
        },
    };
    let runs = [EffectRun {
        definition: data.get(effect).unwrap().clone(),
        on: game_table::effect::target(on),
        on_point: matches!(on, Subject::Player { .. }),
        particle_seed: match on {
            Subject::Object { slot } => Some(u32::from(slot.0)),
            _ => None,
        },
        start: 1_000,
        duration: 10_000,
        curve: if effect == "glow" {
            Curve::EASE_OUT
        } else {
            Curve::LINEAR
        },
        step: 4,
    }];
    let mut output = String::new();
    let mut frame = Frame::with_capacity(0, 256, 0);
    frame.looking = true;
    for tier in [Tier::Baseline, Tier::Enhanced] {
        for elapsed in [0, 1000, 2000, 3000, 4000, 5000, 6000, 7500, 9000, 9990] {
            let now = runs[0].start + elapsed;
            let mut surface = card::Look {
                pose,
                tint: [190, 210, 230],
                dissolve: 0.0,
                whiten: 0.0,
                face: Some(1),
                full: None,
                state: 0,
                interactive: false,
            };
            let mut builder =
                FrameBuilder::new(&mut frame, tier, crate::test_data::data().card.size());
            card::look(tier, &runs, runs[0].on, now, None, &mut surface);
            card::write(data, &runs, runs[0].on, now, &surface, &mut builder);
            light::write(data, &runs, now, |_, _| Some(reach), Vec3::Z, &mut builder);
            particles::write(data, &runs, now, |_| Some(reach), &mut builder);
            let frame = builder.finish();
            writeln!(
                output,
                "{tier:?} {elapsed}: {:?}, {:?}",
                surface.tint,
                [surface.dissolve, surface.whiten, surface.pose.opacity]
            )
            .unwrap();
            for look in &frame.looks.effects {
                writeln!(output, "{:?} {:?}", look.model, look.opacity).unwrap();
            }
            for draw in &frame.draws {
                writeln!(output, "{draw:?}").unwrap();
            }
        }
    }
    output
}

#[test]
fn ported_effects_keep_the_original_draw_records() {
    let data = crate::test_data::data();
    for (effect, original) in [
        ("glow", include_str!("../fixtures/glow.txt")),
        ("dissolve", include_str!("../fixtures/dissolve.txt")),
        ("loss", include_str!("../fixtures/loss.txt")),
    ] {
        let got = records(effect, &data.effects);
        assert_eq!(got.lines().count(), original.lines().count(), "{effect:?}");
        for (line, (got, original)) in got.lines().zip(original.lines()).enumerate() {
            assert_eq!(got, original, "{effect:?} record {}", line + 1);
        }
    }
}

fn remaining_records(effect: &str, on: Subject, moving: bool, data: &Effects) -> String {
    const MS: u64 = 1_000_000;
    let run = EffectRun {
        definition: data.get(effect).unwrap().clone(),
        on: game_table::effect::target(on),
        on_point: matches!(on, Subject::Player { .. }),
        particle_seed: match on {
            Subject::Object { slot } => Some(u32::from(slot.0)),
            _ => None,
        },
        start: 100 * MS,
        duration: 1_000 * MS,
        curve: Curve::LINEAR,
        step: 4,
    };
    let pose = Pose {
        position: Vec3::new(12.0, -20.0, 2.0),
        rotation: Quat::from_rotation_z(0.5),
        scale: 0.87,
        opacity: 0.91,
        ..Pose::default()
    };
    let half = if matches!(on, Subject::Player { .. }) {
        Vec2::splat(19.0)
    } else {
        Vec2::new(32.0, 44.5) * pose.scale
    };
    let mut output = String::new();
    let mut frame = Frame::with_capacity(0, 256, 0);
    frame.looking = true;
    for tier in [Tier::Baseline, Tier::Enhanced] {
        let runs = std::slice::from_ref(&run);
        for ms in [0, 71, 150, 250, 300, 360, 500, 660, 800, 999] {
            let now = run.start + ms * MS;
            let mut surface = card::Look::new(pose, Some(1), Some([2, 3]), 0, false);
            surface.tint = [190, 210, 230];
            let mut builder =
                FrameBuilder::new(&mut frame, tier, crate::test_data::data().card.size());
            card::look(tier, runs, run.on, now, Some(4), &mut surface);
            card::write(data, runs, run.on, now, &surface, &mut builder);
            effects::write(
                data,
                runs,
                now,
                |_, ago| {
                    let mut pose = pose;
                    if ago != 0 {
                        assert_eq!(ago, 70 * MS);
                    }
                    if moving && ago != 0 {
                        pose.position -= Vec3::new(23.0, 12.0, 0.5);
                    }
                    Some(Reach { pose, half })
                },
                (Vec3::Z, 2.0),
                &mut builder,
            );
            let frame = builder.finish();
            let text = effects::text(&run, now, tier, kit_base::tokens::color::LIFE);
            writeln!(
                output,
                "{tier:?} {ms}: {surface:?} {:?} {text:?}",
                frame.shake
            )
            .unwrap();
            for look in &frame.looks.effects {
                writeln!(output, "{look:?}").unwrap();
            }
            for draw in &frame.draws {
                writeln!(output, "{draw:?}").unwrap();
            }
        }
    }
    output
}

#[test]
fn remaining_effects_keep_the_original_draw_records() {
    let data = crate::test_data::data();
    for effect in [
        "settle", "dim", "blanch", "cover", "impact", "shake", "trail", "flash", "beam", "pulse",
        "gain",
    ] {
        let object = Subject::Object { slot: ObjectId(7) };
        let player = Subject::Player { player: Side::You };
        let target = if effect == "gain" { player } else { object };
        let mut got = remaining_records(effect, target, true, &data.effects);
        if effect == "impact" {
            got += &remaining_records(effect, player, true, &data.effects);
        } else if effect == "trail" {
            got += &remaining_records(effect, object, false, &data.effects);
        }
        let path = format!("rust/beryl3d/crates/game_probe/tests/fixtures/{effect}.txt");
        let original = beryl_test_files::read_to_string(path).expect("original draw records");
        assert_eq!(got.lines().count(), original.lines().count(), "{effect:?}");
        for (line, (got, original)) in got.lines().zip(original.lines()).enumerate() {
            assert!(
                same_record(got, original),
                "{effect:?} record {}\nnew: {got}\nold: {original}",
                line + 1
            );
        }
    }
}

/// Compare every field in order. The data curves can regroup float operations, so allow
/// rounding in their positions and strengths while requiring exact colors and draw counts.
fn same_record(got: &str, original: &str) -> bool {
    if got == original {
        return true;
    }
    let separator =
        |c: char| c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ':');
    let got = got.split(separator);
    let original = original.split(separator);
    got.clone().count() == original.clone().count()
        && got.zip(original).all(|(a, b)| {
            a == b
                || (a.contains(['.', 'e'])
                    && b.contains(['.', 'e'])
                    && matches!((a.parse::<f32>(), b.parse::<f32>()),
                    (Ok(a), Ok(b)) if (a - b).abs() <= 0.000_05))
        })
}
