//! `emerge`: an ability lifts out of its source and goes on the stack. It starts as a small
//! copy on its source, `verbs.emerge.start_scale` of the source's size and offset from its
//! middle by `verbs.emerge.start_offset` of its width and height (`data/beryl3d/space.json`,
//! from the web interface's `emerging` in `ui/src/stage/desktop.ts`), and travels to its place
//! on the stack over the step along the step's curve, growing as it goes. The source does not
//! move. An ability whose source is not on the table fades in at its place.

use beryl_protocol::protocol::StackItem;
use beryl_protocol::script::{Act, Subject};
use glam::Vec3;

use super::{StepIn, StepOut};
use kit_base::data::{space, verbs::Emerge};
use kit_base::pose::{Pose, Segment};

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let (Some(subject), Some(to)) = (step.subject, step.to) else {
        return;
    };
    let source = match step.act {
        Act::Emerge {
            item: StackItem::Ability {
                source: Some(slot), ..
            },
            ..
        } => step
            .places
            .pose(Subject::Object { slot: *slot }, step.own.from),
        _ => None,
    };
    let from = match source.filter(|source| source.opacity > 0.0) {
        Some(source) => emerging(source, to, &step.verbs.emerge),
        None => Pose { opacity: 0.0, ..to },
    };
    out.segment(subject, Segment::between(from, to, step.own, step.curve));
}

/// Where an ability that rests at `rest` starts, as a small copy lying on its source at
/// `source`: turned as it will rest, and one card's thickness above the source, so that the
/// source does not hide it.
fn emerging(source: Pose, rest: Pose, tuning: &Emerge) -> Pose {
    let size = source.scale * Vec3::new(space::card::WIDTH, space::card::HEIGHT, 0.0);
    let offset = tuning.start_offset.extend(0.0) * size;
    Pose {
        position: source.position + source.rotation * offset + Vec3::Z * space::card::LAYER,
        scale: source.scale * tuning.start_scale,
        ..rest
    }
}
