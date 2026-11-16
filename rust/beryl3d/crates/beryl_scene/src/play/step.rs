//! One step of a script as it is compiled: the table as the script plays over it (`Motion`),
//! and what a verb is given (`StepIn`, `Places`) and what it writes (`StepOut`)
//! (docs/beryl-3d.md, "Verbs").

use beryl_protocol::protocol::{ObjectId, ObjectView, PlayerView, ZoneName};
use beryl_protocol::script::{Act, Step, StepTiming, Subject, Table as Played, Verb, mover_of};

use super::compile::{nanoseconds, subject_of};
use super::index::{self, Index};
use super::record::{Cause, SegmentRecord};
use crate::beat::Laid;
use crate::motion::Track;
use crate::pose::{Curve, Pose, Segment};
use crate::table::lifted::Look;
use crate::table::{Group, NameIds, PileRules, Placement, Rest, Table, Zones};
use crate::verbs::{self, Interval};

mod abilities;
mod counts;
mod out;
mod placement;
mod places;
mod room;
mod still;
mod tracks;

use room::{only_height, room_of};
use tracks::same;
pub(crate) use tracks::{add, numbers_of};

pub use out::StepOut;
pub(crate) use out::Written;
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

/// The table as a script plays over it, and the tracks its steps have made so far.
pub(crate) struct Motion<'a> {
    table: &'a Table,
    pub played: Played,
    /// Every object's resting place as the table now stands.
    rests: Placement,
    /// The slots each group placed when it was last placed, by `Group::index`.
    members: [Vec<ObjectId>; Group::ALL.len()],
    /// The group each slot was last placed in, by `Group::index`, indexed by slot.
    groups: Vec<Option<usize>>,
    /// The order of the cards of `played`'s hands and exile piles, by slot (`table::arrival`).
    arrival: Vec<u32>,
    /// `played`'s objects and their groups by slot, kept up to date step by step, for a script
    /// with steps enough to repay building it (`index::LEAST_STEPS`).
    index: Option<Index<'a>>,
    /// Whether the next step places every group, not only the ones its act changed: the step
    /// after one the table could not take, which may have changed part of it.
    whole: bool,
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
    next: [Vec<ObjectId>; Group::ALL.len()],
    /// The slots the step's act named.
    touched: Vec<ObjectId>,
    piles: PileRules,
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
    /// What is closing up after a step of the current beat, as the web stage's `closedBy`
    /// finds it on each entry's track: a later step of the beat that moves the same card
    /// again takes its place (`Closing::gone`).
    closing: Vec<Closing>,
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
        let mut members = [const { Vec::new() }; Group::ALL.len()];
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
        let steps = timings.len();
        let index = (steps >= index::LEAST_STEPS).then(|| Index::new(before, played));
        Self {
            table,
            names: NameIds::of(&before.objects),
            played: played.clone(),
            arrival: crate::table::arrival(before, after),
            rests,
            members,
            groups,
            index,
            whole: false,
            unplaced: 0,
            first: true,
            fresh: Placement::default(),
            next: [const { Vec::new() }; Group::ALL.len()],
            touched: Vec::new(),
            piles: PileRules::of_view(before),
            marks: Vec::new(),
            affected: Vec::new(),
            tracks,
            ability_tracks: Vec::new(),
            moved: Vec::new(),
            segments: Vec::new(),
            records,
            written: Written::default(),
            closing: Vec::new(),
            look: Look::opening(
                (before, after),
                timings.iter().any(|timing| timing.duration > 0.0),
            ),
        }
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
        let (played, view) = (self.played_zones(), Zones::of_view(view, abilities));
        self.table.placement_reusing(&view, (&played, &self.rests))
    }

    /// The zones of the table played over, every group listed from it.
    fn played_zones(&self) -> Zones<'_> {
        Zones::of_table(&self.played)
            .handled(self.look.handled())
            .arrived(&self.arrival)
    }

    /// Where the object in `slot` rests after the step, given the groups `dirty` it placed again.
    fn now(&self, slot: usize, dirty: &[bool]) -> Option<&Rest> {
        if let Some(rest) = self.fresh.rests.get(slot).and_then(Option::as_ref) {
            return Some(rest);
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
            self.rests.rests.get(slot).and_then(Option::as_ref)
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
                    if self.fresh.get(slot).is_none() {
                        self.rests.unset(slot);
                        self.groups[usize::from(slot.0)] = None;
                    }
                }
            }
        }
        if dirty[Group::Stack.index()] {
            std::mem::swap(&mut self.rests.abilities, &mut self.fresh.abilities);
            self.fresh.abilities.clear();
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
                same(&self.rests, &self.table.placement_of(&self.played_zones())),
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
        (beat, index): (&'a [Step], usize),
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
        let (room, regroup) = room_of(&step.act, timing, laid, own);
        let Some(dirty) = self.place(&step.act, room.duration() > 0 || own.duration() > 0) else {
            return;
        };
        // A streak may carry a new record for its target. If that changes the target's
        // resting place, it slides with the other cards the new placement moved.
        let subject = if step.act.verb() == Verb::Streak {
            None
        } else {
            subject_of(step)
        };
        // A `set` moves nothing: what it places again, a library it reorders, is at its new
        // place as the step begins. Only the cards that stay in a hand regroup round one it
        // takes out of the fan or puts back, as on the web stage (`carried` in `tracks.ts`).
        let still = matches!(step.act, Act::Set { .. });
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
            self.pile_count((slot, number), &dirty, (own, still));
            if Some(slot) == subject {
                let from = self.pose(slot, own.from);
                let to = match self.now(slot, &dirty) {
                    Some(rest) => Some(rest.pose),
                    None if self.rests.rests.get(slot).is_some_and(Option::is_some) => Some(Pose {
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
            if let Some((segment, cause)) =
                self.follow(slot, (&dirty, still), number, own, (room, regroup))
            {
                self.push(slot, segment, cause);
                if matches!(cause, Cause::Slide(_)) && segment.end() > own.to {
                    self.note_closing(Some(slot), segment.end(), number);
                }
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
        if self.follow_abilities(number, (mover, &dirty), own, room) && room.to > own.to {
            self.note_closing(None, room.to, number);
        }
        self.commit(&dirty);
    }

    /// Notes that something the `number`th step does not move itself, the object in `slot` or
    /// else its abilities, slides until `end`, in nanoseconds, after the step is over: what
    /// closes up after a card that left (`StepTiming::closes`), or the cards that stay in a hand
    /// closing up in the time the step's `room` gives. The beat lasts until it has, as the web
    /// stage's does (`closedBy` in `ui/src/stage/tracks.ts`), unless a later step of the beat
    /// moves the same object again first (`push`).
    fn note_closing(&mut self, slot: Option<usize>, end: u64, number: usize) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a script lasts seconds, far below f64's exact range"
        )]
        let end = end as f64 / 1e6;
        self.closing.push(Closing {
            slot,
            end,
            number,
            gone: false,
        });
    }

    /// Forgets what closes up after the steps of the last beat, as the next one begins.
    pub(crate) fn begin_beat(&mut self) {
        self.closing.clear();
    }

    /// Sets `closes`, which holds an entry for each step of the current beat from the script's
    /// `first`th step, to when what closed up after each step closes, in milliseconds on the
    /// script's clock, as the beat's steps so far leave it.
    pub(crate) fn closings_into(&mut self, first: usize, closes: &mut [Option<f64>]) {
        // Each step a close-up is gone from is worked out again from those left, which a step
        // with none left has none of.
        for closing in self.closing.iter().filter(|closing| closing.gone) {
            closes[closing.number - first] = None;
        }
        self.closing.retain(|closing| !closing.gone);
        for closing in &self.closing {
            let entry = &mut closes[closing.number - first];
            *entry = Some(entry.map_or(closing.end, |end| end.max(closing.end)));
        }
    }

    /// The segment that carries the object in `slot`, which the `number`th step does not move
    /// itself, from where it `rested` to where it rests `now` that the step is done: a slide
    /// over `room` (or `regroup`, for a card that stays in a hand), or an instant change at the
    /// start of the step's `own` time where it comes onto the table or leaves it.
    fn follow(
        &mut self,
        slot: usize,
        (dirty, still): (&[bool], bool),
        number: usize,
        own: Interval,
        (room, regroup): (Interval, Option<Interval>),
    ) -> Option<(Segment, Cause)> {
        let rested = self
            .rests
            .rests
            .get(slot)
            .and_then(Option::as_ref)
            .map(|rest| &rest.pose);
        let now = self.now(slot, dirty).map(|rest| &rest.pose);
        if still && !(regroup.is_some() && self.stays_in_hand(slot)) {
            return self.change(slot, (rested.copied(), now.copied()), number, own);
        }
        if rested == now {
            return None;
        }
        match (rested.copied(), now.copied()) {
            (Some(rested), Some(now)) => {
                let room = match regroup {
                    Some(regroup) if self.stays_in_hand(slot) => regroup,
                    _ => room,
                };
                if self.retarget(slot, room.from, rested, now) {
                    return None;
                }
                let from = self.pose(slot, room.from);
                if only_height(from, now) {
                    // A card that only lies higher or lower among the cards of its row (one
                    // `LAYER` for each card placed before it, so that two never flicker where
                    // they overlap) has not moved: it takes its new height at once. The web
                    // stage gives a card no such height, so nothing closes up after the step
                    // for it there.
                    Some((verbs::instant(from, now, room.from), Cause::Change(number)))
                } else {
                    Some((verbs::slide(from, now, room), Cause::Slide(number)))
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
                        ..now
                    },
                    now,
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
            self.push_ability(id, segment, Cause::Verb(number));
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
        // Its new segment takes the place of what it was closing up along, as a new track does
        // on the web stage, whose beat then waits only for the track the card is on.
        for closing in &mut self.closing {
            closing.gone |= closing.slot == Some(slot);
        }
        let segment = Segment {
            step: cause.step_time(),
            ..segment
        };
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

/// Something that closes up after a step of the current beat (`Motion::note_closing`).
struct Closing {
    /// The slot of the object that slides; none for the abilities on the stack.
    slot: Option<usize>,
    /// When it is where it rests, in milliseconds on the script's clock.
    end: f64,
    /// The step it closes up after, by its number in the script.
    number: usize,
    /// A later step of the beat moved the object again, so the beat no longer waits for this.
    gone: bool,
}
