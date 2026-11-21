//! Draw records and surface values of the three original effects at ten times in each tier.

use std::fmt::Write;

use beryl_protocol::protocol::{ObjectId, Side};
use beryl_protocol::script::Subject;
use glam::{Quat, Vec2, Vec3};
use kit_base::data::effect::Effects;
use kit_base::pose::{Curve, Pose};
use kit_frame::frame::{Frame, FrameBuilder};
use kit_fx::effects::{Effect, EffectRun, Reach, card, light, particles};
use kit_ui::input::Tier;

fn records(effect: Effect, data: &Effects) -> String {
    let on = match effect {
        Effect::Loss => Subject::Player { player: Side::You },
        _ => Subject::Object { slot: ObjectId(7) },
    };
    let pose = Pose {
        position: Vec3::new(12.0, -20.0, 2.0),
        rotation: Quat::from_rotation_z(0.5),
        ..Pose::default()
    };
    let reach = Reach {
        pose,
        half: if effect == Effect::Loss {
            Vec2::splat(19.0)
        } else {
            Vec2::new(32.0, 44.5)
        },
    };
    let runs = [EffectRun {
        effect,
        on,
        start: 1_000,
        duration: 10_000,
        curve: if effect == Effect::Glow {
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
            let mut builder = FrameBuilder::new(&mut frame, tier);
            card::look((data, tier), &runs, on, now, None, &mut surface);
            card::write(data, &runs, on, now, &surface, &mut builder);
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
        (Effect::Glow, include_str!("../fixtures/glow.txt")),
        (Effect::Dissolve, include_str!("../fixtures/dissolve.txt")),
        (Effect::Loss, include_str!("../fixtures/loss.txt")),
    ] {
        let got = records(effect, &data.effects);
        assert_eq!(got.lines().count(), original.lines().count(), "{effect:?}");
        for (line, (got, original)) in got.lines().zip(original.lines()).enumerate() {
            assert_eq!(got, original, "{effect:?} record {}", line + 1);
        }
    }
}
