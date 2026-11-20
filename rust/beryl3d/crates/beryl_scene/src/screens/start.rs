//! The start screen, where a person picks an opponent and starts a game, as the web interface's
//! `StartScreen` (`ui/src/screens/StartScreen.tsx`, `start-panel.css`) shows it: one panel in
//! the middle of the window with the opponents, the seed, the fair-deal box and the buttons, the
//! manual tables' entries (`table_entries`) and the export of bug reports, and the sound button in
//! the corner.

use beryl_protocol::AgentView;
use beryl_protocol::protocol::Status;

use super::{Screen, ScreenIn, Sheet};
use crate::element::{Align, Border, Element, Justify, Position, Press, column, row, text};
use crate::frame::Role;
use crate::text::TypeStyle;
use crate::tokens::{color, ratio, size, space, type_style};
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
pub(super) const PANEL_WIDTH: f32 = size::START_PANEL_WIDTH;
pub(super) const PANEL_PADDING: [f32; 4] = [
    size::START_PANEL_PADDING_TOP,
    size::START_PANEL_PADDING_SIDE,
    size::START_PANEL_PADDING_BOTTOM,
    size::START_PANEL_PADDING_SIDE,
];
pub(super) const PANEL_GAP: f32 = size::START_PANEL_GAP;
pub(super) const PANEL_RADIUS: f32 = size::PANEL_RADIUS;

/// The height of everything in the window but the opponents' rows: the panel's other contents,
/// its padding and gaps, and a margin above and below it.
const AROUND_AGENTS: f32 = size::START_AROUND_AGENTS;

/// The least height the opponents' rows are given, about two rows.
const AGENTS_LEAST: f32 = size::START_AGENTS_LEAST;

/// The seed field's width (`.start-seed input`).
const SEED_WIDTH: f32 = size::START_SEED_WIDTH;

/// How far the sound button stands from the window's right and bottom edges (`.sound-corner`).
const CORNER: f32 = size::START_CORNER;

