//! One step of a script as it is compiled: the table as the script plays over it (`Motion`),
//! and what a verb is given (`StepIn`, `Places`) and what it writes (`StepOut`)
//! (docs/beryl-3d.md, "Verbs").

use beryl_protocol::protocol::{ObjectId, ObjectView, PlayerView, StackItem, ZoneName};
use beryl_protocol::script::{Act, Step, StepTiming, Subject, Table as Played, mover_of};

use super::compile::{nanoseconds, subject_of};
use super::record::{Cause, SegmentRecord};
use crate::beat::Laid;
use crate::hud::counts::CountChange;
use crate::light::{SpanChange, StreakRun};
use crate::motion::Track;
use crate::notice::NoticeRun;
use crate::pose::{Curve, Pose, Segment};
use crate::table::lifted::Look;
use crate::table::{Group, NameIds, Placement, Rest, Table, Zones};
use crate::verbs::{self, Interval};

mod abilities;
mod counts;
mod places;
mod room;

use room::room_of;

pub use places::Places;

/// What a verb is given for one step.
pub struct StepIn<'a> {
    pub act: &'a Act,
    /// The spell, ability, permanent or player responsible, if any.
    pub cause: Option<Subject>,
    pub timing: &'a StepTiming,
    /// The step's index in its script.
    pub number: usize,
    /// The steps of the step's beat, the steps played together, for a verb whose motion
    /// depends on the others (two creatures that strike each other meet half way).
    pub beat: &'a [Step],
    /// The step's index in `beat`.
    pub index: usize,
    /// When the step plays, on the script's clock.
    pub own: Interval,
    /// `Curve::named(timing.curve)`.
    pub curve: Curve,
    /// What the step moves (`mover_of`).
    pub subject: Option<Subject>,
    /// The subject's record before the act, when the subject is a card.
    pub was: Option<&'a ObjectView>,
    /// The subject's pose as the step begins; `None` when it has no scene object.
    pub from: Option<Pose>,
    /// Where the subject rests after the act, or `from` faded out where the act takes it off
    /// the table; `None` when it then has no scene object.
    pub to: Option<Pose>,
    /// The script's table after the act.
    pub table: &'a Played,
    pub places: &'a Places<'a>,
}

/// What the verbs of a script have written so far, beside the segments `Motion` adds to its
/// tracks. Kept between steps, so that a step need not allocate it.
#[derive(Default)]
pub(crate) struct Written {
    /// The segments of the step being played, by slot, until `Motion` adds them to its tracks.
    segments: Vec<(usize, Segment)>,
    /// The same for abilities, by number.
    ability_segments: Vec<(u32, Segment)>,
    pub spans: Vec<SpanChange>,
    pub streaks: Vec<StreakRun>,
    pub notices: Vec<NoticeRun>,
    pub counts: Vec<CountChange>,
}

/// What a verb writes for one step.
pub struct StepOut<'a> {
    number: usize,
    own: Interval,
    subject: Option<Subject>,
    from: Option<Pose>,
    to: Option<Pose>,
    written: &'a mut Written,
}

impl StepOut<'_> {
    /// Gives `subject` `segment`, made by this step's verb. A player has no scene object to
    /// move: a segment for one is dropped.
    pub fn segment(&mut self, subject: Subject, segment: Segment) {
        match subject {
            Subject::Object { slot } => self.written.segments.push((usize::from(slot.0), segment)),
            Subject::Ability { id } => self.written.ability_segments.push((id, segment)),
            Subject::Player { .. } => {}
        }
    }

    /// The subject is at `to` from `own.from` on: an instant change. One that changes nothing
    /// needs no segment.
    pub fn instant(&mut self) {
        if let (Some(subject), Some(from), Some(to)) = (self.subject, self.from, self.to)
            && from != to
        {
            self.segment(subject, verbs::instant(from, to, self.own.from));
        }
    }

    /// A span begins or ends.
    pub fn span(&mut self, change: SpanChange) {
        self.written.spans.push(SpanChange {
            step: self.number,
            ..change
        });
    }

    /// Mana or damage streams from one thing to another.
    pub fn streak(&mut self, run: StreakRun) {
        self.written.streaks.push(StreakRun {
            step: self.number,
            ..run
        });
    }

    /// A banner or pill is shown.
    pub fn notice(&mut self, run: NoticeRun) {
        self.written.notices.push(NoticeRun {
            step: self.number,
            ..run
        });
    }
}

