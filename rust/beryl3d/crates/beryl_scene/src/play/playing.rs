//! The scripts the show holds: those waiting for the one before to end, and the one under way.

use beryl_protocol::GameUpdate;
use beryl_protocol::script::{self, StepTiming, Table as Played};

use super::compile::Compiled;
use crate::hud::counts::Counts;
use crate::light::{Lit, StreakRun};
use crate::table::Motion;
use crate::verbs::Interval;

/// An update and the timings of its script's steps, compiled to play after the scripts before
/// it.
pub(super) struct Waiting {
    /// How many scripts the show was given before this one.
    pub(super) number: usize,
    pub(super) update: Box<GameUpdate>,
    /// Overrides the timings in the update where a caller supplies its own.
    pub(super) timings: Option<Vec<StepTiming>>,
    /// The table the script before it ended with, which it is played from.
    pub(super) from: Option<Played>,
    pub(super) compiled: Compiled,
}

/// The script under way.
pub(super) struct Playing {
    /// How many scripts the show was given before this one.
    pub(super) number: usize,
    /// Its update, until the table shows it. A script that moves nothing (a reset, the first
    /// view) shows its view as it starts and then only holds the time its beats take.
    pub(super) update: Option<Box<GameUpdate>>,
    /// Overrides the timings in the update where a caller supplies its own.
    pub(super) timings: Option<Vec<StepTiming>>,
    /// The table the script before it ended with, which it is played from.
    pub(super) from: Option<Played>,
    pub(super) compiled: Compiled,
    /// When it started, in nanoseconds since the client started.
    pub(super) start: u64,
    /// How many times faster than real time its clock runs.
    pub(super) rate: u64,
    /// How many of its `compiled.spans` and `compiled.streaks` have reached the table's `Lit`.
    pub(super) spans_given: usize,
    pub(super) streaks_given: usize,
    pub(super) counts_given: usize,
    /// How many of its `compiled.sounds` the frames have listed.
    pub(super) sounds_given: usize,
}

/// A session update already owns its timings. Other callers may supply their own.
pub(super) fn timings_of<'a>(
    update: &'a GameUpdate,
    timings: Option<&'a [StepTiming]>,
) -> &'a [StepTiming] {
    timings.unwrap_or_else(|| {
        update
            .view
            .script
            .as_ref()
            .map_or(&[], |script| &script.timing)
    })
}

impl Playing {
    /// What moves on the table at `now` while this script plays, once its update is in hand.
    pub(super) fn motion(&self, now: u64) -> Option<Motion<'_>> {
        let update = self.update.as_ref()?;
        Some(Motion {
            tracks: &self.compiled.tracks,
            ability_tracks: &self.compiled.ability_tracks,
            t: self.clock(now),
            finals: &self.compiled.finals,
            view: &update.view,
            turns: &self.compiled.turns,
            pile_counts: &self.compiled.pile_counts,
        })
    }

    /// The time on the script's own clock at `now`.
    pub(super) fn clock(&self, now: u64) -> u64 {
        now.saturating_sub(self.start).saturating_mul(self.rate)
    }

    /// The real time at which its clock reads `t`, rounded up.
    pub(super) fn real(&self, t: u64) -> u64 {
        self.start + t.div_ceil(self.rate)
    }

    /// `over`, on its clock, in real time.
    pub(super) fn real_interval(&self, over: Interval) -> Interval {
        Interval {
            from: self.real(over.from),
            to: self.real(over.to),
        }
    }

    /// Gives `lit` each span change and streak, and `counts` each count change, of the script
    /// whose real time is at or before `now`, in order, that it has not given yet.
    pub(super) fn light(&mut self, now: u64, lit: &mut Lit, counts: &mut Counts) {
        self.give_counts(now, counts);
        while let Some(change) = self.compiled.spans.get(self.spans_given) {
            let over = self.real_interval(change.over);
            if over.from > now {
                break;
            }
            match change.end {
                script::Span::Begin => {
                    lit.begin_in_script(change.span, over, self.start, change.over.to);
                }
                script::Span::End => lit.end(&change.span, over),
            }
            self.spans_given += 1;
        }
        while let Some(run) = self.compiled.streaks.get(self.streaks_given) {
            let over = self.real_interval(run.over);
            if over.from > now {
                break;
            }
            lit.streak(StreakRun { over, ..*run });
            self.streaks_given += 1;
        }
    }
}
