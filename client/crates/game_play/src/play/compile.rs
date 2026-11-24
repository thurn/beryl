//! Compiling a script: every step of every beat turned into segments at times on the script's
//! own clock, before the script starts, so that where each object is at any moment of the script
//! is a function of the time alone (docs/beryl-3d.md, "Determinism").
//!
//! The script is played over a copy of the table it starts from: after each step, the resting
//! places of the parts of the table its act changed are worked out again, and the step's verb
//! carries its object there. An object no step names whose resting place the step changed (the
//! rest of a hand) slides there as the step's `StepTiming::room` says: over the step, from a
//! share of it, or for the cards that stay in a hand for a set time from when the act is
//! applied; or over the time the step leaves for what it opened or closed.

use std::sync::Arc;

use beryl_protocol::protocol::{PlayerView, StackItem};
use beryl_protocol::script::{
    Act, Edge, Script, Step, StepTiming, Subject, Table as Played, Tally, Verb, mover_of, replay,
};

use super::record::{MoveKey, Plant, Planted, SegmentRecord, StepRecord};
use super::relay::settle;
use super::step::{Motion, add, numbers_of};
use crate::beat::{Laid, beat};
use crate::hud::counts::CountChange;
use crate::light::{MarkSpan, Span, SpanChange, StreakRun};
use crate::named_curve;
use crate::notice::NoticeRun;
use crate::play::TurnChange;
use crate::sound::{SoundRun, Sounds};
use crate::verbs::{self, Interval};
use game_table::table::{Objects, PileCount, Table, Turn};
use kit_base::data::effect::{Effects, Target};
use kit_base::pose::Segment;
use kit_fx::effects::EffectRun;
use kit_space::motion::Track;

/// A script laid out in time, ready to start.
#[derive(Clone, Debug, Default)]
pub(crate) struct Compiled {
    /// One track for each object that moves, by slot. An object whose track is empty stands at
    /// its resting place in the view the table showed before the script.
    pub tracks: Vec<Track>,
    /// One track for each ability on the stack that moves, sorted by number.
    pub ability_tracks: Vec<(u32, Track)>,
    /// The scene objects at rest in the view the script leads to, which the table takes when
    /// the script ends.
    pub finals: Objects,
    /// How long the script lasts on its own clock, in nanoseconds.
    pub end: u64,
    /// The verbs among its steps that are drawn as instant changes, one bit per verb in the order
    /// of `Verb::ALL`.
    pub unbuilt: u32,
    /// Every verb among its steps, built or not, one bit per verb in the order of `Verb::ALL`,
    /// for the host's log of late frames.
    pub verbs: u32,
    /// Every step, as it was laid out, for the interface checks.
    pub steps: Vec<StepRecord>,
    /// Every segment of `tracks`, in the order it was added, with the step that made it.
    pub segments: Vec<SegmentRecord>,
    /// How many of its steps the table played over could not take, each of which made the step
    /// after it place the whole table instead of moving its objects.
    pub unplaced: usize,
    /// Whether it starts over from its view (`Script::reset`), which drops every span shown.
    pub reset: bool,
    /// Whether the table takes its steps at once, with no table before it or with a reset: only
    /// a reset's announcements play (`lay_out`).
    pub immediate: bool,
    /// The spans its verbs begin and end, ordered by when each begins on its clock, and as its steps wrote them
    /// where two begin together.
    pub spans: Vec<SpanChange>,
    /// The streaks its verbs play, ordered by when each begins on its clock, and as its steps wrote them
    /// where two begin together.
    pub streaks: Vec<StreakRun>,
    /// The banners and pills its verbs show, ordered by when each begins on its clock, and as its steps wrote them
    /// where two begin together.
    pub notices: Vec<NoticeRun>,
    /// The effects its steps begin (`StepTiming::effects`), ordered by when each begins on its
    /// clock, and as its steps wrote them where two begin together.
    pub effects: Vec<EffectRun>,
    /// The life totals and pools its `count` steps set, ordered by when each is set on its
    /// clock, and as its steps wrote them where two are set together.
    pub counts: Vec<CountChange>,
    /// The steps of the turn its `set` steps begin, ordered by when each begins on its clock.
    pub turn_changes: Vec<TurnChange>,
    /// The sounds its steps ask for, ordered by when each begins on its clock.
    pub sounds: Vec<SoundRun>,
    /// When cards show the record the script leads to, where a step says, ordered by time.
    pub turns: Vec<Turn>,
    pub pile_counts: Vec<PileCount>,
    /// When each of its steps plays, in order, as its beats were laid out; empty when it has no
    /// steps to play (no script, or timings that do not match its steps).
    pub laid: Vec<Laid>,
    /// Room for laying every step out again as the script starts (`relay`), so that doing so
    /// allocates nothing.
    pub scratch: Vec<Laid>,
    /// For each step, when what closed up after it has closed, in milliseconds on the script's
    /// clock, if anything did: its beat lasts until then.
    pub closes: Vec<Option<f64>>,
}

