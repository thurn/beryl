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

use beryl_protocol::protocol::{ObjectId, PlayerView, ZoneName};
use beryl_protocol::script::{
    Act, Room, Script, Step, StepTiming, Subject, Table as Played, Verb, mover_of,
};

use super::record::{Cause, MoveKey, Plant, SegmentRecord, StepRecord};
use crate::beat::{Laid, beat};
use crate::motion::Track;
use crate::pose::{Curve, Pose, Segment};
use crate::table::{Group, NameIds, Placement, Rest, SceneObject, Table, Zones};
use crate::verbs::{self, Interval};

/// A script laid out in time, ready to start.
#[derive(Clone, Debug)]
pub(crate) struct Compiled {
    /// One track for each object that moves, by slot. An object whose track is empty stands at
    /// its resting place in the view the table showed before the script.
    pub tracks: Vec<Track>,
    /// The scene objects at rest in the view the script leads to, which the table takes when
    /// the script ends.
    pub finals: Vec<Option<SceneObject>>,
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
}

impl Compiled {
    /// Changes the script to hold a fault a test plants, and lays its tracks out again from its
    /// segments. Returns whether the script had a step to plant it on.
    pub(crate) fn plant(&mut self, plant: &Plant) -> bool {
        if !plant.apply(&mut self.steps, &mut self.segments) {
            return false;
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
            .iter()
            .map(|object| object.map(|object| object.pose()))
            .collect()
    }
}

/// Lays out `after`'s script, played from `before`, the view the table shows when it starts,
/// with one timing per step in `timings`. With `records`, it also keeps a record of each step and
/// of the step that made each segment, for the interface checks; a running game does without,
/// so that compiling allocates no more than playing needs. `shown` says whether `before` is the
/// view the table shows at rest, whose resting places the table holds already. With no `before`
/// (the first view), with a reset, or
/// with timings that do not match the steps, nothing moves: the table takes `after` as it stands
/// when the script ends. A reset's beats still hold the time their timings give.
pub(crate) fn compile<'a>(
    before: Option<&'a PlayerView>,
    after: &PlayerView,
    timings: &[StepTiming],
    table: &'a Table,
    records: bool,
    shown: bool,
) -> Compiled {
    let at_rest = || Table::objects_for(after, &table.placement(after));
    let mut compiled = Compiled {
        tracks: Vec::new(),
        finals: Vec::new(),
        end: 0,
        unbuilt: 0,
        verbs: 0,
        steps: Vec::new(),
        segments: Vec::new(),
    };
    let Some(script) = after.script.as_ref() else {
        compiled.finals = at_rest();
        return compiled;
    };
    compiled.verbs = script
        .beats
        .iter()
        .flatten()
        .fold(0, |verbs, step| verbs | bit(step.act.verb()));
    if timings.len() != script.beats.iter().map(Vec::len).sum::<usize>() {
        compiled.finals = at_rest();
        return compiled;
    }
    let mut motion = before
        .filter(|_| script.reset.is_none())
        .map(|before| Motion::new(before, table, records, shown));
    let mut steps = Vec::new();
    compiled.end = lay_out(
        script,
        timings,
        &mut compiled.unbuilt,
        |(beat, index), step, timing, laid| {
            if let Some(motion) = &mut motion {
                motion.step(steps.len(), step, timing, laid);
            }
            if !records {
                return;
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
        },
    );
    compiled.steps = steps;
    match motion {
        Some(motion) => {
            compiled.finals = Table::objects_for(after, &motion.placement_of(after));
            compiled.tracks = motion.tracks;
            compiled.segments = motion.segments;
        }
        None => compiled.finals = at_rest(),
    }
    compiled
}

/// Lays out each beat of `script` after the one before, calling `each` with every step's beat
/// and index in its beat, the step, its timing and when it plays, and returns when the last beat
/// is over, in nanoseconds. Each verb drawn as an instant change sets its bit in `unbuilt`.
fn lay_out(
    script: &Script,
    timings: &[StepTiming],
    unbuilt: &mut u32,
    mut each: impl FnMut((usize, usize), &Step, &StepTiming, &Laid),
) -> u64 {
    let mut laid = Vec::new();
    let mut at = 0.0;
    let mut first = 0;
    for (number, steps) in script.beats.iter().enumerate() {
        let timings = &timings[first..first + steps.len()];
        // No span is open and nothing passes over the table yet: the flat layer draws neither.
        let end = beat(steps, timings, at, |_| None, at, &mut laid);
        for (index, ((step, timing), laid)) in steps.iter().zip(timings).zip(&laid).enumerate() {
            let verb = step.act.verb();
            if !verbs::built(verb) {
                *unbuilt |= bit(verb);
            }
            each((number, index), step, timing, laid);
        }
        at = end;
        first += steps.len();
    }
    nanoseconds(at)
}

/// The slot of the scene object `step` moves, if it moves one.
fn subject_of(step: &Step) -> Option<usize> {
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
fn nanoseconds(milliseconds: f64) -> u64 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a script lasts seconds and starts at 0"
    )]
    let nanoseconds = (milliseconds.max(0.0) * 1e6).round() as u64;
    nanoseconds
}

