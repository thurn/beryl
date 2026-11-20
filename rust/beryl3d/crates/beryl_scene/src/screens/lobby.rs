//! The manual table's lobby, as the web interface's `Lobby` (`ui/src/screens/Lobby.tsx`)
//! shows it: one panel in the middle of the window, in the start screen's style, with the
//! table's heading and 'Leave table', the match's seats and score once a second player sits
//! down, the viewer's deck, their assists, the game's settings and the button that starts the
//! game, or marks the guest ready. Under the deck, the deck picker (`DeckPicker.tsx`) lists the
//! saved decks or holds a pasted decklist, and the sideboard swap (`SideboardSwap.tsx`) moves
//! cards between the loaded deck's main deck and sideboard. The panel scrolls when it is
//! taller than the window. On a phone the same sections stand in one column over the safe area
//! (`lobby_phone`), every control at least a finger's 44 points, since the web interface has no
//! phone lobby to follow.

use beryl_protocol::ipc::LobbyView;
use beryl_protocol::ipc::decks::SavedDeck;

use super::ScreenIn;
use super::start::{
    LABEL, PANEL_GAP, PANEL_PADDING, PANEL_RADIUS, PANEL_WIDTH, TITLE, checkbox, small,
};
use crate::element::{
    Align, Border, Element, Justify, LobbyPress, Overflow, Press, column, row, text,
};
use crate::manual::commands::{COMMANDS, Scope};
use crate::manual::lobby::{
    ASSISTS, DECK_NAME, DECKLIST, DeckPanel, Lobby, STARTING_HAND_SIZE, STARTING_LIFE,
    asked_settings, unreadable,
};
use crate::text::TypeStyle;
use crate::tokens::{color, size, space, type_style};
use crate::widgets::{FieldState, button, field, primary};

/// `.lobby-deck h2` and the section headings: semibold 20-point serif.
const HEADING: TypeStyle = type_style::LOBBY_HEADING;

/// Body text: 15-point sans.
const BODY: TypeStyle = type_style::SCREEN_BODY;

/// The width of a game setting's number field.
const NUMBER_WIDTH: f32 = size::LOBBY_NUMBER_WIDTH;

/// The least height of the decklist field: six lines, as the web's `<textarea rows={6}>`.
const DECKLIST_HEIGHT: f32 = size::LOBBY_DECKLIST_HEIGHT;

/// The space the panel keeps from the window's top and bottom edges.
const MARGIN: f32 = size::SCREEN_MARGIN;

/// What the flat layer shows on the lobby screen. Until the session sends the lobby, only the
/// heading.
#[must_use]
pub fn lobby(input: &ScreenIn) -> Element {
    if super::phone(input) {
        return lobby_phone(input);
    }
    let panel = column()
        .name("lobby panel")
        .width(PANEL_WIDTH)
        .max_height((input.window.size.height - 2.0 * MARGIN).max(0.0))
        .overflow(Overflow::Scroll)
        .align_items(Align::Center)
        .gap(PANEL_GAP)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(PANEL_RADIUS);
    let mut panel = sections(panel, input, Fit::Desktop);
    panel.style.padding = PANEL_PADDING;
    // The layer draws a screen's top element as a panel unless its style says otherwise; this
    // one fills the window and only holds the panel, so it is clear.
    let screen = column()
        .name("lobby")
        .background([0; 4])
        .border(([0; 4], 0.0))
        .grow(1.0)
        .align_self(Align::Stretch)
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .child(panel);
    shortcuts(screen, input.lobby)
}

/// The lobby on a phone: the panel's sections in one column over the safe area, which scrolls
/// when it is taller than the window, with no panel around them. Each button spans the column
/// as the phone's other screens draw their actions, and the switches and fields stand one
/// above the other, at least 44 points tall.
fn lobby_phone(input: &ScreenIn) -> Element {
    let content = column()
        .name("lobby column")
        .gap(crate::data::space::flat::phone::start::GAP);
    let content = sections(content, input, Fit::Phone);
    shortcuts(super::phone_screen("lobby", input, content), input.lobby)
}

/// Which window the lobby is laid out for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fit {
    Desktop,
    Phone,
}