impl Compiled {
    /// Changes the script to hold a fault a test plants, and lays its tracks out again from its
    /// segments. Returns whether the script had a step to plant it on.
    pub(crate) fn plant(&mut self, plant: &Plant) -> bool {
        if !plant.apply(&mut self.steps, &mut self.segments) {
            return false;
        }
        if plant.kind == Planted::Mark
            && let Some(index) = plant.step(&self.steps)
            && let Some(slot) = self.steps[index].subject
        {
            let step = &self.steps[index];
            let slot = u16::try_from(slot).expect("a slot fits an object id");
            self.spans.push(SpanChange {
                span: Span::Mark(MarkSpan {
                    mark: beryl_protocol::script::Mark::Selected,
                    on: Subject::Object {
                        slot: beryl_protocol::protocol::ObjectId(slot),
                    },
                }),
                end: beryl_protocol::script::Span::Begin,
                over: Interval {
                    from: step.from,
                    to: step.to,
                },
                step: index,
            });
        }
        self.tracks.clear();
        for record in &self.segments {
            add(&mut self.tracks, record.slot, record.segment);
        }
        true
    }
}

/// Lays out `after`'s script, played from `before`, the view the table shows when it starts,
/// and `played`, the table the script before it ended with, with one timing per step in
/// `timings`. Returns it with the table this script ends with, which the next script is played
/// from: the scripts number the abilities on the stack, and a table made from a view alone
/// knows none of those numbers. With `records`, it also keeps a record of each step and
/// of the step that made each segment, for the interface checks; a running game does without,
/// so that compiling allocates no more than playing needs. `shown` says whether `before` is the
/// view the table shows at rest, whose resting places the table holds already. With no `before`
/// (the first view), with a reset, or
/// with timings that do not match the steps, nothing moves: the table takes `after` as it stands
/// when the script ends. Of the first view's script, nothing plays, and of a reset's only its
/// announcements, as on the web stage.
pub(crate) fn compile<'a>(
    before: Option<(&'a PlayerView, &'a Played)>,
    after: &'a PlayerView,
    timings: &'a [StepTiming],
    table: &Table,
    records: bool,
    shown: bool,
) -> (Compiled, Played) {
    let mut compiled = Compiled::default();
    let mut compiling = Compiling::new(
        before,
        (after, timings),
        table,
        (records, shown),
        &mut compiled,
    );
    let played = compiling.finish(table, &mut compiled);
    (compiled, played)
}

/// How many steps of a script `Compiling::advance` compiles at least each time it is called,
/// beyond those it must: enough to reach the later beats of a long script well before they
/// begin, which leaves the frames that must compile a beat as it begins little to do.
pub const STEPS_PER_ADVANCE: usize = 4;

