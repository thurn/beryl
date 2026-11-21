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
//! - `glow`: light round the card's edge, the two box shadows of `.stage-effect-glow` (the
//!   `HALO` sprite, `halo` in `beryl_assets::light`), which grows `spread` larger as it fades.
//!   The web draws it under the card, but a card lies on the table itself, so a light under it
//!   would be under the table: the halo lies over the card instead, empty where the card is,
//!   so that it lights the margin round the card and not its face. The sprite holds the warm
//!   shadow's 75% opacity, and the tint is that shadow's color at 75% again: the web lays the
//!   halo over the table, but this light adds to it, which at the sprite's own strength looked
//!   far brighter than the web's.
//! - `impact`: a burst over what was struck, `wide` of its longer half across each way as the
//!   blow lands and `spread` wider at its end, fading as `(1 - progress)^fade` (`IMPACT` and
//!   the `impact` branch of `flareLight` in flare.ts). The web also lays the orange gradient of
//!   `.stage-effect-impact` at 0.55 opacity under that burst, which the white burst hides.
//! - `flash`: a burst over the spell, `wide` of its longer half across each way at once and
//!   `spread` wider at its end, fading as `(1 - progress)^fade` (the last branch of
//!   `flareLight`).
//!
//! The bursts are the `BURST` sprite, the web's burst as its painter draws it at its brightest
//! (`burst` in `beryl_assets::light`). The glow material fades a record by `1 - progress`, so
//! each burst's tint fades by the rest of its power.
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
//!
//! - `loss` and `gain`: a ring round the life orb's face in the color its total flares, red for
//!   a loss and green for a gain, fading as it goes (`[data-life] > .stage-effect` in
//!   effects.css). The life total's text is tinted too (`hud::counts`).
//!
//! While the frame is built for the match tools, each light is also recorded as an `EffectLook`
//! with the box and opacity of the web's `.stage-effect` node for it (`node`), not its quad.

use beryl_protocol::script::Subject;
use glam::{Affine3A, Quat, Vec2, Vec3};

use super::{Effect, EffectRun, Reach};
use kit_base::data::space::effects::{beam, flash, glow, impact, pulse, trail};
use kit_base::data::space::light::LIFT;
use kit_base::pose::Pose;
use kit_base::tokens::color;
use kit_frame::frame::{Band, Draw, EffectLook, FrameBuilder, Sprite, sprites};

/// How much larger than the life orb's face the quad of a `loss` or `gain` ring is drawn, so that
/// the `RING` sprite's line, three quarters of the way out, lies just outside the face, where
/// the web's ring glows (`[data-life] > .stage-effect` in `ui/src/stage/effects.css`).
const LIFE_RING: f32 = 1.45;

/// Nanoseconds in one sixtieth of a second, the web stage's frame.
const FRAME: f32 = 1e9 / 60.0;

/// Writes the records of the runs in `runs` that are light and under way at `now`, one each.
/// `reach_at` gives the target's pose and size a given number of nanoseconds before the frame.
pub fn write(
    runs: &[EffectRun],
    now: u64,
    reach_at: impl Fn(Subject, u64) -> Option<Reach>,
    eye: Vec3,
    builder: &mut FrameBuilder<'_>,
) {
    for run in runs {
        if !matches!(
            run.effect,
            Effect::Glow
                | Effect::Impact
                | Effect::Flash
                | Effect::Beam
                | Effect::Pulse
                | Effect::Trail
                | Effect::Loss
                | Effect::Gain
        ) {
            continue;
        }
        // Under way from its start until, not at, its end, as `Show::frame` keeps it.
        if now < run.start || now >= run.start + run.duration {
            continue;
        }
        let Some(reach) = reach_at(run.on, 0) else {
            continue;
        };
        let half = reach.half;
        #[expect(
            clippy::cast_precision_loss,
            reason = "a time inside an effect that lasts about a second"
        )]
        let share = (now - run.start) as f32 / run.duration as f32;
        let progress = run.curve.ease(share);
        let pose = reach.pose;
        // The web sizes a burst by the longer half of what it is on (`flareLight`).
        let longer = half.x.max(half.y);
        let draw = match run.effect {
            Effect::Glow => {
                let size = sprites::halo_size(half) * (1.0 + glow::SPREAD * progress);
                lying(&pose, size, color::GLOW, sprites::HALO, progress)
            }
            Effect::Impact => {
                let across = 2.0 * longer * (impact::WIDE + impact::SPREAD * progress);
                lying(
                    &pose,
                    Vec2::splat(across),
                    fade(color::FLASH, (1.0 - progress).powf(impact::FADE - 1.0)),
                    sprites::BURST,
                    progress,
                )
            }
            Effect::Flash => {
                let across = 2.0 * longer * (flash::WIDE + flash::SPREAD * progress);
                lying(
                    &pose,
                    Vec2::splat(across),
                    fade(color::FLASH, (1.0 - progress).powf(flash::FADE - 1.0)),
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
            // Not drawn while the card has hardly moved, but its node is still recorded, as the
            // web keeps its trail node for the whole run.
            Effect::Trail => {
                let Some(draw) = trail(run.on, reach, progress, &reach_at, eye) else {
                    record(builder, run, &pose, half, progress);
                    continue;
                };
                draw
            }
            Effect::Loss | Effect::Gain => {
                let tint = if run.effect == Effect::Loss {
                    color::LOSS
                } else {
                    color::GAIN
                };
                lying(&pose, half * 2.0 * LIFE_RING, tint, sprites::RING, progress)
            }
            _ => continue,
        };
        record(builder, run, &pose, half, progress);
        builder.draw(Band::Glows, draw);
    }
}

