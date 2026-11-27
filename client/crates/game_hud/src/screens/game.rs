//! The game screen: the flat layer over the table while a game goes on, from its opening hand
//! to its end. Each piece of it is built in a file of its own (`hud`, `inspect`, `prompts`
//! and this module's `opening` and `game_over`), and this function puts them together. The
//! inspector and the Revealed Cards pill are not among them: the client shows each as a float
//! (`inspect::inspector`, `inspect::revealed`).

use glam::Vec2;

use super::opening;
use crate::hud::{HudIn, phase};
use crate::prompts;
use kit_base::data::profiles::Piece;
use kit_ui::element::{Action, Anchor, Element, holder, row};

/// The source of the game screen composition, for the client map.
pub const SOURCE: &str = file!();

/// The registered views the active profile places on the game screen.
#[derive(Default)]
pub struct Furniture {
    pub prompt: Option<Element>,
    pub players: Option<Element>,
    pub steps: Option<Element>,
    pub dock_full_control: Option<Element>,
    pub band_full_control: Option<Element>,
    pub viewer: Option<Element>,
}

/// Composes the HUD in the order its profile gives. The opening dims the
/// furniture underneath it. The prompt and its controls remain above the scrim.
#[must_use]
pub fn game(
    input: &HudIn,
    mut views: Furniture,
    log: Option<Element>,
    result: Option<Element>,
    menu: Action,
) -> Element {
    let layout = &input.profile.data().hud;
    let opening = opening::opening(input);
    let is_opening = opening.is_some();
    let prompt = views.prompt.filter(|_| !opening::choosing_start(input));
    let shade = prompt.as_ref().and_then(|_| prompts::question_shade(input));
    let covers = input.prompt.is_some_and(prompts::covers_table);
    let viewing = views.viewer.is_some();
    let overlays = overlays(input, log, result, views.viewer);
    let mut menu = Some(menu);
    let mut game = holder().name("game");

    let under_prompt = (!is_opening && !viewing && covers && layout.table.contains(&Piece::Phase))
        .then(|| middle(input, views.band_full_control.take(), true))
        .flatten();
    let mut furniture = Vec::with_capacity(layout.table.len());

    if !viewing && (!is_opening || layout.table_under_opening) {
        for &piece in &layout.table {
            let element = match piece {
                Piece::Players => views.players.take(),
                Piece::StepStops => views.steps.take(),
                Piece::FullControl if !is_opening => views.dock_full_control.take(),
                Piece::Phase if !covers => middle(
                    input,
                    views.band_full_control.take(),
                    prompt.is_none() || !prompts::bar::draws_phase(input),
                ),
                Piece::Menu if input.sheet.is_none() && input.view.is_some() => menu
                    .take()
                    .and_then(|press| phase::menu(input.profile, press)),
                _ => None,
            };
            furniture.extend(element);
        }
    }
    if !is_opening
        && shade.is_none()
        && furniture.is_empty()
        && overlays.is_empty()
        && !layout.anchored_to_window
    {
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
    game.children.extend(shade);
    game.children.extend(overlays);
    if layout.anchored_to_window {
        game.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    }
    game
}

fn middle(input: &HudIn, full_control: Option<Element>, with_phase: bool) -> Option<Element> {
    phase::middle(input.view?, full_control, input.profile, with_phase)
}

fn overlays(
    input: &HudIn,
    mut log: Option<Element>,
    mut result: Option<Element>,
    mut viewer: Option<Element>,
) -> Vec<Element> {
    let viewing = viewer.is_some();
    input
        .profile
        .data()
        .hud
        .overlays
        .iter()
        .filter_map(|piece| match piece {
            Piece::LogColumn | Piece::LogSheet if !viewing => log.take(),
            Piece::Viewer => viewer.take(),
            // The result blurs everything drawn before it, including floats.
            Piece::Result => result.take().map(Element::waits),
            _ => None,
        })
        .collect()
}
