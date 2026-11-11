//! The rule invariants (plan K24): what a script's arrows, lines, marks and lifts must hold
//! to whatever cards it shows. Replay cannot check them, since a span changes no table; the
//! session runs these beside the replay check in a debug build, wherever it builds a script.
//!
//! - No arrow, line or mark is on a thing that is moving or has left: where it begins, both
//!   its ends are on the table, and neither moves in that beat. The one exception is a spell
//!   or ability that goes on the stack in the beat, which its own arrows may leave from: a
//!   line joins an ability to its source from the frame it lifts.
//! - An arrow is ended before its target moves: in a beat before the one that moves either
//!   of its ends, not in that beat and not after.
//! - One visual per thing: nothing moves twice in a beat, nothing comes to be that is on the
//!   table already, and no span begins that is begun already. `showcase` is the verb that
//!   says otherwise, and so is no move here: a card held up to be read lies where it was too.
//! - Every beginning has an end, in this script or carried: a step ends only what was begun
//!   and is still open, and what a script leaves open is handed to the seat's next one
//!   ([`carried`] returns it), which a reset drops.
//!
//! A thing *moves* in a step that takes it from its place: a `fly`, an `appear`, a `vanish`,
//! and the `emerge` that puts an ability on the stack. Turning, tapping, striking and
//! counting leave it where it is.

use std::fmt::{self, Display, Formatter};

use super::replay::number;
use super::{Act, Arrow, Mark, Script, Span, Subject, Table};
use crate::protocol::{CardHandling, ZoneName};

/// An arrow, a mark or a lift that has begun and not ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Open {
    Point {
        arrow: Arrow,
        from: Subject,
        to: Subject,
    },
    Mark {
        mark: Mark,
        on: Subject,
    },
    Lift {
        handling: CardHandling,
    },
}

impl Open {
    /// The span a step begins or ends, and which.
    pub fn of(act: &Act) -> Option<(Span, Open)> {
        match *act {
            Act::Point {
                span,
                arrow,
                from,
                to,
            } => Some((span, Open::Point { arrow, from, to })),
            Act::Mark { span, mark, on } => Some((span, Open::Mark { mark, on })),
            Act::Lift { span, handling } => Some((span, Open::Lift { handling })),
            _ => None,
        }
    }

    /// The step that begins or ends it.
    pub fn act(self, span: Span) -> Act {
        match self {
            Open::Point { arrow, from, to } => Act::Point {
                span,
                arrow,
                from,
                to,
            },
            Open::Mark { mark, on } => Act::Mark { span, mark, on },
            Open::Lift { handling } => Act::Lift { span, handling },
        }
    }

    /// Where it starts, and what it points at or is on. A lift names cards by their pile and
    /// count, not one by one, and has neither.
    pub(super) fn ends(self) -> (Option<Subject>, Option<Subject>) {
        match self {
            Open::Point { from, to, .. } => (Some(from), Some(to)),
            Open::Mark { on, .. } => (None, Some(on)),
            Open::Lift { .. } => (None, None),
        }
    }

    pub(super) fn touches(self, subject: Subject) -> bool {
        let (from, to) = self.ends();
        from == Some(subject) || to == Some(subject)
    }
}

impl Display for Open {
    /// As the step that begins it reads.
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.act(Span::Begin))
    }
}

/// What `act` takes from its place, if anything.
pub(super) fn moves(act: &Act) -> Option<Subject> {
    match act {
        Act::Fly { object, .. } | Act::Appear { object, .. } => {
            Some(Subject::Object { slot: object.slot })
        }
        Act::Vanish { subject, .. } => Some(*subject),
        Act::Emerge { id, .. } => Some(Subject::Ability { id: *id }),
        _ => None,
    }
}

/// Whether `act` puts its subject on the stack of `table`, where it is not yet.
pub(super) fn lands(table: &Table, act: &Act) -> bool {
    match act {
        Act::Emerge { .. } => true,
        Act::Fly { object, .. } | Act::Appear { object, .. } => {
            object.zone == ZoneName::Stack
                && table
                    .objects
                    .get(&object.slot)
                    .is_none_or(|o| o.zone != ZoneName::Stack)
        }
        _ => false,
    }
}

