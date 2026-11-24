//! The steps of each player's turn about their life orb, as Arena's phase stops (UI Conventions
//! exception 1): twelve icons, two rows of three on each side of the orb, read in the turn's
//! order, the same for both seats. The web interface draws them with `StepStops`
//! (`ui/src/components/Steps.tsx`) in the grid of `.step-stops`
//! (`ui/src/design/components/phase-tracker.css`).
//!
//! - An icon is filled where the player stops with the stack empty in that player's turns
//!   (`Settings::stops`), outlined and dimmer where they do not. A press on it sends
//!   `Message::Toggle`, which sets or clears that stop.
//! - No player gets priority in the untap step or, normally, in cleanup, so those two are never
//!   stops: their icons are dimmer still and take no press. [CR 502.4]: "No player receives
//!   priority during the untap step, so no spells can be cast or resolved and no abilities can
//!   be activated or resolved." [CR 514.3]: "Normally, no player receives priority during the
//!   cleanup step, so no spells can be cast and no abilities can be activated."
//! - Combat's two damage steps share one icon, and its stop stops in both.
//! - The rows centre on the orb's core, each side's inner edge 70 points from it, as
//!   `.turn-controls .step-stops` places them (`ui/src/screens/styles/controls.css`).
//! - The current step burns gold in the active player's rows. It is the step the table shows
//!   now (`HudIn::turn`), which each `set` step of a script that begins a step of the turn
//!   changes as it begins (`TurnChange`), as the web draws the row from the table it shows
//!   step by step. Until the table has shown a view, it is the latest view's step.
//! - On the phone layout the icons are too small to touch, so they are not shown, and they are
//!   hidden while a pile's viewer is open over the table.
//! - On the manual table (`table_tracks`) the icons are the step track: none is a stop, the
//!   table's phase lights in the active player's rows, and a press on an icon sends
//!   `TablePress::Step`, which sets the phase (`manual::turn`).
//!
//! [CR 502.4]: https://yawgatog.com/resources/magic-rules/#R5024
//! [CR 514.3]: https://yawgatog.com/resources/magic-rules/#R5143

use super::{Inputs, Message};
use beryl_protocol::protocol::{Side, Step, StepSet, TurnView};
use glam::Vec2;

use kit_base::tokens::ratio;

use crate::hud::players::face_centre;
use kit_base::data::{flat::Tooltip, hud::StepTrack};
use kit_base::pose::Curve;
use kit_base::tokens::{color, shadow, size};
use kit_frame::frame::Role;
use kit_ui::element::{
    Action, Anchor, Display, Element, Shadow, Shown, Timing, TipPlacement, TipSide, Track, holder,
};
use kit_ui::widgets::{hover_tip, icons};

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
fn steps(input: &Inputs) -> Option<Element> {
    if !input.visible() {
        return None;
    }
    let (active, step, number) = input.current()?;
    Some(
        holder().name("steps").children(
            [
                (Side::You, input.stops.yours, input.centres[0]),
                (Side::Opponent, input.stops.theirs, input.centres[1]),
            ]
            .map(|(side, set, centre)| {
                rows(
                    side,
                    Mode::Stops(set),
                    (active, step, number),
                    centre,
                    input.tuning,
                    (!input.opening).then_some(input.tooltip),
                )
            }),
        ),
    )
}

/// The two manual step tracks. The manual table maps each icon press to its phase command.
pub fn table_tracks(
    turn: &TurnView,
    lit: usize,
    input: &crate::hud::HudIn,
    press: fn(usize, bool) -> Action,
) -> Element {
    let mode = Mode::Table { lit, press };
    holder()
        .name("steps")
        .children([Side::You, Side::Opponent].map(|side| {
            rows(
                side,
                mode,
                (turn.active, turn.step, turn.own_number),
                face_centre(side, input.profile),
                input.data.hud.steps,
                Some(input.data.flat.tooltip),
            )
        }))
}

/// A game's stop settings or the manual table's phase track.
#[derive(Clone, Copy)]
enum Mode {
    Stops(StepSet),
    Table {
        lit: usize,
        press: fn(usize, bool) -> Action,
    },
}

/// `side`'s twelve icons about their orb, for `mode`, in a turn whose active player is
/// `active`.
fn rows(
    side: Side,
    mode: Mode,
    (active, step, number): (Side, Step, u16),
    centre: Vec2,
    tuning: StepTrack,
    tooltip: Option<Tooltip>,
) -> Element {
    let whose = match side {
        Side::You => "Your",
        Side::Opponent => "Opponent's",
    };
    let active = active == side;
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
    element.anchor = Some(Anchor::Point(centre, Vec2::new(0.5, 0.5)));
    element.children(STEP_STOPS.iter().enumerate().flat_map(|(at, stop)| {
        let here = active
            && match mode {
                Mode::Stops(_) => stop.steps.contains(&step) || NEVER.contains(&(at, step)),
                Mode::Table { lit, .. } => at == lit,
            };
        let name = format!("{whose} {}", stop.label);
        let icon = match mode {
            Mode::Stops(set) => icon(at, &name, side, set, here, tuning),
            Mode::Table { press, .. } => track_icon(at, &name, here, press(at, active), tuning),
        };
        let tip = tooltip.map(|metrics| {
            let words = if here {
                format!("Turn {number} · {}", step_title(step))
            } else {
                name.clone()
            };
            let side = match side {
                Side::You => TipSide::Above,
                Side::Opponent => TipSide::Below,
            };
            hover_tip(
                words,
                &name,
                metrics,
                TipPlacement {
                    target: icon.role,
                    side,
                },
            )
        });
        let icon = icon
            .grid_column(column_of(at))
            .grid_row(row_of(at))
            .margin([0.0, margin, 0.0, margin]);
        [Some(icon), tip].into_iter().flatten()
    }))
}

