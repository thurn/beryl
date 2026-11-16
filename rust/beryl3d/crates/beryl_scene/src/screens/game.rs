//! The game screen: the flat layer over the table while a game goes on, from its opening hand
//! to its end. Each piece of it is built in a file of its own (`hud`, `inspect`, `prompts`
//! and this module's `opening` and `game_over`), and this function puts them together. The
//! inspector and the Revealed Cards pill are not among them: the client shows each as a float
//! (`inspect::inspector`, `inspect::revealed`).

use super::{ScreenIn, game_over, opening};
use crate::element::{Element, row};
use crate::hud::{log, players, steps};
use crate::inspect::viewer;
use crate::prompts;

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
        players::players(input),
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
    row().name("game").children(pieces).children(prompt)
}
