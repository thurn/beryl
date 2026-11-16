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

use beryl_protocol::protocol::{PlayerView, StackItem};
use beryl_protocol::script::{
    Act, Edge, Script, Step, StepTiming, Subject, Table as Played, Tally, Verb, mover_of, replay,
};

use super::record::{MoveKey, Plant, Planted, SegmentRecord, StepRecord};
use super::step::{Motion, add, numbers_of};
use crate::beat::{Laid, beat};
use crate::effects::{Effect, EffectRun};
use crate::hud::counts::CountChange;
use crate::light::{MarkSpan, Span, SpanChange, StreakRun};
use crate::motion::Track;
use crate::notice::NoticeRun;
use crate::pose::{Curve, Pose};
use crate::sound::{SoundRun, Sounds};
use crate::table::{Objects, Table};
use crate::verbs::{self, Interval};

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
    /// The sounds its steps ask for, ordered by when each begins on its clock.
    pub sounds: Vec<SoundRun>,
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

    /// Where each object rests in the view the script leads to, by slot.
    pub(crate) fn final_poses(&self) -> Vec<Option<Pose>> {
        self.finals
            .cards
            .iter()
            .map(|object| object.map(|object| object.pose()))
            .collect()
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
    before: Option<(&'a PlayerView, &Played)>,
    after: &'a PlayerView,
    timings: &[StepTiming],
    table: &'a Table,
    records: bool,
    shown: bool,
) -> (Compiled, Played) {
    let at_rest = |played: &Played| {
        let numbers = numbers_of(played);
        let placement = table.placement(after, &numbers);
        Table::objects_for(after, numbers, &placement)
    };
    let mut compiled = Compiled::default();
    let Some(script) = after.script.as_ref() else {
        let played = numbered(Played::of(after));
        compiled.finals = at_rest(&played);
        return (compiled, played);
    };
    compiled.reset = script.reset.is_some();
    compiled.verbs = script
        .beats
        .iter()
        .flatten()
        .fold(0, |verbs, step| verbs | bit(step.act.verb()));
    if timings.len() != script.beats.iter().map(Vec::len).sum::<usize>() {
        let played = ended(before.map(|(_, played)| played), after, script);
        compiled.finals = at_rest(&played);
        return (compiled, played);
    }
    let mut motion = before
        .filter(|_| script.reset.is_none())
        .map(|(before, played)| {
            Motion::new(before, played, (table, after, timings), records, shown)
        });
    let mut steps = Vec::new();
    // The steps sound whether or not they move anything.
    let mut sounds = Sounds::new(script, after, motion.is_some());
    let effects = &mut compiled.effects;
    // With no table before it, or with a reset, the table takes the script's steps at once,
    // and only a reset's announcements play, as the web stage's `play` does (`resetSteps` in
    // `ui/src/stage/table.ts`).
    let immediate = motion.is_none();
    compiled.immediate = immediate;
    compiled.end = lay_out(
        script,
        timings,
        immediate,
        (
            &mut compiled.unbuilt,
            &mut compiled.laid,
            &mut compiled.closes,
        ),
        |number, (beat, index), step, timing, laid| {
            sounds.step(
                motion.as_ref().map(|motion| &motion.played),
                (&script.beats[beat], index, number),
                timing,
                laid,
            );
            let closed = motion.as_mut().and_then(|motion| {
                motion.step(number, (&script.beats[beat], index), timing, laid);
                motion.take_closed()
            });
            effects_of(number, step, timing, laid, effects);
            if !records {
                return closed;
            }
            steps.push(StepRecord {
                view: 0,
                beat,
                index,
                verb: step.act.verb(),
                subject: subject_of(step),
                from: nanoseconds(laid.from),
                to: nanoseconds(laid.to),
                timed: nanoseconds(timing.duration),
                key: MoveKey::of(&step.act),
            });
            closed
        },
    );
    compiled.steps = steps;
    compiled.sounds = sounds.finish();
    compiled.scratch.clone_from(&compiled.laid);
    // The steps of a beat need not begin in the order they are written, nor their effects.
    compiled.effects.sort_by_key(|run| run.start);
    let Some(motion) = motion else {
        let played = ended(before.map(|(_, played)| played), after, script);
        compiled.finals = at_rest(&played);
        return (compiled, played);
    };
    // After a step the table could not take, it no longer matches the game, and the view's
    // table is the best guess.
    let fallback = (motion.unplaced != 0).then(|| numbered(Played::of(after)));
    let numbers = numbers_of(fallback.as_ref().unwrap_or(&motion.played));
    let placement = motion.placement_of(after, &numbers);
    compiled.finals = Table::objects_for(after, numbers, &placement);
    compiled.tracks = motion.tracks;
    compiled.ability_tracks = motion.ability_tracks;
    compiled.segments = motion.segments;
    compiled.unplaced = motion.unplaced;
    // The steps of a beat need not begin in the order they are written, and the show gives
    // the table each record once its time comes, in order.
    compiled.spans = motion.written.spans;
    compiled.spans.sort_by_key(|change| change.over.from);
    compiled.streaks = motion.written.streaks;
    compiled.streaks.sort_by_key(|run| run.over.from);
    compiled.notices = motion.written.notices;
    compiled.notices.sort_by_key(|run| run.over.from);
    compiled.counts = motion.written.counts;
    compiled.counts.sort_by_key(|change| change.at);
    (compiled, fallback.unwrap_or(motion.played))
}

/// The table `script`, which came with `after`, ends with when it is not played step by step:
/// `played`, the table the script before it ended with, once its steps have played over it,
/// or with a reset, `after`'s table with the abilities the reset numbers
/// (`beryl_protocol::script::replay`). With no table before it, or a step that table cannot
/// take, it is `after`'s table, numbered as best it can be.
fn ended(played: Option<&Played>, after: &PlayerView, script: &Script) -> Played {
    let start = match (&script.reset, played) {
        (Some(_), _) => Played::of(after),
        (None, Some(played)) => played.clone(),
        (None, None) => return numbered(Played::of(after)),
    };
    replay(start, script).unwrap_or_else(|_| numbered(Played::of(after)))
}

/// Lays out each beat of `script` after the one before, calling `each` with every step's number
/// in the script, its beat and index in its beat, the step, its timing and when it plays, and
/// returns when the last beat is over, in nanoseconds. Each verb drawn as an instant change sets
/// its bit in `unbuilt`, and each step's times go to `all`. `each` returns when what closed up
/// after its step has closed, in milliseconds, if anything did, which goes to `closed_by`: a beat
/// is not over until then, as the web stage's is not (`closedBy` in `ui/src/stage/tracks.ts`).
///
/// When `immediate`, the table takes the script's steps at once: only a reset's announcements
/// play, beat after beat, and `each` is called for them alone. Every other step takes no time
/// where its beat begins.
fn lay_out<'s>(
    script: &'s Script,
    timings: &[StepTiming],
    immediate: bool,
    (unbuilt, all, closed_by): (&mut u32, &mut Vec<Laid>, &mut Vec<Option<f64>>),
    mut each: impl FnMut(usize, (usize, usize), &'s Step, &StepTiming, &Laid) -> Option<f64>,
) -> u64 {
    let mut laid = Vec::new();
    all.reserve_exact(timings.len());
    closed_by.reserve_exact(timings.len());
    let plays = |step: &Step| {
        !immediate || (script.reset.is_some() && matches!(step.act, Act::Announce { .. }))
    };
    let mut at = 0.0;
    let mut first = 0;
    for (number, steps) in script.beats.iter().enumerate() {
        let timings = &timings[first..first + steps.len()];
        let mut end = if immediate {
            // The steps that play, laid out as a beat of their own.
            let (playing, timed): (Vec<Step>, Vec<StepTiming>) = steps
                .iter()
                .zip(timings)
                .filter(|(step, _)| plays(step))
                .map(|(step, timing)| (step.clone(), timing.clone()))
                .unzip();
            let mut own = Vec::new();
            let end = beat(&playing, &timed, at, |_| None, at, &mut own);
            let mut own = own.into_iter();
            laid.clear();
            laid.extend(steps.iter().map(|step| {
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
            beat(steps, timings, at, |_| None, at, &mut laid)
        };
        for (index, ((step, timing), laid)) in steps.iter().zip(timings).zip(&laid).enumerate() {
            let verb = step.act.verb();
            if !verbs::built(verb) {
                *unbuilt |= bit(verb);
            }
            let closed = if plays(step) {
                each(first + index, (number, index), step, timing, laid)
            } else {
                None
            };
            if let Some(closed) = closed {
                end = end.max(closed);
            }
            all.push(*laid);
            closed_by.push(closed);
        }
        at = end;
        first += steps.len();
    }
    nanoseconds(at)
}

/// Adds to `runs` the effects that `step`, the script's step `number`, begins, on the script's
/// clock, as the web stage begins them (`Effects::begin` in ui/src/stage/effects.ts). Each is on
/// what its `on` names, or else on what the step's act is about (`lit_by`); with neither it is
/// dropped. It begins
/// at the edge of the step its `when` names, `into` the step's length after that, and then
/// `delay` later, and it lasts its `duration`, or the step's length where it has none. One that
/// would last no time is dropped.
fn effects_of(
    number: usize,
    step: &Step,
    timing: &StepTiming,
    laid: &Laid,
    runs: &mut Vec<EffectRun>,
) {
    let span = laid.to - laid.from;
    for effect in &timing.effects {
        let Some(on) = effect.on.or_else(|| lit_by(&step.act)) else {
            continue;
        };
        let Some(named) = Effect::from_name(&effect.name) else {
            debug_assert!(false, "no effect is named {:?}", effect.name);
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
            effect: named,
            on,
            start,
            duration: end - start,
            curve: Curve::named(effect.curve),
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
fn numbered(mut played: Played) -> Played {
    for (at, entry) in played.stack.iter_mut().enumerate() {
        if matches!(entry.item, StackItem::Ability { .. }) && entry.id.is_none() {
            entry.id = Some(u32::MAX - u32::try_from(at).unwrap_or(0));
        }
    }
    played
}
