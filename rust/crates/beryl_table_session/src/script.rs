//! Turns the host's ordered, per-seat animation projection into stage steps.

mod member;
mod proposals;
mod spans;

use beryl_session::protocol::{ObjectId, PlayerView};
use beryl_session::script::{
    Act, Notice, Open, Reset, Script, Span, Step, Subject, Table as Stage, replay,
};
use beryl_table::redact::{AnimationEvent, ResetReason, visit_animation};
use beryl_table::{Entry, Refusal, Seat, Table};

use crate::view::{Slots, Viewer};

pub(super) use proposals::proposal_marks;

/// The stage slots affected by the viewer's checked proposal lines in each view.
#[derive(Clone, Copy, Default)]
pub struct ProposalMarks<'a> {
    pub before: &'a [ObjectId],
    pub after: &'a [ObjectId],
}

/// Builds the script when the viewer has pending proposals. A panel-only update
/// such as toggling a line has no entry but can still begin or end marks.
pub(super) fn script_with_proposal_marks(
    before: &PlayerView,
    entry: Option<(&Entry, &Table)>,
    after: &PlayerView,
    seat: Seat,
    slots_before: &Slots,
    slots_after: &Slots,
    marks: ProposalMarks<'_>,
) -> Result<Script, Refusal> {
    let viewer = Viewer(seat);
    let start = Stage::of(before);
    let end = Stage::of(after);
    let open = spans::of_view(before, marks.before);
    let wanted = spans::of_view(after, marks.after);
    let mut builder = Builder {
        stage: start.clone(),
        spans: open.clone(),
        beats: Vec::new(),
        cause: entry.map(|(entry, _)| Subject::Player {
            player: viewer.side(entry.by),
        }),
        error: None,
    };
    let mut members = Vec::new();
    if let Some((entry, table_before)) = entry {
        visit_animation(entry, table_before, seat, |events, projected| {
            members.push((events.to_vec(), projected));
        });
    }
    if let Some(reason) = last_reset(&members) {
        let notice = match reason {
            ResetReason::Rewind { to } => Some(Notice::Rewind { to }),
            ResetReason::Game { number } => Some(Notice::Game { number }),
            ResetReason::Lobby => None,
        };
        return Ok(reset(&end, &wanted, notice, builder.cause));
    }
    builder.end_withdrawn_marks(&wanted);
    let mut reserved = slots_after.clone();
    for (events, _) in members.iter().rev() {
        reserved.reserve_owner_transfers(events);
    }
    let mut slots = slots_before.clone();
    for (events, projected) in &members {
        builder.member(events, projected, viewer, &mut slots, &reserved)?;
    }
    builder.end_spans(&wanted, &[], false);
    builder.begin_spans(&wanted);

    let script = Script {
        reset: None,
        beats: builder.beats,
        timing: Vec::new(),
    }
    .timed(&start);
    let failure = builder
        .error
        .or_else(|| replay::failure(&start, &script, after));
    #[cfg(debug_assertions)]
    let failure = failure.or_else(|| {
        beryl_session::script::carried(start, &open, &script)
            .and_then(|carried| {
                if carried.len() == wanted.len() && wanted.iter().all(|span| carried.contains(span))
                {
                    Ok(())
                } else {
                    Err("script carried different spans than the view".into())
                }
            })
            .err()
    });
    if let Some(failure) = failure {
        debug_assert!(
            false,
            "manual table entry {:?} does not replay: {failure}",
            entry.map(|(entry, _)| entry.n)
        );
        return Ok(reset(&end, &wanted, None, None));
    }
    Ok(script)
}

// A reset takes the final view whole, even if a batch contains other members.
fn last_reset(members: &[(Vec<AnimationEvent>, Table)]) -> Option<ResetReason> {
    members
        .iter()
        .rev()
        .flat_map(|(events, _)| events)
        .find_map(|event| match event {
            AnimationEvent::Reset(reason) => Some(*reason),
            _ => None,
        })
}

fn reset(end: &Stage, spans: &[Open], notice: Option<Notice>, cause: Option<Subject>) -> Script {
    let mut beats = Vec::new();
    if !spans.is_empty() {
        beats.push(
            spans
                .iter()
                .map(|span| Step {
                    act: span.act(Span::Begin),
                    cause: None,
                })
                .collect(),
        );
    }
    if let Some(notice) = notice {
        beats.push(vec![Step {
            act: Act::Announce { notice },
            cause,
        }]);
    }
    Script {
        reset: Some(Reset {
            abilities: end.stack.iter().filter_map(|item| item.id).collect(),
        }),
        beats,
        timing: Vec::new(),
    }
    .timed(end)
}

struct Builder {
    stage: Stage,
    spans: Vec<Open>,
    beats: Vec<Vec<Step>>,
    cause: Option<Subject>,
    error: Option<String>,
}

impl Builder {
    fn emit(&mut self, beat: &mut Vec<Step>, act: Act) {
        if self.error.is_none() {
            self.error = self.stage.apply(&act).err();
        }
        beat.push(Step {
            act,
            cause: self.cause,
        });
    }

    fn finish(&mut self, beat: Vec<Step>) {
        if !beat.is_empty() {
            self.beats.push(beat);
        }
    }
}
