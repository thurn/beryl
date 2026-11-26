//! The interface checks that read a script's segments and steps (docs/beryl-3d.md, "Interface
//! checks"): no jumps, rest equals placement, each step played for its timing's duration, every
//! step shown and not crowded. The last two are ports of the web interface's stage audit
//! (`judgeStep` and `crowds` in `ui/scripts/stage-audit.mjs`), with its thresholds, which are in
//! `data/client/audit.json`.
//!
//! All but "every step shown" read the script records alone, with no frame. That one reads a
//! sample of the step's subject from each frame evaluated while the step plays.

use beryl_protocol::script::Verb;
use glam::Vec2;

use game_play::play::record::{Cause, ScriptRecord, SegmentRecord, StepRecord};
use game_table::table::Camera;
use game_table::table::Place;
use kit_base::data::Audit;
use kit_base::pose::{Pose, Segment};
use kit_frame::frame::{Frame, Role};
use kit_probe::agent::motion::{self, Jump, same, seen};
use kit_probe::agent::{Judged, Rule};
use kit_scene::hit::{bounds, covered, hit};
use kit_space::motion::pose_at;
use kit_ui::input::{Rect, Size};

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

/// `audit.step_ms` in nanoseconds: the shortest motion the eye takes as motion.
#[must_use]
pub fn step_nanoseconds(audit: Audit) -> u64 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a threshold of milliseconds"
    )]
    let nanoseconds = (f64::from(audit.step_ms) * 1e6) as u64;
    nanoseconds
}

/// Every segment and step check on each script of `history` that has started.
#[must_use]
pub fn segment_checks(history: &[ScriptRecord], audit: Audit) -> Vec<Found> {
    let mut found = Vec::new();
    for script in history.iter().filter(|script| script.start.is_some()) {
        found.extend(no_jumps(script));
        found.extend(rest_equals_placement(script));
        found.extend(step_times(script));
        found.extend(crowded(script, audit));
    }
    found
}

