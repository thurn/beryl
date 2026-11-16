//! Game over, shown on the game screen once the game has ended: a port of the web interface's
//! `GameOver` (`ui/src/screens/GameOver.tsx`) with the rules of `.game-over` in
//! `ui/src/screens/styles/opening.css`. A scrim over the table, the result in large gold
//! capitals at the window's vertical middle, why it ended beneath it, the card that ended it
//! (when the session saw one) and a button back to the start screen.
//!
//! The scrim lets the pointer through, so the table stays reachable until the "View Battlefield"
//! button, which the web uses to uncover it, is built.
//!
//! Not built yet: the web's small icon beside the reason, and its "View Battlefield" button,
//! which needs the client to remember that the result was put aside.

use beryl_protocol::protocol::{Culprit, LossReason, OutcomeView, Side, Stall, Status};
use glam::Vec2;

use super::{Screen, ScreenIn};
use crate::element::{Align, Anchor, Element, Justify, Press, Shown, column, row, text};
use crate::faces::FaceList;
use crate::frame::Role;
use crate::input::FaceSize;
use crate::table::{CARD_HEIGHT, CARD_WIDTH};
use crate::text::TypeStyle;
use crate::text::fonts::Font;
use crate::tokens::color;
use crate::widgets::primary;

/// The width of the card that ended the game, the web's 230 on the desktop.
const CULPRIT_WIDTH: f32 = 230.0;

/// The result's title, `.game-over-title`: 72 points, bold.
const TITLE: TypeStyle = TypeStyle {
    font: Font::CrimsonPro,
    weight: 700,
    size: 72.0,
    line_height: 1.1,
};

/// Why the game ended, `.game-over-reason`: 24 points of interface text.
const REASON: TypeStyle = TypeStyle {
    font: Font::OpenSans,
    weight: 400,
    size: 24.0,
    line_height: 1.3,
};

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

/// The result over the game, once the view says it has ended.
#[must_use]
pub fn game_over(input: &ScreenIn) -> Option<Element> {
    let Ended(outcome) = shown(input)?;
    let title = result_title(outcome);
    let mut heading = text(title).style(TITLE).color(color::GOLD).passes(true);
    heading.text = Some(title.to_uppercase());
    let culprit = outcome
        .and_then(|outcome| match outcome {
            OutcomeView::Win { by, .. } => by.as_ref(),
            _ => None,
        })
        .filter(|_| outcome.and_then(reason).is_some())
        .and_then(|culprit| culprit_card(culprit, input.faces?));
    let mut screen = column()
        .name("game over")
        .passes(true)
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .child(heading)
        .children(outcome.and_then(reason).map(|why| {
            let mut line = text(why).style(REASON).color(color::TEXT).passes(true);
            line.style.margin = [14.0, 0.0, 0.0, 0.0];
            line
        }))
        .children(culprit);
    // The flat layer's root puts a screen in its bottom right corner, so the group is anchored
    // to the window's middle instead.
    screen.anchor = Some(Anchor::Point(middle(input), Vec2::splat(0.5)));
    Some(screen)
}

/// The button back to the start screen, at the bottom middle of the window where the web's dock
/// puts the result's buttons.
#[must_use]
pub fn buttons(input: &ScreenIn) -> Option<Element> {
    shown(input)?;
    let at = Vec2::new(
        input.window.size.width / 2.0,
        input.window.size.height - BUTTONS_BOTTOM,
    );
    let mut dock = row()
        .name("game over buttons")
        .child(primary("New Game").on_press(Press::Open(Screen::Start)));
    dock.anchor = Some(Anchor::Point(at, Vec2::new(0.5, 1.0)));
    Some(dock)
}

/// How far the buttons' bottom edge stands above the window's: the root's dock offset.
const BUTTONS_BOTTOM: f32 = 87.0;

/// The middle of the window.
fn middle(input: &ScreenIn) -> Vec2 {
    Vec2::new(input.window.size.width, input.window.size.height) / 2.0
}

/// The backdrop over the table (`screens::backdrop`), which the pointer goes through so that
/// the table stays reachable beneath it.
#[must_use]
pub fn scrim(input: &ScreenIn) -> Option<Element> {
    shown(input)?;
    Some(super::backdrop(input))
}

/// The card that ended the game, drawn flat and whole, or as the plate of the ability that did
/// it when its card's text has one for it. None until the card's face is known.
fn culprit_card(culprit: &Culprit, faces: &FaceList) -> Option<Element> {
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
        .width(CULPRIT_WIDTH)
        .height(CULPRIT_WIDTH * CARD_HEIGHT / CARD_WIDTH)
        .margin([22.0, 0.0, 0.0, 0.0]),
    )
}
