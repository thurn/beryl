//! What the writers of light and effects are given to ask where things are in a frame, whether
//! they are moving, and which way their controllers face.

use beryl_protocol::protocol::Side;
use beryl_protocol::script::Subject;

use super::Show;
use crate::effects::Reach;
use crate::table::{Table, ability_track};

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

    /// Where `subject` was `ago` nanoseconds before the frame's time, on the playing script's
    /// track: at the script's start for a time before it, and where it rests once no script
    /// moves it.
    #[must_use]
    pub fn reach_before(&self, subject: Subject, ago: u64) -> Option<Reach> {
        self.show.reach(subject, self.now.saturating_sub(ago))
    }

    /// The table, for its camera and layout.
    #[must_use]
    pub fn table(&self) -> &Table {
        self.show.table()
    }
}

impl Scene<'_> {
    /// Whether `subject` is moving at the frame's time: a card or an ability on a track of the
    /// playing script that changes its pose then. A player's anchor never moves.
    #[must_use]
    pub fn moving(&self, subject: Subject) -> bool {
        let Some(playing) = self
            .show
            .playing
            .as_ref()
            .filter(|playing| playing.update.is_some())
        else {
            return false;
        };
        let t = playing.clock(self.now);
        let track = match subject {
            Subject::Object { slot } => playing.compiled.tracks.get(usize::from(slot.0)),
            Subject::Ability { id } => ability_track(&playing.compiled.ability_tracks, id),
            Subject::Player { .. } => None,
        };
        track.is_some_and(|track| track.moving(t))
    }

    /// Which way across the table `subject`'s controller faces, on the screen: -1 up it for the
    /// viewer's, 1 down it for the opponent's, as the web stage's `Reach::toward` has it. An
    /// ability is the viewer's way, as there; a player faces neither way.
    #[must_use]
    pub fn toward(&self, subject: Subject) -> f32 {
        match subject {
            Subject::Object { slot } => {
                let controller = self.show.shown.as_ref().and_then(|update| {
                    update
                        .view
                        .objects
                        .iter()
                        .find(|object| object.slot == slot)
                        .map(|object| object.controller)
                });
                if controller == Some(Side::Opponent) {
                    1.0
                } else {
                    -1.0
                }
            }
            Subject::Ability { .. } => -1.0,
            Subject::Player { .. } => 0.0,
        }
    }
}
