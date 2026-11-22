//! Checks that visible, named game cards have their own faces.

use kit_frame::frame::{Frame, Role, StateBits};
use kit_probe::agent::{Context, Fault, Rule, Subject, tree::locator_of};
use kit_scene::hit::bounds;

/// The cards whose face the viewer can see and whose name the viewer knows.
pub(super) fn face_up(frame: &Frame) -> impl Iterator<Item = u32> + '_ {
    (0..frame.resolved.len() as u32).filter(|&index| {
        let element = &frame.resolved[index as usize];
        element.role == Role::Card
            && element.opacity > 0.0
            && !element.state.has(StateBits::FACE_DOWN)
            && !frame.name(element.name).is_empty()
    })
}

/// Checks for cards drawn face up with no face of their own.
#[must_use]
pub fn blank_cards(frame: &Frame, context: &Context) -> Vec<Fault> {
    face_up(frame)
        .filter(|&index| frame.resolved[index as usize].face.is_none())
        .map(|index| {
            let subject = Subject {
                locator: locator_of(frame, index),
                rect: bounds(&frame.resolved[index as usize]),
            };
            let message = format!(
                "{} is drawn face up with no face of its own",
                subject.describe()
            );
            Fault {
                rule: Rule::BlankCard,
                elements: vec![subject],
                at: context.at,
                message,
            }
        })
        .collect()
}
