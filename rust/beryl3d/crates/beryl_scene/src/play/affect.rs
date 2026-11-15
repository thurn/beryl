//! The effect runs a starting script adds to the show (docs/beryl-3d.md, "Effects").

use super::{LIGHTS, Playing};
use crate::effects::EffectRun;
use crate::verbs::Interval;

impl Playing {
    /// Adds each of its effect runs to `effects`, in real time, as the script starts.
    pub(super) fn affect(&self, effects: &mut Vec<EffectRun>) {
        for run in &self.compiled.effects {
            let over = self.real_interval(Interval {
                from: run.start,
                to: run.start + run.duration,
            });
            begin(
                effects,
                EffectRun {
                    start: over.from,
                    duration: over.duration(),
                    ..*run
                },
            );
        }
    }
}

/// Adds `run` to `effects`, the runs the show holds. As on the web stage
/// (`Effects::begin` in ui/src/stage/effects.ts), a run of the same effect on the same thing
/// gives way to it: one yet to begin is dropped, and one under way ends as `run` begins. When
/// the list is full, `run` replaces the run that ends first, so that it allocates nothing.
fn begin(effects: &mut Vec<EffectRun>, run: EffectRun) {
    effects.retain_mut(|other| {
        if other.effect != run.effect || other.on != run.on {
            return true;
        }
        if other.start >= run.start {
            return false;
        }
        other.duration = other.duration.min(run.start - other.start);
        true
    });
    if effects.len() < LIGHTS {
        effects.push(run);
    } else if let Some(first) = effects
        .iter_mut()
        .min_by_key(|other| other.start + other.duration)
    {
        *first = run;
    }
}