impl Fit {
    /// `button` as an action of the lobby: as the desktop draws it, or spanning a phone's
    /// column (`phone_action`).
    fn action(self, button: Element) -> Element {
        match self {
            Self::Desktop => button,
            Self::Phone => super::phone_action(button),
        }
    }

    /// A row of `name` holding controls side by side on the desktop, and one above the other
    /// across a phone's column, which is too narrow for them.
    fn group(self, name: &str) -> Element {
        match self {
            Self::Desktop => row().name(name),
            Self::Phone => column().name(name).align_self(Align::Stretch),
        }
    }

    /// The least height of a control a finger presses: none on the desktop.
    fn least(self, element: Element) -> Element {
        match self {
            Self::Desktop => element,
            Self::Phone => element.min_height(crate::data::audit::PHONE_TARGET_POINTS),
        }
    }
}

/// `holder` with the heading and, once the session sent the lobby, every section under it, and
/// the table's error, laid out for `fit`.
fn sections(holder: Element, input: &ScreenIn, fit: Fit) -> Element {
    let lobby = input.lobby;
    let heading = row()
        .name("heading")
        .align_self(Align::Stretch)
        .align_items(Align::Center)
        .justify_content(Justify::SpaceBetween)
        .child(text("Table").style(TITLE).color(color::GOLD))
        .child(button("Leave table").on_press(Press::Lobby(LobbyPress::Leave)));
    let mut holder = holder.child(heading);
    if let Some(view) = &lobby.view {
        holder = holder
            .children(summary(view))
            .child(deck(lobby, view, fit))
            .children(lobby.picker.map(|panel| picker(lobby, panel, input, fit)))
            .children(sideboard(lobby, view, fit))
            .child(assists(view, fit))
            .child(if lobby.hosting() {
                settings(view, input, fit)
            } else {
                settings_summary(view)
            })
            .child(fit.action(start(lobby, view)));
    }
    if let Some(error) = lobby.error {
        holder = holder.child(text(error).style(BODY).color(color::EMBER));
    }
    if let Some(error) = &lobby.link_error {
        holder = holder.child(text(error).style(BODY).color(color::EMBER));
    }
    holder
}

/// The lobby's keys, from `data/table-commands.json`, as the web's lobby resolves them: 'Paste
/// decklist' opens the picker's paste field, 'Change' opens the picker (on the web, its file
/// panel, which Beryl 3D does not have), 'Unload deck' unloads a loaded deck and 'Sideboard'
/// opens or closes its sideboard swap, 'Ready' marks a guest with a deck ready or not, and
/// 'Leave table' leaves. Written as the Mac's bindings, as the table's are.
fn shortcuts(mut screen: Element, lobby: &Lobby) -> Element {
    let loaded = lobby.own().is_some_and(|own| own.loaded);
    for command in COMMANDS {
        let press = match command.id {
            "lobby.change_deck" if command.scope == Scope::Lobby => LobbyPress::Change,
            "lobby.paste_deck" => LobbyPress::Paste,
            "lobby.unload_deck" if loaded => LobbyPress::Unload,
            "lobby.sideboard" if loaded => LobbyPress::Sideboard,
            "lobby.ready" if loaded && !lobby.hosting() => LobbyPress::Ready,
            "table.leave" => LobbyPress::Leave,
            _ => continue,
        };
        for key in command.key.iter().chain(command.key_aliases) {
            screen = screen.shortcut(key.mac, Press::Lobby(press));
        }
    }
    screen
}

/// A section of the panel with a heading, as the web's `.lobby-section` fieldsets have.
fn section(name: &str) -> Element {
    column()
        .name(name)
        .align_self(Align::Stretch)
        .gap(size::LOBBY_ROW_GAP)
        .child(
            text(&name.to_uppercase())
                .style(LABEL)
                .color(color::TEXT_DIM),
        )
}

