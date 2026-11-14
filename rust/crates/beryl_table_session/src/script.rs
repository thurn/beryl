//! Turns the host's ordered, per-seat animation projection into stage steps.

mod member;

use beryl_session::protocol::PlayerView;
use beryl_session::script::{Act, Reset, Script, Step, Subject, Table as Stage, replay};
use beryl_table::redact::visit_animation;
use beryl_table::{Entry, Seat, Table};

use crate::view::{Slots, Viewer};

/// All inputs belong to the same seat and entry. Take a copy of the previous
/// slots before publishing `after` with `view`, which advances those slots.
#[must_use]
pub fn script(
    before: &PlayerView,
    entry: &Entry,
    table_before: &Table,
    after: &PlayerView,
    seat: Seat,
    slots_before: &Slots,
    slots_after: &Slots,
) -> Script {
    let viewer = Viewer(seat);
    let start = Stage::of(before);
    let end = Stage::of(after);
    let mut builder = Builder {
        stage: start.clone(),
        beats: Vec::new(),
        cause: Some(Subject::Player {
            player: viewer.side(entry.by),
        }),
        error: None,
    };
    let mut members = Vec::new();
    visit_animation(entry, table_before, seat, |events, projected| {
        members.push((events.to_vec(), projected));
    });
    let mut reserved = slots_after.clone();
    for (events, _) in members.iter().rev() {
        reserved.reserve_owner_transfers(events);
    }
    let mut slots = slots_before.clone();
    let reset = members.iter().any(|(events, projected)| {
        builder.member(entry, events, projected, viewer, &mut slots, &reserved)
    });

    let script = Script {
        reset: None,
        beats: builder.beats,
    };
    if !reset {
        let failure = builder
            .error
            .or_else(|| replay::failure(&start, &script, after));
        if let Some(failure) = failure {
            debug_assert!(
                false,
                "manual table entry {} does not replay: {failure}",
                entry.n
            );
        } else {
            return script;
        }
    }
    Script {
        reset: Some(Reset {
            abilities: end.stack.iter().filter_map(|item| item.id).collect(),
        }),
        beats: Vec::new(),
    }
}

struct Builder {
    stage: Stage,
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
