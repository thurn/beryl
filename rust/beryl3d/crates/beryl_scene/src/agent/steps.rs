//! The interface checks that read a script's segments and steps (docs/beryl-3d.md, "Interface
//! checks"): no jumps, rest equals placement, each step played for its timing's duration, every
//! step shown and not crowded. The last two are ports of the web interface's stage audit
//! (`judgeStep` and `crowds` in `ui/scripts/stage-audit.mjs`), with its thresholds, which are in
//! the `audit` object of `data/beryl3d/gates.json`.
//!
//! All but "every step shown" read the script records alone, with no frame. That one reads a
//! sample of the step's subject from each frame evaluated while the step plays.

use beryl_protocol::script::Verb;
use glam::Vec2;

use super::fault::Rule;
use super::hit::{bounds, covered};
use crate::data::audit;
use crate::frame::{Frame, Place, Role};
use crate::input::{Rect, Size};
use crate::motion::pose_at;
use crate::play::record::{Cause, ScriptRecord, SegmentRecord, StepRecord};
use crate::pose::{Pose, Segment};

/// A fault the segment and step checks found, before the frame that names its objects is
/// known.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub rule: Rule,
    /// The slots of the scene objects it concerns.
    pub slots: Vec<usize>,
    /// The moment, in nanoseconds since the client started.
    pub at: u64,
    /// What is wrong, said of the objects, which the fault's message names before it.
    pub message: String,
}

/// One frame at 60 Hz, in nanoseconds: how far apart two frames can fall, which a measured time
/// is allowed to be off by.
const FRAME: u64 = 16_666_667;
/// Two poses closer than these are the same.
const SAME_MILLIMETRES: f32 = 0.01;
const SAME_FRACTION: f32 = 1e-3;

/// `audit.step_ms` in nanoseconds: the shortest motion the eye takes as motion.
#[must_use]
pub fn step_nanoseconds() -> u64 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a threshold of milliseconds"
    )]
    let nanoseconds = (f64::from(audit::STEP_MS) * 1e6) as u64;
    nanoseconds
}

/// Every segment and step check on each script of `history` that has started.
#[must_use]
pub fn segment_checks(history: &[ScriptRecord]) -> Vec<Found> {
    let mut found = Vec::new();
    for script in history.iter().filter(|script| script.start.is_some()) {
        found.extend(no_jumps(script));
        found.extend(rest_equals_placement(script));
        found.extend(step_times(script));
        found.extend(crowded(script));
    }
    found
}

/// Whether a verb carries its object along a path, so that its object may not jump. The verbs
/// still drawn as instant changes, `set` (which moves nothing) and the verbs that place an object
/// with no path (`appear`, `vanish`) may.
fn travels(verb: Verb) -> bool {
    verb == Verb::Fly
}

/// No jumps: where one of an object's segments begins, the object is where the segments before
/// left it, and no segment of a step that carries its object along a path moves it in no time.
/// A change while the object is transparent is no jump: nobody sees it.
#[must_use]
pub fn no_jumps(script: &ScriptRecord) -> Vec<Found> {
    let mut found = Vec::new();
    for slot in script.slots() {
        let track = script.track(slot);
        let segments: Vec<Segment> = track.iter().map(|record| record.segment).collect();
        for (index, record) in track.iter().enumerate() {
            let segment = record.segment;
            let what = cause_text(script, record);
            if index > 0 {
                let before = pose_at(&segments[..index], segment.start);
                if !same(before, segment.from) && seen(before) && seen(segment.from) {
                    found.push(Found {
                        rule: Rule::Jump,
                        slots: vec![slot],
                        at: real(script, segment.start),
                        message: format!(
                            "jumped {} mm as {what} began",
                            distance(before, segment.from)
                        ),
                    });
                }
            }
            let exempt = match record.cause {
                Cause::Hold | Cause::Change(_) => true,
                Cause::Verb(step) => !travels(script.steps[step].verb),
                Cause::Slide(_) | Cause::Planted => false,
            };
            if segment.duration == 0
                && !exempt
                && !same(segment.from, segment.to)
                && seen(segment.from)
                && seen(segment.to)
            {
                found.push(Found {
                    rule: Rule::Jump,
                    slots: vec![slot],
                    at: real(script, segment.start),
                    message: format!(
                        "moved {} mm in no time, with no path, in {what}",
                        distance(segment.from, segment.to)
                    ),
                });
            }
        }
    }
    found
}