/// The match (`LobbySummary.tsx`), once a second seat exists: the invite code, each seat's name
/// and deck, whether the guest is ready, the score, and who won the last game.
fn summary(view: &LobbyView) -> Option<Element> {
    if view.seats.len() < 2 {
        return None;
    }
    let mut match_section = section("Match");
    if let Some(code) = &view.invite {
        match_section = match_section.child(
            text(&format!("Invite code {code}"))
                .style(BODY)
                .color(color::TYPE),
        );
    }
    for seat in &view.seats {
        let name = seat.name.as_deref().unwrap_or("Guest");
        let deck = if seat.loaded {
            format!("{} main · {} sideboard", seat.main_count, seat.side_count)
        } else {
            "No deck loaded".to_owned()
        };
        let ready = if seat.seat.0 == 1 && seat.ready {
            " · Ready"
        } else {
            ""
        };
        match_section = match_section.child(
            text(&format!("{name}: {deck}{ready}"))
                .style(BODY)
                .color(color::TYPE),
        );
    }
    let score: Vec<String> = view
        .seats
        .iter()
        .map(|seat| seat.wins.to_string())
        .collect();
    match_section = match_section.child(
        text(&format!("Match score {}", score.join("–")))
            .style(BODY)
            .color(color::TYPE),
    );
    if view.game_number > 1 {
        let winner = (view.seats.iter()).find(|seat| Some(seat.seat) == view.last_winner);
        let result = winner.map_or_else(
            || "No result".to_owned(),
            |seat| {
                let name = seat.name.as_deref().unwrap_or("Guest");
                format!("{name} won game {}", view.game_number - 1)
            },
        );
        match_section = match_section.child(text(&result).style(BODY).color(color::TEXT_DIM));
    }
    if view.seats.get(1).is_some_and(|seat| seat.name.is_none()) {
        match_section = match_section.child(
            text("Waiting for guest…")
                .style(BODY)
                .color(color::TEXT_DIM),
        );
    }
    Some(match_section)
}

/// The viewer's deck: the name of the one loaded, or that one is loaded or none is, its counts,
/// the buttons that change it, open its sideboard swap and unload it, and the names the table
/// could not read as cards.
fn deck(lobby: &Lobby, view: &LobbyView, fit: Fit) -> Element {
    let own = lobby.own();
    let loaded = own.is_some_and(|own| own.loaded);
    let mut words = column().name("deck status").child(
        text(match &lobby.deck_name {
            _ if !loaded => "No deck loaded",
            Some(name) => name,
            None => "Deck loaded",
        })
        .style(HEADING)
        .color(color::TYPE),
    );
    if let Some(own) = own.filter(|own| own.loaded) {
        let counts = format!("{} main · {} sideboard", own.main_count, own.side_count);
        words = words.child(text(&counts).style(BODY).color(color::TEXT_DIM));
    }
    let actions =
        (fit.group("deck actions"))
            .gap(space::M)
            .child(fit.action(button("Change").on_press(Press::Lobby(LobbyPress::Change))))
            .child(
                fit.action(
                    button("Sideboard")
                        .enabled(loaded)
                        .selected(lobby.sideboard && loaded)
                        .on_press(Press::Lobby(LobbyPress::Sideboard)),
                ),
            )
            .children(loaded.then(|| {
                fit.action(button("Unload deck").on_press(Press::Lobby(LobbyPress::Unload)))
            }));
    let mut deck = section("Deck").child(words).child(actions);
    if !view.unknown.is_empty() {
        deck = deck.child(text("Unknown cards").style(LABEL).color(color::EMBER));
        for name in &view.unknown {
            deck = deck.child(text(name).style(BODY).color(color::TYPE));
        }
    }
    deck
}

