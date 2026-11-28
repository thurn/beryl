//! The prompt bar: what the open prompt asks and its buttons, each as the session sent it
//! (docs/beryl-3d.md, "Elements"). Its words, which buttons it has and whether each can be
//! pressed are the session's.

use kit_base::data::profiles::SlotName;
use std::borrow::Cow;

use beryl_protocol::prompt::{ButtonId, PromptButton, PromptView, Purpose, ShapeBody};
use beryl_protocol::protocol::{Side, Status};
use glam::Vec2;

use super::Message;
use super::question::{fit_question_dock, question_title};
use crate::hud::HudIn;
use crate::hud::phase::{clear_of_shared_piles, phase_box};
use crate::hud::placement_scene::{Group, PlacementScene, inset, rect};
use crate::prompts::{bar_buttons, covers_table, holds_up};
use game_table::profiles::Profile;
use kit_base::data::profiles::{FloatingPrompt, Piece};
use kit_base::geometry::Rect;
use kit_base::tokens::{color, shadow, size, space, type_style};
use kit_scene::NodeQuery;
use kit_text::text::TypeStyle;
use kit_ui::element::{
    Align, Anchor, Element, LayerDepth, Position, TextAlign, column, holder, row, text,
};
use kit_ui::layout::{DOCK_BOTTOM, DOCK_RIGHT};
use kit_ui::widgets::{self, button, icon_button, icons, primary};

mod pill;
use pill::{PillPlace, pill};

/// Controls a prompt adds to its bar, such as a number's steps, and about how large they are
/// drawn, in logical points: an estimate on the generous side, as `button_width` makes, which
/// only decides where they stand on the phone.
pub(in crate::prompts) struct Controls {
    pub element: Element,
    pub size: (f32, f32),
}

/// The prompt title with the side and any remaining amount the bar displays.
#[must_use]
pub fn title(prompt: &PromptView, acting_for: bool) -> Cow<'_, str> {
    let asks = match (acting_for, prompt.title.is_empty()) {
        (false, _) => Cow::Borrowed(prompt.title.as_str()),
        (true, true) => Cow::Borrowed("For opponent"),
        (true, false) => Cow::Owned(format!("For opponent: {}", prompt.title)),
    };
    match &prompt.body {
        ShapeBody::Pay { cost, left } if super::super::pay::shows_remaining(prompt, cost, left) => {
            Cow::Owned(format!("{asks} ({left})"))
        }
        ShapeBody::Divide { left, .. } => Cow::Owned(format!("{asks} ({left})")),
        _ => asks,
    }
}

/// The prompt bar for `prompt`: on the desktop, the action dock (`captioned_dock`), with what the prompt
/// asks at its top, its other buttons stacked under that in the order the session sent them,
/// the first at the foot, and the button it puts forward at the dock's foot as the turn button,
/// with the one that passes on beside it and its caption under it, as the web's `PromptDock`
/// (`ui/src/stage/PromptBar.tsx`) lays them out. The ways to use a card stand over the table
/// instead (`ways`).
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
pub(super) fn bar(prompt: &PromptView, input: &HudIn, controls: Option<Controls>) -> Element {
    let acting_for = input.view.is_some_and(acting_for);
    let asks = title(prompt, acting_for);
    let held_up = holds_up(prompt);
    let question = matches!(prompt.body, ShapeBody::YesNo { .. });
    if let Some(slots) = PromptSlots::new(input, held_up || covers_table(prompt)) {
        let mut bar = floating_bar(prompt, &asks, controls, &slots, held_up);
        if question && held_up {
            bar.layer_depth = LayerDepth::AboveCards;
        }
        return bar;
    }
    if let Some(heading) = super::heading::hand_heading(prompt, input) {
        return super::heading::bar(prompt, &asks, input, controls, heading);
    }
    // A discard title shares the button's width so the dock stays beside the hand.
    let fitted_dock = question || prompt.is(Purpose::Discard);
    let title = (!asks.is_empty()).then(|| {
        if fitted_dock {
            question_title(&asks, prompt.title.encode_utf16().count())
        } else {
            text(&asks)
                .style(type_style::TITLE)
                .text_align(TextAlign::Center)
        }
    });
    let subtitle = subtitle(prompt, type_style::BODY.size);
    let buttons: Vec<&PromptButton> = bar_buttons(prompt).collect();
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
                    .on_press(Message::Button(button_view.id)),
            )
        })
        .collect();
    let dock = bar_column(input.profile)
        .children(title)
        .children(subtitle)
        .children(others)
        .children(controls.map(|controls| controls.element))
        .children(forward.map(|forward| {
            turn_action(
                dock_sized(
                    primary(&forward.label)
                        .enabled(forward.enabled)
                        .on_press(Message::Button(forward.id)),
                ),
                pass_on,
                &forward.caption,
            )
            .children(pass_on.filter(|button| button.enabled).map(|button| {
                widgets::tooltip(&button.label, &button.label, input.data.flat.tooltip)
            }))
        }));
    if fitted_dock {
        return fit_question_dock(dock, input.profile);
    }
    dock
}

