//! The effects that are light of their own, drawn over the table as one record each in
//! `Band::Glows`: `glow`, `impact`, `flash`, `beam`, `pulse` and `trail` (docs/beryl-3d.md,
//! "Effects"). Each is shaped as the web stage shapes it, in `ui/src/stage/effects.css` and in
//! `flaresOf` (`ui/src/stage/flare.ts`), and drawn where the scene has its subject on this
//! frame, so that a light on a card in flight follows it. A run on a player is at their life
//! total's anchor. An effect moves nothing.
//!
//! Every light lies just over the face of what it is on, `light.lift` millimetres along the
//! face's normal as a mark does, which the depth test can tell from the face. The values are
//! `effects` in `data/beryl3d/space.json`, as shares of the card or of the run's time:
//!
//! - `glow`: a soft card-shaped outline round the card, the card's own shape grown by `halo` of
//!   its width on each side (the halo of `.stage-effect-glow`), which grows `spread` larger as
//!   it fades. The web draws it under the card, but a card lies on the table itself, so a light
//!   under it would be under the table: the outline lies over the card instead, empty in the
//!   middle, so that it lights the margin round the card and not its face.
//! - `impact`: a burst over what was struck, `wide` of its widths across as the blow lands and
//!   `spread` wider at its end (`IMPACT` in flare.ts), at `strength` of full light (the 0.55
//!   opacity of `.stage-effect-impact`).
//! - `flash`: a burst over the spell, `wide` of its width across at once and `spread` wider at
//!   its end, which fades twice as fast as a glow (`(1 - flash)²` in `.stage-effect-flash`).
//! - `beam`: the web's `BEAM`, a cone from the card's side toward the middle of the table, its
//!   light rising over the first `rise` of its time.
//! - `pulse`: a ring round the card's frame, through the corners of a card `round` times its
//!   size.
//! - `trail`: the web's `TRAIL`, a streak back to where the card was `frames` sixtieths of a
//!   second ago, `wide` of its longer half across at the card. It is not drawn while the card
//!   has moved less than `least` of its longer half in that time.
//!
//! A record's `params[0]` is the run's eased progress, which the glow material fades the light
//! by; where the web's light fades or rises otherwise, the tint's opacity carries the rest.

use beryl_protocol::script::Subject;
use glam::{Affine3A, Quat, Vec2, Vec3};

use super::{Effect, EffectRun};
use crate::data::space::effects::{beam, flash, glow, impact, pulse, trail};
use crate::data::space::light::LIFT;
use crate::frame::{Band, Draw, FrameBuilder, Sprite, sprites};
use crate::light::{marks, streaks};
use crate::play::{Reach, Scene};
use crate::pose::Pose;
use crate::tokens::color;

/// Nanoseconds in one sixtieth of a second, the web stage's frame.
const FRAME: f32 = 1e9 / 60.0;

/// Writes the records of the runs in `runs` that are light and under way at `now`, one each.
/// Allocates nothing.
pub fn write(runs: &[EffectRun], now: u64, scene: &Scene<'_>, builder: &mut FrameBuilder<'_>) {
    for run in runs {
        if !matches!(
            run.effect,
            Effect::Glow
                | Effect::Impact
                | Effect::Flash
                | Effect::Beam
                | Effect::Pulse
                | Effect::Trail
        ) {
            continue;
        }
        // Under way from its start until, not at, its end, as `Show::frame` keeps it.
        if now < run.start || now >= run.start + run.duration {
            continue;
        }
        let Some(reach) = scene.reach(run.on) else {
            continue;
        };
        let half = match run.on {
            Subject::Player { .. } => marks::player_half(scene, reach.pose.scale),
            Subject::Object { .. } | Subject::Ability { .. } => reach.half,
        };
        #[expect(
            clippy::cast_precision_loss,
            reason = "a time inside an effect that lasts about a second"
        )]
        let share = (now - run.start) as f32 / run.duration as f32;
        let progress = run.curve.ease(share);
        let pose = reach.pose;
        let draw = match run.effect {
            Effect::Glow => {
                let size = (half + Vec2::splat(glow::HALO * 2.0 * half.x)) * 2.0;
                let size = size * (1.0 + glow::SPREAD * progress);
                lying(&pose, size, color::GLOW, sprites::OUTLINE, progress)
            }
            Effect::Impact => {
                let across = 2.0 * half.x * (impact::WIDE + impact::SPREAD * progress);
                lying(
                    &pose,
                    Vec2::splat(across),
                    fade(color::IMPACT, impact::STRENGTH),
                    sprites::BURST,
                    progress,
                )
            }
            Effect::Flash => {
                let across = 2.0 * half.x * (flash::WIDE + flash::SPREAD * progress);
                lying(
                    &pose,
                    Vec2::splat(across),
                    fade(color::FLASH, 1.0 - progress),
                    sprites::BURST,
                    progress,
                )
            }
            Effect::Pulse => lying(
                &pose,
                half * 2.0 * pulse::ROUND * std::f32::consts::SQRT_2,
                color::PULSE,
                sprites::RING,
                progress,
            ),
            Effect::Beam => beam(&pose, half, progress),
            Effect::Trail => {
                let Some(draw) = trail(run.on, reach, half, progress, scene) else {
                    continue;
                };
                draw
            }
            _ => continue,
        };
        builder.draw(Band::Glows, draw);
    }
}

