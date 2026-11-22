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

use super::Message;
use beryl_protocol::ipc::decks::SavedDeck;
use beryl_protocol::ipc::{LobbyChange, LobbyView};
use kit_scene::Shortcut;

use crate::manual::commands::{COMMANDS, Scope};
use crate::manual::lobby::{
    ASSISTS, DECK_NAME, DECKLIST, DeckPanel, Lobby, STARTING_HAND_SIZE, STARTING_LIFE,
    asked_settings, hosting, own, swapped, toggled, unreadable,
};
use game_table::table::Window;
use kit_base::tokens::type_style::{START_LABEL as LABEL, START_TITLE as TITLE};
use kit_base::tokens::{color, size, space, type_style};
use kit_frame::frame::Role;
use kit_text::text::TypeStyle;
use kit_ui::element::{
    Align, Border, Display, Element, Justify, Overflow, Position, Track, column, row, text,
};
use kit_ui::widgets::screen::{ScreenColumn, whole_window};
use kit_ui::widgets::start_panel::{PANEL_PADDING, check_square, checkbox, panel_look, small};
use kit_ui::widgets::{FieldState, Fields, button, field, primary};

/// Inputs from the window and the layer's field editor.
#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub window: Window,
    pub column: Option<ScreenColumn>,
    pub fields: &'a Fields,
    pub view: Option<&'a LobbyView>,
}

pub type ViewKey = (
    Window,
    Option<ScreenColumn>,
    [FieldState; 4],
    Option<LobbyView>,
);

pub(super) fn key(input: &Inputs<'_>) -> ViewKey {
    (
        input.window,
        input.column,
        [DECK_NAME, DECKLIST, STARTING_LIFE, STARTING_HAND_SIZE].map(|name| input.fields.get(name)),
        input.view.cloned(),
    )
}

pub(super) fn render(lobby: &Lobby, input: Inputs<'_>) -> Element {
    render_lobby(&LobbyIn {
        window: input.window,
        column: input.column,
        lobby,
        fields: input.fields,
        view: input.view,
    })
}

/// The manual lobby's state and text fields for the current window.
struct LobbyIn<'a> {
    pub window: Window,
    column: Option<ScreenColumn>,
    lobby: &'a Lobby,
    view: Option<&'a LobbyView>,
    pub fields: &'a Fields,
}

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

/// The desktop's legends and labels (`.lobby-section legend, .lobby-panel label`): semibold
/// 15-point sans on a 20-point line.
const PANEL_LABEL: TypeStyle = type_style::LOBBY_LABEL;

/// The height of one line of `PANEL_LABEL`.
const LABEL_LINE: f32 = PANEL_LABEL.size * PANEL_LABEL.line_height;

/// What the flat layer shows on the lobby screen. Until the session sends the lobby, only the
/// heading.
#[must_use]
fn render_lobby(input: &LobbyIn) -> Element {
    if let Some(layout) = input.column {
        return lobby_column(input, layout);
    }
    // `.start-panel.lobby-panel`: wider than the start screen's panel, with its sections
    // stretched across it.
    let panel = column()
        .name("lobby panel")
        .width(size::LOBBY_PANEL_WIDTH)
        .max_height((input.window.size.height - 2.0 * MARGIN).max(0.0))
        .overflow(Overflow::Scroll)
        .align_items(Align::Stretch)
        .gap(size::LOBBY_PANEL_GAP);
    let panel = panel_look(panel);
    let mut panel = sections(panel, input, Fit::Regular);
    panel.style.padding = PANEL_PADDING;
    // The layer draws a screen's top element as a panel unless its style says otherwise; this
    // one fills the window and only holds the panel, so it is clear.
    let screen = column()
        .name("lobby")
        .background([0; 4])
        .border(([0; 4], 0.0))
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .child(panel);
    whole_window(input.window.size, screen)
}

/// The lobby on a phone: the panel's sections in one column over the safe area, which scrolls
/// when it is taller than the window, with no panel around them. Each button spans the column
/// as the phone's other screens draw their actions, and the switches and fields stand one
/// above the other, at least 44 points tall.
fn lobby_column(input: &LobbyIn, layout: ScreenColumn) -> Element {
    let content = column().name("lobby column").gap(layout.metrics.start.gap);
    let content = sections(content, input, Fit::Column(layout));
    layout.scrolling("lobby", input.window, content)
}

/// Which window the lobby is laid out for.
#[derive(Clone, Copy, PartialEq)]
enum Fit {
    Regular,
    Column(ScreenColumn),
}

