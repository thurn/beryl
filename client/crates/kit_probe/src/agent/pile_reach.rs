//! The "reachable" interface check for piles (docs/beryl-3d.md, "Interface checks"). A pile
//! (`Role::Pile`) is not marked interactive, so the card rule of "reachable" never judged it,
//! but a press on it opens its browser and hovering it shows its count. A desktop layout once
//! drew the stack over the whole shared graveyard, so neither worked while a spell waited to
//! resolve, which is when a player most wants to look.

use super::checks::{fault, subject};
use super::fault::{Context, Fault, Rule};
use kit_frame::frame::{Frame, Place, Resolved, Role, StateBits};
use kit_scene::hit::{Overlaps, bounds, covering, has_free_rect_ignoring};
use kit_ui::input::{Rect, Size};

/// How far, as a share of the pile's width and height, a card's box may differ from the pile's
/// and still count as a card at the pile's scale.
const PILE_SCALE_SLACK: f32 = 0.25;

/// Piles are reachable: on a frame where nothing on the table moves, each pile keeps a free
/// area where a press reaches it, as large as the profile's target size or half the pile's
/// width and height, whichever is less, so a pile drawn smaller than a touch target is judged
/// on how much of it is covered, not on its size.
///
/// Only the table's other objects count as covering it, and of those not the cards drawn at
/// the pile's own scale: the pile's top card lies on it, and the cards a draw, a look or a
/// shuffle moves lie on it or beside it until they settle. A pile drawn under the flat layer,
/// such as a scrim, a browser or a panel, is not judged at all: the table takes no presses
/// behind a modal surface, and "overlap" judges the flat layer's own controls.
#[must_use]
pub fn piles_reachable(frame: &Frame, context: &Context) -> Vec<Fault> {
    let still = !frame
        .resolved
        .iter()
        .any(|element| element.state.has(StateBits::MOVING) || element.state.has(StateBits::HELD));
    if !still {
        return Vec::new();
    }
    let side = context.checks.target_points;
    let mut faults = Vec::new();
    // Built on the first pile, so a frame with none builds no index.
    let mut overlaps: Option<Overlaps> = None;
    for (index, pile) in frame.resolved.iter().enumerate() {
        if pile.role != Role::Pile || pile.interactive {
            continue;
        }
        let index = index as u32;
        let overlaps = overlaps.get_or_insert_with(|| Overlaps::new(frame));
        let over = covering(frame, overlaps, index);
        if over
            .iter()
            .any(|&other| !table_object(&frame.resolved[other as usize]))
        {
            continue;
        }
        let pile_box = bounds(pile);
        let at_pile_scale = |other: u32| {
            let other = &frame.resolved[other as usize];
            other.role == Role::Card && same_size(bounds(other), pile_box)
        };
        let min = Size {
            width: side.min(pile_box.width / 2.0),
            height: side.min(pile_box.height / 2.0),
        };
        if has_free_rect_ignoring(frame, overlaps, index, min, at_pile_scale) {
            continue;
        }
        let subject = subject(frame, index);
        let names: Vec<String> = over
            .into_iter()
            .filter(|&other| !at_pile_scale(other))
            .map(|other| self::subject(frame, other).describe())
            .collect();
        faults.push(fault(
            Rule::Reachable,
            context,
            format!(
                "{} has no uncovered area {:.0} by {:.0} points where a press reaches it; under \
                 {}",
                subject.describe(),
                min.width,
                min.height,
                names.join(", ")
            ),
            vec![subject],
        ));
    }
    faults
}

/// Whether `element` is an object on the table, such as a card or a stack item, rather than
/// part of the flat layer drawn over the table.
fn table_object(element: &Resolved) -> bool {
    element.place != Place::Layer && matches!(element.role, Role::Card | Role::Custom(_))
}

/// Whether two boxes are the same size, within `PILE_SCALE_SLACK` of `pile`'s.
fn same_size(other: Rect, pile: Rect) -> bool {
    (other.width - pile.width).abs() <= pile.width * PILE_SCALE_SLACK
        && (other.height - pile.height).abs() <= pile.height * PILE_SCALE_SLACK
}
