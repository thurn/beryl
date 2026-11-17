//! The prompt bar: what the open prompt asks and its buttons, each as the session sent it
//! (docs/beryl-3d.md, "Elements"). Its words, which buttons it has and whether each can be
//! pressed are the session's.

use std::borrow::Cow;

use beryl_protocol::prompt::{ButtonId, PromptButton, PromptView};
use beryl_protocol::protocol::{Side, Status};
use glam::Vec2;

use super::Piles;
use crate::data::space::hud::phone as data;
use crate::element::{Align, Anchor, Element, Press, TextAlign, column, holder, row, text};
use crate::hud::phone::{Occupied, asked_top, clear_at_right};
use crate::screens::ScreenIn;
use crate::table::{PhoneBands, phone_bands};
use crate::text::TypeStyle;
use crate::tokens::{color, space, type_style};
use crate::widgets::{button, icon_button, icons, primary};

/// The prompt bar for `prompt`: its title, then its buttons in the order the session sent them,
/// the one the prompt puts forward drawn as the primary button. The ways to use a card stand
/// over the table instead (`super::ways`).
/// A prompt with no title, such as a priority stop's, shows only its buttons, as the web
/// interface's prompt bar does (`ui/src/stage/PromptBar.tsx`): an empty text would still take
/// a gap beside them.
///
/// While the viewer controls the opponent's turn, a decision they make for the opponent says
/// "For opponent" before what it asks, and a priority says only that, as the web's `Asks` does
/// ([CR 724.5]): "While controlling another player, a player makes all choices and decisions
/// the controlled player is allowed to make or is told to make by the rules or by any objects."
///
/// [CR 724.5]: https://yawgatog.com/resources/magic-rules/#R7245
#[must_use]
pub fn prompt_bar(prompt: &PromptView, input: &ScreenIn) -> Element {
    bar(prompt, input, None, None)
}

/// The prompt bar for `prompt`, as `prompt_bar` draws it, with `after` written in brackets
/// after what it asks, as the web's `Asks` writes how much of a cost is still to pay, and
/// `controls` just before the primary button, as the web's dock stands a number's steps
/// (`Stepper` in `ui/src/stage/PromptBar.tsx`), or after the buttons where none is primary.
pub(super) fn bar(
    prompt: &PromptView,
    input: &ScreenIn,
    after: Option<&str>,
    controls: Option<Element>,
) -> Element {
    let acting_for = input.view.is_some_and(|view| {
        matches!(&view.status, Status::Deciding { decision } if decision.acting_for.is_some())
    });
    let asks: Cow<str> = match (acting_for, prompt.title.is_empty()) {
        (false, _) => Cow::Borrowed(&prompt.title),
        (true, true) => Cow::Borrowed("For opponent"),
        (true, false) => Cow::Owned(format!("For opponent: {}", prompt.title)),
    };
    let asks = match after {
        Some(after) => Cow::Owned(format!("{asks} ({after})")),
        None => asks,
    };
    if input.window.phone() {
        return phone_bar(prompt, input, &asks, controls);
    }
    let title = (!asks.is_empty()).then(|| text(&asks).style(type_style::TITLE).grow(1.0));
    let ways = || super::ways(prompt).map(|way| way.id);
    let buttons =
        (prompt.buttons.iter()).filter(|button_view| !ways().any(|id| id == button_view.id));
    let primary_at = buttons.clone().position(|button_view| button_view.primary);
    let mut controls = controls;
    let mut bar = row()
        .name("prompt")
        .gap(space::M)
        .padding(space::L)
        .children(title);
    for (at, button_view) in buttons.enumerate() {
        if Some(at) == primary_at {
            bar.children.extend(controls.take());
        }
        let widget = if button_view.primary { primary } else { button };
        let pressed = widget(&button_view.label)
            .enabled(button_view.enabled)
            .on_press(Press::Button(button_view.id));
        bar.children.push(if button_view.primary {
            captioned(pressed, &button_view.caption)
        } else {
            pressed
        });
    }
    bar.children.extend(controls);
    bar
}

