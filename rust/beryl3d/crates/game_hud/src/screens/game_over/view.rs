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

use beryl_protocol::protocol::{LossReason, OutcomeView, Side, Stall};
use glam::Vec2;

use super::{Inputs, Message, State};

use game_table::table::{CARD_HEIGHT, CARD_WIDTH};
use kit_base::tokens::{color, ratio, size, space, type_style};
use kit_frame::frame::Role;
use kit_text::text::TypeStyle;
use kit_ui::element::{Align, Anchor, Element, Filter, Justify, Shown, column, holder, text};
use kit_ui::input::FaceSize;
use kit_ui::widgets::screen::backdrop;
use kit_ui::widgets::{button, primary};

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

/// What the game screen shows once the view says the game has ended: while the result is shown,
/// one element holding the scrim, the result and its dock, which blurs and darkens everything
/// drawn under it (`Style::backdrop`); while the result is put aside, the dock with the
/// button that brings it back. The holder has no size, so that it covers nothing, and each piece
/// in it stands at its own point of the window.
#[must_use]
pub(super) fn game_over(state: &State, input: &Inputs) -> Element {
    let outcome = input.outcome.as_ref();
    let title = result_title(outcome);
    if state.aside {
        let back = button(&format!("Back to {title}")).on_press(Message::Shown(true));
        return dock(input, vec![back]);
    }
    let buttons = vec![
        button("View Battlefield").on_press(Message::Shown(false)),
        primary("New Game").on_press(Message::NewGame),
    ];
    let mut screen = holder()
        .name("game over screen")
        // The web's dialog takes every press over the table, so the scrim does too.
        .child(backdrop(input.window.size).passes(false))
        .child(result(input, outcome))
        .child(dock(input, buttons));
    screen.style.filter = DIMMED.to_vec();
    screen.style.backdrop = true;
    screen.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    screen
}

/// The web's `.table-dimmed` filter: `blur(2.5px) brightness(0.68) saturate(0.85)`.
const DIMMED: [Filter; 3] = [
    Filter::Blur(size::GAME_OVER_DIM_BLUR),
    Filter::Brightness(ratio::GAME_OVER_DIM_BRIGHTNESS),
    Filter::Saturate(ratio::GAME_OVER_DIM_SATURATE),
];

/// The result's title, why the game ended and the card that ended it, in a column at the
/// window's middle.
fn result(input: &Inputs, outcome: Option<&OutcomeView>) -> Element {
    let layout = input.layout;
    let title_style = TypeStyle {
        size: layout.title,
        ..TITLE
    };
    let reason_style = TypeStyle {
        size: layout.reason,
        ..REASON
    };
    let title = result_title(outcome);
    let mut heading = text(title)
        .style(title_style)
        .color(color::GOLD)
        .passes(true);
    heading.text = Some(title.to_uppercase());
    let culprit = outcome.and_then(|outcome| match outcome {
        OutcomeView::Win { by: Some(by), .. } if reason(outcome).is_some() => input
            .culprit
            .map(|face| culprit_card(&by.name, face, culprit_size(input))),
        _ => None,
    });
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
            line.style.margin = [layout.reason_gap, 0.0, 0.0, 0.0];
            line
        }))
        .children(culprit);
    // The flat layer's root puts a screen in its bottom right corner, so the group is anchored
    // to the window's middle instead.
    let area = input.area;
    let centre = Vec2::new(area.x + area.width / 2.0, area.y + area.height / 2.0);
    screen.anchor = Some(Anchor::Point(centre, Vec2::splat(0.5)));
    screen
}

/// The result's dock, `buttons` from the top down, where the web's action dock stands (`.dock` in
/// `ui/src/screens/styles/controls.css`): `right: 39px; bottom: 87px`, its buttons stacked
/// `--button-gap` apart and as wide as the widest, at least `--button-width`. The web's
/// `DockButtons` puts its first button at the foot, so `New Game` comes last here.
fn dock(input: &Inputs, buttons: Vec<Element>) -> Element {
    let area = input.dock;
    if let Some(layout) = input.column {
        return layout.actions_at_foot("game over buttons", buttons, area);
    }
    let mut dock = column()
        .name("game over buttons")
        .gap(space::M)
        .align_items(Align::Stretch)
        .min_width(size::BUTTON_WIDTH)
        .children(buttons);
    dock.anchor = Some(Anchor::Point(
        Vec2::new(area.x + area.width, area.y + area.height),
        Vec2::ONE,
    ));
    dock
}

/// The width of the card that ended the game and the gap above it.
fn culprit_size(input: &Inputs) -> (f32, f32) {
    let layout = input.layout;
    let width = layout
        .narrow_card
        .filter(|card| input.window.size.width < card.below)
        .map_or(layout.card, |card| card.width);
    (width, layout.card_gap)
}

/// The card that ended the game, or its ability plate.
fn culprit_card(name: &str, face: kit_ui::input::FaceId, (width, gap): (f32, f32)) -> Element {
    Element {
        shows: Some(Shown::Face(face, FaceSize::Full)),
        ..Element::new(Role::Region)
    }
    .name(name)
    .passes(true)
    .width(width)
    .height(width * CARD_HEIGHT / CARD_WIDTH)
    .margin([gap, 0.0, 0.0, 0.0])
}