/// The line under the title of `prompt` that says how to answer, where the session sends one,
/// in body type `size` points high: Arena draws "Click an option below to select it." under
/// "Choose One" (`SN9QFe2kM4Y 14:02.5`).
pub(super) fn subtitle(prompt: &PromptView, size: f32) -> Option<Element> {
    (!prompt.subtitle.is_empty()).then(|| {
        text(&prompt.subtitle)
            .style(TypeStyle {
                size,
                ..type_style::BODY
            })
            .text_align(TextAlign::Center)
            .passes(true)
    })
}

/// The desktop's action dock, as the web's `.dock` in `ui/src/screens/styles/controls.css`
/// lays it out in the window's bottom right corner (`layout::DOCK_RIGHT` and `DOCK_BOTTOM`): a
/// column of buttons of one width, at least a turn button's (`--button-width`), with what the
/// prompt asks over them and the turn button at the foot. It draws no panel of its own, as the
/// web's dock draws none.
pub fn captioned_dock() -> Element {
    column()
        .name("prompt")
        .gap(space::M)
        .align_items(Align::Stretch)
        .min_width(size::BUTTON_WIDTH)
        .max_width(size::DOCK_MAX_WIDTH)
        .background([0; 4])
        .border(([0; 4], 0.0))
}

fn bar_column(profile: Profile) -> Element {
    let mut dock = captioned_dock();
    if let Some(slot) = profile.slot(SlotName::PromptBar) {
        let bounds = slot.rect;
        let window = profile.window();
        let root_right = window.size.width - window.safe_area.right - DOCK_RIGHT;
        let root_bottom = window.size.height - window.safe_area.bottom - DOCK_BOTTOM;
        let left = bounds.x + bounds.width - root_right;
        let top = bounds.y + bounds.height - root_bottom;
        // Keep the column in the root's flex layout so its title wraps with the buttons.
        dock = dock
            .max_width(bounds.width)
            .inset([Some(top), None, None, Some(left)]);
    }
    dock
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
            .on_press(Message::Button(button_view.id))
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
///   right end, and the stacked buttons there, beside the dock, or at the band's left end.
///   The pill is narrowed to the room between them. On a table whose players share their
///   piles, the band starts right of the shared piles' column.
/// - What it asks, `asks`, is a pill on the middle band's line, as Arena writes it across the
///   middle of its table, or at the nearest height clear of every card where one lies on that
///   line. While the prompt holds cards up, or lists what it asks
///   about over the table, the table lies under a shade and the pill stands over the strip,
///   beside the stacked buttons. The pill lets the pointer through, so it never covers a card
///   the prompt picks.
fn floating_bar(
    prompt: &PromptView,
    asks: &str,
    controls: Option<Controls>,
    slots: &PromptSlots,
    held_up: bool,
) -> Element {
    let buttons: Vec<&PromptButton> = bar_buttons(prompt).collect();
    let forward = buttons
        .iter()
        .copied()
        .find(|button_view| button_view.primary);
    let pass_on = (buttons.iter().copied())
        .find(|button_view| button_view.id == ButtonId::PassOn && !button_view.primary);
    let others = (buttons.iter().copied())
        .filter(|button_view| Some(button_view.id) != forward.map(|b| b.id))
        .filter(|button_view| Some(button_view.id) != pass_on.map(|b| b.id));
    let gap = slots.metrics.gap;
    let height = slots.dock.height;
    let left = slots.bar.x;
    let right = slots.dock.x + slots.dock.width;
    let mut beside_right = 0.0;
    let mut beside_left = 0.0;
    let mut dock_bottom = slots.dock_bottom();
    let dock = (forward.is_some() || pass_on.is_some()).then(|| {
        let mut dock = dock(forward, pass_on, slots);
        let width = forward.map_or(0.0, |b| slots.button_width(&b.label) + gap)
            + pass_on.map_or(0.0, |_| height);
        let (placed, slot) = slots.dock("dock", width, None);
        if slot != 0 {
            beside_right = right - placed.x;
        } else {
            dock_bottom = placed.y + placed.height;
        }
        dock.anchor = Some(Anchor::Point(
            Vec2::new(placed.x + placed.width, placed.y + height / 2.0),
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
        .children(others.clone().map(|button| slots.button(button)));
    let strip_over = if slots.opening {
        dock_bottom - height - gap
    } else if beside_right > 0.0 {
        slots.strip_top() - gap
    } else {
        (dock_bottom - height).min(slots.strip_top()) - gap
    };
    let over = if held_up && dock.is_none() {
        dock_bottom
    } else {
        strip_over
    };
    let more = (!more.children.is_empty()).then(|| {
        let (width, tall) = others
            .map(|b| (slots.button_width(&b.label), height))
            .chain(controls_size)
            .fold(
                (slots.metrics.button_min_width, -gap),
                |(width, tall), (w, h)| (width.max(w), tall + h + gap),
            );
        let beside = if beside_right > 0.0 {
            beside_right + gap
        } else {
            0.0
        };
        let strip = rect(right - width, over - tall, width, tall);
        let middle_right = slots.middle(right - beside - width, width, tall);
        let middle_left = slots.middle(slots.middle_start(), width, tall);
        let (slot, placed) = slots.place(
            "stacked prompt controls",
            "prompt.stacked",
            strip,
            [middle_right, middle_left],
        );
        match slot {
            1 => beside_right = right - placed.x,
            2 => beside_left = placed.x + placed.width - left,
            _ => {}
        }
        more.anchor = Some(Anchor::Point(
            Vec2::new(placed.x + placed.width, placed.y + placed.height),
            Vec2::ONE,
        ));
        more
    });
    let pill = (!asks.is_empty()).then(|| {
        let place = PillPlace::beside_controls(slots.held, strip_over, beside_left, beside_right);
        pill(asks, prompt, place, slots)
    });
    let phase = slots.phase_icon(pill.as_ref().map(|(_, bounds)| *bounds));
    // The pill comes first, so that the buttons are drawn over it where they meet.
    holder()
        .name("prompt")
        .children(pill.map(|(pill, _)| pill))
        .children(more)
        .children(dock)
        .children(phase)
}

/// Whether the prompt bar for `input`'s prompt draws the phase icon in the middle band, in place
/// of the game screen's furniture: while it stands on a table it does not shade, so that it can
/// leave the icon out where its pill needs the icon's room. In a crowded band, the pill beside
/// the icon and the bar's buttons can be too narrow for the longest word of what it asks.
#[must_use]
pub fn draws_phase(input: &HudIn) -> bool {
    input.prompt.is_some_and(|prompt| {
        let held = holds_up(prompt) || covers_table(prompt);
        !held && PromptSlots::new(input, held).is_some_and(|slots| slots.phase_icon.is_some())
    })
}

/// Resolves the slots used by a prompt whose controls can move around the cards.
struct PromptSlots<'a> {
    bar: Rect,
    dock: Rect,
    middle: Rect,
    clearance: Rect,
    asked: Rect,
    metrics: FloatingPrompt,
    /// The least size of text the profile's checks allow, in points, or 0.
    text_points: f32,
    scene: Option<&'a PlacementScene>,
    held: bool,
    opening: bool,
    phase: (f32, f32),
    /// The phase icon the bar draws in the middle band, with the view whose phase it shows.
    phase_icon: Option<(&'a beryl_protocol::PlayerView, Rect)>,
}

impl<'a> PromptSlots<'a> {
    fn new(input: &HudIn<'a>, held: bool) -> Option<Self> {
        let profile = input.profile;
        let metrics = profile.data().hud.floating_prompt?;
        let [bar, dock, middle, clearance, asked, phase] = profile.slots([
            SlotName::PromptBar,
            SlotName::PromptDock,
            SlotName::PromptStacked,
            SlotName::PromptClearance,
            SlotName::PromptAsked,
            SlotName::Phase,
        ]);
        let shared = input.view.is_some_and(|view| view.shared.is_some());
        // The shared piles' column stands at the band's left end, and a pill moved up or down
        // to a height clear of the cards would cover the graveyard or exile pile beside it.
        let bar = clear_of_shared_piles(profile, bar?.rect, shared);
        // A profile that gives the phase icon a slot of its own draws it there instead
        // (`phase::corner`), leaving the whole band to the bar.
        let phase_icon = phase
            .filter(|_| {
                profile.data().hud.table.contains(&Piece::Phase)
                    && profile.slot(SlotName::PhaseIcon).is_none()
            })
            .zip(input.view)
            .map(|(phase, view)| {
                let band = clear_of_shared_piles(profile, phase.rect, shared);
                (view, phase_box(band, shared))
            });
        let phase = phase_icon.map_or((bar.x, bar.x), |(_, bounds)| {
            (bounds.x, bounds.x + bounds.width)
        });
        let opening = input.view.is_some_and(crate::screens::opening::in_opening);
        let mut dock = dock?.rect;
        if opening
            && let (Some(wrapped), Some(room)) = (
                profile.data().hud.opening.wrapped_hand,
                profile.slot(SlotName::Opening),
            )
        {
            dock.y = room.rect.y + room.rect.height - wrapped.dock_bottom - dock.height;
        }
        Some(Self {
            bar,
            dock,
            middle: clear_of_shared_piles(profile, middle?.rect, shared),
            clearance: clearance?.rect,
            asked: asked?.rect,
            metrics,
            text_points: profile.data().checks.text_points.unwrap_or(0.0),
            scene: input.placement_scene,
            held,
            opening,
            phase,
            phase_icon,
        })
    }

    /// The phase icon the bar draws in the middle band while the table is not shaded
    /// (`draws_phase`), unless the pill, whose box is `pill`, needed the icon's room.
    fn phase_icon(&self, pill: Option<Rect>) -> Option<Element> {
        let (view, bounds) = self.phase_icon.filter(|_| !self.held)?;
        let covered = pill.is_some_and(|pill| {
            pill.x < bounds.x + bounds.width
                && bounds.x < pill.x + pill.width
                && pill.y < bounds.y + bounds.height
                && bounds.y < pill.y + pill.height
        });
        (!covered).then(|| crate::hud::phase::icon(view, bounds))
    }

    /// Where the middle band's room for a pill or a button starts and ends: beside the phase
    /// icon, which stands at one of the band's ends (`phase_box`), and otherwise at the band's
    /// ends.
    fn middle_room(&self) -> (f32, f32) {
        let (phase_left, phase_right) = self.phase;
        let (start, end) = (self.middle.x, self.middle.x + self.middle.width);
        if phase_right <= phase_left {
            (start, end)
        } else if phase_left + phase_right > start + end {
            (start, end.min(phase_left - self.metrics.gap))
        } else {
            (start.max(phase_right + self.metrics.gap), end)
        }
    }

    fn middle_start(&self) -> f32 {
        self.middle_room().0
    }

    fn middle_end(&self) -> f32 {
        self.middle_room().1
    }

    fn strip_top(&self) -> f32 {
        self.clearance.y + self.clearance.height
    }

    fn dock_bottom(&self) -> f32 {
        let bottom = self.dock.y + self.dock.height;
        if self.opening {
            return bottom;
        }
        self.scene
            .into_iter()
            .flat_map(|scene| scene.group(Group::Hand))
            .map(|card| card.y - self.metrics.gap)
            .fold(bottom, f32::min)
    }

    fn dock(&self, name: &'static str, width: f32, narrow: Option<f32>) -> (Rect, usize) {
        let bottom = self.dock_bottom();
        let height = self.dock.height;
        let strip = rect(
            (self.dock.x + self.dock.width) - width,
            bottom - height,
            width,
            height,
        );
        let raised = bottom < self.dock.y + height;
        let middle = |width| {
            raised
                .then(|| self.middle((self.dock.x + self.dock.width) - width, width, height))
                .flatten()
        };
        if raised && let Some(scene) = self.scene {
            scene.raise_dock(
                name,
                Rect {
                    y: self.dock.y,
                    ..strip
                },
            );
        }
        let (slot, placed) = self.place(
            name,
            if raised {
                "prompt.dock raised above hand"
            } else {
                "prompt.dock"
            },
            strip,
            [middle(width), narrow.and_then(middle)],
        );
        (placed, slot)
    }

    /// A box `width` by `height` on the middle band's line, starting at `left` or as far left
    /// of it as keeps the box within the band's room beside the phase icon, if it fits there.
    /// A control may reach a few points past the band's top and foot, where the table's card
    /// piles do too, but not past its ends, or into the shared piles' column at its left.
    fn middle(&self, left: f32, width: f32, height: f32) -> Option<Rect> {
        let left = left.min(self.middle_end() - width);
        (!self.held
            && height <= self.middle.height + 2.0 * self.metrics.overlap + 1.0
            && left >= self.middle_start())
        .then(|| {
            rect(
                left,
                self.middle.y + (self.middle.height - height) / 2.0,
                width,
                height,
            )
        })
    }

    fn place<const N: usize>(
        &self,
        name: &'static str,
        strip_slot: &'static str,
        strip: Rect,
        middle: [Option<Rect>; N],
    ) -> (usize, Rect) {
        let Some(scene) = self.scene else {
            return (0, strip);
        };
        let of = if self.held {
            Vec::new()
        } else {
            let mut of = table_rows();
            of.push(PlacementScene::phase_query());
            of
        };
        let candidates: Vec<_> = std::iter::once(Some(strip))
            .chain(middle)
            .enumerate()
            .filter_map(|(index, bounds)| Some((index, bounds?)))
            .collect();
        // The first candidate is also the fallback on a crowded board, where the buttons
        // must stay available even when no place can clear the table.
        let boxes = candidates.iter().map(|&(index, bounds)| {
            let slot = if index == 0 {
                strip_slot
            } else {
                "prompt.stacked (middle band)"
            };
            (slot, inset(bounds, self.metrics.overlap))
        });
        scene
            .place_named(name, of, boxes)
            .map_or((0, strip), |(index, _)| candidates[index])
    }
}

fn table_rows() -> Vec<NodeQuery> {
    vec![
        Group::YourRows.children(),
        Group::TheirRows.children(),
        Group::Stack.children(),
    ]
}

/// The dock: the button a prompt puts forward, drawn as the primary button, and the one that
/// passes on beside it.
fn dock(
    forward: Option<&PromptButton>,
    pass_on: Option<&PromptButton>,
    slots: &PromptSlots,
) -> Element {
    row()
        .name("dock")
        .gap(slots.metrics.gap)
        .align_items(Align::Center)
        .children(forward.map(|button| slots.button(button)))
        .children(pass_on.map(|button_view| {
            // The web's pass-on button is not an icon button: its face (`.pass-on-face` in
            // `controls.css`) casts no box shadow.
            icon_button(icons::PASS_ON, &button_view.label)
                .box_shadow(Vec::new())
                .drop_shadow(shadow::PASS_ON.to_vec())
                .enabled(button_view.enabled)
                .on_press(Message::Button(button_view.id))
        }))
}

impl PromptSlots<'_> {
    /// Estimates how much room a button needs when finding a clear spot for it.
    fn button_width(&self, label: &str) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a button's label is a few words"
        )]
        let characters = label.chars().count() as f32;
        (characters * self.metrics.button_text * self.metrics.character_width
            + 2.0 * self.metrics.button_padding)
            .max(self.metrics.button_min_width)
    }

    /// Estimates how wide the longest word of `asks` is in the pill, which a line must hold.
    fn word_width(&self, asks: &str) -> f32 {
        #[expect(clippy::cast_precision_loss, reason = "a word is a few letters")]
        let characters = asks
            .split_whitespace()
            .map(|word| word.chars().count())
            .max()
            .unwrap_or(0) as f32;
        characters * self.metrics.pill_text * self.metrics.character_width
    }

    /// Estimates how wide the pill must be to say `asks` on one line.
    fn pill_width(&self, asks: &str) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "what a prompt asks is a sentence"
        )]
        let characters = asks.chars().count() as f32;
        characters * self.metrics.pill_text * self.metrics.character_width
            + 2.0 * self.metrics.pill_padding
    }

    fn button(&self, button_view: &PromptButton) -> Element {
        let widget = if button_view.primary { primary } else { button };
        self.sized(
            widget(&button_view.label)
                .enabled(button_view.enabled)
                .on_press(Message::Button(button_view.id)),
        )
    }

    fn sized(&self, element: Element) -> Element {
        let mut element = element
            .height(self.dock.height)
            .min_width(self.metrics.button_min_width)
            .style(TypeStyle {
                size: self.metrics.button_text,
                ..type_style::BUTTON
            });
        let padding = self.metrics.button_padding;
        element.style.padding = [0.0, padding, 0.0, padding];
        element
    }
}

