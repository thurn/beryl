//! Game-specific target lookup for frame checks.

use super::hit::bounds;
use super::{Context, Fault, checks};
use kit_frame::frame::{Frame, Place, Ringed};
use kit_ui::input::Rect;

/// Runs the frame checks with the game's mark targets.
#[must_use]
pub fn check(frame: &Frame, context: &Context) -> Vec<Fault> {
    checks::check(frame, context, |on| ring_target(frame, on))
}

/// Checks the marks in a game frame against the cards, abilities and life totals they ring.
#[must_use]
pub fn marks_on(frame: &Frame, context: &Context) -> Vec<Fault> {
    checks::marks_on(frame, context, |on| ring_target(frame, on))
}

/// The index and box of the ring's target in the frame. A player's mark rings the core of
/// their life orb in the flat layer.
fn ring_target(frame: &Frame, on: Ringed) -> Option<(u32, Rect)> {
    let found = match on {
        Ringed::Element(id) => frame
            .resolved
            .iter()
            .position(|element| element.id == id && element.place != Place::Layer),
        Ringed::Player(side) => {
            let name = crate::hud::players::orb_name(side);
            frame.resolved.iter().position(|element| {
                element.place == Place::Layer && frame.name(element.name) == name
            })
        }
    }?;
    let box_on_screen = bounds(&frame.resolved[found]);
    let target = match on {
        Ringed::Element(_) => box_on_screen,
        Ringed::Player(side) => crate::hud::players::core_box(side, box_on_screen),
    };
    Some((u32::try_from(found).ok()?, target))
}
