//! Seeded particle emitters. A frame drawn after skipped frames shows the same particles as a
//! frame drawn after each tick, because a particle depends on the run, its index and its age.
//! The subject's pose puts the particles just above its surface. If the emitters together
//! request more records than the frame holds, they each get an equal share of the budget.

use beryl_protocol::script::Subject;
use glam::{Affine3A, Vec2, Vec3};

use super::{EffectRun, Reach, player};
use kit_base::data::effect::{Effects, Emitter, Primitive, Vortex};
use kit_frame::frame::{Band, Draw, FrameBuilder, PARTICLES, sprites};
use kit_ui::input::Tier;

#[derive(Clone, Copy)]
enum Kind<'a> {
    Sweep(&'a Emitter),
    Vortex(&'a Vortex),
}

impl Kind<'_> {
    fn count(self) -> usize {
        match self {
            Self::Sweep(emitter) => usize::from(emitter.count),
            Self::Vortex(emitter) => usize::from(emitter.count),
        }
    }
}

fn emitters<'a>(
    runs: &'a [EffectRun],
    now: u64,
    tier: Tier,
) -> impl Iterator<Item = (Subject, Seeds, Kind<'a>, f32)> {
    runs.iter()
        .filter_map(|run| {
            let Subject::Object { slot } = run.on else {
                return None;
            };
            let seeds = Seeds {
                object: slot.0,
                step: run.step,
            };
            Some((run, seeds))
        })
        .flat_map(move |(run, seeds)| {
            player::layers(run, now, tier).filter_map(move |(primitive, progress)| {
                let kind = match primitive {
                    Primitive::Particles { emitter } => Kind::Sweep(emitter),
                    Primitive::Vortex { emitter } => Kind::Vortex(emitter),
                    _ => return None,
                };
                (kind.count() > 0).then_some((run.on, seeds, kind, progress))
            })
        })
}

/// Writes the visible particle emitters under way at `now` in `Band::Particles`.
pub fn write(
    effects: &Effects,
    runs: &[EffectRun],
    now: u64,
    reach_at: impl Fn(Subject) -> Option<Reach>,
    builder: &mut FrameBuilder<'_>,
) {
    let tier = builder.tier();
    let lift = effects.lift();
    let emitters = || emitters(runs, now, tier);
    let (wanted, writing) = emitters().fold((0, 0), |(wanted, writing), (_, _, kind, _)| {
        (wanted + kind.count(), writing + 1)
    });
    let most = if wanted > PARTICLES {
        PARTICLES / writing
    } else {
        usize::MAX
    };
    for (on, seeds, kind, progress) in emitters() {
        let Some(reach) = reach_at(on) else { continue };
        for index in 0..kind.count().min(most) {
            let particle = match kind {
                Kind::Sweep(emitter) => {
                    sweep_particle(emitter, (&reach, lift), seeds, index, progress)
                }
                Kind::Vortex(emitter) => {
                    vortex_particle(emitter, (&reach, lift), seeds, index, progress)
                }
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
fn lying(
    (reach, lift): (&Reach, f32),
    at: Vec2,
    size: f32,
    tint: [u8; 4],
    age: f32,
    seed: f32,
) -> Draw {
    let pose = &reach.pose;
    let size = size * pose.scale;
    let transform = Affine3A::from_scale_rotation_translation(
        Vec3::new(size, size, 1.0),
        pose.rotation,
        pose.position + pose.rotation * at.extend(lift),
    );
    Draw::particle(transform, tint, sprites::DISC, age, seed)
}

/// Each row emits at a different point of the layer clock. The index supplies a stable
/// point along the row, a drift direction, and a size for the particle.
fn sweep_particle(
    emitter: &Emitter,
    (reach, lift): (&Reach, f32),
    seeds: Seeds,
    index: usize,
    progress: f32,
) -> Option<Draw> {
    let row = seeds.random(index, 1);
    let age = age(progress, emitter.sweep.at(row), emitter.life)?;
    let start = Vec2::new(
        (2.0 * seeds.random(index, 2) - 1.0) * reach.half.x,
        (1.0 - 2.0 * row) * reach.half.y,
    );
    let drift = (2.0 * seeds.random(index, 4) - 1.0) * emitter.drift.at(progress);
    let travel = emitter.travel.at(age);
    let at = start + Vec2::new(drift, -emitter.fall.at(progress)) * travel * reach.pose.scale;
    Some(lying(
        (reach, lift),
        at,
        emitter.size.at(seeds.random(index, 3)),
        emitter.tint,
        age,
        seeds.random(index, 0),
    ))
}

/// Emits a particle whose seeded starting point rotates and shrinks toward the center.
fn vortex_particle(
    emitter: &Vortex,
    (reach, lift): (&Reach, f32),
    seeds: Seeds,
    index: usize,
    progress: f32,
) -> Option<Draw> {
    let birth = emitter.birth.at(seeds.random(index, 4));
    let age = age(progress, birth, emitter.life)?;
    let half = reach.half;
    let start = Vec2::new(
        (2.0 * seeds.random(index, 1) - 1.0) * half.x,
        (2.0 * seeds.random(index, 2) - 1.0) * half.y,
    );
    let turn = Vec2::from_angle(emitter.turns * std::f32::consts::TAU * age);
    let shrink = 1.0 - age;
    Some(lying(
        (reach, lift),
        turn.rotate(start) * shrink,
        emitter.size.at(seeds.random(index, 3)) * shrink,
        emitter.tint,
        age,
        seeds.random(index, 0),
    ))
}
