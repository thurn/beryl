//! The notices a starting script adds to the show, kept in real time until each ends, and when
//! every notice that passes over the table is over (docs/beryl-3d.md, "Timing a script").

use beryl_protocol::protocol::PlayerView;

use super::{Playing, Show};
use crate::notice::{self, Announced, NoticeRun, Place};

/// How many notices the show holds before holding more allocates.
pub(super) const NOTICES: usize = 8;

impl Playing {
    /// Adds each of its notices to `notices`, in real time, as the script starts. When the list
    /// is full, a notice replaces the one that ends first, so that it allocates nothing.
    pub(super) fn announce(&self, notices: &mut Vec<Announced>) {
        for run in &self.compiled.notices {
            let announced = Announced {
                place: Place::of(&run.notice),
                key: notice::key(self.number, run.step),
                over: self.real_interval(run.over),
                passes: run.passes,
            };
            if notices.len() < NOTICES {
                notices.push(announced);
            } else if let Some(first) = notices.iter_mut().min_by_key(|other| other.over.to) {
                *first = announced;
            }
        }
    }
}

impl Show {
    /// The notices the show holds, each in nanoseconds since the client started, in the order
    /// their scripts started them: those under way and those that begin later in the playing
    /// script, until each ends. A script with a reset drops them all.
    #[must_use]
    pub fn notices(&self) -> &[Announced] {
        &self.notices
    }

    /// The real time by which every notice that passes over the table is over, or 0 when none
    /// is held: what `beat` waits for before a step that is `alone`, as the web stage's
    /// `Overlay::passed` (`ui/src/stage/overlay.ts`).
    #[must_use]
    pub fn passed(&self) -> u64 {
        (self.notices.iter())
            .filter(|announced| announced.passes)
            .map(|announced| announced.over.to)
            .max()
            .unwrap_or(0)
    }

    /// The notices of the playing script and of each waiting one, with the number of the script
    /// (for `notice::key`) and the view it leads to, for the client to lay out their elements
    /// as each update arrives.
    pub fn coming_notices(&self) -> impl Iterator<Item = (usize, &[NoticeRun], &PlayerView)> {
        let playing = self.playing.as_ref().and_then(|playing| {
            let view = (playing.update.as_deref())
                .or(self.shown.as_deref())
                .map(|update| &update.view)?;
            Some((playing.number, playing.compiled.notices.as_slice(), view))
        });
        let waiting = (self.waiting.iter()).map(|waiting| {
            let notices = waiting.compiled.notices.as_slice();
            (waiting.number, notices, &waiting.update.view)
        });
        playing.into_iter().chain(waiting)
    }
}
