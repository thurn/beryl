use beryl_session::protocol::{CardHandling, PlayerView, TableArrow};
use beryl_session::script::{Act, Open, Span, Step, Subject};

use super::Builder;

impl Builder {
    /// A span ends in its own beat before a card it touches moves. A
    /// same-zone move leaves the arrow on the table, so begin it again once
    /// the card has settled.
    pub(super) fn end_spans(&mut self, wanted: &[Open], beat: &[Step], break_lift: bool) {
        let mut ends = Vec::new();
        for span in std::mem::take(&mut self.spans) {
            let ends_now = !wanted.contains(&span)
                || match span {
                    Open::Point { from, to, .. } => beat
                        .iter()
                        .any(|step| moved(&step.act, from) || moved(&step.act, to)),
                    Open::Lift { .. } => break_lift,
                    Open::Mark { .. } => false,
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
        _ => false,
    }
}

pub(super) fn wanted(handling: Option<CardHandling>, arrows: &[TableArrow]) -> Vec<Open> {
    let mut open = Vec::new();
    for &TableArrow { arrow, from, to } in arrows {
        let point = Open::Point { arrow, from, to };
        if !open.contains(&point) {
            open.push(point);
        }
    }
    open.extend(handling.map(|handling| Open::Lift { handling }));
    open
}

pub(super) fn of_view(view: &PlayerView) -> Vec<Open> {
    wanted(
        view.handling,
        view.table.as_ref().map_or(&[], |table| &table.arrows),
    )
}
