//! Laying a script out again as it starts (docs/beryl-3d.md, "Timing a script"). A script is
//! compiled when its update arrives, which may be seconds before it starts, and two of the
//! inputs of `beat` are known only as it starts: since when the span a step ends has stood
//! whole (`StepTiming::hold`: a target arrow's end waits until the arrow has stood whole for
//! 290 ms), and until when something passes over the table (`StepTiming::alone`: a coin waits
//! until the turn's banner has gone). The web interface lays each beat out as it begins, with
//! both read from what its overlay shows then (`Overlay::whole` and `Overlay::passed` in
//! `ui/src/stage/overlay.ts`).
//!
//! When a script with such a step starts, every beat is laid out again with both answered, and
//! each thing the script holds that has a time (segments, spans, streaks, notices, effects,
//! counts and sounds) moves by how much later the step it keeps to
//! now plays. No pose is worked out again, which is exact because the steps that wait move
//! nothing: the session ends a span in a beat of its own before the beat that moves either of
//! its ends (`beryl_protocol::script`, "Spans"), and a coin moves no card.

use beryl_protocol::script::{self, Act, Script, StepTiming};

use super::Show;
use super::compile::{Compiled, nanoseconds};
use super::playing::Waiting;
use crate::beat::{Laid, lay};
use crate::light::{ArrowSpan, Lit, MarkSpan, Span};
use crate::motion::Track;
use crate::verbs::{Follows, StepTime};

impl Show {
    /// How many scripts were laid out again as they started, for tests: a script with no step
    /// that holds a span or waits for the table to itself is not.
    #[must_use]
    pub fn relays(&self) -> usize {
        self.relays
    }

    /// Lays `waiting`'s script out again as it starts at `start`, its clock running `rate` times
    /// faster than real time, if any of its steps waits for a span to stand whole or for the
    /// table to itself. Allocates nothing unless the show keeps a history, whose record of the
    /// script then shows the steps' new times.
    pub(super) fn relay(&mut self, waiting: &mut Waiting, start: u64, rate: u64) {
        let Some(script) = waiting.update.view.script.as_ref() else {
            return;
        };
        let compiled = &mut waiting.compiled;
        // A script the table takes at once has no wait to lay out again, beyond the announcements
        // of a reset, which wait for nothing a script before them shows.
        if compiled.immediate
            || compiled.laid.is_empty()
            || compiled.laid.len() != waiting.timings.len()
        {
            return;
        }
        let waits = waiting
            .timings
            .iter()
            .any(|timing| timing.hold.is_some() || timing.alone);
        if waits {
            let passed = self.passed();
            let starting = Starting {
                lit: &self.lit,
                passed: (passed > 0).then_some(passed),
                start,
                rate,
            };
            let end = again(compiled, script, &waiting.timings, &starting);
            shift(compiled, end);
            self.relays += 1;
            self.write_record(waiting.number, &waiting.compiled);
        }
    }
}

/// What the table shows as a script starts at `start`, in nanoseconds since the client started,
/// its clock running `rate` times faster than real time: the spans open (`lit`) and when the
/// last announcement of the scripts before it to pass over the table ends (`Show::passed`), if
/// one is held.
struct Starting<'a> {
    lit: &'a Lit,
    passed: Option<u64>,
    start: u64,
    rate: u64,
}

impl Starting<'_> {
    /// `real`, in nanoseconds since the client started, on the script's clock in milliseconds:
    /// below zero before the script starts.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a client runs for hours, far below f64's exact range in nanoseconds"
    )]
    fn clock(&self, real: u64) -> f64 {
        (real as f64 - self.start as f64) * self.rate as f64 / 1e6
    }
}

/// The span `act` begins or ends and which, for an arrow or a mark: the spans the web
/// interface's `Spans::whole` answers for.
fn span_of(act: &Act) -> Option<(Span, script::Span)> {
    match *act {
        Act::Point {
            span,
            arrow,
            from,
            to,
        } => Some((Span::Arrow(ArrowSpan { arrow, from, to }), span)),
        Act::Mark { span, mark, on } => Some((Span::Mark(MarkSpan { mark, on }), span)),
        _ => None,
    }
}