fn present(table: &Table, subject: Subject) -> bool {
    match subject {
        Subject::Object { slot } => table.objects.contains_key(&slot),
        Subject::Ability { id } => table.stack.iter().any(|e| e.id == Some(id)),
        Subject::Player { .. } => true,
    }
}

/// What one beat has done so far.
#[derive(Default)]
struct Beat {
    moved: Vec<Subject>,
    /// Of `moved`, what went on the stack.
    landed: Vec<Subject>,
    ended: Vec<Open>,
}

/// The spans `script` leaves open, for the seat's next script to end, as a screen that shows
/// `table` with the spans `open` plays it: `table` is the table of the view before, or with a
/// reset, the table of the view the script came with. An error names the first step that
/// breaks a rule invariant, and the rule.
pub fn carried(mut table: Table, open: &[Open], script: &Script) -> Result<Vec<Open>, String> {
    let mut open = open.to_vec();
    if let Some(reset) = &script.reset {
        // A reset drops every span with the table.
        open.clear();
        number(&mut table, reset)?;
    }
    for steps in &script.beats {
        let mut beat = Beat::default();
        for step in steps {
            rule(&mut table, &mut open, &mut beat, &step.act)
                .map_err(|e| format!("`{step}`: {e}"))?;
        }
    }
    Ok(open)
}

/// Holds one step to the rules and plays it.
fn rule(table: &mut Table, open: &mut Vec<Open>, beat: &mut Beat, act: &Act) -> Result<(), String> {
    match Open::of(act) {
        Some((Span::Begin, span)) => {
            if open.contains(&span) {
                return Err("it is begun already: one visual per thing".to_owned());
            }
            let (from, to) = span.ends();
            for end in [from, to].into_iter().flatten() {
                if !present(table, end) {
                    return Err(format!("{end} has left the table"));
                }
            }
            let leaves = from.filter(|from| !beat.landed.contains(from));
            if let Some(end) = [leaves, to]
                .into_iter()
                .flatten()
                .find(|end| beat.moved.contains(end))
            {
                return Err(format!("{end} is moving"));
            }
            open.push(span);
        }
        Some((Span::End, span)) => {
            let at = open
                .iter()
                .position(|&o| o == span)
                .ok_or("it ends what was never begun")?;
            open.remove(at);
            beat.ended.push(span);
        }
        None => {
            if let Some(subject) = moves(act) {
                moving(table, open, beat, act, subject)?;
            }
            let lands = lands(table, act);
            table.apply(act)?;
            if let Some(subject) = moves(act) {
                beat.moved.push(subject);
                if lands {
                    beat.landed.push(subject);
                }
            }
        }
    }
    Ok(())
}

/// `act` is about to move `subject`.
fn moving(
    table: &Table,
    open: &[Open],
    beat: &Beat,
    act: &Act,
    subject: Subject,
) -> Result<(), String> {
    if beat.moved.contains(&subject) {
        return Err(format!(
            "{subject} moves twice in one beat: one visual per thing"
        ));
    }
    if let Some(span) = open.iter().find(|o| o.touches(subject)) {
        return Err(format!("`{span}` is not ended before {subject} moves"));
    }
    if let Some(span) = beat.ended.iter().find(|o| o.touches(subject)) {
        return Err(format!(
            "`{span}` ends in the beat that moves {subject}, not before it"
        ));
    }
    let twice = match act {
        Act::Fly { from, object, .. } | Act::Appear { from, object, .. } => {
            *from == ZoneName::Nowhere && table.objects.contains_key(&object.slot)
        }
        Act::Emerge { .. } => present(table, subject),
        _ => false,
    };
    if twice {
        return Err(format!(
            "{subject} is on the table already: one visual per thing"
        ));
    }
    Ok(())
}