/// The table as a script plays over it, and the tracks its steps have made so far.
pub(crate) struct Motion<'a> {
    table: &'a Table,
    pub played: Played,
    /// Every object's resting place as the table now stands.
    rests: Placement,
    /// The slots each group placed when it was last placed, by `Group::index`.
    members: Vec<Vec<ObjectId>>,
    /// The group each slot was last placed in, by `Group::index`, indexed by slot.
    groups: Vec<Option<usize>>,
    /// Whether the next step places every group, not only the ones its act changed: the step
    /// after one the table could not take, which may have changed part of it.
    whole: bool,
    /// How many steps the table could not take.
    pub unplaced: usize,
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
    pub tracks: Vec<Track>,
    /// One track for each ability that moves, sorted by number.
    pub ability_tracks: Vec<(u32, Track)>,
    /// The abilities a step changed the resting place of, with where each rested before and
    /// rests after, kept between steps so that a step need not allocate them.
    moved: Vec<(u32, Option<Rest>, Option<Rest>)>,
    /// Every segment added to `tracks`, in order, with why, if it keeps `records`.
    pub segments: Vec<SegmentRecord>,
    records: bool,
    /// The spans, streaks and notices the verbs wrote.
    pub written: Written,
    /// When the latest slide of what closed up after a step's subject left ends, in
    /// milliseconds on the script's clock, until `take_closed` reads it.
    closed: Option<f64>,
    /// The cards the opponent is handling as the steps so far leave the table, which the table
    /// played over does not keep.
    look: Look,
}

impl<'a> Motion<'a> {
    /// The table `played`, which shows `before`, ready for the steps of the script that leads
    /// to `after`, timed by `timings`, to play over it.
    pub(crate) fn new(
        before: &'a PlayerView,
        played: &Played,
        (table, after, timings): (&'a Table, &PlayerView, &[StepTiming]),
        records: bool,
        shown: bool,
    ) -> Self {
        let rests = if shown {
            table.rests()
        } else {
            table.placement(before, &numbers_of(played))
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
        Self {
            table,
            names: NameIds::of(&before.objects),
            played: played.clone(),
            rests,
            members,
            groups,
            whole: false,
            unplaced: 0,
            first: true,
            fresh: Placement::default(),
            next: vec![Vec::new(); Group::ALL.len()],
            touched: Vec::new(),
            marks: Vec::new(),
            affected: Vec::new(),
            tracks,
            ability_tracks: Vec::new(),
            moved: Vec::new(),
            segments: Vec::new(),
            records,
            written: Written::default(),
            closed: None,
            look: Look::opening(
                (before, after),
                timings.iter().any(|timing| timing.duration > 0.0),
            ),
        }
    }

    /// Applies `act` to the table and places again every group whose resting places it may
    /// have changed: the groups that held or now hold an object the act named, and the stack,
    /// whose abilities an act may add or take away without naming an object. Their resting
    /// places go to `fresh` and their slots to `next`; `rests` is left as it was. Returns which
    /// groups it placed, or none if the table could not take the act.
    fn place(&mut self, act: &Act, timed: bool) -> Option<[bool; Group::ALL.len()]> {
        let touched = &mut self.touched;
        touched.clear();
        if self
            .played
            .apply_noting(act, &mut |slot| touched.push(slot))
            .is_err()
        {
            self.whole = true;
            self.unplaced += 1;
            return None;
        }
        let mut dirty = [self.whole; Group::ALL.len()];
        dirty[Group::Stack.index()] = true;
        self.look.follow((act, timed), &self.played, &mut dirty);
        let zones = Zones::of_table(&self.played).handled(self.look.handled());
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
    /// `abilities` numbers the abilities on `view`'s stack.
    pub(crate) fn placement_of(&self, view: &PlayerView, abilities: &[u32]) -> Placement {
        if self.whole {
            return self.table.placement(view, abilities);
        }
        let played = Zones::of_table(&self.played).handled(self.look.handled());
        self.table
            .placement_reusing(&Zones::of_view(view, abilities), (&played, &self.rests))
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
        // Every step places the stack again.
        std::mem::swap(&mut self.rests.abilities, &mut self.fresh.abilities);
        self.fresh.abilities.clear();
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
                    &self
                        .table
                        .placement_of(&Zones::of_table(&self.played).handled(self.look.handled()))
                ),
                "placing only the groups a step changed gives what placing every group does"
            );
        }
    }

