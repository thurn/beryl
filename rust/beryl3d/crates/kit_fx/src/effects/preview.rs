//! The effects on a plain rectangle in a toolkit scene.

use glam::{Vec2, Vec3};

use super::{Effect, EffectRun, Reach, card, light, shake};
use kit_base::data::effect::Effects;
use kit_base::geometry::{Rect, plane::ScreenPlane};
use kit_base::pose::Pose;
use kit_frame::frame::{Band, Draw, FrameBuilder};

/// The length of the standalone preview, in nanoseconds.
pub const DURATION: u64 = 600_000_000;

/// The position of the rectangle. A trail needs a moving subject to be visible.
#[must_use]
pub fn position(effect: Effect, rect: Rect, now: u64) -> Rect {
    #[expect(
        clippy::cast_precision_loss,
        reason = "elapsed nanoseconds in a subsecond preview"
    )]
    let progress = now.min(DURATION) as f32 / DURATION as f32;
    let distance = if effect == Effect::Trail {
        rect.width * progress
    } else {
        0.0
    };
    Rect {
        x: rect.x + distance,
        ..rect
    }
}

/// Draws a painted rectangle and the effect on it. A color wash uses the same tint and
/// fade as a card. With a solid paint, dissolve reduces opacity and cover has no face swap.
pub fn write(
    data: &Effects,
    effect: Effect,
    rect: Rect,
    now: u64,
    plane: &ScreenPlane,
    builder: &mut FrameBuilder<'_>,
) {
    let run = EffectRun::preview(effect, DURATION);
    let on = run.on;
    let runs = [run];
    let reach = |before| {
        let rect = position(effect, rect, now.saturating_sub(before));
        Reach {
            pose: Pose {
                position: plane.layer_transform(rect).translation.into(),
                ..Pose::default()
            },
            half: Vec2::new(rect.width, rect.height) / 2.0,
        }
    };
    let mut look = card::Look {
        pose: reach(0).pose,
        tint: [75, 104, 144],
        dissolve: 0.0,
        whiten: 0.0,
        face: None,
        full: None,
        state: 0,
        interactive: false,
    };
    card::look((data, builder.tier()), &runs, on, now, None, &mut look);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "normalized channels"
    )]
    let byte = |value: f32| value.round() as u8;
    let [r, g, b] = look
        .tint
        .map(|channel| byte(f32::from(channel) + (255.0 - f32::from(channel)) * look.whiten));
    let alpha = byte(255.0 * look.pose.opacity * (1.0 - look.dissolve));
    let shown = position(effect, rect, now);
    builder.draw(
        Band::RaisedMarks,
        Draw::panel(
            plane.layer_transform(shown),
            [r, g, b, alpha],
            [0; 4],
            Vec2::new(rect.width, rect.height),
            0.0,
            0.0,
        ),
    );
    card::write(data, &runs, on, now, &look, builder);
    light::write(
        data,
        &runs,
        now,
        |_, before| Some(reach(before)),
        Vec3::ZERO,
        builder,
    );
    shake::write(data, &runs, now, plane.scale(), builder);
}
