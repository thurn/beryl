use beryl_session::protocol::{CardHandling, ObjectId, PlayerView, TableArrow};
use beryl_session::script::{Act, Fact, Mark, Open, Span, Step, Subject};

use super::Builder;

impl Builder {
    pub(super) fn end_withdrawn_marks(&mut self, wanted: &[Open]) {
        let mut ends = Vec::new();
        for span in std::mem::take(&mut self.spans) {
            if matches!(span, Open::Mark { .. }) && !wanted.contains(&span) {
                self.emit(&mut ends, span.act(Span::End));
            } else {
                self.spans.push(span);
            }
        }
        self.finish(ends);
    }

    /// A span ends in its own beat before a card it touches moves. A
    /// same-zone move leaves the arrow on the table, so begin it again once
    /// the card has settled.
    pub(super) fn end_spans(&mut self, wanted: &[Open], beat: &[Step], break_lift: bool) {
        let mut ends = Vec::new();
        for span in std::mem::take(&mut self.spans) {
            let ends_now = !wanted.contains(&span)
                || match span {
                    Open::Lift { .. } => break_lift,
                    Open::Point { from, to, .. } => beat
                        .iter()
                        .any(|step| moved(&step.act, from) || moved(&step.act, to)),
                    Open::Mark { on, .. } => beat.iter().any(|step| moved(&step.act, on)),
                };
            if ends_now {
                self.emit(&mut ends, span.act(Span::End));
            } else {
                self.spans.push(span);
            }
        }
        self.finish(ends);
    }

    pub(super) fn begin_spans(&mut self, wanted: &[Open]) {
        let mut begins = Vec::new();
        for &span in wanted {
            if !self.spans.contains(&span) {
                self.emit(&mut begins, span.act(Span::Begin));
                self.spans.push(span);
            }
        }
        self.finish(begins);
    }
}

fn moved(act: &Act, subject: Subject) -> bool {
    let Subject::Object { slot } = subject else {
        return false;
    };
    match act {
        Act::Fly { object, .. } | Act::Appear { object, .. } => object.slot == slot,
        Act::Vanish {
            subject: leaving, ..
        } => *leaving == subject,
        Act::Set {
            fact: Fact::Forget { slots },
        } => slots.contains(&slot),
        _ => false,
    }
}

pub(super) fn wanted(handling: Option<CardHandling>, arrows: &[TableArrow]) -> Vec<Open> {
    let mut open = Vec::new();
    for &TableArrow {
        arrow, from, to, ..
    } in arrows
    {
        let point = Open::Point { arrow, from, to };
        if !open.contains(&point) {
            open.push(point);
        }
    }
    open.extend(handling.map(|handling| Open::Lift { handling }));
    open
}

pub(super) fn of_view(view: &PlayerView, marks: &[ObjectId]) -> Vec<Open> {
    let mut open = wanted(
        view.handling,
        view.table.as_ref().map_or(&[], |table| &table.arrows),
    );
    for &slot in marks {
        let mark = Open::Mark {
            mark: Mark::Suggested,
            on: Subject::Object { slot },
        };
        if !open.contains(&mark) {
            open.push(mark);
        }
    }
    open
}