    /// Plays one step, the `index`th of `beat` and the `number`th of the script: applies its act, has its verb play it
    /// (`verbs::play`), and gives every other object whose resting place changed a segment to
    /// the new place.
    pub(crate) fn step(
        &mut self,
        number: usize,
        (beat, index): (&[Step], usize),
        timing: &StepTiming,
        laid: &Laid,
    ) {
        let step = &beat[index];
        let mover = mover_of(&step.act);
        let was = match mover {
            Some(Subject::Object { slot }) => self.played.objects.get(&slot).cloned(),
            _ => None,
        };
        // A step the table cannot take changes nothing; the table takes the view as it stands
        // when the script ends, whatever the steps did.
        let own = Interval {
            from: nanoseconds(laid.from),
            to: nanoseconds(laid.to),
        };
        let (room, regroup) = room_of(timing, laid, own);
        let Some(dirty) = self.place(&step.act, room.duration() > 0 || own.duration() > 0) else {
            return;
        };
        let subject = subject_of(step);
        self.mark_affected(&dirty, subject);
        let affected = std::mem::take(&mut self.affected);
        // A step that moves no card of its own (a player's, an ability's, a span's) has its verb
        // play it first. The cards a look lifts or puts back are the `lift` verb's to move, so
        // the step leaves them to it.
        let mut played = subject.is_none();
        if played {
            let poses = match mover {
                Some(Subject::Ability { id }) => self.ability_poses(id, own.from),
                _ => (None, None),
            };
            self.play(
                number,
                (beat, index),
                timing,
                own,
                (mover, was.as_ref()),
                poses,
                &dirty,
            );
        }
        for &slot in &affected {
            if self
                .look
                .lifting()
                .contains(&ObjectId(u16::try_from(slot).unwrap_or(u16::MAX)))
            {
                continue;
            }
            let rested = self.rests.rests.get(slot).copied().flatten();
            let now = self.now(slot, &dirty);
            if Some(slot) == subject {
                let from = self.pose(slot, own.from);
                let to = match now {
                    Some(rest) => Some(rest.pose),
                    None if rested.is_some() => Some(Pose {
                        opacity: 0.0,
                        ..from
                    }),
                    None => None,
                };
                let poses = (Some(from), to);
                self.play(
                    number,
                    (beat, index),
                    timing,
                    own,
                    (mover, was.as_ref()),
                    poses,
                    &dirty,
                );
                played = true;
                continue;
            }
            let segment = self.follow(slot, (rested, now), number, own, (room, regroup));
            if let Some((segment, cause)) = segment {
                if matches!(cause, Cause::Slide(_)) && segment.end() == room.to {
                    self.note_closing(timing, laid);
                }
                self.push(slot, segment, cause);
            }
        }
        if !played {
            self.play(
                number,
                (beat, index),
                timing,
                own,
                (mover, was.as_ref()),
                (None, None),
                &dirty,
            );
        }
        self.affected = affected;
        if self.follow_abilities(mover, own, room) {
            self.note_closing(timing, laid);
        }
        self.commit(&dirty);
    }

    /// Notes that something slid over the room of a step with this `timing`, `laid` out so. If
    /// that room is what closes up after the step's subject left (`StepTiming::closes`), the
    /// beat lasts until it has closed, as the web stage's does (`closedBy` in
    /// `ui/src/stage/tracks.ts`).
    fn note_closing(&mut self, timing: &StepTiming, laid: &Laid) {
        if let (None, Some(closes)) = (laid.opens, timing.closes) {
            let end = laid.to + closes;
            self.closed = Some(self.closed.map_or(end, |closed| closed.max(end)));
        }
    }

