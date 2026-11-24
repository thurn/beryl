//! The life totals and pools the table shows, and those the scripts still to play will set,
//! for the client to lay out their floats ahead (`hud::counts`).

use beryl_protocol::protocol::PlayerView;

use super::{Playing, Show};
use crate::hud::counts::{CountChange, Counts};

impl Playing {
    /// Gives `counts` each count change of the script whose real time is at or before `now`, in
    /// order, that it has not given yet.
    pub(super) fn give_counts(&mut self, now: u64, counts: &mut Counts) {
        while let Some(change) = self.compiled.counts.get(self.counts_given) {
            if self.real(change.at) > now {
                break;
            }
            counts.apply(change);
            self.counts_given += 1;
        }
    }
}

impl Show {
    /// The life totals and pools the table shows: those of the view shown, changed by each
    /// `count` step of the playing script as it begins.
    #[must_use]
    pub fn counts(&self) -> &Counts {
        &self.counts
    }

    /// The count changes still to come, in the order they play, each script's with the view
    /// it leads to: the rest of the playing script's, then each waiting script's.
    pub fn coming_counts(&self) -> impl Iterator<Item = (&[CountChange], &PlayerView)> {
        let playing = self.playing.as_ref().and_then(|playing| {
            // A script that moves nothing shows its view as it starts.
            let view = (playing.update.as_deref())
                .or(self.shown.as_deref())
                .map(|update| &update.view)?;
            Some((&playing.compiled.counts[playing.counts_given..], view))
        });
        let waiting = (self.waiting.iter())
            .map(|waiting| (waiting.compiled.counts.as_slice(), &waiting.update.view));
        playing.into_iter().chain(waiting)
    }
}
