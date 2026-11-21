//! The start screen, where a person picks an opponent and starts a game, as the web interface's
//! `StartScreen` (`ui/src/screens/StartScreen.tsx`, `start-panel.css`) shows it: one panel in
//! the middle of the window with the opponents, the seed, the fair-deal box and the buttons, the
//! manual tables' entries (`table_entries`) and the export of bug reports, and the sound button in
//! the corner.

use beryl_protocol::AgentView;
use beryl_protocol::SessionId;

use super::super::bug_report;
use super::super::settings::Message as SettingsMessage;
use super::{Inputs, Message, Screen, State, seed};
use kit_base::tokens::{color, ratio, size, space, type_style};
use kit_frame::frame::Role;
use kit_text::text::TypeStyle;
use kit_ui::element::{Align, Border, Element, Justify, Position, column, row, text};
use kit_ui::widgets::screen::{ScreenColumn, whole_window};
use kit_ui::widgets::start_panel::{
    PANEL_GAP, PANEL_PADDING, PANEL_WIDTH, checkbox, panel_look, small,
};
use kit_ui::widgets::{self, FieldState, button, field, icon_button, list, primary, tooltip};

/// The agents that play on the true state, hidden cards included, which the screen marks as
/// cheating: the web's `CHEATING`.
const CHEATING: [&str; 2] = ["mcts-omniscient-quick", "pimc18-omniscient-30m-anchor"];

/// The height of everything in the window but the opponents' rows: the panel's other contents,
/// its padding and gaps, and a margin above and below it.
const AROUND_AGENTS: f32 = size::START_AROUND_AGENTS;

/// The least height the opponents' rows are given, about two rows.
const AGENTS_LEAST: f32 = size::START_AGENTS_LEAST;

/// The seed field's width (`.start-seed input`).
const SEED_WIDTH: f32 = size::START_SEED_WIDTH;

/// How far the sound button stands from the window's right and bottom edges (`.sound-corner`).
const CORNER: f32 = size::START_CORNER;

/// The opponent radio dot's side.
const RADIO: f32 = size::START_RADIO;

/// `.start-title`: bold 38-point serif.
pub(super) const TITLE: TypeStyle = type_style::START_TITLE;

/// `.start-subtitle`: 19-point serif.
const SUBTITLE: TypeStyle = type_style::START_SUBTITLE;

/// `.start-agents legend`, `.start-seed` and `.start-fair`: semibold 15-point sans.
pub(super) const LABEL: TypeStyle = type_style::START_LABEL;

/// `.start-agent-name`: semibold 18-point serif.
const AGENT_NAME: TypeStyle = type_style::START_AGENT_NAME;

/// `.start-cheating`: 14-point serif.
const CHEATING_NOTE: TypeStyle = type_style::START_CHEATING_NOTE;

/// `.start-agent-about`: 14-point sans.
const ABOUT: TypeStyle = type_style::START_ABOUT;

