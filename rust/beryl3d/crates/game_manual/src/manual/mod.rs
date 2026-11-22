//! The manual table (docs/manual-multiplayer.md): a table where the players move cards by
//! hand, with no engine. Its commands (`commands`), the context menu a right click opens on a
//! card, a pile, a hand, a life total or the table (`menus`), what the pointer is on
//! (`target_at`), which a menu or a key acts on, and what the table shows over itself: the
//! command list (`palette`) with the keys the player bound (`bindings`), the ask prompt
//! (`ask`), the token search and custom token form (`tokens`), the library order tray (`order`)
//! and the log (`log`); what the assists show (`assists`): the proposal panel, state nudges'
//! labels and the sorting tray; the pile viewers (`viewers`), the selection (`selection`), dropping
//! held cards (`drop`) and the arrows a player draws (`arrows`); the guided start's play or
//! draw choice and opening hands (`opening`), and the banner that freezes the table while the
//! other seat's connection is down (`link`).

pub mod arrows;
pub mod ask;
pub mod assists;
pub mod bindings;
pub mod commands;
pub mod drop;
pub mod link;
pub mod lobby;
pub mod log;
pub mod menus;
pub mod opening;
pub mod order;
pub mod palette;
pub(crate) mod pile_viewer;
pub mod pools;
mod profiles;
pub mod quit;
pub mod selection;
pub mod tokens;
pub mod turn;
pub mod viewers;

use beryl_protocol::PlayerView;
use beryl_protocol::ipc::TableSoloRequest;
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use glam::Vec2;

use crate::features::table::Message as TableMessage;
use bindings::Bindings;
use commands::AppliesTo;
use game_table::table::Place;
use kit_frame::frame::{Frame, Role};
use kit_scene::hit::hit;
use tokens::TokenChoice;

/// What the client knows beyond the table's view that decides which commands it offers and
/// what they show: whether it runs on the Mac, which writes keys its own way; the keys the
/// player bound (`bindings`); the last token the player made, which Create another token
/// makes again; and the selected cards, for each of which a command given to one of them acts.
#[derive(Clone, Copy, Debug)]
pub struct Context<'a> {
    pub mac: bool,
    pub bindings: &'a Bindings,
    pub last_token: Option<&'a TokenChoice>,
    pub selected: &'a [ObjectId],
    /// Whether a pile viewer is open, and whether it shows a search of a whole library its
    /// player may shuffle, the conditions `open_viewer` and `whole_library`.
    pub viewer_open: bool,
    pub whole_library: bool,
}

impl<'a> Context<'a> {
    /// The context with the player's `bindings`, on the Mac if `mac`, and no token made yet.
    #[must_use]
    pub const fn new(mac: bool, bindings: &'a Bindings) -> Self {
        Self {
            mac,
            bindings,
            last_token: None,
            selected: &[],
            viewer_open: false,
            whole_library: false,
        }
    }

    /// The words a menu row or the command list shows for the key of `command`, if it has one.
    #[must_use]
    pub fn key(&self, command: &commands::Command) -> Option<String> {
        let binding = self.bindings.get(command, self.mac)?;
        Some(kit_ui::element::describe(binding, self.mac))
    }
}

/// What a table action the client sent asks of the table's answer to it: to remember the
/// token it makes once the table takes it, to show the log lines that stop an undo or a rewind
/// the table refuses, to open the library viewer of a look the table takes, or nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Sent {
    Token(TokenChoice),
    History,
    Look(viewers::Look),
    Other,
}

/// The element a press sent a table action from, which shakes if the table refuses the action,
/// as the web's `send` names the element that `shakeRefusal` shakes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionSource {
    /// The card in this slot.
    Card(ObjectId),
    /// The flat-layer button a `TableMessage` is the press of: a mana or player counter, or the
    /// minus button beside one.
    Button(TableMessage),
}

/// A table action sent and not yet answered: what its answer asks of the client, and the
/// element a press sent it from.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub sent: Sent,
    pub from: Option<ActionSource>,
}

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
    /// card in the viewer's hand a hand card, one on the stack a stack entry, one in the library
    /// (which only an open viewer shows) a viewer card, and one anywhere else a pile card.
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
                    // A card in an open viewer of the library or of revealed cards is the web's
                    // `viewer_card` (`kindOf` in `ui/src/table/menuRows.ts`), which offers
                    // Reveal; one in a graveyard or exile viewer is a pile card.
                    Some((ZoneName::Library, _)) => AppliesTo::ViewerCard,
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
    target_from_hit(frame, hit(frame, point))
}

/// Resolves a manual-table target from the shared scene hit.
pub fn target_from_hit(frame: &Frame, hit: Option<u32>) -> Target {
    let Some(mut index) = hit else {
        return Target::Table;
    };
    loop {
        let element = &frame.resolved[index as usize];
        match (element.role, Place::in_frame(element.place)) {
            (Role::Card | Role::Pile, Some(Place::Card { zone, side, .. })) => {
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

/// The request that starts a solo table with the sample WU Control deck, which is also the
/// deck the lobby's 'Change' loads.
///
/// # Panics
///
/// Never: the build script writes the request from the sample deck.
#[must_use]
pub fn solo_request() -> TableSoloRequest {
    serde_json::from_str(commands::SOLO_REQUEST).expect("the sample deck is a solo request")
}