/// The primary button `pressed` with what it says under its label, `caption`, beneath it, as
/// the web's `.turn-action` stands a `.turn-caption` under the turn button: where a pass leads
/// ("Next" over "To Combat"), or the value an X is set to ("Pay" over "X = 1"). A button with
/// no caption stands alone.
pub(crate) fn captioned(pressed: Element, caption: &str) -> Element {
    if caption.is_empty() {
        return pressed;
    }
    column()
        .name("turn action")
        .gap(space::S)
        .align_items(Align::Center)
        .child(pressed)
        .child(
            text(caption)
                .style(type_style::TURN_CAPTION)
                .color(color::TURN_CAPTION)
                .text_align(TextAlign::Center),
        )
}

/// The prompt bar on the phone layout, as the web interface's `PhonePromptDock` and
/// `PhonePromptOver` (`ui/src/stage/PromptBar.tsx`) compose it from the same buttons:
///
/// - The button the prompt puts forward stands at the right of the viewer's strip, where the
///   turn button is, with the one that passes on beside it. When the viewer's hand holds more
///   cards than one fan has room for, its back row stands in the strip, so the dock rises
///   above the hand's top and leaves each card a strip to press.
/// - Its other buttons, and a number's steps, are stacked over the dock. While the prompt holds
///   cards up and puts no button forward, they stand on the dock's line instead, so that they
///   stay clear of the cards the tray lays out over the viewer's rows (`table::phone::tray`).
/// - Where the raised dock or the stacked buttons would stand over a permanent of the viewer's
///   back row, which they would leave without a square to press, they stand at the right end
///   of the middle band instead, if they fit there and no stack item lies there, and the pill
///   beside them is narrowed to the band's left.
/// - What it asks, `asks`, is a pill on the middle band's line, as Arena writes it across the
///   middle of its table, or at the nearest height clear of every card where one lies on that
///   line (`hud::phone::asked_top`). While the prompt holds cards up, or lists what it asks
///   about over the table, the table lies under a shade and the pill stands over the strip,
///   beside the stacked buttons. The pill lets the pointer through, so it never covers a card
///   the prompt picks.
fn phone_bar(
    prompt: &PromptView,
    input: &ScreenIn,
    asks: &str,
    controls: Option<Element>,
) -> Element {
    let bands = phone_bands(input.window).unwrap_or_default();
    let ways = || super::ways(prompt).map(|way| way.id);
    let buttons: Vec<&PromptButton> = (prompt.buttons.iter())
        .filter(|button_view| !ways().any(|id| id == button_view.id))
        .collect();
    let forward = buttons
        .iter()
        .copied()
        .find(|button_view| button_view.primary);
    let pass_on = (buttons.iter().copied())
        .find(|button_view| button_view.id == ButtonId::PassOn && !button_view.primary);
    let others = (buttons.iter().copied())
        .filter(|button_view| Some(button_view.id) != forward.map(|b| b.id))
        .filter(|button_view| Some(button_view.id) != pass_on.map(|b| b.id));
    let gap = data::DOCK_GAP;
    let height = data::BUTTON_HEIGHT;
    let strip = bands.your_strip;
    let mut middle = strip.middle();
    if let Some(hand_top) = input.occupied.hand_top {
        middle = middle.min(hand_top - gap - height / 2.0);
    }
    let held_up = Piles::of(prompt).is_some();
    let held = held_up || super::covers_table(prompt);
    let right_end = RightEnd {
        bands: &bands,
        occupied: input.occupied,
        held,
    };
    let mut taken: Option<f32> = None;
    let dock = (forward.is_some() || pass_on.is_some()).then(|| {
        let mut dock = dock(forward, pass_on);
        // A dock raised over the hand's back row stands over the viewer's back row, unless the
        // middle band has room for it.
        let size = (
            forward.map_or(0.0, |b| button_width(&b.label) + gap) + pass_on.map_or(0.0, |_| height),
            height,
        );
        if middle < strip.middle()
            && let Some(top) = right_end.moved(size, middle - height / 2.0)
        {
            middle = top + height / 2.0;
            taken = Some(size.0);
        }
        dock.anchor = Some(Anchor::Point(
            Vec2::new(bands.right, middle),
            Vec2::new(1.0, 0.5),
        ));
        dock
    });
    let mut more = column()
        .name("more")
        .gap(gap)
        .align_items(Align::Stretch)
        .children(controls)
        .children(others.clone().map(phone_button));
    let over = if held_up && forward.is_none() && pass_on.is_none() {
        middle + height / 2.0
    } else if taken.is_some() {
        strip.top - gap
    } else {
        (middle - height / 2.0).min(strip.top) - gap
    };
    let stacked = more.children.len();
    #[expect(
        clippy::cast_precision_loss,
        reason = "a prompt has a handful of buttons"
    )]
    let size = (
        others
            .map(|b| button_width(&b.label))
            .fold(data::BUTTON_MIN_WIDTH, f32::max),
        stacked as f32 * (height + gap) - gap,
    );
    let moved = (stacked > 0 && taken.is_none())
        .then(|| right_end.moved(size, over - size.1))
        .flatten();
    if moved.is_some() {
        taken = Some(size.0);
    }
    let more_bottom = moved.map_or(over, |top| top + size.1);
    more.anchor = Some(Anchor::Point(
        Vec2::new(bands.right, more_bottom),
        Vec2::new(1.0, 1.0),
    ));
    let more = (stacked > 0).then_some(more);
    let pill = (!asks.is_empty()).then(|| {
        let place = match (held, taken) {
            (true, _) => PillPlace::Over(over),
            (false, Some(taken)) => PillPlace::Beside(taken),
            (false, None) => PillPlace::Middle,
        };
        pill(asks, place, &bands, input.occupied)
    });
    // The pill comes first, so that the buttons are drawn over it where they meet.
    holder()
        .name("prompt")
        .children(pill)
        .children(more)
        .children(dock)
}