/// When the last of the announcements among `script`'s steps, laid out as `laid`, that pass
/// over the table ends, in milliseconds on its clock: what a step that waits for the table to
/// itself waits for (`Overlay::passed`). One that lasts no time is not shown.
fn passing(script: &Script, timings: &[StepTiming], laid: &[Laid]) -> Option<f64> {
    script
        .beats
        .iter()
        .flatten()
        .zip(timings)
        .zip(laid)
        .filter(|((step, timing), laid)| {
            matches!(step.act, Act::Announce { .. }) && timing.passes && laid.to > laid.from
        })
        .map(|(_, laid)| laid.to)
        .reduce(f64::max)
}

/// When the room `laid` leaves for what makes room or closes up round its step begins, as
/// `Follows::Room` keeps to it: as its act is applied when that goes ahead of the step, and as
/// the step begins otherwise.
fn room(laid: &Laid) -> f64 {
    laid.opens.map_or(laid.from, |opens| opens.at)
}

/// How much later than `old` the times of a step laid out as `new` are, in nanoseconds, for
/// what keeps to `follows`. Laying a script out again only makes a step wait longer, never less.
fn later(old: &Laid, new: &Laid, follows: Follows) -> u64 {
    let room = follows == Follows::Room && old.opens.is_some() && new.opens.is_some();
    let (was, now) = if room {
        (self::room(old), self::room(new))
    } else {
        (old.from, new.from)
    };
    nanoseconds(now).saturating_sub(nanoseconds(was))
}

/// Lays every beat of `compiled`'s script out again into its `scratch`, as `starting` finds the
/// table, and returns when the last beat is over, in nanoseconds. The span a step ends has stood
/// whole since the step of this script that began it ended, or since `Lit` says, for one a
/// script before began; the announcements that pass over the table are this script's from the
/// beats before, and the scripts' before it.
fn again(
    compiled: &mut Compiled,
    script: &Script,
    timings: &[StepTiming],
    starting: &Starting<'_>,
) -> u64 {
    let carried = starting.passed.map_or(0.0, |passed| starting.clock(passed));
    let mut at = 0.0;
    let mut first = 0;
    for steps in &script.beats {
        let (done, rest) = compiled.scratch.split_at_mut(first);
        let done = &*done;
        let before = || script.beats.iter().flatten().take(first);
        let whole = |act: &Act| {
            let (span, _) = span_of(act)?;
            // The latest step before this beat that begins or ends the span, if any does.
            let latest = before()
                .zip(done)
                .filter_map(|(step, laid)| {
                    span_of(&step.act)
                        .filter(|(other, _)| *other == span)
                        .map(|(_, edge)| (edge, laid.to))
                })
                .last();
            match latest {
                Some((script::Span::Begin, whole)) => Some(whole),
                Some((script::Span::End, _)) => None,
                None => starting
                    .lit
                    .whole_since(&span)
                    .map(|since| starting.clock(since)),
            }
        };
        let passed =
            passing(script, &timings[..first], done).map_or(carried, |own| own.max(carried));
        let own = first..first + steps.len();
        let laid = &mut rest[..steps.len()];
        let mut end = lay(steps, &timings[own.clone()], at, whole, passed, laid);
        for (index, new) in own.zip(laid.iter()) {
            if let Some(closed) = compiled.closes[index] {
                let old = &compiled.laid[index];
                end = end.max(closed + room(new) - room(old));
            }
        }
        at = end;
        first += steps.len();
    }
    nanoseconds(at)
}