/// A script being compiled one step at a time, in order, so that its compiling can be spread
/// over several frames (docs/beryl-3d.md, "Threads"). Laying a beat out in time is cheap and
/// is done when its first step is compiled; playing each step over the table (`Motion::step`)
/// is what costs. Every step is compiled before the frame in which it begins, and before any
/// segment it could change ends: a step changes a segment an earlier step made only where it
/// sends a card that is still on its way to a new place (`retarget`), and only a segment that
/// ends after the step begins. So compiling at most the steps `advance` asks for leaves every
/// pose up to its time as compiling the whole script would.
///
/// What the steps compiled so far made is in a `Compiled` the caller holds and passes to every
/// call, so that the frames read it as they read a script compiled whole: its tracks, and its
/// spans, streaks, notices, counts, sounds, turns and pile counts, each list in the order
/// compiling the whole script gives (`advance`). Its effects are kept in the order the steps
/// wrote them until `finish`. Its `end` and `finals` are set by `finish`.
pub(crate) struct Compiling<'a> {
    script: Option<&'a Script>,
    after: &'a PlayerView,
    timings: &'a [StepTiming],
    /// The table the script before it ended with, if there was a view before it.
    from: Option<&'a Played>,
    records: bool,
    /// The table as the script plays over it; none when nothing moves (`immediate`). Its tracks
    /// are in the caller's `Compiled` between calls.
    motion: Option<Motion<'a>>,
    sounds: Option<Sounds>,
    /// The table the script ends with, once it is known without playing the script: with no
    /// script, or timings that do not match its steps.
    ended: Option<Played>,
    /// The beat of the next step to compile, the index of that step in its beat, and its number
    /// in the script.
    beat: usize,
    index: usize,
    number: usize,
    /// When the beat of the next step begins, in milliseconds on the script's clock.
    at: f64,
    /// The steps of the beat of the next step, laid out, once its first step is compiled.
    laid: Vec<Laid>,
    /// When the beat of the next step ends, as it was laid out, before anything closes up.
    end: f64,
    /// Whether the beat of the next step has been laid out.
    beat_laid: bool,
}

impl<'a> Compiling<'a> {
    /// `after`'s script, ready to compile, as `compile` describes, timed by `timings`, with
    /// `out` made ready to hold it.
    pub(crate) fn new(
        before: Option<(&'a PlayerView, &'a Played)>,
        (after, timings): (&'a PlayerView, &'a [StepTiming]),
        table: &Table,
        (records, shown): (bool, bool),
        out: &mut Compiled,
    ) -> Self {
        let mut compiling = Self {
            script: after.script.as_ref(),
            after,
            timings,
            from: before.map(|(_, played)| played),
            records,
            motion: None,
            sounds: None,
            ended: None,
            beat: 0,
            index: 0,
            number: 0,
            at: 0.0,
            laid: Vec::new(),
            end: 0.0,
            beat_laid: false,
        };
        let Some(script) = compiling.script else {
            compiling.ended = Some(numbered(Played::of(after)));
            return compiling;
        };
        out.reset = script.reset.is_some();
        out.verbs = script
            .beats
            .iter()
            .flatten()
            .fold(0, |verbs, step| verbs | bit(step.act.verb()));
        if timings.len() != script.beats.iter().map(Vec::len).sum::<usize>() {
            compiling.ended = Some(ended(compiling.from, after, script));
            return compiling;
        }
        compiling.motion = before
            .filter(|_| script.reset.is_none())
            .map(|(before, played)| {
                Motion::new(before, played, (table, after, timings), records, shown)
            });
        // The steps sound whether or not they move anything.
        compiling.sounds = Some(Sounds::new(script, after, compiling.motion.is_some()));
        // With no table before it, or with a reset, the table takes the script's steps at once,
        // and only a reset's announcements play, as the web stage's `play` does (`resetSteps` in
        // `ui/src/stage/table.ts`).
        out.immediate = compiling.motion.is_none();
        out.laid.reserve_exact(timings.len());
        out.closes.reserve_exact(timings.len());
        if let Some(motion) = &mut compiling.motion {
            // The tracks are the caller's between calls (`advance`).
            std::mem::swap(&mut motion.tracks, &mut out.tracks);
        }
        compiling
    }

