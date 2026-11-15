//! What the writers of light and effects are given to ask where things are in a frame.

use beryl_protocol::script::Subject;
use glam::Vec2;

use super::Show;
use crate::pose::Pose;
use crate::table::Table;

/// Where something is and how large, for the light drawn to or on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub pose: Pose,
    /// Half its width and height, in millimetres. A player's anchor is a point.
    pub half: Vec2,
}

/// The show as a frame draws it, for the writers of light and effects to ask where things are.
#[derive(Clone, Copy)]
pub struct Scene<'a> {
    show: &'a Show,
    now: u64,
}

impl<'a> Scene<'a> {
    pub(super) fn new(show: &'a Show, now: u64) -> Self {
        Self { show, now }
    }
}

impl Scene<'_> {
    /// Where `subject` is at the frame's time (`Show::reach`).
    #[must_use]
    pub fn reach(&self, subject: Subject) -> Option<Reach> {
        self.show.reach(subject, self.now)
    }

    /// The table, for its camera and layout.
    #[must_use]
    pub fn table(&self) -> &Table {
        self.show.table()
    }
}
