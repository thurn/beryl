//! The scripts the show holds: those waiting for the one before to end, and the one under way.

use std::rc::Rc;

use beryl_protocol::script::{self, StepTiming, Table as Played, Turn};
use beryl_protocol::{GameUpdate, PlayerView};

use super::Show;
use super::compile::Compiled;
use super::part::Part;
use crate::hud::counts::Counts;
use crate::light::{Lit, StreakRun};
use crate::verbs::Interval;
use game_table::table::{Motion, Objects};

/// An update and the timings of its script's steps, compiled to play after the scripts before
/// it.
pub(super) struct Waiting {
    /// How many scripts the show was given before this one.
    pub(super) number: usize,
    pub(super) update: Rc<GameUpdate>,
    /// Overrides the timings in the update where a caller supplies its own.
    pub(super) timings: Option<Rc<[StepTiming]>>,
    /// The table the script before it ended with, which it is played from.
    pub(super) from: Option<Rc<Played>>,
    /// What its steps compiled so far made, or the whole script once `part` is done.
    pub(super) compiled: Compiled,
    /// The script, while it is compiled over frames rather than whole (`super::part`).
    pub(super) part: Option<Part>,
    /// Whether its compiling has not begun: a script that arrives behind another is compiled
    /// as it starts (`Show::begin`), and until then `compiled` holds only its `finals`, and its
    /// `from` is the table the script before it ends with as `replay` works it out.
    pub(super) pending: bool,
}

/// The script under way.
pub(super) struct Playing {
    /// How many scripts the show was given before this one.
    pub(super) number: usize,
    /// Its update, until the table shows it. A script that moves nothing (a reset, the first
    /// view) shows its view as it starts and then only holds the time its beats take.
    pub(super) update: Option<Rc<GameUpdate>>,
    /// Overrides the timings in the update where a caller supplies its own.
    pub(super) timings: Option<Rc<[StepTiming]>>,
    /// The table the script before it ended with, which it is played from.
    pub(super) from: Option<Rc<Played>>,
    /// What its steps compiled so far made, or the whole script once `part` is done.
    pub(super) compiled: Compiled,
    /// The script, while it is compiled over frames rather than whole (`super::part`).
    pub(super) part: Option<Part>,
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
    /// How many of its `compiled.effects` and `compiled.notices` the show holds (`affect`,
    /// `announce`).
    pub(super) effects_given: usize,
    pub(super) notices_given: usize,
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
    /// `waiting`'s script, started at `start`, in nanoseconds since the client started, its
    /// clock running `rate` times faster than real time, with nothing given to the table yet.
    pub(super) fn started(waiting: Waiting, start: u64, rate: u64) -> Self {
        Self {
            number: waiting.number,
            update: Some(waiting.update),
            timings: waiting.timings,
            from: waiting.from,
            compiled: waiting.compiled,
            part: waiting.part,
            start,
            rate,
            spans_given: 0,
            streaks_given: 0,
            counts_given: 0,
            sounds_given: 0,
            effects_given: 0,
            notices_given: 0,
        }
    }

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

    /// The turn and step the table shows at `now`, in real time, while this script plays,
    /// from `before`, which it showed as the script started: the last its `set` steps have
    /// begun by then (`TurnChange`).
    fn turn_at(&self, now: u64, before: Option<Turn>) -> Option<Turn> {
        (self.compiled.turn_changes.iter())
            .take_while(|change| self.real(change.at) <= now)
            .last()
            .map_or(before, |change| Some(change.turn))
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

impl Show {
    /// The latest view’s resting objects, including any scripts still to play.
    #[must_use]
    pub fn latest_objects(&self) -> &Objects {
        (self.waiting.back().map(|waiting| &waiting.compiled.finals))
            .or(self
                .playing
                .as_ref()
                .map(|playing| &playing.compiled.finals))
            .unwrap_or(self.table.objects())
    }

    /// The turn and step the table shows at `now`, in nanoseconds since the client started:
    /// the view's it last showed at rest, changed by each `set` step of the playing script that
    /// has begun a step of the turn by `now`; `None` until the table has shown a view. The web
    /// draws its step row from the table it shows step by step in the same way, so the row
    /// shows a step of the turn as the script reaches it, not when its view arrives.
    #[must_use]
    pub fn turn_at(&self, now: u64) -> Option<Turn> {
        self.playing
            .as_ref()
            .map_or(self.turn, |playing| playing.turn_at(now, self.turn))
    }

    /// Every turn and step the table may show from now on, in order, with repeats: each the
    /// playing script's `set` steps begin and the step of the view it leads to, then each
    /// waiting script's, so that the client can build the step row for each ahead
    /// (`Client::prepare`). The playing script's are listed whether or not its clock has
    /// passed them: a script that starts in a frame starts at the time the one before ended,
    /// which may be before the last frame. A waiting script's are read from its steps, since
    /// one may be compiled only as it starts, in the frame that plays its first steps.
    pub fn coming_turns(&self) -> impl Iterator<Item = Turn> + '_ {
        let playing = self.playing.iter().flat_map(|playing| {
            let changes = (playing.compiled.turn_changes.iter()).map(|change| change.turn);
            let view = (playing.update.as_deref()).map(|update| turn_of(&update.view));
            changes.chain(view)
        });
        let waiting = self.waiting.iter().flat_map(|waiting| {
            let view = &waiting.update.view;
            let steps = view
                .script
                .iter()
                .flat_map(|script| script.beats.iter().flatten());
            let changes = steps.filter_map(|step| match step.act {
                script::Act::Set {
                    fact:
                        script::Fact::Turn {
                            number,
                            own_number,
                            active,
                            step,
                        },
                } => Some(Turn {
                    number,
                    own_number,
                    active,
                    step,
                }),
                _ => None,
            });
            changes.chain(std::iter::once(turn_of(view)))
        });
        playing.chain(waiting)
    }
}

/// The turn and step `view` is in.
pub(super) fn turn_of(view: &PlayerView) -> Turn {
    Turn {
        number: view.turn.number,
        own_number: view.turn.own_number,
        active: view.turn.active,
        step: view.turn.step,
    }
}
