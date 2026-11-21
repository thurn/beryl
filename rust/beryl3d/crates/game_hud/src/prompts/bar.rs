//! The prompt bar: what the open prompt asks and its buttons, each as the session sent it
//! (docs/beryl-3d.md, "Elements"). Its words, which buttons it has and whether each can be
//! pressed are the session's.

use beryl_protocol::prompt::Click;
use std::borrow::Cow;

use beryl_protocol::prompt::{ButtonId, PromptButton, PromptView};
use beryl_protocol::protocol::{Side, Status};
use glam::Vec2;

use super::Piles;
use crate::hud::HudIn;
use crate::hud::phone::{Occupied, asked_top, clear_at_right, phase_span};
use game_table::table::{PhoneBands, phone_bands};
use kit_base::data::space::hud::phone as data;
use kit_base::tokens::{color, shadow, size, space, type_style};
use kit_text::text::TypeStyle;
use kit_ui::element::{Align, Anchor, Element, Position, TextAlign, column, holder, row, text};
use kit_ui::widgets::{self, button, icon_button, icons, primary};

/// The prompt bar for `prompt`: on the desktop, the action dock (`desktop_dock`), with what the prompt
/// asks at its top, its other buttons stacked under that in the order the session sent them,
/// the first at the foot, and the button it puts forward at the dock's foot as the turn button,
/// with the one that passes on beside it and its caption under it, as the web's `PromptDock`
/// (`ui/src/stage/PromptBar.tsx`) lays them out. The ways to use a card stand over the table
/// instead (`super::ways`).
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
pub fn prompt_bar(prompt: &PromptView, input: &HudIn) -> Element {
    bar(prompt, input, None, None)
}

/// Controls a prompt adds to its bar, such as a number's steps, and about how large they are
/// drawn, in logical points: an estimate on the generous side, as `button_width` makes, which
/// only decides where they stand on the phone.
pub(super) struct Controls {
    pub element: Element,
    pub size: (f32, f32),
}

/// The prompt bar for `prompt`, as `prompt_bar` draws it, with `after` written in brackets
/// after what it asks, as the web's `Asks` writes how much of a cost is still to pay, and
/// `controls` over the turn button, as the web's dock stands a number's steps (`Stepper` in
/// `ui/src/stage/PromptBar.tsx`).
pub(super) fn bar(
    prompt: &PromptView,
    input: &HudIn,
    after: Option<&str>,
    controls: Option<Controls>,
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
    let title = (!asks.is_empty()).then(|| {
        text(&asks)
            .style(type_style::TITLE)
            .text_align(TextAlign::Center)
    });
    let ways = || super::ways(prompt).map(|way| way.id);
    let buttons: Vec<&PromptButton> = (prompt.buttons.iter())
        .filter(|button_view| !ways().any(|id| id == button_view.id))
        .collect();
    let forward = buttons
        .iter()
        .copied()
        .find(|button_view| button_view.primary);
    let pass_on = (buttons.iter().copied()).find(|button_view| button_view.id == ButtonId::PassOn);
    let same = |a: Option<&PromptButton>, b: &PromptButton| a.is_some_and(|a| a.id == b.id);
    // The first of the other buttons stands at the foot and the rest stack over it, as the
    // web's `.dock-buttons` lays them out in a reversed column.
    let others: Vec<Element> = (buttons.iter().copied())
        .filter(|button_view| !same(forward, button_view) && !same(pass_on, button_view))
        .rev()
        .map(|button_view| {
            dock_sized(
                button(&button_view.label)
                    .enabled(button_view.enabled)
                    .on_press(Click::Button {
                        button: button_view.id,
                    }),
            )
        })
        .collect();
    desktop_dock()
        .children(title)
        .children(others)
        .children(controls.map(|controls| controls.element))
        .children(forward.map(|forward| {
            turn_action(
                dock_sized(
                    primary(&forward.label)
                        .enabled(forward.enabled)
                        .on_press(Click::Button { button: forward.id }),
                ),
                pass_on,
                &forward.caption,
            )
        }))
}

/// The desktop's action dock, as the web's `.dock` in `ui/src/screens/styles/controls.css`
/// lays it out in the window's bottom right corner (`layout::DOCK_RIGHT` and `DOCK_BOTTOM`): a
/// column of buttons of one width, at least a turn button's (`--button-width`), with what the
/// prompt asks over them and the turn button at the foot. It draws no panel of its own, as the
/// web's dock draws none.
pub fn desktop_dock() -> Element {
    column()
        .name("prompt")
        .gap(space::M)
        .align_items(Align::Stretch)
        .min_width(size::BUTTON_WIDTH)
        .max_width(size::DOCK_MAX_WIDTH)
        .background([0; 4])
        .border(([0; 4], 0.0))
}

