//! The start screen, where a person picks an opponent and starts a game, as the web interface's
//! `StartScreen` (`ui/src/screens/StartScreen.tsx`, `start-panel.css`) shows it: one panel in
//! the middle of the window with the opponents, the seed, the fair-deal box and the buttons, the
//! solo table's entry and the export of bug reports, and the sound button in the corner.

use beryl_protocol::AgentView;
use beryl_protocol::protocol::Status;

use super::{Screen, ScreenIn, Sheet};
use crate::element::{Align, Element, Justify, LobbyPress, Position, Press, column, row, text};
use crate::frame::Role;
use crate::text::TypeStyle;
use crate::text::fonts::Font;
use crate::tokens::{color, size, type_style};
use crate::widgets::{self, FieldState, button, field, icon_button, list, primary, tooltip};

/// The name of the seed's text field.
pub const SEED: &str = "seed";

/// The agents that play on the true state, hidden cards included, which the screen marks as
/// cheating: the web's `CHEATING`.
const CHEATING: [&str; 2] = ["mcts-omniscient-quick", "pimc18-omniscient-30m-anchor"];

/// The opponent offered first, the web's `DEFAULT_AGENT`: strong, and answers in about two
/// seconds.
const DEFAULT_AGENT: &str = "mcts-hidden";

/// The panel's width and padding (`.start-panel`).
pub(super) const PANEL_WIDTH: f32 = 620.0;
pub(super) const PANEL_PADDING: [f32; 4] = [28.0, 36.0, 32.0, 36.0];
pub(super) const PANEL_GAP: f32 = 18.0;
pub(super) const PANEL_RADIUS: f32 = 12.0;

/// The height of everything in the window but the opponents' rows: the panel's other contents,
/// its padding and gaps, and a margin above and below it.
const AROUND_AGENTS: f32 = 470.0;

/// The least height the opponents' rows are given, about two rows.
const AGENTS_LEAST: f32 = 130.0;

/// The seed field's width (`.start-seed input`).
const SEED_WIDTH: f32 = 160.0;

/// How far the sound button stands from the window's right and bottom edges (`.sound-corner`).
const CORNER: f32 = 18.0;

/// The radio dot's and the fair-deal box's sides.
const RADIO: f32 = 16.0;
const CHECKBOX: f32 = 18.0;

/// `.start-title`: bold 38-point serif.
pub(super) const TITLE: TypeStyle = TypeStyle {
    font: Font::CrimsonPro,
    weight: 700,
    size: 38.0,
    line_height: 1.2,
};

/// `.start-subtitle`: 19-point serif.
const SUBTITLE: TypeStyle = TypeStyle {
    font: Font::CrimsonPro,
    weight: 400,
    size: 19.0,
    line_height: 1.2,
};

/// `.start-agents legend`, `.start-seed` and `.start-fair`: semibold 15-point sans.
pub(super) const LABEL: TypeStyle = TypeStyle {
    font: Font::OpenSans,
    weight: 600,
    size: 15.0,
    line_height: 1.3,
};

/// `.start-agent-name`: semibold 18-point serif.
const AGENT_NAME: TypeStyle = TypeStyle {
    font: Font::CrimsonPro,
    weight: 600,
    size: 18.0,
    line_height: 1.25,
};

/// `.start-cheating`: 14-point serif.
const CHEATING_NOTE: TypeStyle = TypeStyle {
    font: Font::CrimsonPro,
    weight: 400,
    size: 14.0,
    line_height: 1.25,
};

/// `.start-agent-about`: 14-point sans.
const ABOUT: TypeStyle = TypeStyle {
    font: Font::OpenSans,
    weight: 400,
    size: 14.0,
    line_height: 1.35,
};

/// What the person has chosen on the start screen besides the seed, which is a text field the
/// client keeps in `Fields`. The client keeps it so that the choices stay while the screen is
/// built again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartForm {
    /// The chosen opponent, counting the host's agents from 0, once they have arrived.
    pub agent: Option<u16>,
    /// Whether the game is dealt the deal the AI judges fairest of many.
    pub fair: bool,
    /// Whether a new game was asked for with one in progress, so that the next press of the
    /// start button concedes that game and starts the new one.
    pub confirming: bool,
}

impl StartForm {
    /// The opponent chosen first among `agents`: the web's default agent if the host offers
    /// it, or else the first.
    #[must_use]
    pub fn first_agent(agents: &[AgentView]) -> Option<u16> {
        if agents.is_empty() {
            return None;
        }
        let at = (agents.iter())
            .position(|agent| agent.name == DEFAULT_AGENT)
            .unwrap_or(0);
        u16::try_from(at).ok()
    }
}

/// What the seed field holds when it is neither blank nor a seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotASeed;

