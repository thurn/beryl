//! The manual table's lobby (docs/manual-multiplayer.md, "Setup and match flow"): where a
//! player loads a deck, sets their assists, and, as the host, sets the game and starts it. The
//! session sends its state as `LobbyUpdate`s; this module holds what the client keeps of it and
//! works out the changes the lobby screen's controls send.

use beryl_protocol::SessionError;
use beryl_protocol::ipc::decks::SavedDeck;
use beryl_protocol::ipc::{Assist, LobbyChange, LobbySeat, LobbySettings, LobbyView};

/// The names of the starting life and starting hand size fields of the host's game settings.
pub const STARTING_LIFE: &str = "starting life";
pub const STARTING_HAND_SIZE: &str = "starting hand size";

/// The names of the deck picker's fields: the name a pasted deck is saved under, and the
/// decklist.
pub const DECK_NAME: &str = "deck name";
pub const DECKLIST: &str = "decklist";

/// What the deck picker shows (`DeckPicker.tsx`): the saved decks, with the button that opens
/// the paste field, or the paste field. The web's third panel, a deck file chosen in the
/// system's file dialog, needs a dialog Beryl 3D's hosts do not offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeckPanel {
    Saved,
    Paste,
}

/// The five assists and their switches' labels, in the web's order (`AssistSwitches.tsx`).
pub const ASSISTS: [(Assist, &str); 5] = [
    (Assist::TurnUpkeep, "Turn upkeep"),
    (Assist::Combat, "Combat"),
    (Assist::StateNudges, "State nudges"),
    (Assist::LibraryFlows, "Library flows"),
    (Assist::AbilityMenus, "Ability menus"),
];

/// What the client keeps of the lobby: its latest state, once the session has sent one, and
/// what went wrong with the last change, if anything; the name of the deck the viewer loaded,
/// which the lobby's state does not carry; and the deck picker and the sideboard swap, while
/// they are open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lobby {
    pub view: Option<LobbyView>,
    pub error: Option<&'static str>,
    /// The failure the linked table last reported (`TableEvent::Error`), such as a guest with
    /// another version of Beryl, until the connection is up again.
    pub link_error: Option<String>,
    pub deck_name: Option<String>,
    pub picker: Option<DeckPanel>,
    /// The saved decks the picker lists, once the session has sent them.
    pub saved: Vec<SavedDeck>,
    /// A pasted decklist has gone to the session to be read, and its answer has not come back.
    pub importing: bool,
    /// Why the picker could not list the saved decks or read a decklist.
    pub deck_error: Option<String>,
    pub sideboard: bool,
}

impl Lobby {
    /// No lobby: none has arrived.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            view: None,
            error: None,
            link_error: None,
            deck_name: None,
            picker: None,
            saved: Vec::new(),
            importing: false,
            deck_error: None,
            sideboard: false,
        }
    }

    /// The viewer's own seat of the lobby, once one has arrived.
    #[must_use]
    pub fn own(&self) -> Option<&LobbySeat> {
        let view = self.view.as_ref()?;
        view.seats.iter().find(|seat| seat.seat == view.you)
    }

    /// Whether the viewer is the host, who sets the game and starts it. The host sits in seat 0,
    /// as at a solo table.
    #[must_use]
    pub fn hosting(&self) -> bool {
        self.view.as_ref().is_some_and(|view| view.you.0 == 0)
    }
}

/// What the deck picker says when the session could not list or read a deck: the reason the
/// deck library gave, as the web's picker shows it.
#[must_use]
pub fn deck_error(error: &SessionError) -> String {
    match error {
        SessionError::Deck { reason } | SessionError::Io { reason } => reason.clone(),
        _ => "Could not load the deck.".to_owned(),
    }
}

/// The change that moves one copy of the card at index `at` of `view`'s main deck to the
/// sideboard, or with `from_side`, of its sideboard to the main deck (`SideboardSwap.tsx`).
#[must_use]
pub fn swapped(view: &LobbyView, from_side: bool, at: u16) -> Option<LobbyChange> {
    let zone = if from_side { &view.side } else { &view.main };
    let (name, _) = zone.get(usize::from(at))?;
    let one = vec![(name.clone(), 1)];
    Some(if from_side {
        LobbyChange::Sideboard {
            into_main: one,
            into_side: Vec::new(),
        }
    } else {
        LobbyChange::Sideboard {
            into_main: Vec::new(),
            into_side: one,
        }
    })
}

/// The change that turns `assist` on if `view` has it off, or off if on.
#[must_use]
pub fn toggled(view: &LobbyView, assist: Assist) -> LobbyChange {
    LobbyChange::SetAssist {
        assist,
        on: !view.assists.contains(&assist),
    }
}

/// A game setting's field holds something that is not a number of its kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unreadable;

/// The game settings that the host's two fields ask for over `current`: a field left empty
/// keeps its setting. `Ok(None)` when they ask for no change.
///
/// # Errors
///
/// When a field holds something that is not a starting life (a whole number) or a hand size
/// (a whole number from 0 to 65535). The screen marks the field (`unreadable`).
pub fn asked_settings(
    current: LobbySettings,
    life: &str,
    hand: &str,
) -> Result<Option<LobbySettings>, Unreadable> {
    let life = read(life, current.starting_life)?;
    let hand = read(hand, current.starting_hand_size)?;
    let asked = LobbySettings {
        starting_life: life,
        starting_hand_size: hand,
        ..current
    };
    Ok((asked != current).then_some(asked))
}

/// The number `words` hold, or `otherwise` when they are empty.
fn read<T: std::str::FromStr>(words: &str, otherwise: T) -> Result<T, Unreadable> {
    let words = words.trim();
    if words.is_empty() {
        return Ok(otherwise);
    }
    words.parse().map_err(|_| Unreadable)
}

/// Whether the field holding `words` cannot be read as a `T`, which the screen marks.
#[must_use]
pub fn unreadable<T: std::str::FromStr>(words: &str) -> bool {
    let words = words.trim();
    !words.is_empty() && words.parse::<T>().is_err()
}
