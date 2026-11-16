//! The manual table's arrows (docs/manual-multiplayer.md, "Arrows"), as the web's `TableArrows`
//! (`ui/src/table/arrows.ts`) draws them: a command that acts toward something (Draw arrow,
//! Attach) waits for a click on the object or player it acts toward, with an arrow from its
//! card to the pointer meanwhile, and then sends its action with that target filled in. A click
//! anywhere else, or Escape, gives up. The arrows the view holds (`TableView::arrows`) reach the
//! client as the `point` spans of the table's scripts, which `Lit` draws (`light::arrows`).

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ObjectId, Side, TableAction};
use beryl_protocol::script::{Arrow, Subject};
use glam::Vec2;
use serde_json::Value;

use super::Target;
use super::commands::{self, COMMANDS, Values, seat_of};
use crate::light::Aim;

/// A command waiting for its target: the command, at its index in `COMMANDS`, and the card it
/// was given to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    pub command: u16,
    pub from: ObjectId,
}

/// What a click while a command waits for its target is on: a card other than the command's
/// own, a player's life total, or neither.
#[must_use]
pub fn picked(pick: Pick, clicked: Target) -> Option<Target> {
    match clicked {
        Target::Card(slot) if slot != pick.from => Some(clicked),
        Target::Life(_) => Some(clicked),
        _ => None,
    }
}

/// The arrow from the waiting command's card to the pointer at `to`, as the web's `#trace`
/// draws it in the hostile target's color.
#[must_use]
pub fn aim(pick: Pick, to: Vec2) -> Aim {
    Aim {
        arrow: Arrow::HostileTarget,
        from: Subject::Object { slot: pick.from },
        to,
    }
}

/// The actions the waiting command sends toward `to` at `view`, for its card or, if its card is
/// selected, for every selected card: an arrow names its target as an object or a player
/// (`destination` in arrows.ts), and an attachment names its host as an object. Empty when the
/// target has no reference in the view, or is a player the command cannot act toward.
#[must_use]
pub fn actions(
    pick: Pick,
    to: Target,
    view: &PlayerView,
    selected: &[ObjectId],
) -> Vec<TableAction> {
    let Some(command) = COMMANDS.get(usize::from(pick.command)) else {
        return Vec::new();
    };
    let Some(object) = (match to {
        Target::Card(slot) => seen(view, slot),
        _ => None,
    }) else {
        let Target::Life(side) = to else {
            return Vec::new();
        };
        if command.id != "card.arrow" || !seated(view, side) {
            return Vec::new();
        }
        let player = serde_json::json!({ "player": seat_of(view, side) });
        return with_target(command, pick, view, selected, player);
    };
    let target = if command.id == "card.arrow" {
        serde_json::json!({ "object": object })
    } else {
        object
    };
    // A card never acts toward itself, as the web's `draw` leaves out the target.
    let selected: Vec<ObjectId> = (selected.iter().copied())
        .filter(|&slot| Target::Card(slot) != to)
        .collect();
    with_target(command, pick, view, &selected, target)
}

/// The command's actions for its card, or the selection, with `$target` as `target`.
fn with_target(
    command: &commands::Command,
    pick: Pick,
    view: &PlayerView,
    selected: &[ObjectId],
    target: Value,
) -> Vec<TableAction> {
    let mut values = Values::new();
    values.insert("target", target);
    commands::actions_for(command, Target::Card(pick.from), selected, view, &values)
}

/// The card in `slot` as an action names an object it has seen: its reference and the zone it
/// lies in.
fn seen(view: &PlayerView, slot: ObjectId) -> Option<Value> {
    let table = view.table.as_ref()?;
    let action =
        (table.object_actions.iter()).find(|action| action.subject == Subject::Object { slot })?;
    Some(serde_json::json!({ "id": action.reference, "in": action.zone }))
}

/// Whether `side` has a seat at `view`'s table: the viewer always does, and the opponent at a
/// table of two.
fn seated(view: &PlayerView, side: Side) -> bool {
    side == Side::You || view.table.as_ref().is_some_and(|table| table.seats >= 2)
}