/// `element`, a button of the dock, as wide as the dock (`.dock .button`: `min-width: 0;
/// width: 100%`).
pub fn dock_sized(mut element: Element) -> Element {
    element.style.min_width = Some(0.0);
    element.align_self(Align::Stretch)
}

/// The turn button `pressed` at the dock's foot, as the web's `.turn-action` stands it: the
/// round button that passes on, `pass_on`, to its left, and what it says under its label,
/// `caption`, centred under it. Neither takes room in the dock's column, so the turn button
/// stands at the same place whatever is beside or under it.
pub fn turn_action(pressed: Element, pass_on: Option<&PromptButton>, caption: &str) -> Element {
    let gap = space::M;
    let height = size::BUTTON_HEIGHT;
    let pass_on = pass_on.map(|button_view| {
        // `.dock .turn-action .pass-on-button`: a circle as tall as the turn button, its right
        // edge the dock's gap left of the turn button's left edge.
        widgets::pass_on(&button_view.label, height)
            .enabled(button_view.enabled)
            .on_press(Click::Button {
                button: button_view.id,
            })
            .position(Position::Absolute)
            .inset([Some(0.0), None, None, Some(-(height + gap))])
    });
    let caption = (!caption.is_empty()).then(|| {
        text(caption)
            .style(type_style::TURN_CAPTION)
            .color(color::TURN_CAPTION)
            .text_align(TextAlign::Center)
            .position(Position::Absolute)
            .inset([Some(height + size::TURN_CAPTION_GAP), None, None, None])
    });
    column()
        .name("turn action")
        .align_items(Align::Center)
        .child(pressed)
        .children(pass_on)
        .children(caption)
}