impl Fit {
    /// `button` as an action of the lobby: as the desktop draws it, or spanning a phone's
    /// column.
    fn action(self, button: Element) -> Element {
        match self {
            Self::Regular => button,
            Self::Column(layout) => layout.action(button),
        }
    }

    /// A row of `name` holding controls side by side on the desktop, and one above the other
    /// across a phone's column, which is too narrow for them.
    fn group(self, name: &str) -> Element {
        match self {
            Self::Regular => row().name(name),
            Self::Column(_) => column().name(name).align_self(Align::Stretch),
        }
    }

    /// A section of the panel named `name`: on the desktop a framed fieldset with `name` as its
    /// legend, and on a phone a column under `name` in capitals.
    fn section(self, name: &str) -> Element {
        match self {
            Self::Regular => fieldset(name),
            Self::Column(_) => section(name),
        }
    }

    /// The least height of a control a finger presses: none on the desktop.
    fn least(self, element: Element) -> Element {
        match self {
            Self::Regular => element,
            Self::Column(layout) => element.min_height(layout.target),
        }
    }
}

/// `holder` with the heading and, once the session sent the lobby, every section under it, and
/// the table's error, laid out for `fit`.
fn sections(holder: Element, input: &LobbyIn, fit: Fit) -> Element {
    let lobby = input.lobby;
    let heading = row()
        .name("heading")
        .align_self(Align::Stretch)
        .align_items(Align::Center)
        .justify_content(Justify::SpaceBetween)
        .child(text("Table").style(TITLE).color(color::GOLD))
        .child(button("Leave table").on_press(Message::Leave));
    let mut holder = holder.child(heading);
    if let Some(view) = input.view {
        holder = holder
            .children(summary(view, fit))
            .child(deck(lobby, view, fit))
            .children(lobby.picker.map(|panel| picker(lobby, panel, input, fit)))
            .children(sideboard(lobby, view, fit))
            .child(assists(view, fit))
            .child(if hosting(view) {
                settings(view, input, fit)
            } else {
                settings_summary(view, fit)
            })
            .child(match fit {
                // `.lobby-start` stands in the middle of the panel.
                Fit::Regular => start(view).align_self(Align::Center),
                Fit::Column(_) => fit.action(start(view)),
            });
    }
    if let Some(error) = lobby.error {
        holder = holder.child(text(error).style(BODY).color(color::EMBER));
    }
    if let Some(error) = &lobby.link_error {
        holder = holder.child(text(error).style(BODY).color(color::EMBER));
    }
    holder
}

/// The commands of `data/table-commands.json` whose keys the lobby answers (`shortcut`), as
/// its registry entry declares them.
pub const SHORTCUTS: [Shortcut; 6] = [
    Shortcut::command("lobby.change_deck", "Change deck"),
    Shortcut::command("lobby.paste_deck", "Paste decklist"),
    Shortcut::command("lobby.unload_deck", "Unload deck"),
    Shortcut::command("lobby.sideboard", "Sideboard"),
    Shortcut::command("lobby.ready", "Ready"),
    Shortcut::command("table.leave", "Leave table"),
];

/// What a key of the lobby's command `id` (one of `SHORTCUTS`) does, as the web's lobby
/// resolves it: 'Paste decklist' opens the picker's paste field, 'Change' opens the picker (on
/// the web, its file panel, which Beryl 3D does not have), 'Unload deck' unloads a loaded deck
/// and 'Sideboard' opens or closes its sideboard swap, 'Ready' marks a guest with a deck ready
/// or not, and 'Leave table' leaves. `None` where the command does nothing now.
#[must_use]
pub fn shortcut(id: &str, view: Option<&LobbyView>) -> Option<Message> {
    let own = view.and_then(own);
    let loaded = own.is_some_and(|own| own.loaded);
    let command = COMMANDS.iter().find(|command| command.id == id)?;
    Some(match id {
        "lobby.change_deck" if command.scope == Scope::Lobby => Message::Change,
        "lobby.paste_deck" => Message::Paste,
        "lobby.unload_deck" if loaded => Message::Unload,
        "lobby.sideboard" if loaded => Message::Sideboard,
        "lobby.ready" if loaded && view.is_some_and(|view| !hosting(view)) => {
            Message::ChangeLobby(LobbyChange::Ready {
                ready: !own.is_some_and(|own| own.ready),
            })
        }
        "table.leave" => Message::Leave,
        _ => return None,
    })
}

