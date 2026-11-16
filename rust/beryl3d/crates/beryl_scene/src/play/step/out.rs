//! What a verb writes for one step (`StepOut`), and what the verbs of a script have written so
//! far (`Written`).

use beryl_protocol::script::Subject;

use crate::hud::counts::CountChange;
use crate::light::{SpanChange, StreakRun};
use crate::notice::NoticeRun;
use crate::pose::{Pose, Segment};
use crate::table::{PileCount, Turn};
use crate::verbs::{self, Interval};

/// What the verbs of a script have written so far, beside the segments `Motion` adds to its
/// tracks. Kept between steps, so that a step need not allocate it.
#[derive(Default)]
pub(crate) struct Written {
    /// The segments of the step being played, by slot, until `Motion` adds them to its tracks.
    pub(super) segments: Vec<(usize, Segment)>,
    /// The same for abilities, by number.
    pub(super) ability_segments: Vec<(u32, Segment)>,
    pub spans: Vec<SpanChange>,
    pub streaks: Vec<StreakRun>,
    pub notices: Vec<NoticeRun>,
    pub counts: Vec<CountChange>,
    pub turns: Vec<Turn>,
    pub pile_counts: Vec<PileCount>,
}

/// What a verb writes for one step.
pub struct StepOut<'a> {
    pub(super) number: usize,
    pub(super) own: Interval,
    pub(super) subject: Option<Subject>,
    pub(super) from: Option<Pose>,
    pub(super) to: Option<Pose>,
    pub(super) written: &'a mut Written,
}

impl StepOut<'_> {
    /// Gives `subject` `segment`, made by this step's verb. A player has no scene object to
    /// move: a segment for one is dropped.
    pub fn segment(&mut self, subject: Subject, segment: Segment) {
        match subject {
            Subject::Object { slot } => self.written.segments.push((usize::from(slot.0), segment)),
            Subject::Ability { id } => self.written.ability_segments.push((id, segment)),
            Subject::Player { .. } => {}
        }
    }

    /// The subject is at `to` from `own.from` on: an instant change. One that changes nothing
    /// needs no segment.
    pub fn instant(&mut self) {
        if let (Some(subject), Some(from), Some(to)) = (self.subject, self.from, self.to)
            && from != to
        {
            self.segment(subject, verbs::instant(from, to, self.own.from));
        }
    }

    /// A span begins or ends.
    pub fn span(&mut self, change: SpanChange) {
        self.written.spans.push(SpanChange {
            step: self.number,
            ..change
        });
    }

    /// Mana or damage streams from one thing to another.
    pub fn streak(&mut self, run: StreakRun) {
        self.written.streaks.push(StreakRun {
            step: self.number,
            ..run
        });
    }

    /// The card `subject` shows the record the script leads to from `at` on, and the one the
    /// table showed before the script until then (`Turn`). Anything but a card is left as it is.
    pub fn turn(&mut self, subject: Subject, at: u64) {
        if let Subject::Object { slot } = subject {
            self.written.turns.push(Turn {
                slot: usize::from(slot.0),
                at,
                step: self.number,
            });
        }
    }

    /// A banner or pill is shown.
    pub fn notice(&mut self, run: NoticeRun) {
        self.written.notices.push(NoticeRun {
            step: self.number,
            ..run
        });
    }
}
