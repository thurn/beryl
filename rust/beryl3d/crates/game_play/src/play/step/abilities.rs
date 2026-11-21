//! How a script's steps move the abilities on the stack: the subject's poses for its verb,
//! the slide of each ability whose place on the stack a step changed, and their tracks.

use beryl_protocol::script::Subject;

use super::Motion;
use crate::play::record::Cause;
use crate::verbs::{self, Interval};
use game_table::table::{Group, Rest, ability_track};
use kit_base::pose::{Pose, Segment};
use kit_space::motion::Track;

impl Motion<'_> {
    /// Where the ability numbered `id` rests after a step that placed the groups `dirty` again:
    /// its new place if the step placed the stack again, and otherwise the one it had.
    pub(super) fn ability_now(&self, id: u32, dirty: &[bool]) -> Option<&Rest> {
        if dirty[Group::Stack.index()] {
            self.fresh.ability(id)
        } else {
            self.rests.ability(id)
        }
    }

    /// The poses of the ability numbered `id`, the subject of a step that begins at `from`: where
    /// it is then and where it rests once the step is done, or, where it comes onto the stack or
    /// leaves it, the same faded out.
    pub(super) fn ability_poses(&self, id: u32, from: u64) -> (Option<Pose>, Option<Pose>) {
        let now = self.fresh.ability(id).map(|rest| rest.pose);
        let was = self.ability_placed(id, from);
        let faded = |pose: Pose| Pose {
            opacity: 0.0,
            ..pose
        };
        match (was, now) {
            (Some(was), now) => (Some(was), Some(now.unwrap_or(faded(was)))),
            (None, Some(now)) => (Some(faded(now)), Some(now)),
            (None, None) => (None, None),
        }
    }

    /// Gives every ability on the stack before or after a step, other than the
    /// one it moves itself, a segment to where it rests now: a slide over `room` where the
    /// items below it changed, and an instant change at the start of the step's `own` time
    /// where it comes onto the stack or leaves it without being the step's subject. Returns
    /// whether any ability slid. `number` is the step's index in its script, and `dirty` the
    /// groups it placed again; one that did not place the stack moves no ability.
    pub(super) fn follow_abilities(
        &mut self,
        number: usize,
        (mover, dirty): (Option<Subject>, &[bool]),
        own: Interval,
        room: Interval,
    ) -> bool {
        if !dirty[Group::Stack.index()] {
            return false;
        }
        let subject = match mover {
            Some(Subject::Ability { id }) => Some(id),
            _ => None,
        };
        let mut changed = std::mem::take(&mut self.moved);
        let mut slid = false;
        changed.clear();
        for &(id, rested) in &self.rests.abilities {
            let now = self.fresh.ability(id).copied();
            if now.is_none_or(|now| now.pose != rested.pose) {
                changed.push((id, Some(rested), now));
            }
        }
        for &(id, now) in &self.fresh.abilities {
            if self.rests.ability(id).is_none() {
                changed.push((id, None, Some(now)));
            }
        }
        for &(id, rested, now) in changed.iter().filter(|(id, ..)| Some(*id) != subject) {
            let (segment, cause) = match (rested, now) {
                (Some(_), Some(now)) => {
                    let from = self.ability_placed(id, room.from).unwrap_or(now.pose);
                    slid = true;
                    (verbs::slide(from, now.pose, room), Cause::Slide(number))
                }
                (Some(rested), None) => {
                    let from = self.ability_placed(id, own.from).unwrap_or(rested.pose);
                    let gone = Pose {
                        opacity: 0.0,
                        ..from
                    };
                    (verbs::instant(from, gone, own.from), Cause::Change(number))
                }
                (None, Some(now)) => {
                    let unseen = Pose {
                        opacity: 0.0,
                        ..now.pose
                    };
                    (
                        verbs::instant(unseen, now.pose, own.from),
                        Cause::Change(number),
                    )
                }
                (None, None) => continue,
            };
            self.push_ability(id, segment, cause);
        }
        self.moved = changed;
        slid
    }

    /// Where the ability numbered `id` is at `t` as the steps so far have it: on its track, or
    /// where it rested before the script if nothing has moved it; `None` if it is not on the
    /// stack.
    pub(crate) fn ability_placed(&self, id: u32, t: u64) -> Option<Pose> {
        match ability_track(&self.ability_tracks, id).filter(|t| !t.segments().is_empty()) {
            Some(track) => Some(track.pose_at(t)),
            None => self.rests.ability(id).map(|rest| rest.pose),
        }
    }

    /// Adds `segment` to the track of the ability numbered `id`, as `push` does for a card.
    /// The interface checks keep no record of an ability's segments.
    pub(super) fn push_ability(&mut self, id: u32, segment: Segment, cause: Cause) {
        let segment = Segment {
            step: cause.step_time(),
            ..segment
        };
        let at = match self
            .ability_tracks
            .binary_search_by_key(&id, |(number, _)| *number)
        {
            Ok(at) => at,
            Err(at) => {
                self.ability_tracks.insert(at, (id, Track::default()));
                at
            }
        };
        if self.ability_tracks[at].1.segments().is_empty() {
            let start = self.ability_placed(id, 0).unwrap_or(Pose {
                opacity: 0.0,
                ..segment.from
            });
            let _ = self.ability_tracks[at]
                .1
                .push(verbs::instant(start, start, 0), 0);
        }
        let track = &mut self.ability_tracks[at].1;
        track.cut_after(segment.start);
        if track.push(segment, segment.start).is_err() {
            *track = Track::default();
            let _ = track.push(segment, segment.start);
        }
    }
}