/// A desktop section's frame (`.lobby-section`): a column stretched across the panel with a
/// metal border and rounded corners.
fn frame(name: &str) -> Element {
    column()
        .name(name)
        .align_self(Align::Stretch)
        .gap(size::LOBBY_SECTION_GAP)
        .border((color::METAL, size::LOBBY_SECTION_BORDER))
        .radius(size::LOBBY_SECTION_RADIUS)
}

/// A desktop section with a legend (`fieldset.lobby-section`). As the web draws a fieldset,
/// its border's top line runs through the middle of the legend, open across the legend's box
/// (`Style::legend`), while the section's box is the fieldset's. The legend's words stand at
/// the section's padding with the browser's 2 points on either side, and the section's rows
/// start the section's padding under the legend.
fn fieldset(name: &str) -> Element {
    let padding = size::LOBBY_SECTION_PADDING;
    let mut legend =
        (text(name).style(PANEL_LABEL).color(color::TEXT_DIM)).align_self(Align::Start);
    let side = size::LOBBY_LEGEND_PADDING;
    legend.style.padding = [0.0, side, 0.0, side];
    // The legend stands over the border rather than under it, and keeps the section's padding,
    // less the gap between rows, from the rows under it.
    let border = size::LOBBY_SECTION_BORDER;
    legend.style.margin = [-border, 0.0, padding - size::LOBBY_SECTION_GAP, 0.0];
    let mut section = frame(name).align_items(Align::Start).child(legend);
    section.style.padding = [0.0, padding, padding, padding];
    section.style.legend = true;
    section
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
fn summary(view: &LobbyView, fit: Fit) -> Option<Element> {
    if view.seats.len() < 2 {
        return None;
    }
    let mut match_section = fit.section("Match");
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
    let own = own(view);
    let loaded = own.is_some_and(|own| own.loaded);
    // On the desktop, `.lobby-panel h2` and the counts under it in the panel's own type.
    let (heading, counts_style, counts_color) = match fit {
        Fit::Regular => (
            type_style::LOBBY_DECK,
            type_style::LOBBY_COUNTS,
            color::TYPE,
        ),
        Fit::Column(_) => (HEADING, BODY, color::TEXT_DIM),
    };
    let mut words = column().name("deck status").child(
        text(match &lobby.deck_name {
            _ if !loaded => "No deck loaded",
            Some(name) => name,
            None => "Deck loaded",
        })
        .style(heading)
        .color(color::TYPE),
    );
    if let Some(own) = own.filter(|own| own.loaded) {
        let counts = format!("{} main · {} sideboard", own.main_count, own.side_count);
        words = words.child(text(&counts).style(counts_style).color(counts_color));
    }
    let actions = (fit.group("deck actions"))
        .gap(size::LOBBY_SECTION_GAP)
        .child(fit.action(button("Change").on_press(Message::Change)))
        .child(
            fit.action(
                button("Sideboard")
                    .enabled(loaded)
                    .selected(lobby.sideboard && loaded)
                    .on_press(Message::Sideboard),
            ),
        )
        .children(loaded.then(|| fit.action(button("Unload deck").on_press(Message::Unload))));
    // On the desktop the deck's section has no legend (`section.lobby-section`), and its rows
    // keep to their own widths.
    let holder = match fit {
        Fit::Regular => frame("Deck")
            .align_items(Align::Start)
            .padding(size::LOBBY_SECTION_PADDING),
        Fit::Column(_) => section("Deck"),
    };
    let mut deck = holder.child(words).child(actions);
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
fn picker(lobby: &Lobby, panel: DeckPanel, input: &LobbyIn, fit: Fit) -> Element {
    let mut picker = fit.section("Choose a deck");
    picker = match panel {
        DeckPanel::Saved => picker
            .children(
                (0u16..)
                    .zip(&lobby.saved)
                    .map(|(at, saved)| saved_deck(at, saved, fit)),
            )
            .child(
                (fit.group("picker actions"))
                    .gap(space::M)
                    .child(fit.action(button("Paste decklist").on_press(Message::Paste))),
            ),
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
                            .on_press(Message::Import {
                                name: name.text.trim().to_owned(),
                                text: list.text,
                            }),
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
                .on_press(Message::ClosePicker),
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
        .child(fit.action(button(&saved.deck.name).on_press(Message::Pick(at))))
        .child(text(&counts).style(BODY).color(color::TEXT_DIM))
}

/// The sideboard swap, while it is open and a deck is loaded: the main deck's cards and then
/// the sideboard's, each with its count and a small button that moves one copy to the other,
/// and 'Done'.
fn sideboard(lobby: &Lobby, view: &LobbyView, fit: Fit) -> Option<Element> {
    if !lobby.sideboard || !own(view).is_some_and(|own| own.loaded) {
        return None;
    }
    let mut swap = fit.section("Sideboard swap");
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
                .on_press(Message::ChangeLobby(
                    swapped(view, from_side, at).expect("the card is in the deck"),
                ))
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
                    .on_press(Message::Sideboard),
            ),
        ),
    )
}

