//! `Places`: where things stand as a script's steps so far have them, for a verb.

use beryl_protocol::protocol::{ObjectId, ZoneName};
use beryl_protocol::script::{Act, Subject};

use super::{Motion, hidden};
use game_table::table::{Camera, Group, Layout, Table};
use kit_base::pose::{Interval, Pose, Shape};

impl Motion<'_> {
    /// Where the step's own subject starts at `t`: on its track or resting place (restoring
    /// opacity if it was scrolled out of a container), or—when an `Act::Fly` launches a new
    /// object such as a copy cast from exile, graveyard or library—on top of that origin pile.
    pub(super) fn subject_from(&self, table: &Table, slot: usize, act: &Act, t: u64) -> Pose {
        if let Some(mut pose) = self.placed(slot, t) {
            // A card hidden outside its container's window is seen as its own step moves it,
            // from where it rests (`hidden`).
            if let Some(rest) = self
                .rests
                .rests
                .get(slot)
                .and_then(Option::as_ref)
                .filter(|&rest| hidden::scrolled_out(Some(rest)))
            {
                pose.opacity = rest.pose.opacity;
            }
            return pose;
        }
        if let Act::Fly {
            from,
            object,
            origin,
            ..
        } = act
        {
            let (zone, raw_side) = match origin {
                Some(origin) => (origin.zone, origin.side),
                None => (*from, object.owner),
            };
            if matches!(
                zone,
                ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile
            ) {
                let side = (!table.layout().shape.is_shared()).then_some(raw_side);
                let count = self.members[Group::Pile { side, zone }.index()].len();
                let mut pose = table.pile_top_pose(side, zone, count);
                if zone == ZoneName::Library && object.name.is_none() {
                    pose.turn = 1.0;
                }
                return pose;
            }
        }
        Pose {
            opacity: 0.0,
            ..Pose::default()
        }
    }
}

/// Where things are as a script's steps so far have them, for a verb that needs more than its
/// own subject's poses.
pub struct Places<'a> {
    pub(super) motion: &'a Motion<'a>,
    /// The table the script plays over, for its camera, layout and players' anchors.
    pub(super) table: &'a Table,
    /// The groups the step placed again, by `Group::index`.
    pub(super) dirty: &'a [bool],
}

impl Places<'_> {
    /// Where `subject` is at `t` on the script's clock, as the steps so far have it: a card or
    /// an ability on its track or where it rested before the script, a player at their anchor.
    #[must_use]
    pub fn pose(&self, subject: Subject, t: u64) -> Option<Pose> {
        match subject {
            Subject::Object { slot } => self.motion.placed(usize::from(slot.0), t),
            Subject::Player { player } => Some(self.table.player_pose(player)),
            Subject::Ability { id } => self.motion.ability_placed(id, t),
        }
    }

    /// Where `subject` rests after this step's act.
    #[must_use]
    pub fn rest(&self, subject: Subject) -> Option<Pose> {
        match subject {
            Subject::Object { slot } => self
                .motion
                .now(usize::from(slot.0), self.dirty)
                .map(|rest| rest.pose),
            Subject::Player { player } => Some(self.table.player_pose(player)),
            Subject::Ability { id } => self
                .motion
                .ability_now(id, self.dirty)
                .map(|rest| rest.pose),
        }
    }

    /// The cards this step's look lifts off their pile as it begins, or puts back as it ends,
    /// in the order they left it; none for a step that is not a look's.
    #[must_use]
    pub fn handled(&self) -> &[ObjectId] {
        self.motion.look.lifting()
    }

    #[must_use]
    pub fn layout(&self) -> &Layout {
        self.table.layout()
    }

    #[must_use]
    pub fn camera(&self) -> &Camera {
        self.table.camera()
    }

    /// Whether `subject` has an authored segment carrying it across the scene at `t`
    /// (`Track::crossing`).
    #[must_use]
    pub fn crossing(&self, subject: Subject, t: u64) -> bool {
        let Subject::Object { slot } = subject else {
            return false;
        };
        self.motion
            .tracks
            .get(usize::from(slot.0))
            .is_some_and(|track| track.crossing(t))
    }

    /// Appends every card slot other than `subject` whose track has an authored crossing segment
    /// overlapping `interval`.
    pub fn crossing_peers(&self, subject: Subject, interval: Interval, out: &mut Vec<Subject>) {
        let skip = match subject {
            Subject::Object { slot } => Some(usize::from(slot.0)),
            _ => None,
        };
        for (slot, track) in self.motion.tracks.iter().enumerate() {
            if Some(slot) == skip || track.segments().is_empty() {
                continue;
            }
            let overlaps = track.segments().iter().any(|segment| {
                segment.start < interval.to
                    && segment.end() > interval.from
                    && segment
                        .step
                        .is_some_and(|step| step.follows == kit_base::pose::Follows::Start)
                    && segment.shape != Shape::Fading
                    && (segment.from.position != segment.to.position || segment.arc != 0.0)
            });
            if overlaps && let Ok(id) = u16::try_from(slot) {
                out.push(Subject::Object { slot: ObjectId(id) });
            }
        }
    }
}
