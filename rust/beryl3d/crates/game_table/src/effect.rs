//! Identifies game subjects in the toolkit's timed effects and comparison records.

use beryl_protocol::protocol::{ObjectId, Side};
use beryl_protocol::script::Subject;
use kit_frame::frame::{EffectTarget, ElementId};

/// The same target key is used when compiling an effect and when painting its subject.
#[must_use]
pub const fn target(subject: Subject) -> EffectTarget {
    match subject {
        Subject::Object { slot } => EffectTarget::Key(ElementId(slot.0 as u32)),
        Subject::Ability { id } => EffectTarget::Numbered {
            prefix: "a",
            number: id,
        },
        Subject::Player { player: Side::You } => EffectTarget::Label("you"),
        Subject::Player {
            player: Side::Opponent,
        } => EffectTarget::Label("opponent"),
    }
}

/// The subject whose pose a timed effect follows.
#[must_use]
pub fn subject(target: EffectTarget) -> Option<Subject> {
    match target {
        EffectTarget::Key(ElementId(slot)) => Some(Subject::Object {
            slot: ObjectId(u16::try_from(slot).ok()?),
        }),
        EffectTarget::Numbered {
            prefix: "a",
            number,
        } => Some(Subject::Ability { id: number }),
        EffectTarget::Label("you") => Some(Subject::Player { player: Side::You }),
        EffectTarget::Label("opponent") => Some(Subject::Player {
            player: Side::Opponent,
        }),
        _ => None,
    }
}