/// The five assists' switches (`AssistSwitches.tsx`): on the desktop in two columns of equal
/// width (`.lobby-assists`), and on a phone one above the other.
fn assists(view: &LobbyView, fit: Fit) -> Element {
    let mut boxes = match fit {
        Fit::Regular => row()
            .name("assist switches")
            .display(Display::Grid)
            .grid_template_columns(vec![Track::Fraction(1.0); 2])
            .gap(size::LOBBY_SWITCH_GAP)
            .align_self(Align::Stretch),
        Fit::Column(_) => fit.group("assist switches"),
    };
    for (assist, label) in ASSISTS {
        let on = view.assists.contains(&assist);
        let press = Message::ChangeLobby(toggled(view, assist));
        boxes = boxes.child(match fit {
            Fit::Regular => switch(label, on, press),
            Fit::Column(_) => fit.least(checkbox(label, on).on_press(press)),
        });
    }
    let mut assists = fit.section("Assists").child(boxes);
    if fit == Fit::Regular {
        assists.style.align_items = Some(Align::Stretch);
    }
    assists
}

/// One of the desktop's assists (`.lobby-assists label`): a 12-point checkbox and its label,
/// across its whole column. On the web the label holds the checkbox, so the label's words fill
/// the column, starting past the checkbox, and the checkbox stands over their start.
fn switch(label: &str, on: bool, press: Message) -> Element {
    let side = size::LOBBY_SWITCH_BOX;
    let mut words = text(label)
        .style(PANEL_LABEL)
        .color(color::TEXT_DIM)
        .grow(1.0);
    words.style.padding = [0.0, 0.0, 0.0, side + size::LOBBY_SWITCH_LABEL_GAP];
    let check = check_square(side, on).position(Position::Absolute).inset([
        Some((LABEL_LINE - side) / 2.0),
        None,
        None,
        Some(0.0),
    ]);
    Element::new(Role::Toggle)
        .name(label)
        .selected(on)
        .on_press(press)
        .child(words)
        .child(check)
}

/// A desktop game setting's label `label` over `control`, as the web's label holds its field
/// (`.lobby-panel label:has(input, select)`): the label's words are at least as wide as the
/// control, and stand the control's height and the gap above it taller than their line, with
/// the control in the bottom of that room.
fn labelled(label: &str, control: Element) -> Element {
    let height = control.style.height.unwrap_or_default();
    let mut words = text(label).style(PANEL_LABEL).color(color::TEXT_DIM);
    words.style.min_width = control.style.width;
    words.style.padding = [0.0, 0.0, size::LOBBY_LABEL_GAP + height, 0.0];
    let control = control
        .position(Position::Absolute)
        .inset([None, None, Some(0.0), Some(0.0)]);
    column().name(label).child(words).child(control)
}

/// A desktop game setting's field, as `.lobby-panel input` draws it: 16-point words in a box
/// padded 7 points above and below and 10 at the sides.
fn setting_field(name: &str, state: &FieldState, width: f32) -> Element {
    let mut setting = field(name, state);
    // An empty field shows the setting in force, which the web's field holds as its value, so
    // it is drawn in the value's color rather than faint.
    setting.style.color = Some(color::TYPE);
    setting.style.text = type_style::LOBBY_SETTING;
    let (y, x) = (size::LOBBY_SETTING_PADDING_Y, size::LOBBY_SETTING_PADDING_X);
    setting.style.padding = [y, x, y, x];
    setting.style.width = Some(width);
    setting.style.height = Some(size::LOBBY_SETTING_HEIGHT);
    setting
}

