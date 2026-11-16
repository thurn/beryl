//! The steps of each player's turn about their life orb, as Arena's phase stops (UI Conventions
//! exception 1): twelve icons, two rows of three on each side of the orb, read in the turn's
//! order, the same for both seats. The web interface draws them with `StepStops`
//! (`ui/src/components/Steps.tsx`) in the grid of `.step-stops`
//! (`ui/src/design/components/phase-tracker.css`).
//!
//! - An icon is filled where the player stops with the stack empty in that player's turns
//!   (`Settings::stops`), outlined and dimmer where they do not. A press on it sends
//!   `Press::Stop`, which sets or clears that stop.
//! - No player gets priority in the untap step or, normally, in cleanup, so those two are never
//!   stops: their icons are dimmer still and take no press. [CR 502.4]: "No player receives
//!   priority during the untap step, so no spells can be cast or resolved and no abilities can
//!   be activated or resolved." [CR 514.3]: "Normally, no player receives priority during the
//!   cleanup step, so no spells can be cast and no abilities can be activated."
//! - Combat's two damage steps share one icon, and its stop stops in both.
//! - The current step burns gold in the active player's rows.
//! - On the phone layout the icons are too small to touch, so they are not shown, and they are
//!   hidden while a pile's viewer is open over the table.
//!
//! [CR 502.4]: https://yawgatog.com/resources/magic-rules/#R5024
//! [CR 514.3]: https://yawgatog.com/resources/magic-rules/#R5143

use beryl_protocol::protocol::{PlayerView, Side, Step, StepSet};
use glam::Vec2;

use super::players::orb_centre;
use crate::data::space::hud::{STEP_FIXED, STEP_OFF};
use crate::element::{Anchor, Display, Element, Press, Shown, Track, holder};
use crate::frame::Role;
use crate::screens::ScreenIn;
use crate::table::Window;
use crate::tokens::{color, size};
use crate::widgets::icons;

/// One icon of the twelve: its label and the steps a stop at it stops in, or none for a step
/// that is never a stop. The order is the web's `STEP_STOPS`, which `icons::STEPS` follows.
struct StepStop {
    label: &'static str,
    steps: &'static [Step],
}

const STEP_STOPS: [StepStop; 12] = [
    StepStop {
        label: "untap",
        steps: &[],
    },
    StepStop {
        label: "upkeep",
        steps: &[Step::Upkeep],
    },
    StepStop {
        label: "draw",
        steps: &[Step::Draw],
    },
    StepStop {
        label: "main 1",
        steps: &[Step::PrecombatMain],
    },
    StepStop {
        label: "beginning of combat",
        steps: &[Step::BeginningOfCombat],
    },
    StepStop {
        label: "attackers",
        steps: &[Step::DeclareAttackers],
    },
    StepStop {
        label: "blockers",
        steps: &[Step::DeclareBlockers],
    },
    StepStop {
        label: "damage",
        steps: &[Step::FirstStrikeDamage, Step::CombatDamage],
    },
    StepStop {
        label: "end of combat",
        steps: &[Step::EndOfCombat],
    },
    StepStop {
        label: "main 2",
        steps: &[Step::PostcombatMain],
    },
    StepStop {
        label: "end step",
        steps: &[Step::End],
    },
    StepStop {
        label: "cleanup",
        steps: &[],
    },
];

/// The steps that never stop and the icon each still shows, by index in `STEP_STOPS`.
const NEVER: [(usize, Step); 2] = [(0, Step::Untap), (11, Step::Cleanup)];

/// What the game screen shows for the steps: both players' stops, once a game has a view, on
/// the desktop layout, while no pile's viewer is open.
#[must_use]
pub fn steps(input: &ScreenIn) -> Option<Element> {
    let view = input.view?;
    // A pile's viewer stands over the table and its controls, and its own buttons sit where
    // the stops beside the viewer's own orb stand.
    if phone(input.window) || input.viewing.is_some() {
        return None;
    }
    let stops = &input.settings.stops;
    Some(holder().name("steps").children([
        rows(view, Side::You, stops.yours, input.window),
        rows(view, Side::Opponent, stops.theirs, input.window),
    ]))
}