/// The deck picker, open on `panel`: the saved decks, each a button that loads it beside its
/// counts, and the buttons that open the paste field; or the paste field, with the name to
/// save the deck under, the decklist, and 'Import'. Then why the last deck could not be read,
/// if it could not, and 'Close'.
fn picker(lobby: &Lobby, panel: DeckPanel, input: &ScreenIn, fit: Fit) -> Element {
    let mut picker = section("Choose a deck");
    picker = match panel {
        DeckPanel::Saved => picker
            .children(
                (0u16..)
                    .zip(&lobby.saved)
                    .map(|(at, saved)| saved_deck(at, saved, fit)),
            )
            .child((fit.group("picker actions")).gap(space::M).child(
                fit.action(button("Paste decklist").on_press(Press::Lobby(LobbyPress::Paste))),
            )),
        DeckPanel::Paste => {
            let name = input.fields.get(DECK_NAME);
            let mut list = input.fields.get(DECKLIST);
            "4 Island".clone_into(&mut list.placeholder);
            let mut list_field = field(DECKLIST, &list);
            list_field.style.min_height = Some(DECKLIST_HEIGHT);
            list_field.style.align_self = Some(Align::Stretch);
            let mut name_field = fit.least(field(DECK_NAME, &name));
            name_field.style.align_self = Some(Align::Stretch);
            let ready = !name.text.trim().is_empty() && !list.text.trim().is_empty();
            picker
                .child(text("Deck name").style(LABEL).color(color::TEXT_DIM))
                .child(name_field)
                .child(text("Decklist").style(LABEL).color(color::TEXT_DIM))
                .child(list_field)
                .child(
                    fit.action(
                        primary("Import")
                            .align_self(Align::Start)
                            .enabled(ready && !lobby.importing)
                            .on_press(Press::Lobby(LobbyPress::Import)),
                    ),
                )
        }
    };
    if let Some(error) = &lobby.deck_error {
        picker = picker.child(text(error).style(BODY).color(color::EMBER));
    }
    picker.child(
        fit.action(
            button("Close")
                .align_self(Align::Start)
                .enabled(!lobby.importing)
                .on_press(Press::Lobby(LobbyPress::ClosePicker)),
        ),
    )
}

/// One saved deck of the picker's list: a button showing its name, which loads it, and its
/// counts, beside it on the desktop and under it on a phone.
fn saved_deck(at: u16, saved: &SavedDeck, fit: Fit) -> Element {
    let counts = format!("{} main · {} sideboard", saved.main_count, saved.side_count);
    (fit.group(&format!("saved {}", saved.deck.name)))
        .gap(size::LOBBY_GAP)
        .align_items(Align::Center)
        .child(fit.action(button(&saved.deck.name).on_press(Press::Lobby(LobbyPress::Pick(at)))))
        .child(text(&counts).style(BODY).color(color::TEXT_DIM))
}

/// The sideboard swap, while it is open and a deck is loaded: the main deck's cards and then
/// the sideboard's, each with its count and a small button that moves one copy to the other,
/// and 'Done'.
fn sideboard(lobby: &Lobby, view: &LobbyView, fit: Fit) -> Option<Element> {
    if !lobby.sideboard || !lobby.own().is_some_and(|own| own.loaded) {
        return None;
    }
    let mut swap = section("Sideboard swap");
    for (from_side, heading, zone) in [
        (false, "Main deck", &view.main),
        (true, "Sideboard", &view.side),
    ] {
        swap = swap.child(text(heading).style(HEADING).color(color::TYPE));
        for (at, (name, count)) in (0u16..).zip(zone) {
            let (label, to) = if from_side {
                ("To main", "main deck")
            } else {
                ("To sideboard", "sideboard")
            };
            let move_one = fit
                .least(small(button(label)))
                .on_press(Press::Lobby(LobbyPress::Swap { from_side, at }))
                .name(&format!("Move {name} to {to}"));
            swap = swap.child(
                row()
                    .name(name)
                    .align_self(Align::Stretch)
                    .align_items(Align::Center)
                    .justify_content(Justify::SpaceBetween)
                    .gap(size::LOBBY_GAP)
                    .child(
                        text(&format!("{count} × {name}"))
                            .style(BODY)
                            .color(color::TYPE),
                    )
                    .child(move_one),
            );
        }
    }
    Some(
        swap.child(
            fit.action(
                button("Done")
                    .align_self(Align::Start)
                    .on_press(Press::Lobby(LobbyPress::Sideboard)),
            ),
        ),
    )
}

/// The five assists' switches (`AssistSwitches.tsx`).
fn assists(view: &LobbyView, fit: Fit) -> Element {
    let mut boxes = row().name("assist switches").gap(space::L).wrap(true);
    if fit == Fit::Phone {
        boxes = fit.group("assist switches");
    }
    for (at, (assist, label)) in (0u8..).zip(ASSISTS) {
        let on = view.assists.contains(&assist);
        boxes = boxes.child(fit.least(checkbox(label, on, Press::Lobby(LobbyPress::Assist(at)))));
    }
    section("Assists").child(boxes)
}