/// Moves every timed thing `compiled` holds by how much later the step it keeps to plays in
/// `scratch` than in `laid`, sets the script's end to `end`, and makes `scratch` its `laid`.
/// Allocates nothing.
fn shift(compiled: &mut Compiled, end: u64) {
    let Compiled {
        laid,
        scratch,
        tracks,
        ability_tracks,
        segments,
        steps,
        spans,
        streaks,
        notices,
        effects,
        counts,
        sounds,
        turns,
        ..
    } = compiled;
    // A test may plant a step of its own in the records (`Plant`), which no beat laid out and
    // which stays where it is.
    let later_of = |time: StepTime| match (laid.get(time.number), scratch.get(time.number)) {
        (Some(old), Some(new)) => later(old, new, time.follows),
        _ => 0,
    };
    let start = |number: usize| {
        later_of(StepTime {
            number,
            follows: Follows::Start,
        })
    };
    debug_assert!(
        waiting_steps_move_nothing(laid, scratch, tracks, ability_tracks),
        "a step that waits longer than the steps of its beat before it moves nothing"
    );
    let segment_later = |segment: &crate::pose::Segment| segment.step.map_or(0, later_of);
    for track in tracks
        .iter_mut()
        .chain(ability_tracks.iter_mut().map(|(_, track)| track))
    {
        track.shift(segment_later);
    }
    for record in segments.iter_mut() {
        record.segment.start += segment_later(&record.segment);
    }
    // One record for each step, in order.
    for (number, record) in steps.iter_mut().enumerate() {
        let by = start(number);
        record.from += by;
        record.to += by;
    }
    for change in spans.iter_mut() {
        let by = start(change.step);
        change.over.from += by;
        change.over.to += by;
    }
    settle(spans, |change| change.over.from);
    for run in streaks.iter_mut() {
        let by = start(run.step);
        run.over.from += by;
        run.over.to += by;
    }
    settle(streaks, |run| run.over.from);
    for run in notices.iter_mut() {
        let by = start(run.step);
        run.over.from += by;
        run.over.to += by;
    }
    settle(notices, |run| run.over.from);
    for run in effects.iter_mut() {
        run.start += start(run.step);
    }
    settle(effects, |run| run.start);
    for change in counts.iter_mut() {
        change.at += start(change.step);
    }
    settle(counts, |change| change.at);
    for run in sounds.iter_mut() {
        run.at += start(run.step);
    }
    settle(sounds, |run| run.at);
    for turn in turns.iter_mut() {
        turn.at += start(turn.step);
    }
    settle(turns, |turn| turn.at);
    compiled.end = end;
    std::mem::swap(&mut compiled.laid, &mut compiled.scratch);
}

/// Takes the times of `playing`, the script under way, which may have been laid out again as it
/// started, into `compiled`, the same script compiled again for a window of a new shape.
pub(super) fn follow(compiled: &mut Compiled, playing: &Compiled) {
    if compiled.laid.len() != playing.laid.len() || compiled.laid == playing.laid {
        return;
    }
    compiled.scratch.copy_from_slice(&playing.laid);
    shift(compiled, playing.end);
}

/// Sorts `items` by `key`, keeping the order of two with the same key, without allocating: the
/// items are almost always in order already.
fn settle<T>(items: &mut [T], key: impl Fn(&T) -> u64) {
    for i in 1..items.len() {
        let mut j = i;
        while j > 0 && key(&items[j - 1]) > key(&items[j]) {
            items.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// Whether no step that now waits longer than the step before it has a segment on `tracks` or
/// `ability_tracks`: such a step would carry a card from where it was when it was compiled to play, and
/// no pose is worked out again. The first step of a beat is compared with the last of the beat
/// before, which it follows through the beat's start. Only checked in debug builds.
fn waiting_steps_move_nothing(
    laid: &[Laid],
    scratch: &[Laid],
    tracks: &[Track],
    ability_tracks: &[(u32, Track)],
) -> bool {
    let abilities = ability_tracks.iter().map(|(_, track)| track);
    let mut previous = 0;
    (0..laid.len()).all(|number| {
        let by = later(&laid[number], &scratch[number], Follows::Start);
        let waits = by > previous;
        previous = by;
        !waits
            || !tracks
                .iter()
                .chain(abilities.clone())
                .flat_map(Track::segments)
                .any(|segment| segment.step.is_some_and(|step| step.number == number))
    })
}
