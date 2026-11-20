//! Game over, shown on the game screen once the game has ended: a port of the web interface's
//! `GameOver` (`ui/src/screens/GameOver.tsx`) with the rules of `.game-over` in
//! `ui/src/screens/styles/opening.css`. The table under it blurred and darkened, as the web's
//! `.table-dimmed` filter does (`ui/src/screens/styles/table.css`), a scrim over it, the result
//! in large gold capitals at the window's vertical middle, why it ended beneath it, the card that
//! ended it (when the session saw one), and a dock of "View Battlefield" over "New Game" where
//! the web's action dock stands. "View Battlefield" puts the result aside, and a "Back to"
//! button in the dock brings it back, as the web's `useResult` does.
//!
//! Like the web's dialog, the result is modal: its scrim takes every press, and nothing under it
//! takes one (`FrameBuilder::backdrop`, `Layer::press_at`).
//!
//! Not built yet: the web's small icon beside the reason.

use beryl_protocol::protocol::{Culprit, LossReason, OutcomeView, Side, Stall, Status};
use glam::Vec2;

use super::{Screen, ScreenIn};
use crate::data::space::flat::phone::game_over as metrics;
use crate::data::space::flat::phone::opening::NARROW_BELOW;
use crate::data::space::flat::phone::{ACTION_GAP, GUTTER};
use crate::element::{Align, Anchor, Element, Filter, Justify, Press, Shown, column, holder, text};
use crate::faces::FaceList;
use crate::frame::Role;
use crate::input::FaceSize;
use crate::table::{CARD_HEIGHT, CARD_WIDTH};
use crate::text::TypeStyle;
use crate::tokens::{color, ratio, size, space, type_style};
use crate::widgets::{button, primary};

/// The width of the card that ended the game, the web's 230 on the desktop.
const CULPRIT_WIDTH: f32 = size::GAME_OVER_CULPRIT_WIDTH;

/// The result's title, `.game-over-title`: 72 points, bold.
const TITLE: TypeStyle = type_style::GAME_OVER_TITLE;

/// Why the game ended, `.game-over-reason`: 24 points of interface text.
const REASON: TypeStyle = type_style::GAME_OVER_REASON;

/// The result's title, as the web's `resultTitle` words it for `outcome`; none is a game that
/// was stopped.
#[must_use]
pub fn result_title(outcome: Option<&OutcomeView>) -> &'static str {
    match outcome {
        None => "Game stopped",
        Some(OutcomeView::Win {
            winner: Side::You, ..
        }) => "Victory",
        Some(OutcomeView::Win { .. }) => "Defeat",
        Some(OutcomeView::Draw) => "Draw",
        Some(OutcomeView::Stalled { .. }) => "Stalled",
    }
}

/// Why the game ended in a word, when it was not by damage: the web's `LOSSES` and `STALLS`.
fn reason(outcome: &OutcomeView) -> Option<&'static str> {
    match outcome {
        OutcomeView::Win { reason, .. } => match reason {
            LossReason::Life => None,
            LossReason::Library => Some("Library"),
            LossReason::Effect => Some("Card effect"),
            LossReason::Concede => Some("Conceded"),
        },
        OutcomeView::Draw => None,
        OutcomeView::Stalled { reason } => Some(match reason {
            Stall::DecisionsPerTurn => "too many decisions in one turn",
            Stall::Turns => "too many turns",
            Stall::StateBasedIterations => "state-based actions did not settle",
            Stall::Capacity => "a zone or queue was full",
        }),
    }
}

/// How a game that is no longer going on ended: with an outcome, or none if it was stopped.
struct Ended<'a>(Option<&'a OutcomeView>);

/// How the game ended, or none while it goes on.
fn shown<'a>(input: &ScreenIn<'a>) -> Option<Ended<'a>> {
    match &input.view?.status {
        Status::Over { outcome } => Some(Ended(Some(outcome))),
        Status::Stopped => Some(Ended(None)),
        _ => None,
    }
}