/// What the start screen shows, including the testing shortcut when recordings are available.
#[must_use]
pub(super) fn start(state: &State, input: &Inputs) -> Element {
    if let Some(layout) = input.column {
        return start_column(state, input, layout);
    }
    let Chosen {
        going_on,
        seed_state,
        seed_ok,
        startable,
    } = Chosen::of(state, input);

    let heading = [
        text("New Game").style(TITLE).color(color::GOLD),
        text("WU Control mirror")
            .style(SUBTITLE)
            .color(color::TEXT_DIM)
            .margin([-size::START_SUBTITLE_PULL, 0.0, 0.0, 0.0]),
    ];
    let rows = state.agents.iter().enumerate().map(|(at, agent)| {
        let at = u16::try_from(at).unwrap_or(u16::MAX);
        agent_row(agent, at, state.agent == Some(at), None)
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
                .margin([0.0, 0.0, size::START_LEGEND_GAP, 0.0]),
        )
        .child(list("Opponent", rows).gap(space::S).max_height(room));
    let seed_row = seed_row(&seed_state, seed_ok);
    let buttons = buttons(going_on, state.confirming, startable, input.session);
    let export = small(button("Export Bug Reports")).on_press(bug_report::Message::Export);
    let export_tip = tooltip(
        "Includes screenshots, replays, and logs",
        "Export Bug Reports",
    );

    let panel = column()
        .name("start panel")
        .width(PANEL_WIDTH)
        .align_items(Align::Center)
        .gap(PANEL_GAP);
    let panel = panel_look(panel)
        .children(heading)
        .child(agents)
        .child(seed_row)
        .child(fair_box(state.fair))
        .child(buttons)
        .children(game_manual::screens::table_entries::entries(
            input.entries,
            input.fields,
            None,
        ))
        .child(export)
        .child(export_tip);
    let mut panel = panel;
    panel.style.padding = PANEL_PADDING;

    // The layer draws a screen's top element as a panel unless its style says otherwise; this
    // one fills the window and only holds the panel and the corner, so it is clear.
    let screen = column()
        .name("start")
        .background([0; 4])
        .border(([0; 4], 0.0))
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .child(panel)
        .child(corner());
    let screen = whole_window(input.window.size, screen);
    game_manual::screens::table_entries::with_host_key(with_keys(screen, input, going_on))
}

/// What the screen's fields and choices allow, which both layouts read the same way.
struct Chosen {
    /// Whether an engine game goes on, which a new game concedes first.
    going_on: bool,
    /// The seed field, with its placeholder.
    seed_state: FieldState,
    /// Whether the seed field holds a seed or nothing.
    seed_ok: bool,
    /// Whether the start button can be pressed: an opponent is chosen and the seed is good.
    startable: bool,
}

impl Chosen {
    fn of(state: &State, input: &Inputs) -> Self {
        let going_on = input.in_progress;
        let mut seed_state = input.fields.get(super::SEED);
        "random".clone_into(&mut seed_state.placeholder);
        let seed_ok = seed(&seed_state.text).is_ok();
        let startable = state.agent.is_some() && seed_ok;
        Self {
            going_on,
            seed_state,
            seed_ok,
            startable,
        }
    }
}

/// `screen` with the start screen's keys: Enter does what the primary button does (back to the
/// game in progress, else a new one), G opens the gallery and, in a tools build of the app,
/// which lists the recordings, T opens the testing screen.
fn with_keys(screen: Element, input: &Inputs, going_on: bool) -> Element {
    let enter = if going_on {
        Message::Open(Screen::Game)
    } else {
        Message::Start(input.session)
    };
    let screen = screen
        .shortcut("Enter", enter)
        .shortcut("G", Message::Open(Screen::Gallery));
    if input.has_recordings {
        screen.shortcut("T", Message::Open(Screen::Testing))
    } else {
        screen
    }
}

/// The start screen on a phone, as the web's `.phone-start-screen` lays it out: one column
/// over the safe area with the sound button at its top, the opponents in a list that scrolls
/// and takes the room the rest leaves, then the seed, the fair-deal box, the start buttons one
/// above the other, the solo table and the export of bug reports at its foot. There is no
/// title, as on the web's phone. When the window is too short for even one opponent's row (the
/// system keyboard is up), the whole column scrolls instead, so every action stays in reach.
fn start_column(state: &State, input: &Inputs, layout: ScreenColumn) -> Element {
    let metrics = layout.metrics.start;
    let Chosen {
        going_on,
        seed_state,
        seed_ok,
        startable,
    } = Chosen::of(state, input);
    let target = layout.target;
    let sound = icon_button(widgets::icons::SPEAKER, "Sound")
        .width(target)
        .height(target)
        .radius(target / 2.0)
        .on_press(SettingsMessage::Open);
    let corner = row()
        .name("sound corner")
        .height(metrics.corner)
        .shrink(0.0)
        .align_items(Align::Center)
        .justify_content(Justify::End)
        .child(sound);
    let legend = text("OPPONENT")
        .style(TypeStyle {
            size: metrics.label,
            ..LABEL
        })
        .color(color::TEXT_DIM)
        .shrink(0.0);
    let rows = state.agents.iter().enumerate().map(|(at, agent)| {
        let at = u16::try_from(at).unwrap_or(u16::MAX);
        agent_row(agent, at, state.agent == Some(at), Some(metrics))
    });
    let agents = list("Opponent", rows)
        .gap(metrics.agent_gap)
        .shrink(1.0)
        .min_height(metrics.agents_least);
    let mut seed_row = seed_row(&seed_state, seed_ok);
    seed_row.style.height = Some(metrics.field);
    seed_row.style.shrink = 0.0;
    if let Some(field) = seed_row.children.last_mut() {
        field.style.width = None;
        field.style.grow = 1.0;
        field.style.align_self = Some(Align::Stretch);
    }
    let fair = fair_box(state.fair).min_height(metrics.field).shrink(0.0);
    let (start, back) = start_buttons(going_on, state.confirming, startable, input.session);
    let export = button("Export Bug Reports").on_press(bug_report::Message::Export);
    let content = column()
        .name("start column")
        .gap(metrics.gap)
        .child(corner)
        .child(legend)
        .child(agents)
        // The rest stands at the foot of the column, as the web's `margin-top: auto` puts it.
        .child(row().name("space").grow(1.0))
        .child(seed_row)
        .child(fair)
        .child(layout.action(start))
        .children(back.map(|back| layout.action(back)))
        .children(game_manual::screens::table_entries::entries(
            input.entries,
            input.fields,
            Some(layout),
        ))
        .child(layout.action(export));
    with_keys(
        layout.scrolling("start", input.window, content),
        input,
        going_on,
    )
}

/// The seed's label and text field, holding `state`, drawn in ember unless `ok`.
fn seed_row(state: &FieldState, ok: bool) -> Element {
    let mut seed_field = field(super::SEED, state);
    seed_field.style.width = Some(SEED_WIDTH);
    seed_field.style.padding = [
        size::START_SEED_PADDING_Y,
        size::START_SEED_PADDING_X,
        size::START_SEED_PADDING_Y,
        size::START_SEED_PADDING_X,
    ];
    if !ok {
        seed_field.style.border = Some(Border::uniform(color::EMBER, size::BORDER));
        seed_field.style.color = Some(color::EMBER);
    }
    row()
        .name("Seed")
        .align_items(Align::Center)
        .gap(size::START_SEED_GAP)
        .child(text("Seed").style(LABEL).color(color::TEXT_DIM))
        .child(seed_field)
}

/// The start button, enabled if `startable`, and with a game `going_on`, the button back to it
/// before it (`.start-buttons`). The start button's words say what it will do: start, ask to
/// confirm a new game, or once `confirming`, concede and start.
fn buttons(
    going_on: bool,
    confirming: bool,
    startable: bool,
    session: Option<SessionId>,
) -> Element {
    let (start, back) = start_buttons(going_on, confirming, startable, session);
    // With a game in progress, going back to it comes first.
    row()
        .name("buttons")
        .gap(size::START_BUTTONS_GAP)
        .children(back)
        .child(start)
}

/// The start button and, with a game `going_on`, the button back to it. The start button's
/// words say what it will do: start, ask to confirm a new game, or once `confirming`, concede
/// and start. Going back to a game in progress is the primary button then.
fn start_buttons(
    going_on: bool,
    confirming: bool,
    startable: bool,
    session: Option<SessionId>,
) -> (Element, Option<Element>) {
    let label = if !going_on {
        "Start"
    } else if confirming {
        "Concede & Start"
    } else {
        "New Game"
    };
    let start = if going_on {
        button(label)
    } else {
        primary(label)
    };
    let back = going_on.then(|| primary("Return to Game").on_press(Message::Open(Screen::Game)));
    (
        start.enabled(startable).on_press(Message::Start(session)),
        back,
    )
}

/// The sound button in the window's corner (`SoundCorner`), which opens the settings.
fn corner() -> Element {
    let sound = icon_button(widgets::icons::SPEAKER, "Sound").on_press(SettingsMessage::Open);
    row()
        .name("sound corner")
        .position(Position::Absolute)
        .inset([None, Some(CORNER), Some(CORNER), None])
        .child(sound)
}

/// The row that offers `agent`, the `at`th the host listed, as one of a group of radio
/// buttons (`.start-agent`): a dot, the agent's title, a note if it cheats, and what it does
/// under the title. The chosen row is tinted gold.
fn agent_row(
    agent: &AgentView,
    at: u16,
    chosen: bool,
    compact: Option<kit_base::data::profiles::StartScreen>,
) -> Element {
    // The phone's rows are taller, with a larger dot and smaller type (`.phone-start-screen
    // .start-agent`), so each row is a touch target of its own.
    let radio = compact.map_or(RADIO, |metrics| metrics.radio);
    let (agent_name, about, cheating_note) = if let Some(metrics) = compact {
        (
            TypeStyle {
                size: metrics.agent_name,
                ..AGENT_NAME
            },
            TypeStyle {
                size: metrics.about,
                ..ABOUT
            },
            TypeStyle {
                size: metrics.about,
                ..CHEATING_NOTE
            },
        )
    } else {
        (AGENT_NAME, ABOUT, CHEATING_NOTE)
    };
    let mut dot = row()
        .name("radio")
        .width(radio)
        .height(radio)
        .radius(radio / 2.0)
        .shrink(0.0)
        .border((color::GOLD, size::BORDER))
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .margin([
            compact.map_or(size::START_RADIO_DROP, |_| 0.0),
            0.0,
            0.0,
            0.0,
        ]);
    if chosen {
        dot = dot.child(
            row()
                .name("dot")
                .width(radio / 2.0)
                .height(radio / 2.0)
                .radius(radio * ratio::QUARTER)
                .background(color::GOLD),
        );
    }
    let cheats = CHEATING.contains(&agent.name.as_str());
    let title = row()
        .name("title")
        .gap(space::M)
        .align_items(Align::End)
        .child(text(&agent.title).style(agent_name).color(color::TYPE))
        .children(cheats.then(|| {
            text("cheating: sees your hand")
                .style(cheating_note)
                .color(color::EMBER)
        }));
    let words = column()
        .name("words")
        .grow(1.0)
        .shrink(1.0)
        .child(title)
        .child(text(&agent.description).style(about).color(color::TEXT_DIM));
    let mut element = Element::new(Role::Toggle)
        .name(&agent.title)
        .selected(chosen)
        .gap(size::START_AGENT_GAP)
        .radius(size::START_AGENT_RADIUS)
        .align_items(Align::Start)
        .on_press(Message::Agent(at))
        .child(dot)
        .child(words);
    element.style.padding = if let Some(metrics) = compact {
        [
            metrics.agent_padding_y,
            metrics.agent_padding_x,
            metrics.agent_padding_y,
            metrics.agent_padding_x,
        ]
    } else {
        [
            size::START_AGENT_PADDING_Y,
            size::START_AGENT_PADDING_X,
            size::START_AGENT_PADDING_Y,
            size::START_AGENT_PADDING_X,
        ]
    };
    if let Some(metrics) = compact {
        element = element
            .min_height(metrics.agent_row)
            .align_items(Align::Center)
            .shrink(0.0);
    }
    if chosen {
        element = element
            .background(color::MENU_ACTIVE)
            .border((color::GOLD_SOFT, 1.0));
    }
    element
}

/// The "Fair deal" box (`.start-fair`): a checkbox and its label, which a press turns on or off.
fn fair_box(on: bool) -> Element {
    checkbox("Fair deal", on).on_press(Message::FairDeal)
}
