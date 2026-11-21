//! Where an object is at a time, worked out from the segments it holds (docs/beryl-3d.md,
//! "Determinism": "Positions are functions of time"). Every function here is pure: the same
//! segments and the same time give the same pose, however the frames before it went.

use arrayvec::ArrayVec;
use kit_base::pose::{Curve, Follows, Interval, Pose, Segment};

/// The visible fraction of a fade that comes in over `begins` and goes out over `ends`.
/// A zero-length interval changes immediately at its end.
#[must_use]
pub fn visibility(begins: Interval, ends: Option<Interval>, curve: Curve, now: u64) -> f32 {
    let through = |interval: Interval| {
        if now >= interval.to {
            1.0
        } else if now <= interval.from {
            0.0
        } else {
            #[expect(clippy::cast_precision_loss, reason = "a fade's share of its time")]
            let share = (now - interval.from) as f32 / (interval.to - interval.from) as f32;
            curve.ease(share)
        }
    };
    through(begins) * (1.0 - ends.map_or(0.0, through))
}

/// An object's pose at `t`, from its `segments` sorted by start. The latest segment that has
/// begun by `t` decides it, so a segment that begins replaces any still under way. Before
/// the first begins the object stands at that segment's start pose, and with no segments at
/// all it stands at the default pose.
#[must_use]
pub fn pose_at(segments: &[Segment], t: u64) -> Pose {
    if let Some(segment) = segments.iter().rev().find(|segment| segment.start <= t) {
        segment.pose_at(t)
    } else {
        segments
            .first()
            .map_or_else(Pose::default, |first| first.from)
    }
}

/// The most segments one object holds at once. A script laid out ahead of time can give one
/// card several that lie in the future: drawn, held up to be read, then cast and resolved, each
/// with the slides and holds round it.
pub const SEGMENTS: usize = 16;

/// A track already holds `SEGMENTS` segments and none of them can be dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// One object's segments, sorted by start, in a fixed array so that adding one never
/// allocates.
#[derive(Clone, Debug, Default)]
pub struct Track {
    segments: ArrayVec<Segment, SEGMENTS>,
}

impl Track {
    /// The segments it holds, sorted by start.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        self.segments.as_slice()
    }

    /// The segments it holds, to change their poses. Their starts must not change, so that
    /// they stay sorted.
    pub fn segments_mut(&mut self) -> &mut [Segment] {
        self.segments.as_mut_slice()
    }

    /// The object's pose at `t` (`pose_at`).
    #[must_use]
    pub fn pose_at(&self, t: u64) -> Pose {
        pose_at(self.segments(), t)
    }

    /// Whether the object is moving at `t`: a segment under way at `t` changes its pose. A
    /// segment that holds the object still does not count.
    #[must_use]
    pub fn moving(&self, t: u64) -> bool {
        (self.segments().iter())
            .any(|segment| segment.start <= t && t < segment.end() && segment.moves())
    }

    /// Whether a step's own act holds the object still at `t` between two of the segments it
    /// moves the object along, as a strike holds its creature at its poise and where its blow
    /// lands. The object is drawn at rest there, but it is still in the middle of its verb, so
    /// the checks that judge only cards at rest skip it. A pause between two steps, or after a
    /// step's last move, does not count.
    #[must_use]
    pub fn paused(&self, t: u64) -> bool {
        let segments = self.segments();
        segments.iter().any(|before| {
            before.end() <= t
                && before.moves()
                && before
                    .step
                    .is_some_and(|step| step.follows == Follows::Start)
                && segments
                    .iter()
                    .any(|after| t < after.start && after.moves() && after.step == before.step)
        })
    }

    /// Whether a segment that changes the object's pose has begun by `t`. Until one has, the
    /// object holds the pose it started the track in, or one only a layer higher or lower
    /// (`Segment::shifts`).
    #[must_use]
    pub fn begun(&self, t: u64) -> bool {
        (self.segments().iter()).any(|segment| segment.start <= t && segment.shifts())
    }

    /// Whether every segment that changes the object's pose has ended by `t`. From then on the
    /// object holds the pose it ends the track in.
    #[must_use]
    pub fn settled(&self, t: u64) -> bool {
        (self.segments().iter()).all(|segment| segment.end() <= t || !segment.shifts())
    }

    /// Whether a step's own act moves the object at `t`: a segment its verb made, or that
    /// brought it to the table or took it away, is under way and moves it more than a layer up
    /// or down (`Segment::shifts`). A segment the object slides along because a step changed
    /// where it rests does not count.
    #[must_use]
    pub fn carried(&self, t: u64) -> bool {
        self.segments().iter().any(|segment| {
            segment.start <= t
                && t < segment.end()
                && segment.shifts()
                && segment
                    .step
                    .is_some_and(|step| step.follows == Follows::Start)
        })
    }

    /// Adds `segment` after every segment that starts no later than it, so that of two that
    /// start together the one added last decides the pose. When the track is full it first
    /// drops the segments that ended by `now`, all but the latest of them: that one still
    /// gives the pose until the next begins, and no time from `now` on can reach the others,
    /// because a later segment has begun. A segment that has not ended is never dropped,
    /// since a script laid out ahead of time holds segments that lie in the future, so with
    /// none ended a full track refuses the new one.
    ///
    /// # Errors
    ///
    /// `Full` when the track holds `SEGMENTS` segments and at most one of them has ended.
    pub fn push(&mut self, segment: Segment, now: u64) -> Result<(), Full> {
        if self.segments.is_full() {
            self.drop_ended(now);
            if self.segments.is_full() {
                return Err(Full);
            }
        }
        if self
            .segments
            .last()
            .is_none_or(|last| last.start <= segment.start)
        {
            self.segments.push(segment);
        } else {
            let at = self.segments.partition_point(|s| s.start <= segment.start);
            self.segments.insert(at, segment);
        }
        Ok(())
    }

    /// Appends to a track already cut to the start of `segment`.
    pub fn append(&mut self, segment: Segment) {
        debug_assert!(
            self.segments()
                .last()
                .is_none_or(|last| last.start <= segment.start)
        );
        if self.segments.is_full() {
            self.drop_ended(segment.start);
            if self.segments.is_full() {
                self.segments.clear();
            }
        }
        self.segments.push(segment);
    }

    /// Moves each segment later by what `later` gives for it, keeping them sorted by start and,
    /// of two that start together, in the order they were added. Allocates nothing.
    pub fn shift(&mut self, later: impl Fn(&Segment) -> u64) {
        for segment in self.segments_mut() {
            segment.start += later(segment);
        }
        // An insertion sort, which is stable and allocates nothing; the segments are almost
        // always in order already.
        for i in 1..self.segments.len() {
            let mut j = i;
            while j > 0 && self.segments[j - 1].start > self.segments[j].start {
                self.segments.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    /// Drops every segment that begins after `t`, and says whether there was one.
    pub fn cut_after(&mut self, t: u64) -> bool {
        if self.segments().last().is_none_or(|last| last.start <= t) {
            return false;
        }
        self.segments
            .truncate(self.segments.partition_point(|s| s.start <= t));
        true
    }

    /// Drops every segment that ended by `now` except the latest of them.
    fn drop_ended(&mut self, now: u64) {
        let Some(kept) = self.segments().iter().rposition(|s| s.end() <= now) else {
            return;
        };
        let mut index = 0;
        self.segments.retain(|s| {
            let retain = index == kept || s.end() > now;
            index += 1;
            retain
        });
    }
}
