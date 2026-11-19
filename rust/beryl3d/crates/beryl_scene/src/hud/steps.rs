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
//! - The rows centre on the orb's core, each side's inner edge 70 points from it, as
//!   `.turn-controls .step-stops` places them (`ui/src/screens/styles/controls.css`).
//! - The current step burns gold in the active player's rows.
//! - On the phone layout the icons are too small to touch, so they are not shown, and they are
//!   hidden while a pile's viewer is open over the table.
//! - On the manual table (`table_steps`) the icons are the step track: none is a stop, the
//!   table's phase lights in the active player's rows, and a press on an icon sends
//!   `TablePress::Step`, which sets the phase (`manual::turn`).
//!
//! [CR 502.4]: https://yawgatog.com/resources/magic-rules/#R5024
//! [CR 514.3]: https://yawgatog.com/resources/magic-rules/#R5143

use beryl_protocol::protocol::{PlayerView, Side, Step, StepSet};
use glam::Vec2;

use super::players::face_centre;
use crate::data::space::hud::{STEP_FIXED, STEP_OFF};
use crate::element::{Anchor, Display, Element, Press, Shadow, Shown, Track, holder};
use crate::frame::Role;
use crate::manual::TablePress;
use crate::screens::ScreenIn;
use crate::table::Window;
use crate::tokens::{color, shadow, size};
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
    if input.window.phone() || input.viewing.is_some() {
        return None;
    }
    let stops = &input.settings.stops;
    Some(holder().name("steps").children([
        rows(view, Side::You, Mode::Stops(stops.yours), input.window),
        rows(
            view,
            Side::Opponent,
            Mode::Stops(stops.theirs),
            input.window,
        ),
    ]))
}

/// What the manual table shows for the steps: both players' step tracks, on the desktop
/// layout, while no pile's viewer is open, with the icon of the table's phase lit in the
/// active player's rows, as the web's `StepStops` stands on the manual table with no stops and
/// an `onStep`.
#[must_use]
pub fn table_steps(input: &ScreenIn) -> Option<Element> {
    let view = input.view?;
    let table = view.table.as_ref()?;
    if input.window.phone() || input.viewing.is_some() {
        return None;
    }
    let lit = crate::manual::turn::icon_of(table.phase);
    Some(holder().name("steps").children([
        rows(view, Side::You, Mode::Table(lit), input.window),
        rows(view, Side::Opponent, Mode::Table(lit), input.window),
    ]))
}

/// What a player's rows are for: a game's stops, with the steps of their turns that stop, or
/// the manual table's step track, with the index in `STEP_STOPS` of the icon its phase lights.
#[derive(Clone, Copy)]
enum Mode {
    Stops(StepSet),
    Table(usize),
}

/// `side`'s twelve icons about their orb, for `mode`.
fn rows(view: &PlayerView, side: Side, mode: Mode, window: Window) -> Element {
    let whose = match side {
        Side::You => "Your",
        Side::Opponent => "Opponent's",
    };
    let active = view.turn.active == side;
    // The columns are `STEP_GAP` apart and the rows `STEP_ROW_GAP`, but a grid here has one gap
    // for both, so each column is wider by the difference and its icon stands in its middle.
    let margin = (size::STEP_GAP - size::STEP_ROW_GAP) / 2.0;
    let column = Track::Points(size::STEP + 2.0 * margin);
    // The orb's column takes what the web's grid, `STEPS_WIDTH` wide, leaves after its six
    // icons and six gaps, widened by the two margins its neighbours lost.
    #[expect(clippy::cast_precision_loss, reason = "twelve icons")]
    let icons = STEP_STOPS.len() as f32;
    let orb = Track::Points(
        size::STEPS_WIDTH - icons / 2.0 * (size::STEP + size::STEP_GAP) + 2.0 * margin,
    );
    let row = Track::Points(size::STEP);
    let mut element = Element::new(Role::Region)
        .name(&format!("{whose} steps"))
        .display(Display::Grid)
        .gap(size::STEP_ROW_GAP)
        .grid_template_columns(vec![column, column, column, orb, column, column, column])
        .grid_template_rows(vec![row, row]);
    element.anchor = Some(Anchor::Point(
        face_centre(side, window),
        Vec2::new(0.5, 0.5),
    ));
    element.children((0..STEP_STOPS.len()).map(|at| {
        let icon = match mode {
            Mode::Stops(set) => icon(at, whose, side, set, active.then_some(view.turn.step)),
            Mode::Table(lit) => track_icon(at, whose, active, active && at == lit),
        };
        icon.grid_column(column_of(at))
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

/// The drop shadows of a step icon: `.step-current .step-icon`'s orange glow on the current
/// step's (`here`), and `.step-icon`'s dark shadow on every other (phase-tracker.css).
fn step_shadow(here: bool) -> Vec<Shadow> {
    if here {
        shadow::CURRENT_GLOW.to_vec()
    } else {
        shadow::STEP_ICON.to_vec()
    }
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
        .drop_shadow(step_shadow(here))
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

/// The icon at `at` of a manual table's step track, lit if `here`, which a press sets the phase
/// to; `active` says whether its rows are the active player's.
fn track_icon(at: usize, whose: &str, active: bool, here: bool) -> Element {
    let name = format!("{whose} {}", STEP_STOPS[at].label);
    let mut element = Element::new(Role::Button)
        .name(&name)
        .width(size::STEP)
        .height(size::STEP)
        .color(if here {
            color::STEP_CURRENT
        } else {
            color::STEP
        })
        .opacity(if here { 1.0 } else { STEP_OFF })
        .drop_shadow(step_shadow(here))
        .selected(here)
        .on_press(Press::Table(TablePress::Step {
            #[expect(clippy::cast_possible_truncation, reason = "twelve icons")]
            at: at as u8,
            active,
        }));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an icon is a whole number of points"
    )]
    let side_points = size::STEP as u16;
    element.shows = Some(Shown::Icon(icons::STEPS_OUTLINED[at], side_points));
    element
}