/// Rest equals placement: when the script ends, every object it moved is where the view it leads
/// to puts it, and an object that view does not show is no longer seen.
#[must_use]
pub fn rest_equals_placement(script: &ScriptRecord) -> Vec<Found> {
    let mut found = Vec::new();
    for slot in script.slots() {
        let segments: Vec<Segment> = script
            .track(slot)
            .iter()
            .map(|record| record.segment)
            .collect();
        let end = pose_at(&segments, script.end);
        let message = match script.finals.get(slot).copied().flatten() {
            Some(rest) if !same(end, rest) => format!(
                "ended the script {} mm from its resting place",
                distance(end, rest)
            ),
            None if seen(end) => {
                "is still seen after the script, though the view it leads to has no place for it"
                    .to_owned()
            }
            _ => continue,
        };
        found.push(Found {
            rule: Rule::RestPlacement,
            slots: vec![slot],
            at: real(script, script.end),
            message,
        });
    }
    found
}

/// Each step is played for the duration its `StepTiming` gives: it lasts that long, and the
/// motion its verb gives its object lasts as long as the step.
#[must_use]
pub fn step_times(script: &ScriptRecord) -> Vec<Found> {
    let mut found = Vec::new();
    for (index, step) in script.steps.iter().enumerate() {
        let took = step.to - step.from;
        let slots: Vec<usize> = step.subject.into_iter().collect();
        if took != step.timed {
            found.push(Found {
                rule: Rule::StepTime,
                slots: slots.clone(),
                at: real(script, step.from),
                message: format!(
                    "played {} for {} ms, where its timing gives {} ms",
                    step.describe(),
                    milliseconds(took),
                    milliseconds(step.timed)
                ),
            });
        }
        if !matches!(step.verb, Verb::Fly | Verb::Appear) {
            continue;
        }
        for record in script
            .segments
            .iter()
            .filter(|record| record.cause == Cause::Verb(index))
        {
            if record.segment.duration != took {
                found.push(Found {
                    rule: Rule::StepTime,
                    slots: vec![record.slot],
                    at: real(script, record.segment.start),
                    message: format!(
                        "moved for {} ms in {}, which lasts {} ms",
                        milliseconds(record.segment.duration),
                        step.describe(),
                        milliseconds(took)
                    ),
                });
            }
        }
    }
    found
}

/// Not crowded: no more than `audit.crowded` different moves begin within `audit.step_ms` of one
/// another in one beat. Cards making the same move together read as one move.
#[must_use]
pub fn crowded(script: &ScriptRecord) -> Vec<Found> {
    let window = step_nanoseconds();
    let mut found = Vec::new();
    let beats = script
        .steps
        .iter()
        .map(|step| step.beat)
        .max()
        .map_or(0, |b| b + 1);
    for beat in 0..beats {
        let mut moves: Vec<&StepRecord> = script
            .steps
            .iter()
            .filter(|step| step.beat == beat && step.key.is_some())
            .collect();
        moves.sort_by_key(|step| step.from);
        let mut first = 0;
        while first < moves.len() {
            let start = moves[first].from;
            let group: Vec<&StepRecord> = moves[first..]
                .iter()
                .take_while(|step| step.from - start < window)
                .copied()
                .collect();
            let mut kinds = Vec::new();
            for step in &group {
                if !kinds.contains(&step.key) {
                    kinds.push(step.key);
                }
            }
            #[expect(clippy::cast_precision_loss, reason = "a handful of moves")]
            if kinds.len() as f32 > audit::CROWDED {
                let steps: Vec<String> = group.iter().map(|step| step.describe()).collect();
                found.push(Found {
                    rule: Rule::Crowded,
                    slots: group.iter().filter_map(|step| step.subject).collect(),
                    at: real(script, start),
                    message: format!(
                        "make {} different moves that began within {} ms in beat {}: {}",
                        kinds.len(),
                        audit::STEP_MS,
                        beat + 1,
                        steps.join(", ")
                    ),
                });
            }
            first += group.len();
        }
    }
    found
}