/// The host's game settings (`GameSettings.tsx`): a field for the starting life and one for the
/// starting hand size, each showing the setting while it is empty, and 'Save settings', which
/// is enabled while the fields ask for a change. A field that holds something other than a
/// number of its kind is drawn in ember. Who plays first is shown, not chosen: a solo table has
/// one seat, and a hosted table, which would have two, is not built yet.
fn settings(view: &LobbyView, input: &ScreenIn, fit: Fit) -> Element {
    let current = view.settings;
    let mut life = input.fields.get(STARTING_LIFE);
    let mut hand = input.fields.get(STARTING_HAND_SIZE);
    life.placeholder = current.starting_life.to_string();
    hand.placeholder = current.starting_hand_size.to_string();
    let changed = matches!(asked_settings(current, &life.text, &hand.text), Ok(Some(_)));
    let fields = (fit.group("game fields"))
        .gap(space::L)
        .align_items(match fit {
            Fit::Desktop => Align::Center,
            Fit::Phone => Align::Start,
        })
        .child(number(
            "Starting life",
            STARTING_LIFE,
            &life,
            unreadable::<i32>(&life.text),
            fit,
        ))
        .child(number(
            "Starting hand size",
            STARTING_HAND_SIZE,
            &hand,
            unreadable::<u16>(&hand.text),
            fit,
        ));
    section("Game").child(fields).child(first(view)).child(
        fit.action(
            button("Save settings")
                .align_self(Align::Start)
                .enabled(changed)
                .on_press(Press::Lobby(LobbyPress::SaveSettings)),
        ),
    )
}

/// A game setting's label and number field `name`, holding `state`, drawn in ember if `wrong`,
/// at least a finger's height on a phone.
fn number(label: &str, name: &str, state: &FieldState, wrong: bool, fit: Fit) -> Element {
    let mut number = fit.least(field(name, state));
    number.style.width = Some(NUMBER_WIDTH);
    if wrong {
        number.style.border = Some(Border::uniform(color::EMBER, size::BORDER));
        number.style.color = Some(color::EMBER);
    }
    row()
        .name(label)
        .gap(size::LOBBY_ROW_GAP)
        .align_items(Align::Center)
        .child(text(label).style(LABEL).color(color::TEXT_DIM))
        .child(number)
}

/// The game settings as a guest sees them (`GameSettingsSummary`).
fn settings_summary(view: &LobbyView) -> Element {
    let settings = view.settings;
    section("Game")
        .child(
            text(&format!("Starting life: {}", settings.starting_life))
                .style(BODY)
                .color(color::TYPE),
        )
        .child(
            text(&format!(
                "Starting hand size {}",
                settings.starting_hand_size
            ))
            .style(BODY)
            .color(color::TYPE),
        )
        .child(first(view))
}

/// Who plays first: the named seat, or by default a roll, or after a game the loser's choice.
fn first(view: &LobbyView) -> Element {
    let first = match view.settings.first {
        Some(first) => (view.seats.iter())
            .find(|seat| seat.seat == first)
            .and_then(|seat| seat.name.clone())
            .unwrap_or_else(|| "Guest".to_owned()),
        None if view.last_winner.is_some() => "Loser chooses".to_owned(),
        None => "Roll".to_owned(),
    };
    text(&format!("First: {first}"))
        .style(BODY)
        .color(color::TYPE)
}

/// The host's 'Start', enabled when the table can start, or the guest's 'Ready', enabled once
/// their deck is loaded and selected while they are ready.
fn start(lobby: &Lobby, view: &LobbyView) -> Element {
    if lobby.hosting() {
        return primary("Start")
            .enabled(view.can_start)
            .on_press(Press::Lobby(LobbyPress::Start));
    }
    let own = lobby.own();
    primary("Ready")
        .enabled(own.is_some_and(|own| own.loaded))
        .selected(own.is_some_and(|own| own.ready))
        .on_press(Press::Lobby(LobbyPress::Ready))
}