/// The seed a seed field holding `words` asks for: `Ok(None)` while it is blank, which deals
/// from the clock, and an error unless it is a whole number from 0 to 2^32 - 1.
///
/// # Errors
///
/// When `words` is neither blank nor such a number.
pub fn seed(words: &str) -> Result<Option<u32>, NotASeed> {
    let words = words.trim();
    if words.is_empty() {
        return Ok(None);
    }
    words.parse().map(Some).map_err(|_| NotASeed)
}

/// Whether `view` is of an engine game that goes on, which a new game concedes first: one
/// waiting on a player or playing through a step. A game that is over, a session that stopped
/// and the manual table (`Status::Free`) are not.
#[must_use]
pub fn in_progress(view: Option<&beryl_protocol::PlayerView>) -> bool {
    view.is_some_and(|view| {
        matches!(
            view.status,
            Status::Deciding { .. } | Status::Thinking | Status::Playing
        )
    })
}

/// What the flat layer shows on the screen. In a tools build of the app, which lists the
/// recordings (`ScreenIn::recordings`), the key T opens the testing screen.
#[must_use]
pub fn start(input: &ScreenIn) -> Element {
    let form = input.start;
    let going_on = in_progress(input.view);
    let mut seed_state = input.fields.get(SEED);
    "random".clone_into(&mut seed_state.placeholder);
    let seed_ok = seed(&seed_state.text).is_ok();
    let startable = form.agent.is_some() && seed_ok;

    let heading = [
        text("New Game").style(TITLE).color(color::GOLD),
        text("WU Control mirror")
            .style(SUBTITLE)
            .color(color::TEXT_DIM)
            .margin([-10.0, 0.0, 0.0, 0.0]),
    ];
    let rows = input.agents.iter().enumerate().map(|(at, agent)| {
        let at = u16::try_from(at).unwrap_or(u16::MAX);
        agent_row(agent, at, form.agent == Some(at))
    });
    // The host offers a dozen agents, more than a short window has room for, so the rows
    // scroll inside what the rest of the panel leaves.
    let room = (input.window.size.height - AROUND_AGENTS).max(AGENTS_LEAST);
    let agents = column()
        .name("opponents")
        .align_self(Align::Stretch)
        .child(
            text("OPPONENT")
                .style(LABEL)
                .color(color::TEXT_DIM)
                .margin([0.0, 0.0, 6.0, 0.0]),
        )
        .child(list("Opponent", rows).gap(4.0).max_height(room));
    let seed_row = seed_row(&seed_state, seed_ok);
    let buttons = buttons(going_on, form.confirming, startable);
    let solo = button("Solo table").on_press(Press::Lobby(LobbyPress::Solo));
    let export = small(button("Export Bug Reports")).on_press(Press::ExportBugs);
    let export_tip = tooltip(
        "Includes screenshots, replays, and logs",
        "Export Bug Reports",
    );

    let panel = column()
        .name("start panel")
        .width(PANEL_WIDTH)
        .align_items(Align::Center)
        .gap(PANEL_GAP)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(PANEL_RADIUS)
        .children(heading)
        .child(agents)
        .child(seed_row)
        .child(fair_box(form.fair))
        .child(buttons)
        .child(solo)
        .child(export)
        .child(export_tip);
    let mut panel = panel;
    panel.style.padding = PANEL_PADDING;

    // Enter does what the primary button does: back to the game in progress, else a new one.
    let enter = if going_on {
        Press::Open(Screen::Game)
    } else {
        Press::Start
    };
    // The layer draws a screen's top element as a panel unless its style says otherwise; this
    // one fills the window and only holds the panel and the corner, so it is clear.
    let screen = column()
        .name("start")
        .background([0; 4])
        .border(([0; 4], 0.0))
        .grow(1.0)
        .align_self(Align::Stretch)
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .child(panel)
        .child(corner())
        .shortcut("Enter", enter)
        .shortcut("G", Press::Open(Screen::Gallery));
    // In a tools build of the app, which lists the recordings, the key T opens the testing
    // screen.
    if input.recordings.is_empty() {
        screen
    } else {
        screen.shortcut("T", Press::Open(Screen::Testing))
    }
}

/// The seed's label and text field, holding `state`, drawn in ember unless `ok`.
fn seed_row(state: &FieldState, ok: bool) -> Element {
    let mut seed_field = field(SEED, state);
    seed_field.style.width = Some(SEED_WIDTH);
    seed_field.style.padding = [6.0, 10.0, 6.0, 10.0];
    if !ok {
        seed_field.style.border = Some((color::EMBER, size::BORDER));
        seed_field.style.color = Some(color::EMBER);
    }
    row()
        .name("Seed")
        .align_items(Align::Center)
        .gap(12.0)
        .child(text("Seed").style(LABEL).color(color::TEXT_DIM))
        .child(seed_field)
}