/// The table as a script plays over it, and the tracks its steps have made so far.
struct Motion<'a> {
    table: &'a Table,
    played: Played,
    /// Every object's resting place as the table now stands.
    rests: Placement,
    /// The slots each group placed when it was last placed, by `Group::index`.
    members: Vec<Vec<ObjectId>>,
    /// The group each slot was last placed in, by `Group::index`, indexed by slot.
    groups: Vec<Option<usize>>,
    /// Whether the next step places every group, not only the ones its act changed: the step
    /// after one the table could not take, which may have changed part of it.
    whole: bool,
    /// Whether the next step is the first, which also places the groups that lay their objects
    /// out in the order the table lists them (`Group::in_object_order`): `rests` then holds the
    /// view the script starts from, which lists its hands in its own order, not by slot as the
    /// table played over does.
    first: bool,
    /// The resting places of the groups a step placed again, until it makes them the table's
    /// own. Kept between steps, empty, so that a step need not allocate it.
    fresh: Placement,
    /// The slots each group a step placed again placed, by `Group::index`, until the step makes
    /// them its `members`.
    next: Vec<Vec<ObjectId>>,
    /// The slots the step's act named.
    touched: Vec<ObjectId>,
    /// The numbers of the names of `played`'s objects.
    names: NameIds<'a>,
    /// A bit set by slot that `affected` marks slots in, kept between steps.
    marks: Vec<u64>,
    /// The slots `affected` gives, kept between steps so that a step need not allocate them.
    affected: Vec<usize>,
    tracks: Vec<Track>,
    /// Every segment added to `tracks`, in order, with why, if it keeps `records`.
    segments: Vec<SegmentRecord>,
    records: bool,
}

impl<'a> Motion<'a> {
    fn new(before: &'a PlayerView, table: &'a Table, records: bool, shown: bool) -> Self {
        let rests = if shown {
            table.rests()
        } else {
            table.placement(before)
        };
        // Each object's group, read from where it rests.
        let mut members = vec![Vec::new(); Group::ALL.len()];
        let mut groups = vec![None; rests.rests.len()];
        for (slot, rest) in rests.rests.iter().enumerate() {
            if let Some(group) = rest.and_then(|rest| Group::at(rest.place)) {
                let id = ObjectId(u16::try_from(slot).expect("a slot fits an object id"));
                members[group.index()].push(id);
                groups[slot] = Some(group.index());
            }
        }
        // Room for a track for every object, so that growing `tracks` one slot at a time never
        // copies the tracks it holds.
        let tracks = Vec::with_capacity(rests.rests.len());
        let played = Played::of(before);
        Self {
            table,
            names: NameIds::of(&before.objects),
            played,
            rests,
            members,
            groups,
            whole: false,
            first: true,
            fresh: Placement::default(),
            next: vec![Vec::new(); Group::ALL.len()],
            touched: Vec::new(),
            marks: Vec::new(),
            affected: Vec::new(),
            tracks,
            segments: Vec::new(),
            records,
        }
    }

    /// Applies `act` to the table and places again every group whose resting places it may
    /// have changed: the groups that held or now hold an object the act named, and the stack,
    /// whose abilities an act may add or take away without naming an object. Their resting
    /// places go to `fresh` and their slots to `next`; `rests` is left as it was. Returns which
    /// groups it placed, or none if the table could not take the act.
    fn place(&mut self, act: &Act) -> Option<[bool; Group::ALL.len()]> {
        let touched = &mut self.touched;
        touched.clear();
        if self
            .played
            .apply_noting(act, &mut |slot| touched.push(slot))
            .is_err()
        {
            self.whole = true;
            return None;
        }
        let zones = Zones::of_table(&self.played);
        let mut dirty = [self.whole; Group::ALL.len()];
        dirty[Group::Stack.index()] = true;
        if self.first {
            for group in Group::ALL
                .into_iter()
                .filter(|group| group.in_object_order())
            {
                dirty[group.index()] = true;
            }
        }
        for object in self.touched.iter().filter_map(|&slot| zones.get(slot)) {
            self.names.note(object);
        }
        for &slot in &self.touched {
            if let Some(group) = self.groups.get(usize::from(slot.0)).copied().flatten() {
                dirty[group] = true;
            }
            if let Some(group) = zones.get(slot).and_then(Group::of) {
                dirty[group.index()] = true;
            }
        }
        for (group, next) in Group::ALL.into_iter().zip(&mut self.next) {
            if dirty[group.index()] {
                let zones = (&zones, &self.names);
                self.table.place_group(zones, group, &mut self.fresh, next);
            }
        }
        Some(dirty)
    }