/// The scene object in `slot` as one frame shows it, for "every step shown".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// The frame's moment, in nanoseconds since the client started.
    pub t: u64,
    pub rect: Rect,
    /// Whether a player sees it: inside the window, at least half uncovered and not
    /// transparent.
    pub seen: bool,
    /// What kept it from being seen, if something did.
    pub hidden: Option<&'static str>,
}

/// The index of the card element of the scene object in `slot`, if the frame draws it.
#[must_use]
pub fn card_of(frame: &Frame, slot: usize) -> Option<u32> {
    let id = u32::try_from(slot).ok()?;
    frame
        .resolved
        .iter()
        .position(|element| {
            element.role == Role::Card
                && element.id.0 == id
                && matches!(element.place, Place::Card { .. })
        })
        .and_then(|index| u32::try_from(index).ok())
}

/// The scene object in `slot` as `frame`, drawn at `t` in a window of `size`, shows it.
#[must_use]
pub fn sample(frame: &Frame, slot: usize, t: u64, size: Size) -> Option<Sample> {
    let index = card_of(frame, slot)?;
    let element = &frame.resolved[index as usize];
    let rect = bounds(element);
    let centre = Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let inside =
        centre.x >= 0.0 && centre.y >= 0.0 && centre.x <= size.width && centre.y <= size.height;
    let hidden = if !inside {
        Some("outside the window")
    } else if element.opacity <= audit::TRANSPARENT {
        Some("transparent")
    } else if covered(frame, index) > 1.0 - audit::SEEN {
        Some("covered")
    } else {
        None
    };
    Some(Sample {
        t,
        rect,
        seen: hidden.is_none(),
        hidden,
    })
}

/// Every step shown: what a step concerns is seen while it plays, and a step whose verb gives its
/// subject time to travel moves it at least `audit.step_points` over at least `audit.step_ms`,
/// or half the step if that is shorter; a subject that jumps instead is a fault. `from` and `to`
/// are when the step begins and ends, in real time, and `samples` its subject in each frame
/// evaluated from `from` until (not including) `audit.step_ms` after `to`, in order. Only `fly` and `appear` are
/// judged: the other verbs are drawn as instant changes, or move no card.
#[must_use]
pub fn step_shown(step: &StepRecord, from: u64, to: u64, samples: &[Sample]) -> Option<String> {
    step.subject?;
    let took = to.saturating_sub(from);
    match step.verb {
        Verb::Appear => {
            // Read once it is whole, for as long as a move must take to be seen. The window is
            // open at its far end, which is no check time of its own: what is first seen there,
            // a frame at every tick would not see within it.
            let whole: Vec<&Sample> = samples
                .iter()
                .filter(|sample| sample.t >= to && sample.t < to + step_nanoseconds())
                .collect();
            if whole.is_empty() || whole.iter().any(|sample| sample.seen) {
                return None;
            }
            let why = whole
                .last()
                .and_then(|sample| sample.hidden)
                .unwrap_or("nowhere");
            Some(format!("came to be {why} in {}", step.describe()))
        }
        Verb::Fly => {
            if took == 0 {
                return None;
            }
            let path: Vec<&Sample> = samples
                .iter()
                .filter(|sample| sample.t >= from && sample.t <= to)
                .collect();
            let Some(origin) = path.first() else {
                return Some(format!("was drawn in no frame of {}", step.describe()));
            };
            let visible = path.iter().filter(|sample| sample.seen).count();
            #[expect(clippy::cast_precision_loss, reason = "a count of frames")]
            let unseen = (visible as f32) < path.len() as f32 * audit::SEEN;
            let reach = path
                .iter()
                .map(|sample| apart(sample.rect, origin.rect))
                .fold(0.0, f32::max);
            let changes: Vec<u64> = path
                .windows(2)
                .filter(|pair| apart(pair[1].rect, pair[0].rect) >= 1.0)
                .map(|pair| pair[1].t)
                .collect();
            let moving = match (changes.first(), changes.last()) {
                (Some(&first), Some(&last)) => {
                    let before = path
                        .iter()
                        .rev()
                        .find(|sample| sample.t < first)
                        .map_or(first, |sample| sample.t);
                    last - before
                }
                _ => 0,
            };
            if reach < audit::STEP_POINTS {
                let stays = step.key.is_some_and(|key| key.from == key.to);
                return (!stays).then(|| format!("did not move in {}", step.describe()));
            }
            if unseen {
                let why = most_hidden(&path);
                return Some(format!(
                    "was seen in {visible} of {} frames of {}, mostly {why}",
                    path.len(),
                    step.describe()
                ));
            }
            let least = step_nanoseconds().min(took / 2);
            if took >= step_nanoseconds() && moving + FRAME < least {
                return Some(format!(
                    "moved {} points in {} ms of the {} ms of {}",
                    reach.round(),
                    milliseconds(moving),
                    milliseconds(took),
                    step.describe()
                ));
            }
            None
        }
        _ => None,
    }
}

