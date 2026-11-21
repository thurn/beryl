//! The game screen: the flat layer over the table while a game goes on, from its opening hand
//! to its end. Each piece of it is built in a file of its own (`hud`, `inspect`, `prompts`
//! and this module's `opening` and `game_over`), and this function puts them together. The
//! inspector and the Revealed Cards pill are not among them: the client shows each as a float
//! (`inspect::inspector`, `inspect::revealed`).

use glam::Vec2;

use super::{game_over, opening};
use crate::element::{Anchor, Element, Shortcuts, holder, row};
use crate::hud::{HudIn, full_control, log, phone, players, steps};
use crate::inspect::viewer;
use crate::prompts;
use crate::table::phone_bands;
use kit_base::tokens::ratio;

/// What the flat layer shows over a game. While none of its pieces shows anything, it is the
/// open prompt's element alone, as it has been since the prompt bar was the only thing in the
/// flat layer; with no prompt open, it is an empty region named "game".
#[must_use]
pub fn game(input: &HudIn) -> Element {
    // Play or draw stands in the opening, not in the prompt bar.
    let prompt = waiting(input).filter(|_| !opening::choosing_start(input));
    // The orbs, the steps and the log stay up while scripts play, as the web interface's
    // `Chrome` does; only what waits for the table (`Element::waits`) is held back. The
    // opening does not wait: the web shows it, with the coin flip, while the hands are dealt.
    // The life orbs and the step stops stand under its backdrop, which dims them, as the web's
    // overlay is drawn over its `Hud`; full control's sign is hidden under it.
    let shown = opening::shown(input);
    let hud = [
        players::players(input),
        game_steps(input),
        full_control::full_control(input).filter(|_| !shown),
    ];
    let over = [
        log::log(input),
        viewer::viewer(input),
        // The result blurs everything drawn before it, the floats too (`Style::backdrop`).
        game_over::game_over(input).map(Element::waits),
    ];
    let opening = opening::opening(input);
    if hud.iter().chain(&over).all(Option::is_none) && opening.is_none() {
        return prompt.unwrap_or_else(|| row().name("game"));
    }
    // The flat layer draws in tree order, so the opening, whose scrim dims the whole window,
    // comes before the prompt, whose buttons would otherwise be drawn under it and look
    // disabled. The opening has nothing Tab stops at, so Tab still reaches the prompt's buttons
    // first. The prompt draws no panel, as the web's action dock draws none. Outside the
    // opening the orbs and stops are drawn over the prompt.
    let (under, after) = if opening.is_some() {
        (hud, Default::default())
    } else {
        (Default::default(), hud)
    };
    let mut children = Vec::with_capacity(under.len() + 2 + after.len() + over.len());
    children.extend(under.into_iter().flatten());
    children.extend(opening);
    children.extend(prompt);
    children.extend(after.into_iter().flatten());
    children.extend(over.into_iter().flatten());
    let mut game = holder().name("game");
    game.children = children;
    game
}

/// `element` with no press on it or anything in it, as an element under an overlay takes none.
fn inert(mut element: Element) -> Element {
    element.press = None;
    element.shortcuts = Shortcuts::default();
    element.children = element.children.into_iter().map(inert).collect();
    element
}

/// The step row as the game screen shows it (`steps::steps`): under the opening's backdrop,
/// dimmed and with no presses, since the overlay takes the presses that would reach the stops
/// under it. The client builds it again on its own as the table reaches a step of the turn
/// (`Layer::replace`).
#[must_use]
pub fn game_steps(input: &HudIn) -> Option<Element> {
    let steps = steps::steps(input)?;
    Some(if opening::shown(input) {
        inert(steps).opacity(ratio::OPENING_HUD_OPACITY)
    } else {
        steps
    })
}

/// The open prompt, or what the screen shows while the opponent decides, waiting for the table
/// to settle (`Element::waits`): the web interface's prompt opens once its view has played out.
fn waiting(input: &HudIn) -> Option<Element> {
    input
        .prompt
        .and_then(|prompt| prompts::screen(prompt, input))
        .or_else(|| prompts::waiting(input))
        .map(Element::waits)
}

/// What the flat layer shows over a game on the phone layout (docs/ui-conventions.md, "The
/// phone"), as the web interface's `PhoneStageBoard` composes the desktop's pieces for a phone:
/// both orbs centred in their strips, the menu button at the end of the opponent's strip, the
/// phase in the middle band, and the open prompt split between a pill with what it asks and a
/// dock of its buttons in the viewer's strip (`prompts::prompt_bar`). The log is a sheet the
/// menu opens, and the step stops are not shown. Every piece stands at a point of the window, so
/// the screen is an empty region at the window's corner that draws nothing and covers no card.
#[must_use]
pub fn game_phone(input: &HudIn) -> Element {
    let bands = phone_bands(input.window).unwrap_or_default();
    let opening = opening::shown(input);
    let prompt = waiting(input);
    let middle = input
        .view
        .filter(|_| !opening)
        .map(|view| phone::middle(view, input.stops.full_control, &bands));
    // A prompt that lists what it asks about over the table, such as a catalog's entries, may
    // reach into the middle band. The middle band's furniture then lies under the prompt's shade
    // with the table, so that it is not drawn over the list's buttons, as the web's middle band
    // lies under its `.stage-tray`.
    let covers = input.prompt.is_some_and(prompts::covers_table);
    let (middle_under, middle_over) = if covers {
        (middle, None)
    } else {
        (None, middle)
    };
    let furniture = input.view.filter(|_| !opening).map(|_| {
        [
            players::players(input),
            middle_over,
            // A sheet drawn over the screen, the bug report, has its own controls there.
            input.sheet.is_none().then(|| phone::menu(&bands)),
        ]
    });
    // The flat layer draws in tree order. The opening's scrim comes first, as on the desktop
    // (`game`), so that it dims no control that still takes presses; like the desktop's, it
    // shows while the hands are dealt. The result is drawn over
    // everything else, which it blurs (`Style::backdrop`).
    let mut screen = holder()
        .name("game")
        .children(opening::opening(input))
        .children(middle_under)
        .children(prompt)
        .children(furniture.into_iter().flatten().flatten())
        .children(
            [
                game_over::game_over(input).map(Element::waits),
                phone::log_sheet(input, &bands),
                viewer::viewer(input),
            ]
            .into_iter()
            .flatten(),
        );
    screen.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    screen
}