/// The host's game settings (`GameSettings.tsx`): a field for the starting life and one for the
/// starting hand size, each showing the setting while it is empty, and 'Save settings', which
/// is enabled while the fields ask for a change. A field that holds something other than a
/// number of its kind is drawn in ember. Who plays first is shown, not chosen: a solo table has
/// one seat, and a hosted table, which would have two, is not built yet.
fn settings(view: &LobbyView, input: &LobbyIn, fit: Fit) -> Element {
    let current = view.settings;
    let mut life = input.fields.get(STARTING_LIFE);
    let mut hand = input.fields.get(STARTING_HAND_SIZE);
    life.placeholder = current.starting_life.to_string();
    hand.placeholder = current.starting_hand_size.to_string();
    let change = asked_settings(current, &life.text, &hand.text)
        .ok()
        .flatten();
    let save = button("Save settings")
        .enabled(change.is_some())
        .on_press(Message::SaveSettings(change));
    if fit == Fit::Regular {
        // `.lobby-actions`: the two fields, who plays first and 'Save settings' in one row,
        // along its bottom.
        let controls = row()
            .name("game fields")
            .wrap(true)
            .gap(size::LOBBY_SECTION_GAP)
            .align_items(Align::End)
            .child(settings_number(
                "Starting life",
                STARTING_LIFE,
                &life,
                unreadable::<i32>(&life.text),
            ))
            .child(settings_number(
                "Starting hand size",
                STARTING_HAND_SIZE,
                &hand,
                unreadable::<u16>(&hand.text),
            ))
            .child(first_box(view))
            .child(save);
        return fit.section("Game").child(controls);
    }
    let fields = (fit.group("game fields"))
        .gap(space::L)
        .align_items(match fit {
            Fit::Regular => Align::Center,
            Fit::Column(_) => Align::Start,
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
    section("Game")
        .child(fields)
        .child(first(view))
        .child(fit.action(save.align_self(Align::Start)))
}

/// A desktop game setting's label over its number field `name`, holding `state`, drawn in ember
/// if `wrong`.
fn settings_number(label: &str, name: &str, state: &FieldState, wrong: bool) -> Element {
    let mut number = setting_field(name, state, size::LOBBY_SETTING_WIDTH);
    if wrong {
        number.style.border = Some(Border::uniform(color::EMBER, size::FIELD_BORDER));
        number.style.color = Some(color::EMBER);
    }
    labelled(label, number)
}

/// Who plays first on the desktop: 'First' over a box showing it, in the look of the web's
/// `select`. It is shown, not chosen, as `settings` says.
fn first_box(view: &LobbyView) -> Element {
    let mut shown = Element::new(Role::Region).name(&first_name(view));
    shown.text = Some(first_name(view));
    shown.style.text = type_style::LOBBY_SETTING;
    shown.style.color = Some(color::TYPE);
    let (y, x) = (size::LOBBY_SETTING_PADDING_Y, size::LOBBY_SETTING_PADDING_X);
    shown.style.padding = [y, x, y, x];
    shown.style.width = Some(size::LOBBY_FIRST_WIDTH);
    shown.style.height = Some(size::LOBBY_SETTING_HEIGHT);
    shown.style.background = Some(color::SURFACE_SUNK);
    shown.style.border = Some(Border::uniform(color::METAL, size::FIELD_BORDER));
    shown.style.radius = size::FIELD_RADIUS;
    labelled("First", shown)
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
fn settings_summary(view: &LobbyView, fit: Fit) -> Element {
    let settings = view.settings;
    fit.section("Game")
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
    text(&format!("First: {}", first_name(view)))
        .style(BODY)
        .color(color::TYPE)
}

/// The name of who plays first: the named seat, or by default a roll, or after a game the
/// loser's choice.
fn first_name(view: &LobbyView) -> String {
    match view.settings.first {
        Some(first) => (view.seats.iter())
            .find(|seat| seat.seat == first)
            .and_then(|seat| seat.name.clone())
            .unwrap_or_else(|| "Guest".to_owned()),
        None if view.last_winner.is_some() => "Loser chooses".to_owned(),
        None => "Roll".to_owned(),
    }
}

/// The host's 'Start', enabled when the table can start, or the guest's 'Ready', enabled once
/// their deck is loaded and selected while they are ready.
fn start(view: &LobbyView) -> Element {
    if hosting(view) {
        return primary("Start")
            .enabled(view.can_start)
            .on_press(Message::ChangeLobby(LobbyChange::Start));
    }
    let own = own(view);
    primary("Ready")
        .enabled(own.is_some_and(|own| own.loaded))
        .selected(own.is_some_and(|own| own.ready))
        .on_press(Message::ChangeLobby(LobbyChange::Ready {
            ready: !own.is_some_and(|own| own.ready),
        }))
}