/// The words on the turn button's place while the opponent acts, Arena's label, which the
/// web's dock shows the same way (`waitingLabel` in `ui/src/stage/PromptBar.tsx`).
const OPPONENTS_TURN: &str = "Opponent's Turn";

/// What stands where the prompt's buttons do while no prompt is open and the opponent acts: a
/// disabled button saying "Opponent's Turn". The opponent acts while they decide, or while their
/// turn plays on without the viewer, as the web's `opponentActing` says. `None` at any other time.
#[must_use]
pub(super) fn waiting(input: &HudIn) -> Option<Element> {
    let view = input.view?;
    if !opponent_acts(view) || input.prompt.is_some() {
        return None;
    }
    let label = button(OPPONENTS_TURN).enabled(false);
    let Some(slots) = PromptSlots::new(input, false) else {
        return Some(bar_column(input.profile).child(turn_action(dock_sized(label), None, "")));
    };
    // It stands where the dock would, raised over a hand whose back row stands in the strip. Where
    // that would cover a permanent of the viewer's back row and the label is too wide for the
    // middle band beside the phase icon or stack, it narrows to the room there and shrinks its
    // words to fit.
    let wide = slots.button_width(OPPONENTS_TURN);
    let line = slots.middle.y + slots.middle.height / 2.0;
    let half = slots.dock.height / 2.0;
    let occupied_right = slots
        .scene
        .into_iter()
        .flat_map(|scene| scene.group(Group::Stack))
        .filter(|item| item.y < line + half && item.y + item.height > line - half)
        .map(|item| item.x + item.width)
        .fold(slots.middle_start(), f32::max);
    // In the middle band, the label stays within the room beside the phase icon.
    let right = (slots.dock.x + slots.dock.width).min(slots.middle_end());
    let room = right - occupied_right - slots.metrics.gap;
    let narrow = (slots.metrics.button_min_width..wide)
        .contains(&room)
        .then_some(room);
    let (placed, slot) = slots.dock(OPPONENTS_TURN, wide, narrow);
    let mut label = slots.sized(label);
    label.style.width = narrow.filter(|_| slot == 2);
    label.anchor = Some(Anchor::Point(
        Vec2::new(placed.x + placed.width, placed.y + placed.height / 2.0),
        Vec2::new(1.0, 0.5),
    ));
    Some(holder().name("prompt").child(label))
}

pub(super) fn opponent_acts(view: &beryl_protocol::PlayerView) -> bool {
    match view.status {
        Status::Thinking => true,
        Status::Playing => view.turn.active == Side::Opponent,
        _ => false,
    }
}

pub(super) fn acting_for(view: &beryl_protocol::PlayerView) -> bool {
    matches!(&view.status, Status::Deciding { decision } if decision.acting_for.is_some())
}
