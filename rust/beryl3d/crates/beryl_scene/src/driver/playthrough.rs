//! Playing a recording through on its own clock to find where each of its moments falls.

use beryl_protocol::protocol::Status;

use super::Page;
use kit_probe::driver::Name;

impl Page {
    /// Plays the recording to its end on its own clock, and returns each moment's time. The
    /// client must keep a record of its scripts (`fresh(true)`), whose spans give the steps'
    /// times.
    pub(super) fn play_through(&mut self) -> Vec<(Name, u64)> {
        let mut moments = vec![(Name::Start, 0)];
        let mut prompts = 0;
        loop {
            self.still();
            for view in self.batch..self.released {
                moments.push((Name::View(view as u32 + 1), self.now));
                if matches!(self.views[view].view.status, Status::Deciding { .. }) {
                    prompts += 1;
                    moments.push((Name::Prompt(prompts), self.now));
                }
                self.step_moments(view, &mut moments);
            }
            if self.released >= self.views.len() {
                break;
            }
            let due = self.now + self.reply_delay().0;
            self.advance_to(due);
        }
        moments.push((Name::End, self.now));
        moments
    }

    /// Adds a moment for each step of the script of `view` (counted from 0) at the middle of the
    /// span the client played it over. The client is given one script with each view, in order,
    /// and keeps a record of each from the start, so the view's index is its script's.
    fn step_moments(&self, view: usize, moments: &mut Vec<(Name, u64)>) {
        let Some(record) = self.client.history().get(view) else {
            return;
        };
        let number = view as u32 + 1;
        for step in &record.steps {
            let Some((from, to)) = record.span(step) else {
                continue;
            };
            let name = Name::Step(number, step.beat as u32, step.index as u32);
            moments.push((name, from + (to - from) / 2));
        }
    }
}