    /// Whether the script plays over a table, step by step, so that compiling it costs a step
    /// at a time: not the first view's, a reset's, or one whose timings do not match its steps.
    pub(crate) fn moves(&self) -> bool {
        self.motion.is_some()
    }

    /// Whether every step has been compiled.
    pub(crate) fn done(&self) -> bool {
        self.ended.is_some()
            || self
                .script
                .is_none_or(|script| self.beat == script.beats.len())
    }

    /// Compiles every step that begins by `until`, on the script's clock in nanoseconds, then
    /// every step that could change a segment under way at `until`, and then more steps in
    /// order until it has compiled `least` steps, or all of them, into `out`. Returns how many
    /// it compiled.
    ///
    /// The steps of a beat need not begin in the order they are written, and the show gives
    /// the table each span, streak, count and sound once its time comes, in order, so each of
    /// those lists in `out` is sorted again, keeping the order of two that begin together.
    /// Sorting a whole script's lists so gives the same order. A step compiled later begins
    /// later than every step compiled by `until`, so what it writes sorts after all that has
    /// begun by then.
    pub(crate) fn advance(
        &mut self,
        table: &Table,
        out: &mut Compiled,
        (until, least): (u64, usize),
    ) -> usize {
        if let Some(motion) = &mut self.motion {
            std::mem::swap(&mut motion.tracks, &mut out.tracks);
            std::mem::swap(&mut motion.ability_tracks, &mut out.ability_tracks);
            std::mem::swap(&mut motion.segments, &mut out.segments);
        }
        let mut count = 0;
        while self.next_begins(out).is_some_and(|begins| begins <= until) {
            self.step(table, out);
            count += 1;
        }
        // A step that begins later than `until` makes segments that begin later still, so
        // compiling one does not move this horizon.
        let horizon = self.horizon(until);
        while self.next_begins(out).is_some_and(|begins| begins < horizon) {
            self.step(table, out);
            count += 1;
        }
        while count < least && self.next_begins(out).is_some() {
            self.step(table, out);
            count += 1;
        }
        if let Some(motion) = &mut self.motion {
            std::mem::swap(&mut motion.tracks, &mut out.tracks);
            std::mem::swap(&mut motion.ability_tracks, &mut out.ability_tracks);
            std::mem::swap(&mut motion.segments, &mut out.segments);
            let written = &mut motion.written;
            take(&mut written.spans, &mut out.spans, |change| {
                change.over.from
            });
            take(&mut written.streaks, &mut out.streaks, |run| run.over.from);
            take(&mut written.notices, &mut out.notices, |run| run.over.from);
            take(&mut written.counts, &mut out.counts, |change| change.at);
            take(&mut written.turn_changes, &mut out.turn_changes, |change| {
                change.at
            });
            take(&mut written.turns, &mut out.turns, |turn| {
                (turn.slot, turn.at)
            });
            take(
                &mut written.pile_counts,
                &mut out.pile_counts,
                PileCount::order,
            );
            out.unplaced = motion.unplaced;
        }
        if count > 0
            && let Some(sounds) = &self.sounds
        {
            out.sounds = sounds.runs();
        }
        count
    }

    /// When the latest segment under way at `t` ends, in nanoseconds, or `t` when none is.
    fn horizon(&self, t: u64) -> u64 {
        let Some(motion) = &self.motion else {
            return t;
        };
        let abilities = motion.ability_tracks.iter().map(|(_, track)| track);
        motion
            .tracks
            .iter()
            .chain(abilities)
            .flat_map(Track::segments)
            .filter(|segment| segment.start <= t)
            .map(Segment::end)
            .fold(t, u64::max)
    }