/// What the game screen shows once the view says the game has ended: while the result is shown,
/// one element holding the scrim, the result and its dock, which blurs and darkens everything
/// drawn under it (`Style::backdrop_filter`); while the result is put aside, the dock with the
/// button that brings it back. The holder has no size, so that it covers nothing, and each piece
/// in it stands at its own point of the window.
#[must_use]
pub fn game_over(input: &ScreenIn) -> Option<Element> {
    let Ended(outcome) = shown(input)?;
    let title = result_title(outcome);
    if input.result_aside {
        let back = button(&format!("Back to {title}")).on_press(Press::Result(true));
        return Some(dock(input, vec![back]));
    }
    let buttons = vec![
        button("View Battlefield").on_press(Press::Result(false)),
        primary("New Game").on_press(Press::Open(Screen::Start)),
    ];
    let mut screen = holder()
        .name("game over screen")
        // The web's dialog takes every press over the table, so the scrim does too.
        .child(super::backdrop(input).passes(false))
        .child(result(input, outcome))
        .child(dock(input, buttons));
    screen.style.backdrop_filter = DIMMED.to_vec();
    screen.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    Some(screen)
}

/// The web's `.table-dimmed` filter: `blur(2.5px) brightness(0.68) saturate(0.85)`.
const DIMMED: [Filter; 3] = [
    Filter::Blur(size::GAME_OVER_DIM_BLUR),
    Filter::Brightness(ratio::GAME_OVER_DIM_BRIGHTNESS),
    Filter::Saturate(ratio::GAME_OVER_DIM_SATURATE),
];

/// The result's title, why the game ended and the card that ended it, in a column at the
/// window's middle.
fn result(input: &ScreenIn, outcome: Option<&OutcomeView>) -> Element {
    let phone = super::phone(input);
    // The phone's result is smaller (`.phone-game-over`).
    let (title_style, reason_style, reason_gap) = if phone {
        (
            TypeStyle {
                size: metrics::TITLE,
                ..TITLE
            },
            TypeStyle {
                size: metrics::REASON,
                ..REASON
            },
            metrics::REASON_GAP,
        )
    } else {
        (TITLE, REASON, size::GAME_OVER_REASON_GAP)
    };
    let title = result_title(outcome);
    let mut heading = text(title)
        .style(title_style)
        .color(color::GOLD)
        .passes(true);
    heading.text = Some(title.to_uppercase());
    let culprit = outcome
        .and_then(|outcome| match outcome {
            OutcomeView::Win { by, .. } => by.as_ref(),
            _ => None,
        })
        .filter(|_| outcome.and_then(reason).is_some())
        .and_then(|culprit| culprit_card(culprit, input.faces?, culprit_size(input)));
    let mut screen = column()
        .name("game over")
        .passes(true)
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .child(heading)
        .children(outcome.and_then(reason).map(|why| {
            let mut line = text(why)
                .style(reason_style)
                .color(color::TEXT)
                .passes(true);
            line.style.margin = [reason_gap, 0.0, 0.0, 0.0];
            line
        }))
        .children(culprit);
    // The flat layer's root puts a screen in its bottom right corner, so the group is anchored
    // to the window's middle instead.
    screen.anchor = Some(Anchor::Point(middle(input), Vec2::splat(0.5)));
    screen
}

/// The result's dock, `buttons` from the top down, where the web's action dock stands (`.dock` in
/// `ui/src/screens/styles/controls.css`): `right: 39px; bottom: 87px`, its buttons stacked
/// `--button-gap` apart and as wide as the widest, at least `--button-width`. The web's
/// `DockButtons` puts its first button at the foot, so `New Game` comes last here.
fn dock(input: &ScreenIn, buttons: Vec<Element>) -> Element {
    if super::phone(input) {
        return phone_dock(input, buttons);
    }
    let window = input.window;
    let at = Vec2::new(
        window.size.width - window.safe_area.right - DOCK_RIGHT,
        window.size.height - window.safe_area.bottom - BUTTONS_BOTTOM,
    );
    let mut dock = column()
        .name("game over buttons")
        .gap(BUTTON_GAP)
        .align_items(Align::Stretch)
        .min_width(DOCK_WIDTH)
        .children(buttons);
    dock.anchor = Some(Anchor::Point(at, Vec2::ONE));
    dock
}

