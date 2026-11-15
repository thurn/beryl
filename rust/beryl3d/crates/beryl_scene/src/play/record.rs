//! What a script did, as the interface checks read it (docs/beryl-3d.md, "Interface checks"):
//! each step with when it played and what it moved, and each segment with the step that made
//! it. The show keeps a record of every script only when asked to (`Show::record`), since a
//! running game has no use for one. A test can change a script before it plays (`Plant`), to
//! make a fault that the checks must find.

use beryl_protocol::protocol::{Why, ZoneName};
use beryl_protocol::script::{Act, Verb};
use glam::Vec3;

use crate::motion::pose_at;
use crate::pose::{Pose, Segment};

/// One step of a script, as it was laid out. Times are on the script's own clock, in
/// nanoseconds.
#[derive(Clone, Debug, PartialEq)]
pub struct StepRecord {
    /// The view its script leads to, numbered from 1 as a `step:V.B.S` moment numbers it, once
    /// the script is in a show's history; 0 before.
    pub view: usize,
    /// Its beat's index in the script, and its own index in the beat, from 0.
    pub beat: usize,
    pub index: usize,
    pub verb: Verb,
    /// The slot of the scene object it moves, if it moves one.
    pub subject: Option<usize>,
    /// When it begins and ends.
    pub from: u64,
    pub to: u64,
    /// How long its `StepTiming` says its motion takes.
    pub timed: u64,
    /// What kind of move it is, for "not crowded". `None` for a step that moves nothing from
    /// place to place.
    pub key: Option<MoveKey>,
}

impl StepRecord {
    /// How a fault names it: `step:3.2.0 (fly)`, the moment `just b3d info` lists for it and
    /// `--at` takes, and its verb.
    #[must_use]
    pub fn describe(&self) -> String {
        format!(
            "step:{}.{}.{} ({})",
            self.view,
            self.beat,
            self.index,
            format!("{:?}", self.verb).to_lowercase()
        )
    }
}

/// A move, told apart from others as the web interface's stage audit tells them apart
/// (`crowds` in `ui/scripts/stage-audit.mjs`): the verb, the zones it leaves and enters, and
/// why. Cards that make the same move together read as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveKey {
    pub verb: Verb,
    pub from: Option<ZoneName>,
    pub to: Option<ZoneName>,
    pub why: Option<Why>,
}

/// The verbs that move a card or an ability from one place to another, which "not crowded"
/// counts. Taps, and the other steps that change a card where it is, do not count.
const MOVES: [Verb; 6] = [
    Verb::Fly,
    Verb::Appear,
    Verb::Vanish,
    Verb::Emerge,
    Verb::Flip,
    Verb::Strike,
];

impl MoveKey {
    /// The kind of move `act` makes, if its verb is one that moves.
    #[must_use]
    pub fn of(act: &Act) -> Option<Self> {
        let verb = act.verb();
        if !MOVES.contains(&verb) {
            return None;
        }
        let (from, why) = match act {
            Act::Fly { from, why, .. }
            | Act::Appear { from, why, .. }
            | Act::Vanish { from, why, .. } => (Some(*from), Some(*why)),
            _ => (None, None),
        };
        Some(Self {
            verb,
            from,
            to: act.object().map(|object| object.zone),
            why,
        })
    }
}

/// Why a segment is on its object's track. Each index is a step's in `ScriptRecord::steps`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// It holds the object where it rested before the script, until it first moves.
    Hold,
    /// The step's verb moves the step's subject.
    Verb(usize),
    /// The step changed where the object rests, and it slides there.
    Slide(usize),
    /// The step brought the object to the table or took it away, with no path.
    Change(usize),
    /// A test put it there to make a fault (`Plant`).
    Planted,
}

/// A segment on the track of the object in `slot`, and why it is there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentRecord {
    pub slot: usize,
    pub cause: Cause,
    pub segment: Segment,
}

/// One script as it was laid out and played.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptRecord {
    /// When it started, in nanoseconds since the client started; `None` until it does.
    pub start: Option<u64>,
    /// How many times faster than real time its clock ran.
    pub rate: u64,
    /// How long it lasts on its own clock.
    pub end: u64,
    pub steps: Vec<StepRecord>,
    /// Every segment, in the order they were added to their tracks.
    pub segments: Vec<SegmentRecord>,
    /// Where each object rests in the view the script leads to, by slot; `None` for an object
    /// the view does not show.
    pub finals: Vec<Option<Pose>>,
}

impl ScriptRecord {
    /// The real time at which its clock reads `t`, rounded up, once it has started.
    #[must_use]
    pub fn real(&self, t: u64) -> Option<u64> {
        Some(self.start? + t.div_ceil(self.rate.max(1)))
    }

    /// When `step` began and ended in real time, in nanoseconds since the client started, once
    /// the script has started.
    #[must_use]
    pub fn span(&self, step: &StepRecord) -> Option<(u64, u64)> {
        Some((self.real(step.from)?, self.real(step.to)?))
    }