    /// When the next step to compile, or a step after it in its beat, begins, whichever is
    /// earliest, in nanoseconds; `None` once every step is compiled. Lays the beat out when
    /// the next step is its first, and passes over beats with no steps.
    fn next_begins(&mut self, out: &mut Compiled) -> Option<u64> {
        let script = self.script.filter(|_| self.ended.is_none())?;
        while !self.beat_laid {
            if self.beat == script.beats.len() {
                return None;
            }
            self.lay_beat(script, out);
            if script.beats[self.beat].is_empty() {
                self.end_beat(script, out);
            }
        }
        // The steps of a beat need not begin in the order they are written.
        self.laid[self.index..].iter().map(begins).min()
    }

    /// Lays out the beat of the next step, which is the beat's first, from `at`. When the table
    /// takes the script's steps at once, the steps that play are laid out as a beat of their
    /// own, and every other step takes no time where the beat begins.
    fn lay_beat(&mut self, script: &Script, out: &mut Compiled) {
        let steps = &script.beats[self.beat];
        let timings = &self.timings[self.number..self.number + steps.len()];
        let at = self.at;
        self.end = if out.immediate {
            let plays = |step: &Step| plays(script, true, step);
            let (playing, timed): (Vec<Step>, Vec<StepTiming>) = steps
                .iter()
                .zip(timings)
                .filter(|(step, _)| plays(step))
                .map(|(step, timing)| (step.clone(), timing.clone()))
                .unzip();
            let mut own = Vec::new();
            let end = beat(&playing, &timed, at, |_| None, at, &mut own);
            let mut own = own.into_iter();
            self.laid.clear();
            self.laid.extend(steps.iter().map(|step| {
                let still = Laid {
                    turn: at,
                    from: at,
                    to: at,
                    opens: None,
                };
                if plays(step) {
                    own.next().unwrap_or(still)
                } else {
                    still
                }
            }));
            end
        } else {
            // No span is open and nothing passes over the table yet: the flat layer draws
            // neither.
            beat(steps, timings, at, |_| None, at, &mut self.laid)
        };
        out.closes.resize(self.number + steps.len(), None);
        self.beat_laid = true;
    }

    /// Ends the beat of the steps just compiled: it lasts until the last of them is over and
    /// what closed up after them has closed, as the web stage's does (`closedBy` in
    /// `ui/src/stage/tracks.ts`), and the next beat begins then.
    fn end_beat(&mut self, script: &Script, out: &mut Compiled) {
        let first = self.number - self.index;
        self.at = out.closes[first..]
            .iter()
            .flatten()
            .fold(self.end, |end, &closed| end.max(closed));
        self.beat += 1;
        self.index = 0;
        self.beat_laid = false;
        if self.beat == script.beats.len() {
            out.end = nanoseconds(self.at);
        }
    }

    /// Compiles the next step, whose beat is laid out, into `out`, whose tracks the motion
    /// holds for the call.
    fn step(&mut self, table: &Table, out: &mut Compiled) {
        let Some(script) = self.script else {
            return;
        };
        let (beat, index, number) = (self.beat, self.index, self.number);
        let steps = &script.beats[beat];
        let step = &steps[index];
        let timing = &self.timings[number];
        let laid = self.laid[index];
        let verb = step.act.verb();
        if !verbs::built(verb) {
            out.unbuilt |= bit(verb);
        }
        if plays(script, out.immediate, step) {
            let cues = self.sounds.as_mut().map(|sounds| {
                sounds.step(
                    self.motion.as_ref().map(|motion| &motion.played),
                    (steps, index, number),
                    timing,
                    &laid,
                )
            });
            if let Some(motion) = self.motion.as_mut() {
                if index == 0 {
                    motion.begin_beat();
                }
                motion.step(table, number, (steps, index), timing, &laid);
                if index + 1 == steps.len() {
                    let first = number - index;
                    motion.closings_into(first, &mut out.closes[first..]);
                }
            }
            let definitions = &table.data().effects;
            effects_of(number, step, timing, &laid, definitions, &mut out.effects);
            if self.records {
                out.steps.push(StepRecord {
                    view: 0,
                    beat,
                    index,
                    verb,
                    subject: subject_of(step),
                    from: nanoseconds(laid.from),
                    to: nanoseconds(laid.to),
                    timed: nanoseconds(timing.duration),
                    key: MoveKey::of(&step.act),
                    cues: cues.unwrap_or_default(),
                });
            }
        }
        out.laid.push(laid);
        self.index += 1;
        self.number += 1;
        if self.index == steps.len() {
            self.end_beat(script, out);
        }
    }

