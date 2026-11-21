//! The particles of the Enhanced tier's `vanish` (docs/beryl-3d.md, "The Enhanced tier"): a
//! destroyed permanent falls to ash as it dissolves (`dissolve`), and an exiled one is drawn
//! into a vortex as it whitens (`blanch`). The web interface has no particles; these shapes are
//! Beryl 3D's own. Only the Enhanced tier writes them, so in Baseline this writes nothing.
//!
//! Each particle is a pure function of its run, its index and the frame's time, so a frame
//! drawn after skipped frames is the frame drawn after every tick. Its randomness is a 32-bit
//! integer hash of the object's id, the step that wrote the run, its index and which number it
//! is (docs/beryl-3d.md, "Determinism": seeded effects). Times are shares of the run's eased
//! progress, the same progress `card::look` darkens, breaks up and whitens the card by, so
//! that the ash leaves the card as its breaking edge passes. The values are
//! `enhanced.particles` in `data/beryl3d/space.json`, lengths in millimetres at the card's
//! scale and lives as shares of the run:
//!
//! - ash, for a `dissolve`: particle i is born on a row of the card (a hash, 0 at the top) as
//!   the card's `dissolve`, which runs from 0 to 1 after `dissolve.darkened` of the run and
//!   breaks the card up from the top down, reaches that row. It starts at a point of that row
//!   across the card's width (a hash) and, for `life` of the run, falls `fall` down the card's
//!   face and drifts up to `drift` to one side, eased out. It is as dark as the dissolving
//!   card at its darkest.
//! - vortex, for a `blanch`: particle i starts at a point of the card's face (two hashes), is
//!   born in the first half of the run, and for `life` of the run turns `turns` times round the
//!   card's centre on a spiral that ends there, shrinking to nothing. It is the white the
//!   card is washed toward, the web's `--light` that `.stage-effect-blanch` paints, which the
//!   tokens hold as `color::TRAIL`.
//!
//! Particles lie in the card's plane, `light.lift` over its face as a mark does, placed with
//! the card's pose of this frame so that they follow a card that moves. A frame holds at most
//! `PARTICLES`: when its runs want more, each run gets an equal share of that, its lowest
//! indices first. An effect moves nothing, and this allocates nothing.

use beryl_protocol::script::Subject;
use glam::{Affine3A, Vec2, Vec3};

use super::{Effect, EffectRun, Reach};
use crate::data::space::effects::dissolve;
use crate::data::space::enhanced::particles::{ash, vortex};
use crate::data::space::light::LIFT;
use crate::frame::{Band, Draw, FrameBuilder, PARTICLES, sprites};
use crate::input::Tier;
use crate::tokens::color;

/// The most particles a run of `effect` writes, if it writes any.
fn count(effect: Effect) -> Option<usize> {
    match effect {
        Effect::Dissolve => Some(ash::COUNT),
        Effect::Blanch => Some(vortex::COUNT),
        _ => None,
    }
}

/// Whether `run` writes particles at `now`: a `dissolve` or `blanch` on a card, under way from
/// its start until, not at, its end.
fn writes(run: &EffectRun, now: u64) -> bool {
    count(run.effect).is_some()
        && matches!(run.on, Subject::Object { .. })
        && run.start <= now
        && now < run.start + run.duration
}

/// Writes the particles of the runs in `runs` under way at `now`, in `Band::Particles`, when
/// the frame is drawn in the Enhanced tier. Allocates nothing.
pub fn write(
    runs: &[EffectRun],
    now: u64,
    reach_at: impl Fn(Subject) -> Option<Reach>,
    builder: &mut FrameBuilder<'_>,
) {
    if builder.tier() != Tier::Enhanced {
        return;
    }
    let (mut wanted, mut writing) = (0, 0);
    for run in runs.iter().filter(|run| writes(run, now)) {
        wanted += count(run.effect).unwrap_or(0);
        writing += 1;
    }
    let most = if wanted > PARTICLES {
        PARTICLES / writing
    } else {
        usize::MAX
    };
    for run in runs.iter().filter(|run| writes(run, now)) {
        let Subject::Object { slot } = run.on else {
            continue;
        };
        let Some(reach) = reach_at(run.on) else {
            continue;
        };
        #[expect(
            clippy::cast_precision_loss,
            reason = "a time inside an effect that lasts about a second"
        )]
        let progress = run
            .curve
            .ease((now - run.start) as f32 / run.duration as f32);
        let seeds = Seeds {
            object: slot.0,
            step: run.step,
        };
        let total = count(run.effect).unwrap_or(0).min(most);
        for index in 0..total {
            let particle = match run.effect {
                Effect::Dissolve => ash_particle(&reach, seeds, index, progress),
                Effect::Blanch => vortex_particle(&reach, seeds, index, progress),
                _ => None,
            };
            if let Some(draw) = particle {
                builder.draw(Band::Particles, draw);
            }
        }
    }
}