/// The middle band's right end, where the dock or the stacked buttons stand instead of over a
/// permanent of the viewer's back row.
struct RightEnd<'a> {
    bands: &'a PhoneBands,
    occupied: &'a Occupied,
    /// Whether the prompt shows something over the table, which the middle band then lies under.
    held: bool,
}

impl RightEnd<'_> {
    /// The top of a box `size` large at the middle band's right end, when at the column's right
    /// end with its top at `top` it would stand over a permanent, and it fits in the band where
    /// no stack item lies; `None` where it stays at `top`.
    fn moved(&self, size: (f32, f32), top: f32) -> Option<f32> {
        let middle = self.bands.middle.middle() - size.1 / 2.0;
        (!self.held
            && size.1 <= self.bands.middle.height + 2.0 * data::OVERLAP
            && !clear_at_right(self.bands, self.occupied, size, top)
            && clear_at_right(self.bands, self.occupied, size, middle))
        .then_some(middle)
    }
}

/// The dock: the button a prompt puts forward, drawn as the primary button, and the one that
/// passes on beside it.
fn dock(forward: Option<&PromptButton>, pass_on: Option<&PromptButton>) -> Element {
    row()
        .name("dock")
        .gap(data::DOCK_GAP)
        .align_items(Align::Center)
        .children(forward.map(phone_button))
        .children(pass_on.map(|button_view| {
            icon_button(icons::PASS_ON, &button_view.label)
                .enabled(button_view.enabled)
                .on_press(Press::Button(button_view.id))
        }))
}

/// Where the pill with what a prompt asks stands on the phone.
#[derive(Clone, Copy)]
enum PillPlace {
    /// Centred on the middle band's line, or at the nearest height clear of every card.
    Middle,
    /// At the same height, at the column's left, narrowed for buttons this wide at the middle
    /// band's right end.
    Beside(f32),
    /// Over the viewer's strip, its foot at this height, beside the stacked buttons, while the
    /// table lies under a shade.
    Over(f32),
}