    /// Compiles every step left into `out` and completes it: its end, its effects in the order
    /// they begin, and `finals`, the scene objects at rest in the view the script leads to.
    /// Returns the table the script ends with.
    pub(crate) fn finish(&mut self, table: &Table, out: &mut Compiled) -> Played {
        self.advance(table, out, (u64::MAX, 0));
        let after = self.after;
        let at_rest = |played: &Played| {
            let numbers = numbers_of(played);
            let placement = table.placement(after, &numbers);
            table.objects_for(after, numbers, &placement)
        };
        if let Some(played) = self.ended.take() {
            out.finals = at_rest(&played);
            return played;
        }
        let Some(script) = self.script else {
            unreachable!("a script with no steps to play has ended");
        };
        // Every beat has been laid out, and the last one ends the script, even where there is
        // no beat at all.
        out.end = nanoseconds(self.at);
        if let Some(sounds) = &self.sounds {
            out.sounds = sounds.runs();
        }
        out.scratch.clone_from(&out.laid);
        // The steps of a beat need not begin in the order they are written, nor their effects.
        out.effects.sort_by_key(|run| run.start);
        let Some(motion) = self.motion.take() else {
            let played = ended(self.from, after, script);
            out.finals = at_rest(&played);
            return played;
        };
        // After a step the table could not take, it no longer matches the game, and the view's
        // table is the best guess.
        let fallback = (motion.unplaced != 0).then(|| numbered(Played::of(after)));
        let numbers = numbers_of(fallback.as_ref().unwrap_or(&motion.played));
        let placement = motion.placement_of(table, after, &numbers);
        out.finals = table.objects_for(after, numbers, &placement);
        fallback.unwrap_or(motion.played)
    }
}

/// Whether `step` of `script` plays: every step does, unless the table takes the script's
/// steps at once (`immediate`), when only a reset's announcements do.
fn plays(script: &Script, immediate: bool, step: &Step) -> bool {
    !immediate || (script.reset.is_some() && matches!(step.act, Act::Announce { .. }))
}

/// When anything of a step laid out as `laid` begins, in nanoseconds: the step itself, or
/// what makes room for it as its act is applied ahead of it (`room_of`).
fn begins(laid: &Laid) -> u64 {
    nanoseconds(
        laid.opens
            .map_or(laid.from, |opens| opens.at.min(laid.from)),
    )
}

/// Moves what the steps just compiled wrote, `written`, to the end of `list`, and sorts `list`
/// by `key` again, keeping the order of two with the same key, without allocating beyond
/// `list`'s growth.
fn take<T, K: Ord>(written: &mut Vec<T>, list: &mut Vec<T>, key: impl Fn(&T) -> K) {
    if written.is_empty() {
        return;
    }
    list.append(written);
    settle(list, key);
}

/// The table `script`, which came with `after`, ends with when it is not played step by step:
/// `played`, the table the script before it ended with, once its steps have played over it,
/// or with a reset, `after`'s table with the abilities the reset numbers
/// (`beryl_protocol::script::replay`). With no table before it, or a step that table cannot
/// take, it is `after`'s table, numbered as best it can be.
pub(super) fn ended(played: Option<&Played>, after: &PlayerView, script: &Script) -> Played {
    let start = match (&script.reset, played) {
        (Some(_), _) => Played::of(after),
        (None, Some(played)) => played.clone(),
        (None, None) => return numbered(Played::of(after)),
    };
    replay(start, script).unwrap_or_else(|_| numbered(Played::of(after)))
}