/// What a run's particles take their randomness from: the object's id and the step that
/// wrote the run.
#[derive(Clone, Copy)]
struct Seeds {
    object: u16,
    step: usize,
}

impl Seeds {
    /// The `k`th number of particle `index`, from 0 up to but not including 1.
    fn random(self, index: usize, k: u32) -> f32 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a step's number and a particle's index are far below 2^32, and only \
                      their bits feed the hash"
        )]
        let (step, index) = (self.step as u32, index as u32);
        let mut hash = mix(u32::from(self.object) ^ 0x9e37_79b9);
        hash = mix(hash ^ step);
        hash = mix(hash ^ index);
        hash = mix(hash ^ k);
        #[expect(
            clippy::cast_precision_loss,
            reason = "24 bits, which an f32 holds exactly"
        )]
        let top = (hash >> 8) as f32;
        top / 16_777_216.0
    }
}

/// A 32-bit integer hash that changes about half the bits of its result for each bit of `x`
/// (Chris Wellons's `lowbias32`).
fn mix(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// How far a particle born at `birth` that lives `life` is through its life at `progress`,
/// from 0 to 1, or none if it is not yet born or already gone.
fn age(progress: f32, birth: f32, life: f32) -> Option<f32> {
    let age = (progress - birth) / life;
    (age > 0.0 && age < 1.0).then_some(age)
}

/// The record of a particle of `size` millimetres at `at`, a point of the card's face measured
/// from its centre, lying `light.lift` over the face of the card at `reach`.
fn lying(reach: &Reach, at: Vec2, size: f32, tint: [u8; 4], age: f32, seed: f32) -> Draw {
    let pose = &reach.pose;
    let size = size * pose.scale;
    let transform = Affine3A::from_scale_rotation_translation(
        Vec3::new(size, size, 1.0),
        pose.rotation,
        pose.position + pose.rotation * at.extend(LIFT),
    );
    Draw::particle(transform, tint, sprites::DISC, age, seed)
}

/// A number from `ends.x` to `ends.y`, `share` of the way.
fn between(ends: Vec2, share: f32) -> f32 {
    ends.x + (ends.y - ends.x) * share
}

/// Ash particle `index` of a card dissolving at `reach`, `progress` of the way through its run.
fn ash_particle(reach: &Reach, seeds: Seeds, index: usize, progress: f32) -> Option<Draw> {
    let row = seeds.random(index, 1);
    // `card::look` raises the card's dissolve from 0 to 1 over the run after its darkening,
    // and the shader breaks a row away from the top down as the dissolve passes it.
    let birth = dissolve::DARKENED + row * (1.0 - dissolve::DARKENED);
    let age = age(progress, birth, ash::LIFE)?;
    let eased = 1.0 - (1.0 - age) * (1.0 - age);
    let half = reach.half;
    let start = Vec2::new(
        (2.0 * seeds.random(index, 2) - 1.0) * half.x,
        (1.0 - 2.0 * row) * half.y,
    );
    let drift = (2.0 * seeds.random(index, 4) - 1.0) * ash::DRIFT;
    let at = start + Vec2::new(drift, -ash::FALL) * eased * reach.pose.scale;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a white channel times a factor from 0 to 1"
    )]
    let dark = (255.0 * dissolve::DARK).round() as u8;
    Some(lying(
        reach,
        at,
        between(ash::SIZE, seeds.random(index, 3)),
        [dark, dark, dark, 255],
        age,
        seeds.random(index, 0),
    ))
}

/// Vortex particle `index` of a card blanching at `reach`, `progress` of the way through its
/// run.
fn vortex_particle(reach: &Reach, seeds: Seeds, index: usize, progress: f32) -> Option<Draw> {
    let birth = 0.5 * seeds.random(index, 4);
    let age = age(progress, birth, vortex::LIFE)?;
    let half = reach.half;
    let start = Vec2::new(
        (2.0 * seeds.random(index, 1) - 1.0) * half.x,
        (2.0 * seeds.random(index, 2) - 1.0) * half.y,
    );
    let turn = Vec2::from_angle(vortex::TURNS * std::f32::consts::TAU * age);
    let left = 1.0 - age;
    Some(lying(
        reach,
        turn.rotate(start) * left,
        between(vortex::SIZE, seeds.random(index, 3)) * left,
        color::TRAIL,
        age,
        seeds.random(index, 0),
    ))
}
