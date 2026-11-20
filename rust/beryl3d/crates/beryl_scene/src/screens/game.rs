//! The game screen: the flat layer over the table while a game goes on, from its opening hand
//! to its end. Each piece of it is built in a file of its own (`hud`, `inspect`, `prompts`
//! and this module's `opening` and `game_over`), and this function puts them together. The
//! inspector and the Revealed Cards pill are not among them: the client shows each as a float
//! (`inspect::inspector`, `inspect::revealed`).

use glam::Vec2;

use super::{ScreenIn, game_over, opening};
use crate::element::{Anchor, Element, holder, row};
use crate::hud::{full_control, log, phone, players, steps};
use crate::inspect::viewer;
use crate::prompts;
use crate::table::phone_bands;

/// What the flat layer shows over a game. While none of its pieces shows anything, it is the
/// open prompt's element alone, as it has been since the prompt bar was the only thing in the
/// flat layer; with no prompt open, it is an empty region named "game".
#[must_use]
pub fn game(input: &ScreenIn) -> Element {
    let prompt = input
        .prompt
        .and_then(|prompt| prompts::screen(prompt, input))
        .or_else(|| prompts::waiting(input));
    let pieces: Vec<Element> = [
        players::players(input).filter(|_| !opening::shown(input)),
        // The opening's backdrop covers the stops, as the web's overlay does.
        steps::steps(input).filter(|_| !opening::shown(input)),
        full_control::full_control(input).filter(|_| !opening::shown(input)),
        log::log(input),
        viewer::viewer(input),
        // The result blurs everything drawn before it, the floats too (`Style::backdrop_filter`).
        game_over::game_over(input),
    ]
    .into_iter()
    .flatten()
    .collect();
    let opening = opening::opening(input);
    if pieces.is_empty() && opening.is_none() {
        return prompt.unwrap_or_else(|| row().name("game"));
    }
    // The flat layer draws in tree order, so the opening, whose scrim dims the whole window,
    // comes before the prompt, whose buttons would otherwise be drawn under it and look
    // disabled. The opening has nothing Tab stops at, so Tab still reaches the prompt's buttons
    // first. The prompt draws no panel, as the web's action dock draws none.
    holder()
        .name("game")
        .children(opening)
        .children(prompt)
        .children(pieces)
}

/// What the flat layer shows over a game on the phone layout (docs/ui-conventions.md, "The
/// phone"), as the web interface's `PhoneStageBoard` composes the desktop's pieces for a phone:
/// both orbs centred in their strips, the menu button at the end of the opponent's strip, the
/// phase in the middle band, and the open prompt split between a pill with what it asks and a
/// dock of its buttons in the viewer's strip (`prompts::prompt_bar`). The log is a sheet the
/// menu opens, and the step stops are not shown. Every piece stands at a point of the window, so
/// the screen is an empty region at the window's corner that draws nothing and covers no card.
#[must_use]
pub fn game_phone(input: &ScreenIn) -> Element {
    let bands = phone_bands(input.window).unwrap_or_default();
    let opening = opening::shown(input);
    let prompt = input
        .prompt
        .and_then(|prompt| prompts::screen(prompt, input))
        .or_else(|| prompts::waiting(input));
    let middle = input
        .view
        .filter(|_| !opening)
        .map(|view| phone::middle(view, input.settings.full_control, &bands));
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
    // (`game`), so that it dims no control that still takes presses. The result is drawn over
    // everything else, which it blurs (`Style::backdrop_filter`).
    let mut screen = holder()
        .name("game")
        .children(opening::opening(input))
        .children(middle_under)
        .children(prompt)
        .children(furniture.into_iter().flatten().flatten())
        .children(
            [
                game_over::game_over(input),
                phone::log_sheet(input, &bands),
                viewer::viewer(input),
            ]
            .into_iter()
            .flatten(),
        );
    screen.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    screen
}