/// Begins each effect on its named target or on the step's subject. Its clock starts at
/// the step edge plus `into` times the step's length plus `delay`, as `Effects::begin` does
/// on the web. Its duration comes from the shared timing, or defaults to the step's length.
fn effects_of(
    number: usize,
    step: &Step,
    timing: &StepTiming,
    laid: &Laid,
    definitions: &Effects,
    runs: &mut Vec<EffectRun>,
) {
    let span = laid.to - laid.from;
    for effect in &timing.effects {
        let Some(definition) = definitions.get(&effect.name) else {
            continue;
        };
        let subject = || {
            (definition.target == Target::Subject)
                .then(|| lit_by(&step.act))
                .flatten()
        };
        let Some(on) = effect.on.or_else(subject) else {
            continue;
        };
        let at = match effect.when {
            Edge::Start => laid.from,
            Edge::End => laid.to,
        };
        let from = at + effect.delay + effect.into.unwrap_or(0.0) * span;
        let to = from + effect.duration.unwrap_or(span);
        let (start, end) = (nanoseconds(from), nanoseconds(to));
        if end <= start {
            continue;
        }
        runs.push(EffectRun {
            definition: Arc::clone(definition),
            on: game_table::effect::target(on),
            on_point: matches!(on, Subject::Player { .. }),
            particle_seed: match on {
                Subject::Object { slot } => Some(u32::from(slot.0)),
                _ => None,
            },
            start,
            duration: end - start,
            curve: named_curve(effect.curve),
            step: number,
        });
    }
}

/// What the effects of `act` are on where they name nothing themselves, as the web stage's
/// `litBy` (ui/src/stage/tracks.ts) has it: what a count's number is on, a player's life total
/// or a card, and otherwise what the act moves. A count of a pool or of a player's counters
/// lights nothing.
fn lit_by(act: &Act) -> Option<Subject> {
    match act {
        Act::Count {
            tally: Tally::Life { player, .. },
        } => Some(Subject::Player { player: *player }),
        Act::Count { .. } => act
            .object()
            .map(|object| Subject::Object { slot: object.slot }),
        _ => mover_of(act),
    }
}

/// The slot of the scene object `step` moves, if it moves one.
pub(super) fn subject_of(step: &Step) -> Option<usize> {
    match mover_of(&step.act) {
        Some(Subject::Object { slot }) => Some(usize::from(slot.0)),
        _ => None,
    }
}

/// `verb`'s bit in `Compiled::unbuilt`.
pub(crate) fn bit(verb: Verb) -> u32 {
    let index = Verb::ALL
        .iter()
        .position(|other| *other == verb)
        .expect("every verb is in `Verb::ALL`");
    1 << index
}

/// A stage time in milliseconds, as `beat` gives it, in nanoseconds.
pub(crate) fn nanoseconds(milliseconds: f64) -> u64 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a script lasts seconds and starts at 0"
    )]
    let nanoseconds = (milliseconds.max(0.0) * 1e6).round() as u64;
    nanoseconds
}

/// `played` with a number for each ability on its stack that has none, from its place, as the
/// session's script builder numbers an ability nothing named. A table made from a view alone
/// knows no ability's number, since the numbers come from the scripts before it. It is the
/// table the show falls back on when it has not followed the scripts (the first view, or a
/// script the table could not take), so that a later step naming an ability by its number
/// fails cleanly and is placed whole; unnumbered, the ability could not be compared with a
/// step's subject at all.
pub(super) fn numbered(mut played: Played) -> Played {
    for (at, entry) in played.stack.iter_mut().enumerate() {
        if matches!(entry.item, StackItem::Ability { .. }) && entry.id.is_none() {
            entry.id = Some(u32::MAX - u32::try_from(at).unwrap_or(0));
        }
    }
    played
}
