//! Clocked host inputs and extra time between views of synthetic recordings.

use std::sync::Arc;

use super::{Duration, Page, REPLY_DELAY, STILL_LIMIT};
use crate::frame::Wake;
use crate::input::{Input, Pointer};

#[derive(Default)]
pub(super) struct Cues {
    moves: Arc<[(u64, Pointer)]>,
    pub(super) next: usize,
    pause: Option<(usize, Duration)>,
}

impl Cues {
    pub(super) fn fresh(&self) -> Self {
        Self {
            moves: Arc::clone(&self.moves),
            next: 0,
            pause: self.pause,
        }
    }

    pub(super) fn done(&self) -> bool {
        self.next == self.moves.len()
    }
}

impl Page {
    /// Adds mouse events in time order, all after the initial frame.
    pub fn set_pointer_moves(&mut self, moves: Vec<(u64, Pointer)>) {
        self.cues.moves = moves.into();
        self.cues.next = 0;
    }

    /// Waits for `delay` after `released` views have been received and the table rests.
    pub fn set_reply_delay(&mut self, released: usize, delay: Duration) {
        self.cues.pause = Some((released, delay));
    }

    pub(super) fn reply_delay(&self) -> Duration {
        match self.cues.pause {
            Some((released, delay)) if self.released == released => delay,
            _ => REPLY_DELAY,
        }
    }

    pub(super) fn give_cues(&mut self) {
        while let Some(&(at, pointer)) = self.cues.moves.get(self.cues.next) {
            if at > self.now {
                break;
            }
            self.cues.next += 1;
            self.give(Input::Pointer(pointer));
        }
    }

    pub(super) fn next_frame(&self) -> u64 {
        let wake = match self.client.wake() {
            Wake::Now => self.now + self.tick.0,
            Wake::At(time) => time.max(self.now + 1),
            Wake::Never => u64::MAX,
        };
        // A frame falls when each notice ends, so that the first frame at rest after a notice
        // that outlasted its script is at the same time however often the clock was stopped. A
        // reply due while the notice still showed is given then.
        let wake = self
            .client
            .notice_end()
            .filter(|&end| end > self.now)
            .map_or(wake, |end| end.min(wake));
        self.cues
            .moves
            .get(self.cues.next)
            .map_or(wake, |&(at, _)| at.min(wake))
    }

    /// Runs the clock until nothing on the table moves.
    ///
    /// # Panics
    ///
    /// When the table is still moving after ten minutes of the clock.
    pub fn still(&mut self) {
        let limit = self.now + STILL_LIMIT.0;
        self.render();
        loop {
            let next = self.next_frame();
            if next == u64::MAX {
                return;
            }
            if next > limit {
                let message = format!("still() ran for {STILL_LIMIT} and the table never rested");
                panic!("{}", self.error(&message));
            }
            self.now = next;
            self.render();
        }
    }
}