/// The current step's title, as the web's `STEP_NAMES` gives it in the turn tooltip.
const fn step_title(step: Step) -> &'static str {
    match step {
        Step::Untap => "Untap",
        Step::Upkeep => "Upkeep",
        Step::Draw => "Draw",
        Step::PrecombatMain => "Main 1",
        Step::BeginningOfCombat => "Beginning of combat",
        Step::DeclareAttackers => "Attackers",
        Step::DeclareBlockers => "Blockers",
        Step::FirstStrikeDamage => "First-strike damage",
        Step::CombatDamage => "Combat damage",
        Step::EndOfCombat => "End of combat",
        Step::PostcombatMain => "Main 2",
        Step::End => "End step",
        Step::Cleanup => "Cleanup",
    }
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

/// The glyph of a step icon, an element of its own as the web's `.step-icon` svg is: it carries
/// the color, the fade and the drop shadows, which the `.step-toggle` button around it does not.
fn glyph_element(
    name: &str,
    glyph: icons::IconId,
    color: [u8; 4],
    opacity: f32,
    here: bool,
) -> Element {
    let mut element = Element::new(Role::Icon)
        .name(name)
        .width(size::STEP)
        .height(size::STEP)
        .color(color)
        .opacity(opacity)
        .drop_shadow(step_shadow(here));
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "an icon is a whole number of points"
    )]
    let side_points = size::STEP as u16;
    element.shows = Some(Shown::Icon(glyph, side_points));
    element
}

/// The icon at `at` in `side`'s rows. A step that can stop is a toggle holding its glyph, as the
/// web's `.step-toggle` button holds its `.step-icon`; one that never stops is the glyph alone.
fn icon(at: usize, name: &str, side: Side, set: StepSet, here: bool, tuning: StepTrack) -> Element {
    let stop = &STEP_STOPS[at];
    let on = stop.steps.iter().any(|&step| set.has(step.into()));
    let (glyph, opacity) = match stop.steps.first() {
        None => (icons::STEPS_OUTLINED[at], tuning.fixed),
        Some(_) if on => (icons::STEPS[at], 1.0),
        Some(_) => (icons::STEPS_OUTLINED[at], tuning.off),
    };
    let tint = if here {
        color::STEP_CURRENT
    } else {
        color::STEP
    };
    let glyph = glyph_element(name, glyph, tint, if here { 1.0 } else { opacity }, here);
    let Some(&last_step) = stop.steps.last() else {
        return glyph;
    };
    Element::new(Role::Toggle)
        .name(name)
        .width(size::STEP)
        .height(size::STEP)
        .selected(on)
        .on_press(Message::Toggle {
            step: last_step,
            theirs: side == Side::Opponent,
        })
        .child(hoverable_glyph(glyph, tuning))
}

/// The manual step track icon at `at`, lit if `here`. Every icon is a toggle, as the
/// web's track is a `.step-toggle` button round each icon. An unlit step that never stops a
/// game (Untap and Cleanup) is faded further, as the web's `.step-fixed` is.
fn track_icon(at: usize, name: &str, here: bool, press: Action, tuning: StepTrack) -> Element {
    let faded = if STEP_STOPS[at].steps.is_empty() {
        tuning.fixed
    } else {
        tuning.off
    };
    let tint = if here {
        color::STEP_CURRENT
    } else {
        color::STEP
    };
    let glyph = glyph_element(
        name,
        icons::STEPS_OUTLINED[at],
        tint,
        if here { 1.0 } else { faded },
        here,
    );
    let mut icon = Element::new(Role::Toggle)
        .name(name)
        .width(size::STEP)
        .height(size::STEP)
        .selected(here)
        .child(hoverable_glyph(glyph, tuning));
    icon.press = Some(press);
    icon
}

/// The phase icon fades to full opacity using the web's step icon transition.
fn hoverable_glyph(mut glyph: Element, tuning: StepTrack) -> Element {
    glyph.style.opaque_on_hover = Some(Timing::new(tuning.hover_ms, 0.0, Curve::EASE));
    glyph
}

/// `element` with no press on it or anything in it, as an element under an overlay takes none.
fn inert(mut element: Element) -> Element {
    element.press = None;
    element.children = element.children.into_iter().map(inert).collect();
    element
}

/// The step row as the game screen shows it (`steps::steps`): under the opening's backdrop,
/// dimmed and with no presses, since the overlay takes the presses that would reach the stops
/// under it. The client builds it again on its own as the table reaches a step of the turn
/// (`Layer::replace`).
#[must_use]
pub(crate) fn game_steps(input: &Inputs) -> Option<Element> {
    let steps = steps(input)?;
    Some(if input.opening {
        inert(steps).opacity(ratio::OPENING_HUD_OPACITY)
    } else {
        steps
    })
}
