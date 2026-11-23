//! The timing of the steps a client makes itself while a prompt is open: the marks it draws
//! on cards, the arrows from the spell or ability that asks to what is picked, the lines of a
//! declaration, and the cards it holds up in a tray. They come from the prompt's state, not
//! from a script, so their timing does not depend on the game: it is this short fixed table,
//! which both clients ask for once (the `stage_timings` command, [`own_timings`]). The web
//! stage makes these steps in `ui/src/stage/prompt.ts`.

use serde::{Deserialize, Serialize};

use super::conventions::Conventions;
use super::{Room, StepTiming, effects_of, timing_of};
use crate::protocol::ObjectId;
use crate::script::{Act, Arrow, Mark, Span, Subject, Table, Verb};

/// One shape of step a client makes itself while a prompt is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OwnStep {
    /// A mark of the prompt's comes up on a card or a player.
    MarkBegins { mark: Mark },
    /// A mark goes.
    MarkEnds,
    /// An arrow from the spell or ability that asks to a permanent or player picked as its
    /// target, or a declared attacker's or blocker's line. A prompt's arrow never points at
    /// the stack.
    ArrowBegins { arrow: Arrow },
    /// An arrow or a line goes.
    ArrowEnds,
    /// The cards a prompt holds up come off the table into its tray, or, `sorted`, move
    /// within a tray already up.
    TrayLifts { sorted: bool },
    /// The tray lets its cards go back to where the table has them.
    TrayLetGo,
}

impl OwnStep {
    /// Every shape, each mark and each arrow a prompt draws among them.
    pub fn all() -> Vec<OwnStep> {
        let marks = Mark::ALL.iter().map(|&mark| OwnStep::MarkBegins { mark });
        let arrows = [
            Arrow::FriendlyTarget,
            Arrow::HostileTarget,
            Arrow::Attack,
            Arrow::Block,
        ]
        .map(|arrow| OwnStep::ArrowBegins { arrow });
        marks
            .chain([OwnStep::MarkEnds])
            .chain(arrows)
            .chain([
                OwnStep::ArrowEnds,
                OwnStep::TrayLifts { sorted: false },
                OwnStep::TrayLifts { sorted: true },
                OwnStep::TrayLetGo,
            ])
            .collect()
    }
}

/// How a step of one shape is played.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnTiming {
    pub step: OwnStep,
    /// Its `after` is always empty: the table cannot name the ends of an arrow it does not
    /// know (see `after_ends`).
    pub timing: StepTiming,
    /// It waits until the steps of its beat that move its arrow's ends have ended: the client
    /// fills `after` with the ends of its arrow that are not players.
    pub after_ends: bool,
}

/// The timing of every shape of step a client makes itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnTimings {
    pub entries: Vec<OwnTiming>,
}

/// The timing of each [`OwnStep`]: what [`timing_of`] gives a step of that shape. A tray is no
/// act of a script, so its timing is its case's in the data.
pub fn own_timings() -> OwnTimings {
    // No step of a prompt's reads the table: its marks and arrows are on what it is about.
    let table = Table::empty(0);
    let (from, to) = (object(1), object(2));
    let entries = OwnStep::all()
        .into_iter()
        .map(|step| {
            let act = match step {
                OwnStep::MarkBegins { mark } => Some(Act::Mark {
                    span: Span::Begin,
                    mark,
                    on: to,
                }),
                OwnStep::MarkEnds => Some(Act::Mark {
                    span: Span::End,
                    mark: Mark::Eligible,
                    on: to,
                }),
                OwnStep::ArrowBegins { arrow } => Some(Act::Point {
                    span: Span::Begin,
                    arrow,
                    from,
                    to,
                }),
                OwnStep::ArrowEnds => Some(Act::Point {
                    span: Span::End,
                    arrow: Arrow::HostileTarget,
                    from,
                    to,
                }),
                OwnStep::TrayLifts { .. } | OwnStep::TrayLetGo => None,
            };
            let mut timing = match act {
                Some(act) => StepTiming {
                    effects: effects_of(&act, &table),
                    ..timing_of(&act, &table, None, &[&act])
                },
                None => tray(step),
            };
            let after_ends = !timing.after.is_empty();
            timing.after.clear();
            OwnTiming {
                step,
                timing,
                after_ends,
            }
        })
        .collect();
    OwnTimings { entries }
}

/// A tray's timing: its case's (`sort` for one that only moves cards within a tray already
/// up, `tray` otherwise, its letting go too), with the rest of a hand regrouping as a
/// card's step does.
fn tray(step: OwnStep) -> StepTiming {
    let case = match step {
        OwnStep::TrayLifts { sorted: true } => "sort",
        _ => "tray",
    };
    let data = Conventions::get();
    StepTiming {
        case: Some(case.to_owned()),
        // The cards a tray takes from a hand, or gives back to it, leave room among the rest.
        room: Room::For { ms: data.regroup() },
        ..data.timing(Verb::Lift, Some(case))
    }
}

fn object(slot: u16) -> Subject {
    Subject::Object {
        slot: ObjectId(slot),
    }
}