/// The start button, enabled if `startable`, and with a game `going_on`, the button back to it
/// before it (`.start-buttons`). The start button's words say what it will do: start, ask to
/// confirm a new game, or once `confirming`, concede and start.
fn buttons(going_on: bool, confirming: bool, startable: bool) -> Element {
    let label = if !going_on {
        "Start"
    } else if confirming {
        "Concede & Start"
    } else {
        "New Game"
    };
    // With a game in progress, going back to it comes first.
    let start = if going_on {
        button(label)
    } else {
        primary(label)
    };
    row()
        .name("buttons")
        .gap(14.0)
        .children(going_on.then(|| primary("Return to Game").on_press(Press::Open(Screen::Game))))
        .child(start.enabled(startable).on_press(Press::Start))
}

/// The sound button in the window's corner (`SoundCorner`), which opens the settings.
fn corner() -> Element {
    let sound =
        icon_button(widgets::icons::SPEAKER, "Sound").on_press(Press::Sheet(Some(Sheet::Settings)));
    row()
        .name("sound corner")
        .position(Position::Absolute)
        .inset([None, Some(CORNER), Some(CORNER), None])
        .child(sound)
}

/// The row that offers `agent`, the `at`th the host listed, as one of a group of radio
/// buttons (`.start-agent`): a dot, the agent's title, a note if it cheats, and what it does
/// under the title. The chosen row is tinted gold.
fn agent_row(agent: &AgentView, at: u16, chosen: bool) -> Element {
    let mut dot = row()
        .name("radio")
        .width(RADIO)
        .height(RADIO)
        .radius(RADIO / 2.0)
        .border((color::GOLD, size::BORDER))
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .margin([3.0, 0.0, 0.0, 0.0]);
    if chosen {
        dot = dot.child(
            row()
                .name("dot")
                .width(RADIO / 2.0)
                .height(RADIO / 2.0)
                .radius(RADIO / 4.0)
                .background(color::GOLD),
        );
    }
    let cheats = CHEATING.contains(&agent.name.as_str());
    let title = row()
        .name("title")
        .gap(10.0)
        .align_items(Align::End)
        .child(text(&agent.title).style(AGENT_NAME).color(color::TYPE))
        .children(cheats.then(|| {
            text("cheating: sees your hand")
                .style(CHEATING_NOTE)
                .color(color::EMBER)
        }));
    let words = column()
        .name("words")
        .grow(1.0)
        .shrink(1.0)
        .child(title)
        .child(text(&agent.description).style(ABOUT).color(color::TEXT_DIM));
    let mut element = Element::new(Role::Toggle)
        .name(&agent.title)
        .selected(chosen)
        .gap(8.0)
        .radius(8.0)
        .align_items(Align::Start)
        .on_press(Press::Agent(at))
        .child(dot)
        .child(words);
    element.style.padding = [8.0, 10.0, 8.0, 10.0];
    if chosen {
        element = element
            .background(color::MENU_ACTIVE)
            .border((color::GOLD_SOFT, 1.0));
    }
    element
}

/// The "Fair deal" box (`.start-fair`): a checkbox and its label, which a press turns on or off.
fn fair_box(on: bool) -> Element {
    checkbox("Fair deal", on, Press::FairDeal)
}

/// A checkbox labelled `label`, ticked if `on`, which means `press` when pressed: the start
/// screen's "Fair deal" box and the lobby's assists.
pub(super) fn checkbox(label: &str, on: bool, press: Press) -> Element {
    let check = row()
        .name("checkbox")
        .width(CHECKBOX)
        .height(CHECKBOX)
        .radius(3.0)
        .border((color::GOLD, size::BORDER))
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        // No bundled font has a check mark, so a ticked box is filled with a gold square, as the
        // chosen opponent's radio button is filled with a dot.
        .children(on.then(|| {
            row()
                .name("tick")
                .width(CHECKBOX / 2.0)
                .height(CHECKBOX / 2.0)
                .radius(1.5)
                .background(color::GOLD)
        }));
    Element::new(Role::Toggle)
        .name(label)
        .selected(on)
        .gap(10.0)
        .align_items(Align::Center)
        .on_press(press)
        .child(check)
        .child(text(label).style(LABEL).color(color::TEXT_DIM))
}

/// `button` at the web's small size (`button-small`): shorter, with smaller type.
fn small(mut button: Element) -> Element {
    button.style.height = Some(size::BUTTON_HEIGHT * 0.75);
    button.style.text.size = type_style::BUTTON.size * 0.75;
    button.style.min_width = None;
    button
}