/// The primary button `pressed` with what it says under its label, `caption`, beneath it, as
/// the web's `.turn-action` stands a `.turn-caption` under the turn button: where a pass leads
/// ("Next" over "To Combat"), or the value an X is set to ("Pay" over "X = 1"). A button with
/// no caption stands alone.
pub fn captioned(pressed: Element, caption: &str) -> Element {
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
///   back row, which they would leave without a square to press, they stand in the middle band
///   instead, if they fit there clear of every card and the phase icon: the dock at the band's
///   right end, and the stacked buttons there, beside the dock, or at the band's left end. The
///   pill is narrowed to the room between them.
/// - What it asks, `asks`, is a pill on the middle band's line, as Arena writes it across the
///   middle of its table, or at the nearest height clear of every card where one lies on that
///   line (`hud::phone::asked_top`). While the prompt holds cards up, or lists what it asks
///   about over the table, the table lies under a shade and the pill stands over the strip,
///   beside the stacked buttons. The pill lets the pointer through, so it never covers a card
///   the prompt picks.
fn phone_bar(
    prompt: &PromptView,
    input: &HudIn,
    asks: &str,
    controls: Option<Controls>,
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
    let held_up = Piles::of(prompt).is_some();
    let held = held_up || super::covers_table(prompt);
    let right_end = RightEnd {
        bands: &bands,
        occupied: input.occupied,
        held,
        phase: phase_span(input, &bands),
    };
    let mut taken: Option<f32> = None;
    let mut middle = right_end.dock_line(input.occupied.hand_top, 0.0).0;
    let dock = (forward.is_some() || pass_on.is_some()).then(|| {
        let mut dock = dock(forward, pass_on);
        let width =
            forward.map_or(0.0, |b| button_width(&b.label) + gap) + pass_on.map_or(0.0, |_| height);
        let moved;
        (middle, moved) = right_end.dock_line(input.occupied.hand_top, width);
        if moved {
            taken = Some(width);
        }
        dock.anchor = Some(Anchor::Point(
            Vec2::new(bands.right, middle),
            Vec2::new(1.0, 0.5),
        ));
        dock
    });
    let controls_size = controls.as_ref().map(|controls| controls.size);
    let mut more = column()
        .name("more")
        .gap(gap)
        .align_items(Align::Stretch)
        .children(controls.map(|controls| controls.element))
        .children(others.clone().map(phone_button));
    let over = if held_up && forward.is_none() && pass_on.is_none() {
        middle + height / 2.0
    } else if taken.is_some() {
        strip.top - gap
    } else {
        (middle - height / 2.0).min(strip.top) - gap
    };
    let stacked = more.children.len();
    let size = others
        .map(|b| (button_width(&b.label), height))
        .chain(controls_size)
        .fold((data::BUTTON_MIN_WIDTH, -gap), |(width, tall), (w, h)| {
            (width.max(w), tall + h + gap)
        });
    let mut taken_left = None;
    let place = (stacked > 0)
        .then(|| right_end.stacked(size, over - size.1, taken))
        .flatten();
    match place {
        Some(Stacked::Left) => taken_left = Some(size.0),
        Some(Stacked::Right(right, _)) => taken = Some(bands.right - right + size.0),
        None => {}
    }
    let (more_right, more_bottom) = match place {
        Some(Stacked::Left) => (bands.left + size.0, bands.middle.middle() + size.1 / 2.0),
        Some(Stacked::Right(right, top)) => (right, top + size.1),
        None => (bands.right, over),
    };
    more.anchor = Some(Anchor::Point(
        Vec2::new(more_right, more_bottom),
        Vec2::new(1.0, 1.0),
    ));
    let more = (stacked > 0).then_some(more);
    let pill = (!asks.is_empty()).then(|| {
        let place = match (held, taken, taken_left) {
            (true, ..) => PillPlace::Over(over),
            // Between buttons at both ends the room left is too narrow for the words on one
            // line, and a second line would reach over the creatures' rows, so the pill stays
            // centred at its full width, under the buttons where they meet, as the web's
            // centred pill stands beside its buttons.
            (false, None, None) | (false, Some(_), Some(_)) => PillPlace::Middle,
            (false, right, left) => PillPlace::Beside {
                left: left.unwrap_or(0.0),
                right: right.unwrap_or(0.0),
            },
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

/// Where the stacked buttons stand in the middle band, instead of over the strip.
#[derive(Clone, Copy)]
enum Stacked {
    /// At the band's left end.
    Left,
    /// With their right edge at this distance from the window's left, and their top at this
    /// height: at the band's right end, or beside the dock there.
    Right(f32, f32),
}

/// The middle band's ends, where the dock or the stacked buttons stand instead of over a
/// permanent of the viewer's back row.
struct RightEnd<'a> {
    bands: &'a PhoneBands,
    occupied: &'a Occupied,
    /// Whether the prompt shows something over the table, which the middle band then lies under.
    held: bool,
    /// The left and right edges of the phase icon in the middle band, which a box moved there
    /// keeps clear of, since the icon is drawn over the flat layer's buttons.
    phase: (f32, f32),
}

impl RightEnd<'_> {
    /// The line the middle of a row of buttons `width` wide stands on at the column's right end,
    /// as the dock does, while the top edge of the viewer's hand at rest is `hand_top` points
    /// from the window's top: the middle of the viewer's strip; or higher, the dock's gap over
    /// the hand, where the hand's back row stands in the strip, so that the buttons leave each
    /// card of that row a strip to press; or, where the raised row would stand over a permanent
    /// of the viewer's back row, at the middle band's right end if it fits there (`moved`). It
    /// says too whether the row stands in the middle band. A `width` of nothing gives the
    /// raised line, never moved.
    fn dock_line(&self, hand_top: Option<f32>, width: f32) -> (f32, bool) {
        let height = data::BUTTON_HEIGHT;
        let strip = self.bands.your_strip.middle();
        let middle = hand_top.map_or(strip, |top| strip.min(top - data::DOCK_GAP - height / 2.0));
        if width > 0.0
            && middle < strip
            && let Some(top) = self.moved((width, height), middle - height / 2.0)
        {
            return (top + height / 2.0, true);
        }
        (middle, false)
    }

    /// The top of a box `size` large at the middle band's right end, when at the column's right
    /// end with its top at `top` it would stand over a permanent, and it fits in the band where
    /// no stack item lies; `None` where it stays at `top`.
    fn moved(&self, size: (f32, f32), top: f32) -> Option<f32> {
        self.moved_to(size, top, self.bands.right)
    }

    /// Where the stacked buttons, a box `size` large, stand in the middle band, when over the
    /// strip at the column's right end with their top at `top` they would stand over a
    /// permanent of the viewer's back row: at the band's right end, or beside the dock where
    /// the dock already takes `taken` points there, or else at its left end. `None` where they
    /// stay over the strip.
    fn stacked(&self, size: (f32, f32), top: f32, taken: Option<f32>) -> Option<Stacked> {
        let beside = self.bands.right - taken.map_or(0.0, |taken| taken + data::DOCK_GAP);
        if let Some(moved) = self.moved_to(size, top, beside) {
            return Some(Stacked::Right(beside, moved));
        }
        self.moved_to(size, top, self.bands.left + size.0)
            .map(|_| Stacked::Left)
    }

    /// As `moved`, for a box whose right edge stands at `right` in the middle band, beside
    /// whatever already stands between it and the band's right end. The box must stay inside
    /// the column and clear of the phase icon. A box may be a point taller than the band and its
    /// overlaps, since its size is an estimate on the generous side: a number's stepper with a
    /// cost under its value, 52 points tall, then still fits in the 44-point band at 375 by 647
    /// that the creatures' rows leave, as the check that it is clear of every card confirms.
    fn moved_to(&self, size: (f32, f32), top: f32, right: f32) -> Option<f32> {
        let middle = self.bands.middle.middle() - size.1 / 2.0;
        let (left, gap) = (right - size.0, data::DOCK_GAP);
        let (phase_left, phase_right) = self.phase;
        (!self.held
            && size.1 <= self.bands.middle.height + 2.0 * data::OVERLAP + 1.0
            && left >= self.bands.left
            && (right + gap <= phase_left || left - gap >= phase_right)
            && !clear_at_right(self.bands, self.occupied, size, top, 0.0)
            && clear_at_right(
                self.bands,
                self.occupied,
                size,
                middle,
                self.bands.right - right,
            ))
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
            // The web's pass-on button is not an icon button: its face (`.pass-on-face` in
            // `controls.css`) casts no box shadow.
            icon_button(icons::PASS_ON, &button_view.label)
                .box_shadow(Vec::new())
                .drop_shadow(shadow::PASS_ON.to_vec())
                .enabled(button_view.enabled)
                .on_press(Click::Button {
                    button: button_view.id,
                })
        }))
}

/// Where the pill with what a prompt asks stands on the phone.
#[derive(Clone, Copy)]
enum PillPlace {
    /// Centred on the middle band's line, or at the nearest height clear of every card.
    Middle,
    /// At the same height, narrowed for buttons this wide at the middle band's left or right
    /// end.
    Beside { left: f32, right: f32 },
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
    // The room taken at each end of the column, with the gap beside it.
    let room = |taken: f32| {
        if taken > 0.0 {
            taken + data::DOCK_GAP
        } else {
            0.0
        }
    };
    let (left, right) = match place {
        PillPlace::Middle => (0.0, 0.0),
        PillPlace::Beside { left, right } => (room(left), room(right)),
        PillPlace::Over(_) => (0.0, room(data::BUTTON_MIN_WIDTH)),
    };
    let mut pill = text(asks)
        .style(style)
        .text_align(TextAlign::Center)
        .width(width - left - right)
        .min_height(pill_height)
        .background(color::PANEL)
        .radius(pill_height / 2.0)
        .enter(&kit_ui::widgets::screen::SCREEN_INSTANT)
        .passes(true);
    pill.style.padding = [padding / 2.0, padding, padding / 2.0, padding];
    let top = || asked_top(bands, occupied, pill_height);
    pill.anchor = Some(match place {
        PillPlace::Over(over) => Anchor::Point(Vec2::new(bands.left, over), Vec2::new(0.0, 1.0)),
        PillPlace::Beside { .. } => Anchor::Point(Vec2::new(bands.left + left, top()), Vec2::ZERO),
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
            .on_press(Click::Button {
                button: button_view.id,
            }),
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
pub fn waiting(input: &HudIn) -> Option<Element> {
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
        return Some(desktop_dock().child(turn_action(dock_sized(label), None, "")));
    }
    // It stands where the dock would, raised over a hand whose back row stands in the strip. Where
    // that would cover a permanent of the viewer's back row and the label is too wide for the
    // middle band beside the phase icon, it narrows to the room there, and its words shrink to
    // fit, as a button's do.
    let bands = phone_bands(input.window).unwrap_or_default();
    let right_end = RightEnd {
        bands: &bands,
        occupied: input.occupied,
        held: false,
        phase: phase_span(input, &bands),
    };
    let (hand_top, wide) = (input.occupied.hand_top, button_width(OPPONENTS_TURN));
    let room = bands.right - right_end.phase.1 - data::DOCK_GAP;
    let (mut middle, moved) = right_end.dock_line(hand_top, wide);
    let mut narrowed = None;
    if !moved && (data::BUTTON_MIN_WIDTH..wide).contains(&room) {
        let (line, fits) = right_end.dock_line(hand_top, room);
        if fits {
            (middle, narrowed) = (line, Some(room));
        }
    }
    let mut label = phone_sized(label);
    label.style.width = narrowed;
    label.anchor = Some(Anchor::Point(
        Vec2::new(bands.right, middle),
        Vec2::new(1.0, 0.5),
    ));
    Some(holder().name("prompt").child(label))
}