/// A glow record of `sprite`, `size` millimetres large, lying in the plane of the face of
/// what is at `pose`, just over it.
fn lying(pose: &Pose, size: Vec2, tint: [u8; 4], sprite: Sprite, progress: f32) -> Draw {
    let transform =
        Affine3A::from_scale_rotation_translation(size.extend(1.0), pose.rotation, above(pose));
    Draw::glow(transform, tint, sprite, progress)
}

/// The point `light.lift` millimetres over the middle of what is at `pose`, along its face's
/// normal, as a mark lies, so that a card held up toward the camera has its light in front of
/// it too.
fn above(pose: &Pose) -> Vec3 {
    pose.position + pose.rotation * Vec3::Z * LIFT
}

/// `tint` with its opacity scaled by `by`, from 0 to 1.
fn fade(tint: [u8; 4], by: f32) -> [u8; 4] {
    let [red, green, blue, alpha] = tint;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an opacity from 0 to 255"
    )]
    let alpha = (f32::from(alpha) * by.clamp(0.0, 1.0)).round() as u8;
    [red, green, blue, alpha]
}

/// The cone of a triggered ability's beam, `progress` of the way through: from a point `high`
/// of its half-height above its middle on its left side, toward the middle of the table, as
/// `beamLight` in `ui/src/stage/flare.ts` draws it. It grows to `long` of its half-width by
/// `grown` of its time, and narrows from `wide` of its half-height to each side to `streak` of
/// that between `thick` and `thin`. The cone sprite's point is at the bottom of its cell and it
/// opens upward, so the quad's own y axis runs from the side of the card outward.
fn beam(pose: &Pose, half: Vec2, progress: f32) -> Draw {
    let grow = 1.0 - (1.0 - (progress / beam::GROWN).clamp(0.0, 1.0)).powi(3);
    let length = half.x * beam::LONG * (0.25 + 0.75 * grow + 0.15 * progress);
    let narrow = ((progress - beam::THICK) / (beam::THIN - beam::THICK)).clamp(0.0, 1.0);
    let across = half.y * beam::WIDE * (1.0 - (1.0 - beam::STREAK) * narrow);
    let toward = pose.rotation * Vec3::NEG_X;
    let start = above(pose) + pose.rotation * Vec3::new(-half.x, beam::HIGH * half.y, 0.0);
    // The quad's y axis runs along the beam and its x axis across it, in the card's plane: the
    // card's rotation turned a quarter circle about its normal, which takes the quad's y axis
    // to the card's -x and keeps it facing the same way as the card.
    let turned = pose.rotation * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let transform = Affine3A::from_scale_rotation_translation(
        Vec3::new(2.0 * across, length, 1.0),
        turned,
        start + toward * (length * 0.5),
    );
    let tint = fade(color::BEAM, progress / beam::RISE);
    Draw::glow(transform, tint, sprites::CONE, progress)
}

/// The streak behind a dashing card, `progress` of the way through: from where `on` was
/// `trail.frames` sixtieths of a second ago to where it is now (`reach`), brightest at the card
/// and fading as the run goes, as `flareLight` in `ui/src/stage/flare.ts` draws a trail. None
/// while the card has hardly moved.
fn trail(on: Subject, reach: Reach, half: Vec2, progress: f32, scene: &Scene<'_>) -> Option<Draw> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few frames, in nanoseconds"
    )]
    let ago = (trail::FRAMES * FRAME) as u64;
    let was = scene.reach_before(on, ago)?;
    let longer = half.x.max(half.y);
    let (to, from) = (above(&reach.pose), above(&was.pose));
    let way = to - from;
    let length = way.length();
    if length < longer * trail::LEAST {
        return None;
    }
    let transform = streaks::quad(
        from + way * 0.5,
        way / length,
        scene.table().camera().eye,
        length,
        2.0 * longer * trail::WIDE,
    );
    // The whole quad is lit, its head at the card; the web's light fades as `(1 - share)^0.8`.
    let tint = fade(color::TRAIL, (1.0 - progress).powf(0.8));
    Some(Draw::streak(transform, tint, sprites::COMET, 1.0, 1.0))
}