/// How far the dock's bottom edge stands above the window's: the web's `.dock` `bottom`.
const BUTTONS_BOTTOM: f32 = size::GAME_OVER_BUTTONS_BOTTOM;
/// How far the dock's right edge stands in from the window's: the web's `.dock` `right`.
const DOCK_RIGHT: f32 = size::GAME_OVER_BUTTONS_RIGHT;
/// The gap between the dock's buttons, the web's `--button-gap`.
const BUTTON_GAP: f32 = space::M;
/// The dock's least width, the web's `--button-width`.
const DOCK_WIDTH: f32 = size::BUTTON_WIDTH;

/// The phone's dock (`.phone-game-over > .dock`): as wide as the window less its gutters, up to
/// `game_over.dock_width`, standing the gutter above the safe area's foot, with each button as
/// wide as the dock.
fn phone_dock(input: &ScreenIn, buttons: Vec<Element>) -> Element {
    let window = input.window;
    let safe = window.safe_area;
    let width =
        (window.size.width - safe.left - safe.right - 2.0 * GUTTER).min(metrics::DOCK_WIDTH);
    let at = Vec2::new(
        safe.left + (window.size.width - safe.left - safe.right) / 2.0,
        window.size.height - safe.bottom - GUTTER,
    );
    let mut dock = column()
        .name("game over buttons")
        .width(width)
        .gap(ACTION_GAP)
        .children(buttons.into_iter().map(super::phone_action));
    dock.anchor = Some(Anchor::Point(at, Vec2::new(0.5, 1.0)));
    dock
}

/// The middle of the window or, on the phone, of its safe area.
fn middle(input: &ScreenIn) -> Vec2 {
    let window = input.window;
    if super::phone(input) {
        let safe = window.safe_area;
        Vec2::new(
            safe.left + (window.size.width - safe.left - safe.right) / 2.0,
            safe.top + (window.size.height - safe.top - safe.bottom) / 2.0,
        )
    } else {
        Vec2::new(window.size.width, window.size.height) / 2.0
    }
}

/// The width of the card that ended the game and the gap above it: the web's 230 on the
/// desktop, and 160 on a phone (140 on one narrower than 393 points).
fn culprit_size(input: &ScreenIn) -> (f32, f32) {
    if !super::phone(input) {
        (CULPRIT_WIDTH, size::GAME_OVER_CARD_GAP)
    } else if input.window.size.width < NARROW_BELOW {
        (metrics::CARD_NARROW, metrics::CARD_GAP)
    } else {
        (metrics::CARD, metrics::CARD_GAP)
    }
}

/// The card that ended the game, drawn flat and whole, or as the plate of the ability that did
/// it when its card's text has one for it. None until the card's face is known.
fn culprit_card(culprit: &Culprit, faces: &FaceList, (width, gap): (f32, f32)) -> Option<Element> {
    let plate = culprit.ability.and_then(|ability| {
        use beryl_protocol::protocol::AbilityClass;
        let triggered = match ability.kind {
            AbilityClass::Triggered => true,
            AbilityClass::Activated => false,
            AbilityClass::Static => return None,
        };
        faces.plate(&culprit.name, triggered, ability.nth)
    });
    let face = plate.or_else(|| faces.find(&culprit.name))?;
    Some(
        Element {
            shows: Some(Shown::Face(face, FaceSize::Full)),
            ..Element::new(Role::Region)
        }
        .name(&culprit.name)
        .passes(true)
        .width(width)
        .height(width * CARD_HEIGHT / CARD_WIDTH)
        .margin([gap, 0.0, 0.0, 0.0]),
    )
}
