//! The synthetic battlefield the 3D client draws to choose a graphics tier.

use crate::protocol::{
    CardType, ObjectId, ObjectView, PlayerState, PlayerView, PowerToughness, Side, ZoneName,
};
use crate::script::{Reset, Script};

/// A reset view with `count` distinct permanents. Each has its own name so the client draws
/// each permanent separately instead of grouping identical ones.
#[must_use]
pub fn performance_probe(count: u16) -> PlayerView {
    let mut view = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
    view.objects = (0..count).map(permanent).collect();
    view.battlefield = view.objects.iter().map(|object| object.slot).collect();
    view.script = Some(Script {
        reset: Some(Reset::default()),
        ..Script::empty()
    });
    view
}

/// The permanent in slot `index + 1`: its own name; a creature or a land by turns, a third of
/// them tapped, and the controllers alternating every two so that each kind is split evenly.
#[must_use]
pub fn permanent(index: u16) -> ObjectView {
    let side = if (index / 2).is_multiple_of(2) {
        Side::You
    } else {
        Side::Opponent
    };
    let mut object = card(index, ZoneName::Battlefield, side);
    object.tapped = index.is_multiple_of(3);
    if index.is_multiple_of(2) {
        object.types = vec![CardType::Creature];
        object.pt = Some(PowerToughness {
            power: 2,
            toughness: 2,
        });
    } else {
        object.types = vec![CardType::Land];
    }
    object
}

/// The card in slot `index + 1`, named `Stress <slot>`, in `zone` and owned and controlled by
/// `side`.
#[must_use]
pub fn card(index: u16, zone: ZoneName, side: Side) -> ObjectView {
    ObjectView::card(
        ObjectId(index + 1),
        format!("Stress {:03}", index + 1),
        zone,
        side,
        side,
    )
}