/// The radio dot's and the fair-deal box's sides.
const RADIO: f32 = size::START_RADIO;
const CHECKBOX: f32 = size::CHECKBOX;

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
    if super::phone(input) {
        return start_phone(input);
    }
    let form = input.start;
    let Chosen {
        going_on,
        seed_state,
        seed_ok,
        startable,
    } = Chosen::of(input);

    let heading = [
        text("New Game").style(TITLE).color(color::GOLD),
        text("WU Control mirror")
            .style(SUBTITLE)
            .color(color::TEXT_DIM)
            .margin([-size::START_SUBTITLE_PULL, 0.0, 0.0, 0.0]),
    ];
    let rows = input.agents.iter().enumerate().map(|(at, agent)| {
        let at = u16::try_from(at).unwrap_or(u16::MAX);
        agent_row(agent, at, form.agent == Some(at), false)
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
    let buttons = buttons(going_on, form.confirming, startable);
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
        .children(super::table_entries::entries(input, false))
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
    let screen = super::whole_window(input, screen);
    super::table_entries::with_host_key(with_keys(screen, input, going_on))
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
    fn of(input: &ScreenIn) -> Self {
        let going_on = in_progress(input.view);
        let mut seed_state = input.fields.get(SEED);
        "random".clone_into(&mut seed_state.placeholder);
        let seed_ok = seed(&seed_state.text).is_ok();
        let startable = input.start.agent.is_some() && seed_ok;
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
fn with_keys(screen: Element, input: &ScreenIn, going_on: bool) -> Element {
    let enter = if going_on {
        Press::Open(Screen::Game)
    } else {
        Press::Start
    };
    let screen = screen
        .shortcut("Enter", enter)
        .shortcut("G", Press::Open(Screen::Gallery));
    if input.recordings.is_empty() {
        screen
    } else {
        screen.shortcut("T", Press::Open(Screen::Testing))
    }
}

/// The start screen on a phone, as the web's `.phone-start-screen` lays it out: one column
/// over the safe area with the sound button at its top, the opponents in a list that scrolls
/// and takes the room the rest leaves, then the seed, the fair-deal box, the start buttons one
/// above the other, the solo table and the export of bug reports at its foot. There is no
/// title, as on the web's phone. When the window is too short for even one opponent's row (the
/// system keyboard is up), the whole column scrolls instead, so every action stays in reach.
fn start_phone(input: &ScreenIn) -> Element {
    use crate::data::space::flat::phone::start as metrics;
    let form = input.start;
    let Chosen {
        going_on,
        seed_state,
        seed_ok,
        startable,
    } = Chosen::of(input);
    let target = crate::data::audit::PHONE_TARGET_POINTS;
    let sound = icon_button(widgets::icons::SPEAKER, "Sound")
        .width(target)
        .height(target)
        .radius(target / 2.0)
        .on_press(Press::Sheet(Some(Sheet::Settings)));
    let corner = row()
        .name("sound corner")
        .height(metrics::CORNER)
        .shrink(0.0)
        .align_items(Align::Center)
        .justify_content(Justify::End)
        .child(sound);
    let legend = text("OPPONENT")
        .style(TypeStyle {
            size: metrics::LABEL,
            ..LABEL
        })
        .color(color::TEXT_DIM)
        .shrink(0.0);
    let rows = input.agents.iter().enumerate().map(|(at, agent)| {
        let at = u16::try_from(at).unwrap_or(u16::MAX);
        agent_row(agent, at, form.agent == Some(at), true)
    });
    let agents = list("Opponent", rows)
        .gap(metrics::AGENT_GAP)
        .shrink(1.0)
        .min_height(metrics::AGENTS_LEAST);
    let mut seed_row = seed_row(&seed_state, seed_ok);
    seed_row.style.height = Some(metrics::FIELD);
    seed_row.style.shrink = 0.0;
    if let Some(field) = seed_row.children.last_mut() {
        field.style.width = None;
        field.style.grow = 1.0;
        field.style.align_self = Some(Align::Stretch);
    }
    let fair = fair_box(form.fair).min_height(metrics::FIELD).shrink(0.0);
    let (start, back) = start_buttons(going_on, form.confirming, startable);
    let export = button("Export Bug Reports").on_press(Press::ExportBugs);
    let content = column()
        .name("start column")
        .gap(metrics::GAP)
        .child(corner)
        .child(legend)
        .child(agents)
        // The rest stands at the foot of the column, as the web's `margin-top: auto` puts it.
        .child(row().name("space").grow(1.0))
        .child(seed_row)
        .child(fair)
        .child(super::phone_action(start))
        .children(back.map(super::phone_action))
        .children(super::table_entries::entries(input, true))
        .child(super::phone_action(export));
    with_keys(
        super::phone_screen("start", input, content),
        input,
        going_on,
    )
}

/// The seed's label and text field, holding `state`, drawn in ember unless `ok`.
fn seed_row(state: &FieldState, ok: bool) -> Element {
    let mut seed_field = field(SEED, state);
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
fn buttons(going_on: bool, confirming: bool, startable: bool) -> Element {
    let (start, back) = start_buttons(going_on, confirming, startable);
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
fn start_buttons(going_on: bool, confirming: bool, startable: bool) -> (Element, Option<Element>) {
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
    let back = going_on.then(|| primary("Return to Game").on_press(Press::Open(Screen::Game)));
    (start.enabled(startable).on_press(Press::Start), back)
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
fn agent_row(agent: &AgentView, at: u16, chosen: bool, phone: bool) -> Element {
    use crate::data::space::flat::phone::start as metrics;
    // The phone's rows are taller, with a larger dot and smaller type (`.phone-start-screen
    // .start-agent`), so each row is a touch target of its own.
    let radio = if phone { metrics::RADIO } else { RADIO };
    let (agent_name, about, cheating_note) = if phone {
        (
            TypeStyle {
                size: metrics::AGENT_NAME,
                ..AGENT_NAME
            },
            TypeStyle {
                size: metrics::ABOUT,
                ..ABOUT
            },
            TypeStyle {
                size: metrics::ABOUT,
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
            if phone { 0.0 } else { size::START_RADIO_DROP },
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
        .on_press(Press::Agent(at))
        .child(dot)
        .child(words);
    element.style.padding = if phone {
        [
            metrics::AGENT_PADDING_Y,
            metrics::AGENT_PADDING_X,
            metrics::AGENT_PADDING_Y,
            metrics::AGENT_PADDING_X,
        ]
    } else {
        [
            size::START_AGENT_PADDING_Y,
            size::START_AGENT_PADDING_X,
            size::START_AGENT_PADDING_Y,
            size::START_AGENT_PADDING_X,
        ]
    };
    if phone {
        element = element
            .min_height(metrics::AGENT_ROW)
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
    checkbox("Fair deal", on, Press::FairDeal)
}

/// A checkbox labelled `label`, ticked if `on`, which means `press` when pressed: the start
/// screen's "Fair deal" box and the lobby's assists.
pub fn checkbox(label: &str, on: bool, press: Press) -> Element {
    let check = row()
        .name("checkbox")
        .width(CHECKBOX)
        .height(CHECKBOX)
        .radius(size::START_CHECKBOX_RADIUS)
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
                .radius(size::START_TICK_RADIUS)
                .background(color::GOLD)
        }));
    Element::new(Role::Toggle)
        .name(label)
        .selected(on)
        .gap(space::M)
        .align_items(Align::Center)
        .on_press(press)
        .child(check)
        .child(text(label).style(LABEL).color(color::TEXT_DIM))
}

/// `button` at the web's small size (`button-small`): shorter, with smaller type.
pub fn small(mut button: Element) -> Element {
    button.style.height = Some(size::BUTTON_HEIGHT * ratio::SMALL_SCALE);
    button.style.text.size = type_style::BUTTON.size * ratio::SMALL_SCALE;
    button.style.min_width = None;
    button
}