    /// Every object's resting place in `view`, the view the script leads to. The table played
    /// over should end as that view stands, so each group that holds the same objects in both
    /// keeps the resting places the steps left, and only the others are placed again. After a
    /// step the table could not take, `rests` may not match the table, so every group is.
    fn placement_of(&self, view: &PlayerView) -> Placement {
        if self.whole {
            return self.table.placement(view);
        }
        let played = Zones::of_table(&self.played);
        self.table
            .placement_reusing(&Zones::of_view(view), (&played, &self.rests))
    }

    /// Where the object in `slot` rests after the step, given the groups `dirty` it placed again.
    fn now(&self, slot: usize, dirty: &[bool]) -> Option<Rest> {
        let id = ObjectId(u16::try_from(slot).expect("a slot fits an object id"));
        if let Some(rest) = self.fresh.get(id) {
            return Some(*rest);
        }
        let left = self.whole
            || self
                .groups
                .get(slot)
                .copied()
                .flatten()
                .is_some_and(|group| dirty[group]);
        if left {
            None
        } else {
            self.rests.get(id).copied()
        }
    }

    /// The slots whose resting place the step may have changed, given the groups `dirty` it
    /// placed again, and the step's own object, in order, in `self.affected`. They are marked in
    /// a bit set by slot and read back in order, which costs less than sorting them.
    fn mark_affected(&mut self, dirty: &[bool], subject: Option<usize>) {
        let end = self.rests.rests.len().max(self.fresh.rests.len());
        let end = end.max(subject.map_or(0, |slot| slot + 1));
        let marks = &mut self.marks;
        marks.clear();
        marks.resize(end.div_ceil(64), 0);
        let mut mark = |slot: usize| marks[slot / 64] |= 1 << (slot % 64);
        if self.whole {
            (0..end).for_each(&mut mark);
        } else {
            let placed = dirty.iter().enumerate().filter(|(_, dirty)| **dirty);
            for (group, _) in placed {
                for slot in self.members[group].iter().chain(&self.next[group]) {
                    mark(usize::from(slot.0));
                }
            }
        }
        subject.into_iter().for_each(mark);
        let affected = &mut self.affected;
        affected.clear();
        for (word, &bits) in self.marks.iter().enumerate() {
            let mut bits: u64 = bits;
            while bits != 0 {
                affected.push(word * 64 + bits.trailing_zeros() as usize);
                bits &= bits - 1;
            }
        }
    }

    /// Makes the resting places the step placed again the table's own, and empties `fresh` for
    /// the next step.
    fn commit(&mut self, dirty: &[bool]) {
        // A step that placed every group replaces every resting place; otherwise only the dirty
        // groups' old members leave theirs.
        if self.whole {
            self.rests.rests.clear();
            self.groups.clear();
        }
        for (group, members) in self.members.iter().enumerate() {
            if dirty[group] && !self.whole {
                for &slot in members {
                    self.rests.unset(slot);
                    self.groups[usize::from(slot.0)] = None;
                }
            }
        }
        let placed = dirty.iter().enumerate().filter(|(_, dirty)| **dirty);
        for (group, _) in placed {
            for &slot in &self.next[group] {
                let index = usize::from(slot.0);
                let rest = *self
                    .fresh
                    .get(slot)
                    .expect("a group places each of its slots");
                self.rests.set(slot, rest);
                if self.groups.len() <= index {
                    self.groups.resize(index + 1, None);
                }
                self.groups[index] = Some(group);
                self.fresh.unset(slot);
            }
            std::mem::swap(&mut self.members[group], &mut self.next[group]);
        }
        self.whole = false;
        self.first = false;
        // Checked where the interface checks keep records, so that the allocations the frame gate
        // counts in a debug build are a running game's.
        if cfg!(debug_assertions) && self.records {
            assert!(
                same(
                    &self.rests,
                    &self.table.placement_of(&Zones::of_table(&self.played))
                ),
                "placing only the groups a step changed gives what placing every group does"
            );
        }
    }