/// Whether a verb carries its object along a path, so that its object may not jump. The verbs
/// still drawn as instant changes, `set` (which moves nothing) and the verbs that place an object
/// with no path (`appear`, `vanish`) may.
fn travels(verb: Verb) -> bool {
    matches!(verb, Verb::Fly | Verb::Emerge)
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
        let allows_instant = |index: usize| match track[index].cause {
            Cause::Hold | Cause::Change(_) => true,
            Cause::Verb(step) => !travels(script.steps[step].verb),
            Cause::Slide(_) | Cause::Planted => false,
        };
        for (index, jump) in motion::jumps(&segments, allows_instant) {
            let record = track[index];
            let what = cause_text(script, &record);
            let message = match jump {
                Jump::Start(distance) => format!("jumped {distance} mm as {what} began"),
                Jump::ZeroDuration(distance) => {
                    format!("moved {distance} mm in no time, with no path, in {what}")
                }
            };
            found.push(Found {
                rule: Rule::Jump,
                slots: vec![slot],
                at: real(script, record.segment.start),
                message,
            });
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
/// motion its verb gives each object lasts as long as the step. That motion may be several
/// segments one after another (a card drawn alone flies in, is held and drops): together they
/// last as long as the step, each beginning as the one before it ends.
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
        if !matches!(
            step.verb,
            Verb::Fly | Verb::Appear | Verb::Vanish | Verb::Emerge
        ) {
            continue;
        }
        let records: Vec<&SegmentRecord> = script
            .segments
            .iter()
            .filter(|record| record.cause == Cause::Verb(index))
            .collect();
        let mut moved: Vec<usize> = records.iter().map(|record| record.slot).collect();
        moved.sort_unstable();
        moved.dedup();
        for slot in moved {
            let mut parts: Vec<Segment> = records
                .iter()
                .filter(|record| record.slot == slot)
                .map(|record| record.segment)
                .collect();
            parts.sort_by_key(|segment| segment.start);
            let start = parts[0].start;
            let gap = parts.windows(2).find(|pair| pair[1].start != pair[0].end());
            let lasted = parts.last().map_or(0, Segment::end) - start;
            if let Some(pair) = gap {
                found.push(Found {
                    rule: Rule::StepTime,
                    slots: vec![slot],
                    at: real(script, pair[0].end()),
                    message: format!(
                        "stood still for {} ms between two parts of its motion in {}",
                        milliseconds(pair[1].start.saturating_sub(pair[0].end())),
                        step.describe()
                    ),
                });
            } else if lasted != took {
                found.push(Found {
                    rule: Rule::StepTime,
                    slots: vec![slot],
                    at: real(script, start),
                    message: format!(
                        "moved for {} ms in {}, which lasts {} ms",
                        milliseconds(lasted),
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
pub fn crowded(script: &ScriptRecord, audit: Audit) -> Vec<Found> {
    let window = step_nanoseconds(audit);
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
            if kinds.len() as f32 > audit.crowded {
                let steps: Vec<String> = group.iter().map(|step| step.describe()).collect();
                found.push(Found {
                    rule: Rule::Crowded,
                    slots: group.iter().filter_map(|step| step.subject).collect(),
                    at: real(script, start),
                    message: format!(
                        "make {} different moves that began within {} ms in beat {}: {}",
                        kinds.len(),
                        audit.step_ms,
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

/// Where on a card `sample` looks for it on screen, as shares of its box's width and height
/// (`points` in `ui/scripts/audit.mjs`).
const ON_SCREEN_POINTS: [(f32, f32); 5] =
    [(0.5, 0.5), (0.2, 0.2), (0.8, 0.2), (0.2, 0.8), (0.8, 0.8)];

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
                && matches!(
                    Place::in_frame(element.place),
                    Some(Place::Card { .. } | Place::SharedPile(_))
                )
        })
        .and_then(|index| u32::try_from(index).ok())
}

/// Whether `frame` draws the opening's screen (`screens::opening::SCREEN_NAME`). A step that
/// plays wholly while it does is not judged by "every step shown": the opening shows the hands
/// dealt, the mulligans and the cards put back, over a table that plays them beneath it, as the
/// web interface's stage audit counts every step under its opening as shown (`seen` in
/// `ui/scripts/stage-audit.mjs`).
#[must_use]
pub fn opening_up(frame: &Frame) -> bool {
    (frame.resolved.iter()).any(|element| {
        element.role == Role::Region
            && frame.name(element.name) == game_hud::screens::opening::SCREEN_NAME
    })
}

/// The scene object in `slot` as `frame`, drawn at `t` in a window of `size`, shows it.
#[must_use]
pub fn sample(frame: &Frame, slot: usize, t: u64, size: Size, audit: Audit) -> Option<Sample> {
    let index = card_of(frame, slot)?;
    let element = &frame.resolved[index as usize];
    let rect = bounds(element);
    // On screen when any of five points of it is, as the web interface's audit measures it: its
    // centre and the points a fifth of the way in from each corner. A card held partly past an
    // edge of the window, as the opponent's hand is, is on screen.
    let inside = ON_SCREEN_POINTS.iter().any(|&(x, y)| {
        let point = Vec2::new(rect.x + rect.width * x, rect.y + rect.height * y);
        point.x >= 0.0 && point.y >= 0.0 && point.x <= size.width && point.y <= size.height
    });
    // A buried copy is shown by the pile when its visible top covers the copy's centre.
    // The web stage audit also counts a card under its twin as shown.
    let shown_by_pile = element.count == 0
        && hit(
            frame,
            Vec2::new(rect.x + 0.5 * rect.width, rect.y + 0.5 * rect.height),
        )
        .is_some_and(|top| {
            let top = &frame.resolved[top as usize];
            top.count > 1
                && top.place == element.place
                && top.opacity > audit.transparent
                && frame.name(top.name) == frame.name(element.name)
        });
    // While the opening's screen is up, it shows the hands dealt, the mulligans and the cards
    // put back, over a table that plays them beneath it or holds its hand below the window, so
    // every card counts as seen, as the web interface's stage audit counts it (`seen` in
    // `ui/scripts/stage-audit.mjs`).
    let hidden = if opening_up(frame) {
        None
    } else if !inside {
        Some("outside the window")
    } else if element.opacity <= audit.transparent {
        Some("transparent")
    } else if !shown_by_pile && covered(frame, index) > 1.0 - audit.seen {
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

/// Whether the step numbered `number` of `script` moves its subject from a place on the window
/// back to the same place: its subject's first segment for the step's verb starts, and its last
/// ends, within `audit.step_points` of each other on the window `camera` shows. "Every step
/// shown" does not judge such a step, as it does not judge a card that moves within its zone
/// between two places out of view: the viewer has no change of place to follow. On the phone a
/// card flown from a graveyard into exile does this, since exile lies unseen under the
/// graveyard (docs/ui-conventions.md, "The phone").
#[must_use]
pub fn rests_in_place(script: &ScriptRecord, number: usize, camera: &Camera, audit: Audit) -> bool {
    let Some(subject) = script.steps.get(number).and_then(|step| step.subject) else {
        return false;
    };
    let mut own = script
        .segments
        .iter()
        .filter(|record| record.slot == subject && record.cause == Cause::Verb(number));
    let Some(first) = own.next() else {
        return false;
    };
    let last = own.next_back().unwrap_or(first);
    let start = camera.to_screen(first.segment.from.position);
    let end = camera.to_screen(last.segment.to.position);
    start.distance(end) < audit.step_points
}

/// Every step shown: what a step concerns is seen while it plays, and a step whose verb gives its
/// subject time to travel moves it at least `audit.step_points` over at least `audit.step_ms`,
/// or half the step if that is shorter; a subject that jumps instead is a fault. `from` and `to`
/// are when the step begins and ends, in real time, and `samples` its subject in each frame
/// evaluated from `from` until (not including) `audit.step_ms` after `to`, in order. Only `fly`,
/// `emerge` and `appear` are judged: the other verbs place their subject with no path, are
/// drawn as instant changes, or move no card.
#[must_use]
pub fn step_shown(
    step: &StepRecord,
    from: u64,
    to: u64,
    samples: &[Sample],
    audit: Audit,
) -> Option<String> {
    step.subject?;
    let took = to.saturating_sub(from);
    match step.verb {
        Verb::Appear => {
            // Read once it is whole, for as long as a move must take to be seen. The window is
            // open at its far end, which is no check time of its own: what is first seen there,
            // a frame at every tick would not see within it.
            let whole: Vec<&Sample> = samples
                .iter()
                .filter(|sample| sample.t >= to && sample.t < to + step_nanoseconds(audit))
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
        Verb::Fly | Verb::Emerge => {
            if took == 0 {
                return None;
            }
            let path: Vec<&Sample> = samples
                .iter()
                .filter(|sample| sample.t >= from && sample.t <= to)
                .collect();
            let stays = step.key.is_some_and(|key| key.from == key.to);
            let Some(origin) = path.first() else {
                // A card that moves within its zone between two places out of view, as one in a
                // scrolled row's hidden part moves to that row's end, is never drawn: nothing the
                // viewer can see changes.
                return (!stays).then(|| format!("was drawn in no frame of {}", step.describe()));
            };
            let visible = path.iter().filter(|sample| sample.seen).count();
            #[expect(clippy::cast_precision_loss, reason = "a count of frames")]
            let unseen = (visible as f32) < path.len() as f32 * audit.seen;
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
            if reach < audit.step_points {
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
            let least = step_nanoseconds(audit).min(took / 2);
            if took >= step_nanoseconds(audit) && moving + FRAME < least {
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

/// How far apart two poses' positions are, in whole millimetres.
fn distance(a: Pose, b: Pose) -> f32 {
    a.position.distance(b.position).round()
}

/// Nanoseconds as whole milliseconds, rounded to the nearest.
fn milliseconds(nanoseconds: u64) -> u64 {
    (nanoseconds + 500_000) / 1_000_000
}

/// Counts what the segment and step checks (`steps::segment_checks`) judge in `script`.
pub fn count_script(tally: &mut Judged, script: &ScriptRecord) {
    if script.start.is_none() {
        return;
    }
    tally.add(Rule::Jump, script.segments.len());
    tally.add(Rule::RestPlacement, script.slots().len());
    tally.add(Rule::StepTime, script.steps.len());
    tally.add(
        Rule::Crowded,
        script
            .steps
            .iter()
            .filter(|step| step.key.is_some())
            .count(),
    );
}
