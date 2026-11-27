//! Draws additive light just above each subject, following its pose as it moves.

use glam::{Affine3A, Vec2, Vec3};

use super::{EffectRun, Reach, player};
use kit_base::data::effect::Effects;
use kit_base::pose::Pose;
use kit_frame::frame::{Draw, EffectTarget, FrameBuilder, Sprite, sprites};

/// Writes the table-space light layers. `reach_at` supplies the subject's current pose or
/// its pose a given number of nanoseconds earlier.
pub fn write(
    effects: &Effects,
    runs: &[EffectRun],
    now: u64,
    reach_at: impl Fn(EffectTarget, u64) -> Option<Reach>,
    eye: Vec3,
    builder: &mut FrameBuilder<'_>,
) {
    for run in runs {
        player::write_light(
            run,
            now,
            |ago| reach_at(run.on, ago),
            (eye, effects.lift(), effects.palette()),
            builder,
        );
    }
}

/// Records the box and opacity of a light's matching web element for comparison tools.
pub(super) fn record_as(
    builder: &mut FrameBuilder<'_>,
    run: &EffectRun,
    (pose, lift): (&Pose, f32),
    half: Vec2,
    (scale, opacity): (f32, f32),
) {
    let size = (half * 2.0 * scale).extend(1.0);
    let model = Affine3A::from_scale_rotation_translation(size, pose.rotation, above(pose, lift));
    builder.effect(
        &run.definition.name,
        Some(run.on),
        Draw::glow(model, [0; 4], sprites::HALO, 0.0).model,
        opacity,
    );
}

/// A glow record of `sprite`, `size` millimetres large, lying in the plane of the face of
/// what is at `pose`, just over it.
pub(super) fn lying(
    (pose, lift): (&Pose, f32),
    size: Vec2,
    tint: [u8; 4],
    sprite: Sprite,
    progress: f32,
) -> Draw {
    let transform = Affine3A::from_scale_rotation_translation(
        size.extend(1.0),
        pose.rotation,
        above(pose, lift),
    );
    Draw::glow(transform, tint, sprite, progress)
}

/// The point `light.lift` millimetres over the middle of what is at `pose`, along its face's
/// normal, as a mark lies, so that a card held up toward the camera has its light in front of
/// it too.
pub(super) fn above(pose: &Pose, lift: f32) -> Vec3 {
    pose.position + pose.rotation * Vec3::Z * lift
}

/// `tint` with its opacity scaled by `by`, from 0 to 1.
pub(super) fn fade(tint: [u8; 4], by: f32) -> [u8; 4] {
    let [red, green, blue, alpha] = tint;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an opacity from 0 to 255"
    )]
    let alpha = (f32::from(alpha) * by.clamp(0.0, 1.0)).round() as u8;
    [red, green, blue, alpha]
}

pub(super) fn path_streak(
    reach: Reach,
    was: Reach,
    (eye, lift): (Vec3, f32),
    minimum: f32,
    width: f32,
    tint: [u8; 4],
) -> Option<Draw> {
    let longer = reach.half.x.max(reach.half.y);
    let (to, from) = (above(&reach.pose, lift), above(&was.pose, lift));
    let way = to - from;
    let length = way.length();
    if length <= 0.0 || length < longer * minimum {
        return None;
    }
    let transform = quad(
        from + way * 0.5,
        way / length,
        eye,
        length,
        2.0 * longer * width,
    );
    Some(Draw::streak(transform, tint, sprites::COMET, 1.0, 1.0))
}

/// A sprite's quad at `at`, `length` long along `tangent` and `width` across it, facing `eye`.
pub fn quad(at: Vec3, tangent: Vec3, eye: Vec3, length: f32, width: f32) -> Affine3A {
    let facing = (eye - at).normalize_or(Vec3::Z);
    let across = facing.cross(tangent).normalize_or(Vec3::Y);
    let normal = tangent.cross(across);
    Affine3A::from_cols(
        (tangent * length).into(),
        (across * width).into(),
        normal.into(),
        at.into(),
    )
}