/// Whether `window` takes the phone layout: its shorter side is under 500 points
/// (docs/beryl-3d.md, "Sizes").
fn phone(window: Window) -> bool {
    window.size.width.min(window.size.height) < 500.0
}

/// `side`'s twelve icons about their orb, with `set` the steps of their turns that stop.
fn rows(view: &PlayerView, side: Side, set: StepSet, window: Window) -> Element {
    let whose = match side {
        Side::You => "Your",
        Side::Opponent => "Opponent's",
    };
    let current = (view.turn.active == side).then_some(view.turn.step);
    // The columns are `STEP_GAP` apart and the rows `STEP_ROW_GAP`, but a grid here has one gap
    // for both, so each column is wider by the difference and its icon stands in its middle.
    let margin = (size::STEP_GAP - size::STEP_ROW_GAP) / 2.0;
    let column = Track::Points(size::STEP + 2.0 * margin);
    let orb = Track::Points(size::ORB_WIDTH + 2.0 * size::ORB_BEZEL);
    let row = Track::Points(size::STEP);
    let mut element = Element::new(Role::Region)
        .name(&format!("{whose} steps"))
        .display(Display::Grid)
        .gap(size::STEP_ROW_GAP)
        .grid_template_columns(vec![column, column, column, orb, column, column, column])
        .grid_template_rows(vec![row, row]);
    element.anchor = Some(Anchor::Point(orb_centre(side, window), Vec2::new(0.5, 0.5)));
    element.children((0..STEP_STOPS.len()).map(|at| {
        icon(at, whose, side, set, current)
            .grid_column(column_of(at))
            .grid_row(row_of(at))
            .margin([0.0, margin, 0.0, margin])
    }))
}

/// The grid lines the icon at `at` spans across: the first three columns for the left rows,
/// the last three for the right, as the web's `gridColumn`.
fn column_of(at: usize) -> (i16, i16) {
    #[expect(clippy::cast_possible_truncation, reason = "twelve icons")]
    let column = (at % 3) as i16 + if at < 6 { 1 } else { 5 };
    (column, column + 1)
}

/// The grid lines the icon at `at` spans down: each side's first three icons in the top row.
fn row_of(at: usize) -> (i16, i16) {
    #[expect(clippy::cast_possible_truncation, reason = "twelve icons")]
    let row = (at / 3 % 2) as i16 + 1;
    (row, row + 1)
}

/// The icon at `at` in `side`'s rows.
fn icon(at: usize, whose: &str, side: Side, set: StepSet, current: Option<Step>) -> Element {
    let stop = &STEP_STOPS[at];
    let here = current.is_some_and(|step| {
        stop.steps.contains(&step) || NEVER.iter().any(|&(never, s)| never == at && s == step)
    });
    let name = format!("{whose} {}", stop.label);
    let on = stop.steps.iter().any(|&step| set.has(step.into()));
    let (role, glyph, opacity) = match stop.steps.first() {
        None => (Role::Icon, icons::STEPS_OUTLINED[at], STEP_FIXED),
        Some(_) if on => (Role::Toggle, icons::STEPS[at], 1.0),
        Some(_) => (Role::Toggle, icons::STEPS_OUTLINED[at], STEP_OFF),
    };
    let mut element = Element::new(role)
        .name(&name)
        .width(size::STEP)
        .height(size::STEP)
        .color(if here {
            color::STEP_CURRENT
        } else {
            color::STEP
        })
        .opacity(if here { 1.0 } else { opacity })
        .selected(on);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an icon is a whole number of points"
    )]
    let side_points = size::STEP as u16;
    element.shows = Some(Shown::Icon(glyph, side_points));
    if let Some(&step) = stop.steps.last() {
        element = element.on_press(Press::Stop {
            step,
            theirs: side == Side::Opponent,
        });
    }
    element
}