    /// Plays one step, the `number`th of the script: applies its act, and gives its object and
    /// every object whose resting place changed a segment to the new place.
    fn step(&mut self, number: usize, step: &Step, timing: &StepTiming, laid: &Laid) {
        // A step the table cannot take changes nothing; the table takes the view as it stands
        // when the script ends, whatever the steps did.
        let Some(dirty) = self.place(&step.act) else {
            return;
        };
        let own = Interval {
            from: nanoseconds(laid.from),
            to: nanoseconds(laid.to),
        };
        let (room, regroup) = room_of(timing, laid, own);
        let subject = subject_of(step);
        self.mark_affected(&dirty, subject);
        let affected = std::mem::take(&mut self.affected);
        for &slot in &affected {
            let was = self.rests.rests.get(slot).copied().flatten();
            let now = self.now(slot, &dirty);
            let segment = if Some(slot) == subject {
                let verb = step.act.verb();
                let from = self.pose(slot, own.from);
                let to = match now {
                    Some(rest) => rest.pose,
                    None if was.is_some() => Pose {
                        opacity: 0.0,
                        ..from
                    },
                    None => continue,
                };
                // An instant change that changes nothing needs no segment.
                let moves = matches!(verb, Verb::Fly | Verb::Appear) || from != to;
                moves.then(|| {
                    (
                        verbs::segment(verb, from, to, own, Curve::named(timing.curve)),
                        Cause::Verb(number),
                    )
                })
            } else {
                match (was, now) {
                    (Some(was), Some(now)) if was.pose != now.pose => {
                        let room = match regroup {
                            Some(regroup) if self.stays_in_hand(slot) => regroup,
                            _ => room,
                        };
                        if self.retarget(slot, room.from, was.pose, now.pose) {
                            None
                        } else {
                            Some((
                                verbs::slide(self.pose(slot, room.from), now.pose, room),
                                Cause::Slide(number),
                            ))
                        }
                    }
                    (Some(_), None) => {
                        let from = self.pose(slot, own.from);
                        Some((
                            verbs::instant(
                                from,
                                Pose {
                                    opacity: 0.0,
                                    ..from
                                },
                                own.from,
                            ),
                            Cause::Change(number),
                        ))
                    }
                    (None, Some(now)) => Some((
                        verbs::instant(
                            Pose {
                                opacity: 0.0,
                                ..now.pose
                            },
                            now.pose,
                            own.from,
                        ),
                        Cause::Change(number),
                    )),
                    _ => None,
                }
            };
            if let Some((segment, cause)) = segment {
                self.push(slot, segment, cause);
            }
        }
        self.affected = affected;
        self.commit(&dirty);
    }

    /// Whether the object in `slot` lies in a hand after the step being played: one of the
    /// cards of a hand that regroup round a card that joins or leaves it, where the step does
    /// not move it itself. Where it lay before is not asked: the first step of a script has
    /// no record of the groups.
    fn stays_in_hand(&self, slot: usize) -> bool {
        u16::try_from(slot).is_ok_and(|slot| {
            self.played
                .objects
                .get(&ObjectId(slot))
                .is_some_and(|object| object.zone == ZoneName::Hand)
        })
    }

    /// Sends every segment of the object in `slot` that lands at `was` and has not ended by
    /// `from` to `to` instead, and says whether there was one. When several cards are drawn in
    /// one beat, each later draw makes room in the hand while the cards before it are still in
    /// flight, or have not left yet. A slide from `from` would be cut off by a flight that
    /// begins after it, or would cut short one under way, and the card would end the script
    /// at the place it had before the room was made and jump as the script ends; so the
    /// flight lands at the new place.
    fn retarget(&mut self, slot: usize, from: u64, was: Pose, to: Pose) -> bool {
        let Some(track) = self.tracks.get_mut(slot) else {
            return false;
        };
        let mut found = false;
        for segment in track.segments_mut() {
            if segment.end() > from && segment.to == was {
                segment.to = to;
                found = true;
            }
        }
        // The same segments in the records, which the interface checks read.
        for record in &mut self.segments {
            let segment = &mut record.segment;
            if record.slot == slot && segment.end() > from && segment.to == was {
                segment.to = to;
            }
        }
        found
    }

