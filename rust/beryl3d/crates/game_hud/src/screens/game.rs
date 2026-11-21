//! The game screen: the flat layer over the table while a game goes on, from its opening hand
//! to its end. Each piece of it is built in a file of its own (`hud`, `inspect`, `prompts`
//! and this module's `opening` and `game_over`), and this function puts them together. The
//! inspector and the Revealed Cards pill are not among them: the client shows each as a float
//! (`inspect::inspector`, `inspect::revealed`).

use glam::Vec2;

use super::opening;
use crate::hud::{HudIn, full_control, phase, players, steps};
use crate::inspect::viewer;
use crate::prompts;
use kit_base::data::profiles::Piece;
use kit_base::tokens::ratio;
use kit_ui::element::{Action, Anchor, Element, Shortcuts, holder, row};

/// Composes the HUD in the order its profile gives. The opening dims the
/// furniture underneath it. The prompt and its controls remain above the scrim.
#[must_use]
pub fn game(input: &HudIn, log: Option<Element>, result: Option<Element>, menu: Action) -> Element {
    let layout = &input.profile.data().hud;
    let opening = opening::opening(input);
    let is_opening = opening.is_some();
    let prompt = waiting(input).filter(|_| !opening::choosing_start(input));
    let covers = input.prompt.is_some_and(prompts::covers_table);
    let overlays = overlays(input, log, result);
    let mut menu = Some(menu);
    let mut game = holder().name("game");

    let under_prompt = (!is_opening && covers && layout.table.contains(&Piece::Phase))
        .then(|| middle(input))
        .flatten();
    let mut furniture = Vec::with_capacity(layout.table.len());

    if !is_opening || layout.table_under_opening {
        for &piece in &layout.table {
            let element = match piece {
                Piece::Players => players::players(input),
                Piece::StepStops => game_steps(input),
                Piece::FullControl if !is_opening => full_control::full_control(input),
                Piece::Phase if !covers => middle(input),
                Piece::Menu if input.sheet.is_none() && input.view.is_some() => menu
                    .take()
                    .and_then(|press| phase::menu(input.profile, press)),
                _ => None,
            };
            furniture.extend(element);
        }
    }
    if !is_opening && furniture.is_empty() && overlays.is_empty() && !layout.anchored_to_window {
        return prompt.unwrap_or_else(|| row().name("game"));
    }
    if is_opening && layout.table_under_opening {
        game.children.extend(furniture);
        game.children.extend(opening);
        game.children.extend(prompt);
    } else {
        game.children.extend(opening);
        game.children.extend(under_prompt);
        game.children.extend(prompt);
        game.children.extend(furniture);
    }
    game.children.extend(overlays);
    if layout.anchored_to_window {
        game.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    }
    game
}

fn middle(input: &HudIn) -> Option<Element> {
    phase::middle(input.view?, input.stops.full_control, input.profile)
}

fn overlays(input: &HudIn, mut log: Option<Element>, mut result: Option<Element>) -> Vec<Element> {
    input
        .profile
        .data()
        .hud
        .overlays
        .iter()
        .filter_map(|piece| match piece {
            Piece::LogColumn | Piece::LogSheet => log.take(),
            Piece::Viewer => viewer::viewer(input),
            // The result blurs everything drawn before it, including floats.
            Piece::Result => result.take().map(Element::waits),
            _ => None,
        })
        .collect()
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
    if !input.profile.data().hud.table.contains(&Piece::StepStops) {
        return None;
    }
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
