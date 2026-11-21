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
pub mod quit;
pub mod selection;
pub mod tokens;
pub mod turn;
pub mod viewers;

use beryl_protocol::PlayerView;
use beryl_protocol::ipc::TableSoloRequest;
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use glam::Vec2;

use crate::agent::hit;
use crate::frame::{Frame, Place, Role};
use bindings::Bindings;
use commands::AppliesTo;
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
        Some(crate::element::describe(binding, self.mac))
    }
}

/// A press on something the table shows over itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TablePress {
    /// The `n`th row the command list shows.
    Palette(u16),
    /// The ask prompt's confirm button.
    AskConfirm,
    /// The ask prompt's plus, or its minus if `false`.
    AskStep(bool),
    /// The `n`th common counter name of the ask prompt (`ask::COUNTER_SUGGESTIONS`), which
    /// confirms it with that name.
    AskSuggestion(u8),
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
    /// Open the proposal panel for the proposal with this id, or fold it if `None`, on a phone,
    /// where it is folded to a button (`assists::proposals`).
    ToggleProposals(Option<u32>),
    /// The custom token form's color `n` of `tokens::COLORS`, turned on or off.
    CustomColor(u8),
    /// The custom token form's Create token.
    CustomCreate,
    /// Stop waiting for a key to bind to the command list's highlighted row.
    CancelCapture,
    /// The icon at `at` of a step track, in the active player's rows if `active`
    /// (`turn::step_command`).
    Step { at: u8, active: bool },
    /// The Next button, which advances to the next phase.
    Next,
    /// The counter at `at` of `side`'s row of mana and player counters, which adds one
    /// (`pools`).
    Counter { side: Side, at: u8 },
    /// The minus button beside that counter in the phone's sheet, which removes one.
    CounterRemove { side: Side, at: u8 },
    /// Open the phone's sheet of `side`'s mana and counters, or close it (`pools::sheet`).
    ManaSheet(Option<Side>),
    /// Quit's question was answered Quit.
    Quit,
    /// Close the command list, ask prompt, token search, custom token form or order tray.
    Close,
    /// Turn the line `line` of the proposal `id` on or off (`assists::proposals`).
    ProposalLine { id: u32, line: u32, on: bool },
    /// Apply the checked lines of the proposal `id`, as it stood at `revision`.
    ApplyProposal { id: u32, revision: u32 },
    /// Apply the state nudge whose label has this id (`assists::labels`).
    Label(u32),
    /// A card of the sorting tray (`assists::sort_answer`).
    SortCard(ObjectId),
    /// The sorting tray's Done.
    SortDone,
    /// The sorting tray's Cancel.
    SortCancel,
    /// A button or card of the guided start.
    Opening(opening::OpeningPress),
    /// Leave the table, from the banner of a table the other player left.
    Leave,
    /// The pile viewer's "Group by type", which turns grouping on or off.
    ViewerGroup,
    /// The pile viewer's sort, which lists its cards in this order.
    ViewerSort(viewers::Sort),
    /// Close the search of a whole library, shuffling it first if `true` (Shuffle) or not
    /// (Don't shuffle).
    ViewerFinish(bool),
}

