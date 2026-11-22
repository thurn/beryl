//! The effect runs a starting script adds to the show (docs/beryl-3d.md, "Effects").

use std::sync::Arc;

use super::relay::settle;
use super::{LIGHTS, Playing};
use crate::verbs::Interval;
use kit_fx::effects::EffectRun;

impl Playing {
    /// Adds each of its effect runs not added yet to `effects`, in real time: as the script
    /// starts, and for a script compiled over frames, those of the steps compiled since
    /// (`super::part`), in the order they begin.
    pub(super) fn affect(&mut self, effects: &mut Vec<EffectRun>) {
        let given = self.effects_given.min(self.compiled.effects.len());
        let new = &mut self.compiled.effects[given..];
        settle(new, |run| run.start);
        self.effects_given = self.compiled.effects.len();
        for run in &self.compiled.effects[given..] {
            let over = self.real_interval(Interval {
                from: run.start,
                to: run.start + run.duration,
            });
            begin(
                effects,
                EffectRun {
                    start: over.from,
                    duration: over.duration(),
                    ..run.clone()
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
        if !Arc::ptr_eq(&other.definition, &run.definition) || other.on != run.on {
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
