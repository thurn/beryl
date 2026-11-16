//! The manual table's pile viewers (docs/manual-multiplayer.md, "Viewers"): the view commands
//! of a graveyard and an exile pile open its viewer, the card browser of `inspect::viewer`, as
//! the web's `TableViewers` (`ui/src/table/viewers.ts`) opens a `TablePileViewer`. Each card in
//! the viewer is its scene object shown in a box (`Shown::Object`), and a right click on one
//! opens that card's context menu, with the commands of a card in a pile, as the web's viewer
//! cards carry the table's commands.

use beryl_protocol::protocol::{ObjectId, ZoneName};
use glam::Vec2;

use super::Target;
use crate::element::Viewed;
use crate::frame::{Frame, Role};

/// The viewer the client command `client` opens for `target`, if it is a view command and the
/// target is a pile of the kind it views. A library is looked at through its own commands,
/// which the table answers, and a sideboard has no pile on the table to view.
#[must_use]
pub fn opens(client: &str, target: Target) -> Option<Viewed> {
    let zone = match client {
        "view_graveyard" => ZoneName::Graveyard,
        "view_exile" => ZoneName::Exile,
        _ => return None,
    };
    match target {
        Target::Pile(side, pile) if pile == zone => Some(Viewed::Pile(side, zone)),
        _ => None,
    }
}

/// Whether a press on `zone`'s pile on the table opens its viewer: a graveyard and an exile
/// pile, whose cards everyone may see. A library's cards are hidden, and the table shows them
/// only through its commands.
#[must_use]
pub fn opens_on_press(zone: ZoneName) -> bool {
    matches!(zone, ZoneName::Graveyard | ZoneName::Exile)
}

/// The card of the open viewer at `point` in `frame`, as a target, if the pointer is on one:
/// a card element whose slot `shown` says the viewer holds, the topmost first. A card in the
/// viewer lies in its pile as far as the view goes, so `target_at` would name the pile.
#[must_use]
pub fn card_at(frame: &Frame, point: Vec2, shown: impl Fn(ObjectId) -> bool) -> Option<Target> {
    (frame.resolved.iter().rev())
        .filter(|element| element.role == Role::Card && inside(&element.corners, point))
        .find_map(|element| {
            let slot = ObjectId(u16::try_from(element.id.0).ok()?);
            shown(slot).then_some(Target::Card(slot))
        })
}

/// Whether `point` lies inside the quadrilateral `corners`, given in order round its edge.
#[must_use]
pub fn inside(corners: &[Vec2; 4], point: Vec2) -> bool {
    // The point is inside when it lies on the same side of every edge, or on an edge.
    let sides = (0..4).map(|index| {
        let (a, b) = (corners[index], corners[(index + 1) % 4]);
        (b - a).perp_dot(point - a)
    });
    let (mut left, mut right) = (false, false);
    for cross in sides {
        left |= cross > 0.0;
        right |= cross < 0.0;
    }
    !(left && right)
}
