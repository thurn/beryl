//! The game screen: the flat layer over the table while a game goes on, from its opening hand
//! to its end. Each piece of it is built in a file of its own (`hud`, `inspect`, `prompts`
//! and this module's `opening` and `game_over`), and this function puts them together. The
//! inspector and the Revealed Cards pill are not among them: the client shows each as a float
//! (`inspect::inspector`, `inspect::revealed`).

use super::{ScreenIn, game_over, opening};
use crate::element::{Element, holder, row};
use crate::hud::{log, players, steps};
use crate::inspect::viewer;
use crate::prompts;
use crate::tokens::{color, size};

/// What the flat layer shows over a game. While none of its pieces shows anything, it is the
/// open prompt's element alone, as it has been since the prompt bar was the only thing in the
/// flat layer; with no prompt open, it is an empty region named "game".
#[must_use]
pub fn game(input: &ScreenIn) -> Element {
    let prompt = input
        .prompt
        .and_then(|prompt| prompts::screen(prompt, input));
    let pieces: Vec<Element> = [
        opening::opening(input),
        game_over::scrim(input),
        game_over::game_over(input),
        game_over::buttons(input),
        players::players(input).filter(|_| !opening::shown(input)),
        steps::steps(input),
        log::log(input),
        viewer::viewer(input),
    ]
    .into_iter()
    .flatten()
    .collect();
    if pieces.is_empty() {
        return prompt.unwrap_or_else(|| row().name("game"));
    }
    // The pieces are in a region that draws nothing, so the prompt keeps the panel it has as
    // the screen's top element. It comes first, so that Tab reaches its buttons first.
    holder()
        .name("game")
        .children(prompt.map(on_panel))
        .children(pieces)
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