/// What the client keeps of the manual table: the open context menu, command list, ask
/// prompt, token search, custom token form and library order tray, each if any (at most one of
/// the last five is open), whether the log is open, the log lines that stopped the last undo,
/// the last token made, the table actions sent and not yet answered, and whether Shift was held
/// at the last press of the pointer, which runs a command list row's shifted form. Also the selected
/// cards, the rectangle a sweep has drawn so far (from where it began to the pointer), the
/// command waiting for the object or player it acts toward, where the held card would land, and
/// the selected permanent the selection's summary stands over.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableState {
    pub menu: Option<menus::OpenMenu>,
    pub palette: Option<palette::Palette>,
    pub ask: Option<ask::Ask>,
    pub tokens: Option<tokens::TokenSearch>,
    pub custom: Option<tokens::CustomToken>,
    pub order: Option<order::LibraryOrder>,
    pub log_open: bool,
    /// The id of the first proposal the panel was opened for on a phone, where it is folded to
    /// a button until it is opened (`assists::proposals`). A proposal with another id finds it
    /// folded again.
    pub proposals_open: Option<u32>,
    /// The numbers of the log lines whose actions stopped the last undo or rewind, which the
    /// log highlights (`blocked` in the web's `TableLogControls`).
    pub blocked: Vec<u32>,
    /// The token the table last made at the player's asking (`#lastToken` in the web's
    /// `TableCommands`).
    pub last_token: Option<TokenChoice>,
    /// For each table action sent and not yet answered, oldest first, what the client does
    /// with the table's answer to it.
    pub pending: std::collections::VecDeque<Pending>,
    pub shift: bool,
    pub selection: Vec<ObjectId>,
    pub sweep: Option<(Vec2, Vec2)>,
    pub picking: Option<arrows::Pick>,
    pub landing: Option<drop::Destination>,
    /// The highest selected permanent on the window, which the selection's summary stands
    /// over (`selection::highest`).
    pub summary_over: Option<ObjectId>,
    /// Whether the Quit key asked whether to leave the table and quit (`quit`).
    pub quitting: bool,
    /// What the open pile viewer shows beyond its pile: a library look, its sort and grouping,
    /// and whether it asks to shuffle (`viewers::PileViewer`).
    pub viewer: viewers::PileViewer,
    /// Whose mana and counters the phone's sheet shows, while it is open (`pools::sheet`). A
    /// new view leaves it open, so that a player adding mana sees the count change.
    pub mana_sheet: Option<Side>,
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
pub enum Pressed {
    /// The card in this slot.
    Card(ObjectId),
    /// The flat-layer button a `TablePress` is the press of: a mana or player counter, or the
    /// minus button beside one.
    Button(TablePress),
}

/// A table action sent and not yet answered: what its answer asks of the client, and the
/// element a press sent it from.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub sent: Sent,
    pub from: Option<Pressed>,
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
            custom: None,
            order: None,
            log_open: false,
            proposals_open: None,
            blocked: Vec::new(),
            last_token: None,
            pending: std::collections::VecDeque::new(),
            shift: false,
            selection: Vec::new(),
            sweep: None,
            picking: None,
            landing: None,
            summary_over: None,
            quitting: false,
            viewer: viewers::PileViewer::new(),
            mana_sheet: None,
        }
    }

    /// Whether the command list, ask prompt, token search, custom token form, order tray or
    /// Quit's question is open, which takes the keys from the table.
    #[must_use]
    pub fn overlay_open(&self) -> bool {
        self.palette.is_some()
            || self.ask.is_some()
            || self.tokens.is_some()
            || self.custom.is_some()
            || self.order.is_some()
            || self.quitting
    }

    /// Closes the command list, ask prompt, token search, custom token form, order tray and
    /// Quit's question. Returns whether one was open.
    pub fn close_overlay(&mut self) -> bool {
        let open = self.overlay_open();
        self.palette = None;
        self.ask = None;
        self.tokens = None;
        self.custom = None;
        self.order = None;
        self.quitting = false;
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

/// What a `Press::Lobby` does (`screens::lobby`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyPress {
    /// Open a solo table's lobby, from the start screen.
    Solo,
    /// Open the start screen's form for hosting a table.
    Host,
    /// Open the start screen's form for joining a table.
    Join,
    /// Close the start screen's host or join form.
    CancelEntry,
    /// Host or join a table with what the open form holds.
    OpenTable,
    /// Resume the saved table at this index of the start screen's list.
    Resume(u16),
    /// Open the deck picker on the saved decks ('Change').
    Change,
    /// Open the deck picker on the field a decklist is pasted into ('Paste decklist').
    Paste,
    /// Load the saved deck at this index of the deck picker's list.
    Pick(u16),
    /// Read the pasted decklist, save it under the name typed and load it ('Import').
    Import,
    /// Close the deck picker.
    ClosePicker,
    /// Open the sideboard swap, or close it.
    Sideboard,
    /// Move one copy of the card at index `at` of the loaded deck's main deck to its sideboard,
    /// or with `from_side`, of its sideboard to its main deck.
    Swap { from_side: bool, at: u16 },
    /// Unload the loaded deck.
    Unload,
    /// Mark the guest ready, or no longer ready.
    Ready,
    /// Turn the assist at this index of `manual::lobby::ASSISTS` on or off.
    Assist(u8),
    /// Ask for the starting life and hand size the host's fields hold.
    SaveSettings,
    /// Start the table's game.
    Start,
    /// Leave the table and go back to the start screen.
    Leave,
}

/// A shortcut for a manual table command. The flag indicates Shift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableCommand(pub u16, pub bool);