/// Records `run`, `progress` of the way through on what lies at `pose` with half size `half`,
/// for the match tools, as the web's node for it (`node`).
fn record(builder: &mut FrameBuilder<'_>, run: &EffectRun, pose: &Pose, half: Vec2, progress: f32) {
    let (scale, opacity) = node(run.effect, run.on, progress);
    let size = (half * 2.0 * scale).extend(1.0);
    let model = Affine3A::from_scale_rotation_translation(size, pose.rotation, above(pose));
    builder.effect(EffectLook {
        name: run.effect.name(),
        on: Some(run.on),
        model: Draw::glow(model, [0; 4], sprites::HALO, progress).model,
        opacity,
    });
}

/// How much larger than what it is on, and how opaque, the web's `.stage-effect` node of
/// `effect` is, `progress` of the way through: the `transform: scale(...)` and `opacity` of its
/// rule in `ui/src/stage/effects.css`. The web draws these lights on a canvas (`flaresOf` in
/// `ui/src/stage/flare.ts`) or as box shadows past the node's edge, and its node is only a
/// record the size of the card, or of the face of a player's life orb. The match tools measure
/// an effect's motion from that node, so Beryl 3D records the same box and opacity rather than
/// its light's quad, whose empty margins made every light look far larger than the web's; the
/// light itself is compared in pixels.
fn node(effect: Effect, on: Subject, progress: f32) -> (f32, f32) {
    let rest = 1.0 - progress;
    let player = matches!(on, Subject::Player { .. });
    match effect {
        // `[data-life] > .stage-effect` and `[data-life] > .stage-effect-impact`.
        Effect::Loss | Effect::Gain => (1.0 + 0.45 * progress, rest),
        Effect::Impact if player => (1.0 + 0.9 * progress, rest),
        Effect::Glow => (1.0 + 0.1 * progress, rest),
        Effect::Impact => (1.0 + 0.12 * progress, 0.55 * rest),
        Effect::Flash => (1.0 + 0.08 * progress, rest * rest),
        Effect::Pulse => (1.0, rest),
        // `beam` and `trail` have no rule of their own: the node stands whole over the card.
        _ => (1.0, 1.0),
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
fn trail(
    on: Subject,
    reach: Reach,
    progress: f32,
    reach_at: impl Fn(Subject, u64) -> Option<Reach>,
    eye: Vec3,
) -> Option<Draw> {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few frames, in nanoseconds"
    )]
    let ago = (trail::FRAMES * FRAME) as u64;
    let was = reach_at(on, ago)?;
    let longer = reach.half.x.max(reach.half.y);
    let (to, from) = (above(&reach.pose), above(&was.pose));
    let way = to - from;
    let length = way.length();
    if length < longer * trail::LEAST {
        return None;
    }
    let transform = quad(
        from + way * 0.5,
        way / length,
        eye,
        length,
        2.0 * longer * trail::WIDE,
    );
    // The whole quad is lit, its head at the card; the web's light fades as `(1 - share)^0.8`.
    let tint = fade(color::TRAIL, (1.0 - progress).powf(0.8));
    Some(Draw::streak(transform, tint, sprites::COMET, 1.0, 1.0))
}

/// A sprite's quad at `at`, `length` long along `tangent` and `width` across it, facing `eye`.
pub(crate) fn quad(at: Vec3, tangent: Vec3, eye: Vec3, length: f32, width: f32) -> Affine3A {
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
