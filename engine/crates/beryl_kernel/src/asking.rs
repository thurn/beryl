//! [`Asking`]: the answers a run that may pause is given, carried in the sink so a question
//! can be asked from any depth of the rules (architecture §7.3).
//!
//! A resolution's body asks its questions through the resolution it runs in. A replacement
//! effect's choice (see [CR 616.1]) can come up below code that has no resolution to ask
//! through: the draw of the draw step, or a draw in the middle of a body that never declared
//! it asks. The record of answers, and the question a run stopped at, live here instead,
//! beside the events, which go everywhere the rules do.
//!
//! A run that may pause is run on a copy of the state with the record so far. A question
//! the record cannot answer is kept here and the run stops: the code that asked returns
//! [`Full`](crate::fixed::Full), which every function of the rules already passes up, and
//! whoever opened the run tells the two apart with [`Asking::paused`]. The copy is then
//! discarded and the question waits in the committed state's `pending` with the record.
//!
//! [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
//!
//! Implements: none. Records answers and the question that paused a run.

use crate::decision::{Choice, Decision};
use crate::fixed::Full;
use crate::layout::{Layout, Queue, Small};

/// The answers of the run under way, how many it has used, and the question it stopped at.
#[derive(Debug, Default)]
pub struct Asking<L: Layout = Small> {
    record: L::Queue<L::Index, 32>,
    used: usize,
    /// Whether a question the record cannot answer stops the run: the run is on a copy of the
    /// state, which the caller discards. Outside such a run a question takes its first
    /// candidate.
    open: bool,
    question: Option<Decision<L>>,
    /// Whether the question the run stopped at is a hidden pick, whose answer is recorded as
    /// the slot picked ([`crate::state::Resolving`], "Hidden picks"). It outlives
    /// [`Asking::end`], for the caller to read with [`Asking::take_hidden`].
    hidden: bool,
    /// Whether an entry copy replacement paused the run. Scenario traces read this
    /// after [`Asking::end`] to tell this choice apart from other optional picks.
    enter_as_copy: bool,
}

impl<L: Layout> Asking<L> {
    /// Starts a run that answers from `record`, then stops at the first question it cannot
    /// answer if `open`.
    #[inline]
    pub fn begin(&mut self, record: &[L::Index], open: bool) -> Result<(), Full> {
        self.record.clear();
        for &answer in record {
            self.record.push(answer)?;
        }
        self.used = 0;
        self.open = open;
        self.question = None;
        self.hidden = false;
        self.enter_as_copy = false;
        Ok(())
    }

    /// Ends the run, forgetting its record. Returns the question it stopped at, if it did.
    #[inline]
    pub fn end(&mut self) -> Option<Decision<L>> {
        self.record.clear();
        self.used = 0;
        self.open = false;
        self.question.take()
    }

    /// The next recorded answer, if the record has one left.
    #[inline]
    pub fn recorded(&mut self) -> Option<Choice> {
        let &answer = self.record.as_slice().get(self.used)?;
        self.used += 1;
        Some(Choice(answer.into()))
    }

    /// Whether a question with no recorded answer stops the run.
    #[inline]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Stops the run at `question`. The caller returns [`Full`] to unwind.
    #[inline]
    pub fn stop(&mut self, question: Decision<L>) -> Full {
        debug_assert!(self.open, "a run stopped that cannot pause");
        self.question = Some(question);
        Full
    }

    /// Notes that the question the run is stopping at is a hidden pick.
    #[inline]
    pub fn stop_hidden(&mut self) {
        self.hidden = true;
    }

    /// Whether the question the last run stopped at is a hidden pick, forgetting it.
    #[inline]
    pub fn take_hidden(&mut self) -> bool {
        core::mem::take(&mut self.hidden)
    }

    /// Marks the stop at a copy choice made as a permanent enters the battlefield.
    pub fn mark_enter_as_copy(&mut self) {
        debug_assert!(self.paused());
        self.enter_as_copy = true;
    }

    /// Takes the marker for the stop just reached. The trace takes it at each decision.
    pub fn take_enter_as_copy(&mut self) -> bool {
        core::mem::take(&mut self.enter_as_copy)
    }

    /// Whether the run stopped at a question, so that the [`Full`] it returned is that stop
    /// and not a capacity running out.
    #[inline]
    pub fn paused(&self) -> bool {
        self.question.is_some()
    }
}