    /// Where the object in `slot` is at `t` as the steps so far have it: on its track, or where
    /// it rested before the script if nothing has moved it, or nowhere to be seen.
    fn pose(&self, slot: usize, t: u64) -> Pose {
        match self.tracks.get(slot).filter(|t| !t.segments().is_empty()) {
            Some(track) => track.pose_at(t),
            None => self.rests.rests.get(slot).copied().flatten().map_or(
                Pose {
                    opacity: 0.0,
                    ..Pose::default()
                },
                |rest| rest.pose,
            ),
        }
    }

    /// Adds `segment` to the track of the object in `slot`, made for `cause`. An object's first
    /// segment comes after one that holds it where it rested before the script, so that it
    /// stands there until it moves.
    fn push(&mut self, slot: usize, segment: Segment, cause: Cause) {
        // A segment an earlier step gave the object that has not begun when this one does is
        // out of date: this step has moved the object's resting place since. When three
        // permanents tap at once, the first tap's row waits until its card has turned, but
        // the second card turns from the start of its own step, before that slide began.
        if self
            .tracks
            .get_mut(slot)
            .is_some_and(|track| track.cut_after(segment.start))
        {
            self.segments
                .retain(|record| record.slot != slot || record.segment.start <= segment.start);
        }
        if self
            .tracks
            .get(slot)
            .is_none_or(|track| track.segments().is_empty())
        {
            let start = self.pose(slot, 0);
            let hold = verbs::instant(start, start, 0);
            add(&mut self.tracks, slot, hold);
            self.record(slot, Cause::Hold, hold);
        }
        add(&mut self.tracks, slot, segment);
        self.record(slot, cause, segment);
    }

    /// Notes that `cause` gave the object in `slot` `segment`, if it keeps records.
    fn record(&mut self, slot: usize, cause: Cause, segment: Segment) {
        if self.records {
            self.segments.push(SegmentRecord {
                slot,
                cause,
                segment,
            });
        }
    }
}

/// When what the step makes room among goes to its new place, given its `timing`, when it was
/// `laid` out and its `own` interval: what makes room for it does so before it begins, where its
/// act went ahead of it; after it, where it says how long the room it left takes to close; and
/// otherwise as its `room` says, over the step or from a share of it. And, where its `room` says
/// so, when the cards that stay in a hand a card joins or leaves go to their new places: in the
/// time it gives, from when its act is applied, whatever else the step says, as the web stage's
/// do (`carried` in `ui/src/stage/tracks.ts`).
fn room_of(timing: &StepTiming, laid: &Laid, own: Interval) -> (Interval, Option<Interval>) {
    let room = match (laid.opens, timing.closes, timing.room) {
        (Some(opens), _, _) => Interval {
            from: nanoseconds(opens.at),
            to: nanoseconds(opens.by),
        },
        (None, Some(closes), _) => Interval {
            from: own.to,
            to: nanoseconds(laid.to + closes),
        },
        (None, None, Room::From { share }) => Interval {
            from: nanoseconds(laid.from + share * (laid.to - laid.from)),
            to: own.to,
        },
        (None, None, Room::Step | Room::For { .. }) => own,
    };
    let regroup = match timing.room {
        Room::For { ms } => {
            let applied = laid.opens.map_or(laid.from, |opens| opens.at);
            Some(Interval {
                from: nanoseconds(applied),
                to: nanoseconds(applied + ms),
            })
        }
        Room::Step | Room::From { .. } => None,
    };
    (room, regroup)
}

/// Adds `segment` to the track of the object in `slot`, growing `tracks` to hold it.
fn add(tracks: &mut Vec<Track>, slot: usize, segment: Segment) {
    if tracks.len() <= slot {
        tracks.resize(slot + 1, Track::default());
    }
    // No segment of the object begins after this one (`Motion::push` cuts them), so any that
    // ended before this one starts no longer decides a pose and can go to make room.
    if tracks[slot].push(segment, segment.start).is_err() {
        // Every segment it holds is still under way as this one starts, which only a script that
        // moves one object eight times at once can do. The track starts over from this segment,
        // which decides the object's pose from its start on.
        tracks[slot] = Track::default();
        let _ = tracks[slot].push(segment, segment.start);
    }
}

/// Whether `a` and `b` give every slot the same resting place.
fn same(a: &Placement, b: &Placement) -> bool {
    (0..a.rests.len().max(b.rests.len()))
        .all(|slot| a.rests.get(slot).copied().flatten() == b.rests.get(slot).copied().flatten())
}
