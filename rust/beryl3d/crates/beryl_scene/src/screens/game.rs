//! The game screen: the flat layer over the table while a game goes on, from its opening hand
//! to its end. Each piece of it is built in a file of its own (`hud`, `inspect`, `prompts`
//! and this module's `opening` and `game_over`), and this function puts them together. The
//! inspector and the Revealed Cards pill are not among them: the client shows each as a float
//! (`inspect::inspector`, `inspect::revealed`).

use glam::Vec2;

use super::{ScreenIn, game_over, opening};
use crate::element::{Anchor, Element, holder, row};
use crate::hud::{log, phone, players, steps};
use crate::inspect::viewer;
use crate::prompts;
use crate::table::phone_bands;
use crate::tokens::{color, size};

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
        game_over::scrim(input),
        game_over::game_over(input),
        game_over::buttons(input),
        players::players(input).filter(|_| !opening::shown(input)),
        // The opening's backdrop covers the stops, as the web's overlay does.
        steps::steps(input).filter(|_| !opening::shown(input)),
        log::log(input),
        viewer::viewer(input),
    ]
    .into_iter()
    .flatten()
    .collect();
    let opening = opening::opening(input);
    if pieces.is_empty() && opening.is_none() {
        return prompt.unwrap_or_else(|| row().name("game"));
    }
    // The pieces are in a region that draws nothing, so the prompt keeps the panel it has as
    // the screen's top element. The flat layer draws in tree order, so the opening, whose scrim
    // dims the whole window, comes before the prompt, whose buttons would otherwise be drawn
    // under it and look disabled. The opening has nothing Tab stops at, so Tab still reaches
    // the prompt's buttons first.
    holder()
        .name("game")
        .children(opening)
        .children(prompt.map(on_panel))
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
    let furniture = input.view.filter(|_| !opening).map(|view| {
        [
            players::players(input),
            Some(phone::middle(view, input.settings.full_control, &bands)),
            // A sheet drawn over the screen, the bug report, has its own controls there.
            input.sheet.is_none().then(|| phone::menu(&bands)),
        ]
    });
    // The flat layer draws in tree order. The opening's and the game over's scrims come first,
    // as on the desktop (`game`), so that neither dims a control that still takes presses.
    let mut screen = holder()
        .name("game")
        .children(opening::opening(input))
        .children(game_over::scrim(input))
        .children(prompt)
        .children(furniture.into_iter().flatten().flatten())
        .children(
            [
                game_over::game_over(input),
                game_over::buttons(input),
                phone::log_sheet(input, &bands),
                viewer::viewer(input),
            ]
            .into_iter()
            .flatten(),
        );
    screen.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    screen
}

/// `element` drawn on the panel of a screen's top element, where its style gives none. The
/// panel's border takes room inside the box, which a top element's does not, so the padding
/// gives that room back and the box keeps its size.
fn on_panel(element: Element) -> Element {
    let style = &element.style;
    let (background, border, radius) = (style.background, style.border, style.radius);
    let padding = if border.is_none() {
        style.padding.map(|side| (side - size::BORDER).max(0.0))
    } else {
        style.padding
    };
    let mut element = element
        .background(background.unwrap_or(color::PANEL))
        .border(border.unwrap_or((color::METAL, size::BORDER)))
        .radius(if radius > 0.0 {
            radius
        } else {
            size::PANEL_RADIUS
        });
    element.style.padding = padding;
    element
}
