//! `Places`: where things stand as a script's steps so far have them, for a verb.

use beryl_protocol::protocol::ObjectId;
use beryl_protocol::script::Subject;

use super::Motion;
use crate::pose::Pose;
use crate::table::{Camera, Layout};

/// Where things are as a script's steps so far have them, for a verb that needs more than its
/// own subject's poses.
pub struct Places<'a> {
    pub(super) motion: &'a Motion<'a>,
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
            Subject::Player { player } => Some(self.motion.table.player_pose(player)),
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
            Subject::Player { player } => Some(self.motion.table.player_pose(player)),
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
        self.motion.table.layout()
    }

    #[must_use]
    pub fn camera(&self) -> &Camera {
        self.motion.table.camera()
    }
}