    /// When the latest slide of what closed up after a step since the last call ends, in
    /// milliseconds on the script's clock, if any did.
    pub(crate) fn take_closed(&mut self) -> Option<f64> {
        self.closed.take()
    }

    /// The segment that carries the object in `slot`, which the `number`th step does not move
    /// itself, from where it `rested` to where it rests `now` that the step is done: a slide
    /// over `room` (or `regroup`, for a card that stays in a hand), or an instant change at the
    /// start of the step's `own` time where it comes onto the table or leaves it.
    fn follow(
        &mut self,
        slot: usize,
        (rested, now): (Option<Rest>, Option<Rest>),
        number: usize,
        own: Interval,
        (room, regroup): (Interval, Option<Interval>),
    ) -> Option<(Segment, Cause)> {
        match (rested, now) {
            (Some(rested), Some(now)) if rested.pose != now.pose => {
                let room = match regroup {
                    Some(regroup) if self.stays_in_hand(slot) => regroup,
                    _ => room,
                };
                if self.retarget(slot, room.from, rested.pose, now.pose) {
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
    }

    /// Has the verb of the step `index` of `beat`, the `number`th, play it over `own`, given its subject and that
    /// subject's record before the act, and its poses as the step begins and once it is done.
    /// Adds the segments the verb wrote to the tracks, and keeps what else it wrote.
    #[expect(
        clippy::too_many_arguments,
        reason = "each is one part of the step the verb is given"
    )]
    fn play(
        &mut self,
        number: usize,
        (beat, index): (&[Step], usize),
        timing: &StepTiming,
        own: Interval,
        (subject, was): (Option<Subject>, Option<&ObjectView>),
        (from, to): (Option<Pose>, Option<Pose>),
        dirty: &[bool],
    ) {
        let step = &beat[index];
        let mut written = std::mem::take(&mut self.written);
        let places = Places {
            motion: self,
            dirty,
        };
        let input = StepIn {
            act: &step.act,
            cause: step.cause,
            timing,
            number,
            beat,
            index,
            own,
            curve: Curve::named(timing.curve),
            subject,
            was,
            from,
            to,
            table: &self.played,
            places: &places,
        };
        let mut out = StepOut {
            number,
            own,
            subject,
            from,
            to,
            written: &mut written,
        };
        verbs::play(step.act.verb())(&input, &mut out);
        let mut segments = std::mem::take(&mut written.segments);
        for (slot, segment) in segments.drain(..) {
            self.push(slot, segment, Cause::Verb(number));
        }
        written.segments = segments;
        let mut segments = std::mem::take(&mut written.ability_segments);
        for (id, segment) in segments.drain(..) {
            self.push_ability(id, segment);
        }
        written.ability_segments = segments;
        self.written = written;
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
        self.placed(slot, t).unwrap_or(Pose {
            opacity: 0.0,
            ..Pose::default()
        })
    }

    /// Where the object in `slot` is at `t` as the steps so far have it: on its track, or where
    /// it rested before the script if nothing has moved it; `None` if it has no scene object.
    fn placed(&self, slot: usize, t: u64) -> Option<Pose> {
        match self.tracks.get(slot).filter(|t| !t.segments().is_empty()) {
            Some(track) => Some(track.pose_at(t)),
            None => self
                .rests
                .rests
                .get(slot)
                .copied()
                .flatten()
                .map(|rest| rest.pose),
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

/// Adds `segment` to the track of the object in `slot`, growing `tracks` to hold it.
pub(crate) fn add(tracks: &mut Vec<Track>, slot: usize, segment: Segment) {
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

/// Whether `a` and `b` give every slot and every ability the same resting place.
fn same(a: &Placement, b: &Placement) -> bool {
    a.abilities == b.abilities
        && (0..a.rests.len().max(b.rests.len())).all(|slot| {
            a.rests.get(slot).copied().flatten() == b.rests.get(slot).copied().flatten()
        })
}

/// The numbers of the abilities on `played`'s stack, bottom first.
pub(crate) fn numbers_of(played: &Played) -> Vec<u32> {
    played
        .stack
        .iter()
        .filter(|entry| matches!(entry.item, StackItem::Ability { .. }))
        .filter_map(|entry| entry.id)
        .collect()
}
