//! The manual table (docs/manual-multiplayer.md): a table where the players move cards by
//! hand, with no engine. Its commands (`commands`), the context menu a right click opens on a
//! card, a pile, a hand, a life total or the table (`menus`), and what the pointer is on
//! (`target_at`), which a menu or a key acts on.

pub mod commands;
pub mod menus;

use beryl_protocol::PlayerView;
use beryl_protocol::ipc::TableSoloRequest;
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use glam::Vec2;

use crate::agent::hit;
use crate::frame::{Frame, Place, Role};
use commands::AppliesTo;

/// What a command acts on: a card, a pile (a library, graveyard or exile), a hand, a life
/// total, or the table itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Card(ObjectId),
    Pile(Side, ZoneName),
    Hand(Side),
    Life(Side),
    Table,
}

impl Target {
    /// The kind of thing the target is, which says which commands apply to it: a card on the
    /// battlefield is a permanent of the viewer's or of their opponent's by who controls it, a
    /// card in the viewer's hand a hand card, one on the stack a stack entry, and one anywhere
    /// else a pile card.
    #[must_use]
    pub fn applies_to(self, view: &PlayerView) -> AppliesTo {
        match self {
            Self::Card(slot) => {
                let object = view.objects.iter().find(|object| object.slot == slot);
                match object.map(|object| (object.zone, object.controller)) {
                    Some((ZoneName::Battlefield, Side::You)) => AppliesTo::Permanent,
                    Some((ZoneName::Battlefield, Side::Opponent)) => AppliesTo::TheirPermanent,
                    Some((ZoneName::Hand, _)) => AppliesTo::HandCard,
                    Some((ZoneName::Stack, _)) => AppliesTo::StackEntry,
                    _ => AppliesTo::PileCard,
                }
            }
            Self::Pile(_, ZoneName::Graveyard) => AppliesTo::Graveyard,
            Self::Pile(_, ZoneName::Exile) => AppliesTo::Exile,
            Self::Pile(..) => AppliesTo::Library,
            Self::Hand(_) => AppliesTo::Hand,
            Self::Life(_) => AppliesTo::Life,
            Self::Table => AppliesTo::Table,
        }
    }

    /// Whose pile, hand or life total the target is; `None` for a card or the table, whose
    /// commands act for the viewer.
    #[must_use]
    pub fn side(self) -> Option<Side> {
        match self {
            Self::Pile(side, _) | Self::Hand(side) | Self::Life(side) => Some(side),
            Self::Card(_) | Self::Table => None,
        }
    }
}

/// What the pointer at `point` is on in `frame`, as the web's `pointedTarget`
/// (`ui/src/table/keyboard.ts`) finds it: a card of the battlefield, the stack or the viewer's
/// hand; the pile a card of a library, graveyard or exile lies in, or the pile itself; the
/// opponent's hand, whose cards are hidden; a player's life total; or else the table.
#[must_use]
pub fn target_at(frame: &Frame, point: Vec2) -> Target {
    let Some(mut index) = hit(frame, point) else {
        return Target::Table;
    };
    loop {
        let element = &frame.resolved[index as usize];
        match (element.role, element.place) {
            (Role::Card | Role::Pile, Place::Card { zone, side, .. }) => {
                return match zone {
                    ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile => {
                        Target::Pile(side, zone)
                    }
                    ZoneName::Hand if side == Side::Opponent => Target::Hand(side),
                    _ if element.role == Role::Card => u16::try_from(element.id.0)
                        .map_or(Target::Table, |slot| Target::Card(ObjectId(slot))),
                    _ => Target::Table,
                };
            }
            // A life total is named for whose it is and the number, as "You 20".
            (Role::Player, _) => {
                let name = frame.name(element.name);
                return if name.starts_with("You") {
                    Target::Life(Side::You)
                } else {
                    Target::Life(Side::Opponent)
                };
            }
            _ => {}
        }
        if element.parent == u32::MAX {
            return Target::Table;
        }
        index = element.parent;
    }
}

/// The request that starts a solo table with the sample WU Control deck, as the web's solo
/// table first did. (The web now prepares a solo table in a lobby, which Beryl 3D has no
/// screen for yet.)
///
/// # Panics
///
/// Never: the build script writes the request from the sample deck.
#[must_use]
pub fn solo_request() -> TableSoloRequest {
    serde_json::from_str(commands::SOLO_REQUEST).expect("the sample deck is a solo request")
}