    /// The segments of the object in `slot`, sorted by start, those that start together in the
    /// order they were added, as its track holds them.
    #[must_use]
    pub fn track(&self, slot: usize) -> Vec<SegmentRecord> {
        let mut track: Vec<SegmentRecord> = self
            .segments
            .iter()
            .filter(|record| record.slot == slot)
            .copied()
            .collect();
        track.sort_by_key(|record| record.segment.start);
        track
    }

    /// Every slot that has a segment, in order, each once.
    #[must_use]
    pub fn slots(&self) -> Vec<usize> {
        let mut slots: Vec<usize> = self.segments.iter().map(|record| record.slot).collect();
        slots.sort_unstable();
        slots.dedup();
        slots
    }
}

/// A kind of fault a test can plant in a script, one for each rule the segment and step checks
/// hold, and one span (`Mark`) that tests of how spans reach the table plant, since no verb
/// begins one yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Planted {
    /// The object a step moves jumps 30 millimetres sideways, with no path, and stays there
    /// until its next segment begins.
    Jump,
    /// The last segment of the object a step moves ends 30 millimetres off its resting place.
    OffPlacement,
    /// The verb of a step plays for half the time its timing gives.
    Duration,
    /// The object a step moves makes its move ten metres to the side, outside the window.
    Unseen,
    /// Four moves of different kinds begin with a step, in its beat.
    Crowded,
    /// Not a fault: the card a step moves is marked `selected` from the step on, as if its verb
    /// had begun the mark over the step's time.
    Mark,
}

/// A fault planted at `t` on the clock of the `script`th script the show is given, counted
/// from 0. It concerns the step that plays at `t`, or else the first after it, or the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plant {
    pub kind: Planted,
    pub script: usize,
    pub t: u64,
}

/// How far a planted jump or misplacement moves its object, in millimetres.
const PLANTED_OFFSET: f32 = 30.0;
/// How far a planted unseen move is put to the side, in millimetres.
const PLANTED_AWAY: f32 = 10_000.0;

impl Plant {
    /// Changes `steps` and `segments` to hold the fault. Returns whether the script had a step
    /// to plant it on.
    pub(crate) fn apply(
        &self,
        steps: &mut Vec<StepRecord>,
        segments: &mut Vec<SegmentRecord>,
    ) -> bool {
        let Some(step) = self.step(steps) else {
            return false;
        };
        let verb_segment = segments
            .iter()
            .position(|record| record.cause == Cause::Verb(step));
        let side = Vec3::X * PLANTED_OFFSET;
        match self.kind {
            Planted::Jump => {
                let Some(slot) = steps[step].subject else {
                    return false;
                };
                let mut track: Vec<Segment> = segments
                    .iter()
                    .filter(|record| record.slot == slot)
                    .map(|record| record.segment)
                    .collect();
                track.sort_by_key(|segment| segment.start);
                let from = pose_at(&track, self.t);
                let to = Pose {
                    position: from.position + side,
                    ..from
                };
                segments.push(SegmentRecord {
                    slot,
                    cause: Cause::Planted,
                    segment: crate::verbs::instant(from, to, self.t),
                });
            }
            Planted::OffPlacement => {
                let Some(slot) = steps[step].subject else {
                    return false;
                };
                let Some(last) = segments
                    .iter_mut()
                    .filter(|record| record.slot == slot)
                    .max_by_key(|record| record.segment.start)
                else {
                    return false;
                };
                last.segment.to.position += side;
            }
            Planted::Duration => {
                let Some(index) = verb_segment else {
                    return false;
                };
                let segment = &mut segments[index].segment;
                segment.duration /= 2;
            }
            Planted::Unseen => {
                let Some(index) = verb_segment else {
                    return false;
                };
                let segment = &mut segments[index].segment;
                segment.from.position.x += PLANTED_AWAY;
                segment.to.position.x += PLANTED_AWAY;
            }
            // `Compiled::plant` adds the span, which neither steps nor segments hold.
            Planted::Mark => {}
            Planted::Crowded => {
                let base = steps[step].clone();
                let zones = [
                    ZoneName::Library,
                    ZoneName::Graveyard,
                    ZoneName::Exile,
                    ZoneName::Stack,
                ];
                for from in zones {
                    steps.push(StepRecord {
                        subject: None,
                        verb: Verb::Fly,
                        key: Some(MoveKey {
                            verb: Verb::Fly,
                            from: Some(from),
                            to: Some(ZoneName::Hand),
                            why: None,
                        }),
                        ..base.clone()
                    });
                }
            }
        }
        true
    }

    /// The index of the step the fault concerns.
    pub(crate) fn step(&self, steps: &[StepRecord]) -> Option<usize> {
        let wants_subject = !matches!(self.kind, Planted::Crowded);
        let fits = |step: &StepRecord| !wants_subject || step.subject.is_some();
        steps
            .iter()
            .enumerate()
            .filter(|(_, step)| fits(step) && step.from <= self.t && self.t <= step.to)
            .max_by_key(|(_, step)| step.from)
            .or_else(|| {
                steps
                    .iter()
                    .enumerate()
                    .filter(|(_, step)| fits(step) && step.from > self.t)
                    .min_by_key(|(_, step)| step.from)
            })
            .or_else(|| steps.iter().enumerate().rev().find(|(_, step)| fits(step)))
            .map(|(index, _)| index)
    }
}