/// What most often kept the samples in `path` from being seen.
fn most_hidden(path: &[&Sample]) -> &'static str {
    let reasons: Vec<&'static str> = path.iter().filter_map(|sample| sample.hidden).collect();
    reasons
        .iter()
        .copied()
        .max_by_key(|reason| reasons.iter().filter(|other| *other == reason).count())
        .unwrap_or("seen")
}

/// How far apart two boxes of one element are: their centres, or their sizes, which a turn or a
/// change of zone changes.
fn apart(a: Rect, b: Rect) -> f32 {
    let centre = |r: Rect| Vec2::new(r.x + r.width / 2.0, r.y + r.height / 2.0);
    centre(a)
        .distance(centre(b))
        .max((a.width - b.width).abs())
        .max((a.height - b.height).abs())
}

/// What made a segment, for a fault's words.
fn cause_text(script: &ScriptRecord, record: &SegmentRecord) -> String {
    match record.cause {
        Cause::Hold => "the hold before its first move".to_owned(),
        Cause::Verb(step) => script.steps[step].describe(),
        Cause::Slide(step) => format!(
            "the slide that makes room for {}",
            script.steps[step].describe()
        ),
        Cause::Change(step) => format!("the change of {}", script.steps[step].describe()),
        Cause::Planted => "a planted segment".to_owned(),
    }
}

/// The real time of `t` on `script`'s clock. Only scripts that have started are checked.
fn real(script: &ScriptRecord, t: u64) -> u64 {
    script.real(t).unwrap_or_default()
}

/// Whether two poses are the same to the eye.
fn same(a: Pose, b: Pose) -> bool {
    if !seen(a) && !seen(b) {
        return true;
    }
    a.position.distance(b.position) < SAME_MILLIMETRES
        && a.rotation.angle_between(b.rotation) < SAME_FRACTION
        && (a.scale - b.scale).abs() < SAME_FRACTION
        && (a.opacity - b.opacity).abs() < SAME_FRACTION
        && (a.turn - b.turn).abs() < SAME_FRACTION
}

/// Whether a pose can be seen at all.
fn seen(pose: Pose) -> bool {
    pose.opacity > SAME_FRACTION
}

/// How far apart two poses' positions are, in whole millimetres.
fn distance(a: Pose, b: Pose) -> f32 {
    a.position.distance(b.position).round()
}

/// Nanoseconds as whole milliseconds, rounded to the nearest.
fn milliseconds(nanoseconds: u64) -> u64 {
    (nanoseconds + 500_000) / 1_000_000
}
