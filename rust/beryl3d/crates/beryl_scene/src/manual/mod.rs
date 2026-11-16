//! The manual table (docs/manual-multiplayer.md): a table where the players move cards by
//! hand, with no engine. Its commands (`commands`), the context menu a right click opens on a
//! card, a pile, a hand, a life total or the table (`menus`), what the pointer is on
//! (`target_at`), which a menu or a key acts on, and what the table shows over itself: the
//! command list (`palette`), the ask prompt (`ask`), the token search (`tokens`), the library
//! order tray (`order`) and the log (`log`).

pub mod ask;
pub mod commands;
pub mod lobby;
pub mod log;
pub mod menus;
pub mod order;
pub mod palette;
pub mod tokens;

use beryl_protocol::PlayerView;
use beryl_protocol::ipc::TableSoloRequest;
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use glam::Vec2;

use crate::agent::hit;
use crate::frame::{Frame, Place, Role};
use commands::AppliesTo;

/// A press on something the table shows over itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TablePress {
    /// The `n`th row the command list shows.
    Palette(u16),
    /// The ask prompt's confirm button.
    AskConfirm,
    /// The ask prompt's plus, or its minus if `false`.
    AskStep(bool),
    /// The `n`th row the token search shows.
    Token(u16),
    /// The `n`th card of the library order tray, which moves it to the top.
    OrderCard(u16),
    /// The library order tray's Done.
    OrderDone,
    /// Undo the log's line `n`.
    Undo(u32),
    /// Rewind the table to the moment after the log's line `n`.
    Rewind(u32),
    /// Open or fold the log.
    ToggleLog,
    /// Close the command list, ask prompt, token search or order tray.
    Close,
}

/// What the client keeps of the manual table: the open context menu, command list, ask
/// prompt, token search and library order tray, each if any (at most one of the last four is
/// open), whether the log is open, and whether Shift was held at the last press of the
/// pointer, which runs a command list row's shifted form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableState {
    pub menu: Option<menus::OpenMenu>,
    pub palette: Option<palette::Palette>,
    pub ask: Option<ask::Ask>,
    pub tokens: Option<tokens::TokenSearch>,
    pub order: Option<order::LibraryOrder>,
    pub log_open: bool,
    pub shift: bool,
}

impl TableState {
    /// Nothing shown over the table, and the log folded.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            menu: None,
            palette: None,
            ask: None,
            tokens: None,
            order: None,
            log_open: false,
            shift: false,
        }
    }

    /// Whether the command list, ask prompt, token search or order tray is open, which takes
    /// the keys from the table.
    #[must_use]
    pub fn overlay_open(&self) -> bool {
        self.palette.is_some()
            || self.ask.is_some()
            || self.tokens.is_some()
            || self.order.is_some()
    }

    /// Closes the command list, ask prompt, token search and order tray. Returns whether one
    /// was open.
    pub fn close_overlay(&mut self) -> bool {
        let open = self.overlay_open();
        self.palette = None;
        self.ask = None;
        self.tokens = None;
        self.order = None;
        open
    }
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