/// The pill that says what a prompt asks, `asks`, at `place`: a fixed width, so that a long
/// question is laid out on as many lines as it takes. It lets the pointer through, so it never
/// covers a card the prompt picks.
fn pill(asks: &str, place: PillPlace, bands: &PhoneBands, occupied: &Occupied) -> Element {
    let style = TypeStyle {
        size: data::PILL_TEXT,
        ..type_style::TITLE
    };
    let padding = data::PILL_PADDING;
    let pill_height = style.size * style.line_height + padding;
    let width = bands.right - bands.left;
    let taken = match place {
        PillPlace::Middle => None,
        PillPlace::Beside(taken) => Some(taken),
        PillPlace::Over(_) => Some(data::BUTTON_MIN_WIDTH),
    };
    let mut pill = text(asks)
        .style(style)
        .text_align(TextAlign::Center)
        .width(taken.map_or(width, |taken| width - taken - data::DOCK_GAP))
        .min_height(pill_height)
        .background(color::PANEL)
        .radius(pill_height / 2.0)
        .passes(true);
    pill.style.padding = [padding / 2.0, padding, padding / 2.0, padding];
    let top = || asked_top(bands, occupied, pill_height);
    pill.anchor = Some(match place {
        PillPlace::Over(over) => Anchor::Point(Vec2::new(bands.left, over), Vec2::new(0.0, 1.0)),
        PillPlace::Beside(_) => Anchor::Point(Vec2::new(bands.left, top()), Vec2::ZERO),
        PillPlace::Middle => Anchor::Point(
            Vec2::new(f32::midpoint(bands.left, bands.right), top()),
            Vec2::new(0.5, 0.0),
        ),
    });
    pill
}

/// About how wide a phone button with `label` is drawn, from the count of its characters, at
/// least `BUTTON_MIN_WIDTH`: an estimate on the generous side, which only decides whether the
/// button keeps clear of the table's cards where it stands.
fn button_width(label: &str) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a button's label is a few words"
    )]
    let characters = label.chars().count() as f32;
    (characters * data::BUTTON_TEXT * data::CHARACTER_WIDTH + 2.0 * data::BUTTON_PADDING)
        .max(data::BUTTON_MIN_WIDTH)
}

/// One of a prompt's buttons on the phone, as large as a finger (`.phone-dock .button`): the
/// primary one drawn as the primary button.
fn phone_button(button_view: &PromptButton) -> Element {
    let widget = if button_view.primary { primary } else { button };
    phone_sized(
        widget(&button_view.label)
            .enabled(button_view.enabled)
            .on_press(Press::Button(button_view.id)),
    )
}

/// `element`, a button, as large as a finger (`.phone-dock .button`).
fn phone_sized(element: Element) -> Element {
    let mut element = element
        .height(data::BUTTON_HEIGHT)
        .min_width(data::BUTTON_MIN_WIDTH)
        .style(TypeStyle {
            size: data::BUTTON_TEXT,
            ..type_style::BUTTON
        });
    let padding = data::BUTTON_PADDING;
    element.style.padding = [0.0, padding, 0.0, padding];
    element
}

/// The words on the turn button's place while the opponent acts, Arena's label, which the
/// web's dock shows the same way (`waitingLabel` in `ui/src/stage/PromptBar.tsx`).
const OPPONENTS_TURN: &str = "Opponent's Turn";

/// What stands where the prompt's buttons do while no prompt is open and the opponent acts: a
/// disabled button saying "Opponent's Turn", as the web's dock shows with no spinner under the
/// opponent's orb. The opponent acts while they decide, or while their turn plays on without
/// the viewer, as the web's `opponentActing` says. `None` at any other time.
#[must_use]
pub fn waiting(input: &ScreenIn) -> Option<Element> {
    let view = input.view?;
    let acting = match view.status {
        Status::Thinking => true,
        Status::Playing => view.turn.active == Side::Opponent,
        _ => false,
    };
    if !acting || input.prompt.is_some() {
        return None;
    }
    let label = button(OPPONENTS_TURN).enabled(false);
    if !input.window.phone() {
        return Some(
            row()
                .name("prompt")
                .gap(space::M)
                .padding(space::L)
                .child(label),
        );
    }
    let bands = phone_bands(input.window).unwrap_or_default();
    let mut label = phone_sized(label);
    label.anchor = Some(Anchor::Point(
        Vec2::new(bands.right, bands.your_strip.middle()),
        Vec2::new(1.0, 0.5),
    ));
    Some(holder().name("prompt").child(label))
}
