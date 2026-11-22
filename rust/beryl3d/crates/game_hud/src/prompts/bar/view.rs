//! The prompt bar: what the open prompt asks and its buttons, each as the session sent it
//! (docs/beryl-3d.md, "Elements"). Its words, which buttons it has and whether each can be
//! pressed are the session's.

use std::borrow::Cow;

use beryl_protocol::prompt::{ButtonId, PromptButton, PromptView, ShapeBody};
use beryl_protocol::protocol::{Side, Status};
use glam::Vec2;

use super::Message;
use crate::hud::HudIn;
use crate::hud::phase::phase_box;
use crate::hud::placement_scene::{Group, PlacementScene, inset, rect};
use crate::prompts::{Piles, covers_table, ways};
use game_table::profiles::Profile;
use kit_base::data::profiles::{FloatingPrompt, Piece};
use kit_base::geometry::Rect;
use kit_base::tokens::{color, shadow, size, space, type_style};
use kit_scene::{Candidate, NodeQuery};
use kit_text::text::TypeStyle;
use kit_ui::element::{Align, Anchor, Element, Position, TextAlign, column, holder, row, text};
use kit_ui::layout::{DOCK_BOTTOM, DOCK_RIGHT};
use kit_ui::widgets::{self, button, icon_button, icons, primary};

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
    let held_up = Piles::of(prompt).is_some();
    if let Some(slots) = PromptSlots::new(input, held_up || covers_table(prompt)) {
        return floating_bar(prompt, &asks, controls, &slots, held_up);
    }
    let title = (!asks.is_empty()).then(|| {
        text(&asks)
            .style(type_style::TITLE)
            .text_align(TextAlign::Center)
    });
    let ways = || ways(prompt).map(|way| way.id);
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
                    .on_press(Message::Button(button_view.id)),
            )
        })
        .collect();
    bar_column(input.profile)
        .children(title)
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
        }))
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
    if let Some(slot) = profile.slot("prompt.bar") {
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
///   right end, and the stacked buttons there, beside the dock, or at the band's left end. The
///   pill is narrowed to the room between them.
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
    let ways = || ways(prompt).map(|way| way.id);
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
            beside_right = width;
        } else {
            dock_bottom = placed.y + placed.height;
        }
        dock.anchor = Some(Anchor::Point(
            Vec2::new(right, placed.y + height / 2.0),
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
    let over = if held_up && dock.is_none() {
        dock_bottom
    } else if beside_right > 0.0 {
        slots.strip_top() - gap
    } else {
        (dock_bottom - height).min(slots.strip_top()) - gap
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
        let middle_left = slots.middle(left, width, tall);
        let (slot, placed) = slots.place(
            "stacked prompt controls",
            "prompt.stacked",
            strip,
            [middle_right, middle_left],
        );
        match slot {
            1 => beside_right = right - placed.x,
            2 => beside_left = placed.width,
            _ => {}
        }
        more.anchor = Some(Anchor::Point(
            Vec2::new(placed.x + placed.width, placed.y + placed.height),
            Vec2::ONE,
        ));
        more
    });
    let pill = (!asks.is_empty()).then(|| {
        let place = PillPlace::beside_controls(slots.held, over, beside_left, beside_right);
        pill(asks, place, slots)
    });
    // The pill comes first, so that the buttons are drawn over it where they meet.
    holder()
        .name("prompt")
        .children(pill)
        .children(more)
        .children(dock)
}

/// Resolves the slots used by a prompt whose controls can move around the cards.
struct PromptSlots<'a> {
    bar: Rect,
    dock: Rect,
    middle: Rect,
    clearance: Rect,
    asked: Rect,
    metrics: FloatingPrompt,
    scene: Option<&'a PlacementScene>,
    held: bool,
    phase: (f32, f32),
}

impl<'a> PromptSlots<'a> {
    fn new(input: &HudIn<'a>, held: bool) -> Option<Self> {
        let profile = input.profile;
        let metrics = profile.data().hud.floating_prompt?;
        let [bar, dock, middle, clearance, asked, phase] = profile.slots([
            "prompt.bar",
            "prompt.dock",
            "prompt.stacked",
            "prompt.clearance",
            "prompt.asked",
            "phase",
        ]);
        let bar = bar?.rect;
        let phase = input
            .view
            .filter(|_| profile.data().hud.table.contains(&Piece::Phase))
            .and_then(|view| {
                let bounds = phase_box(view.stack.is_empty(), phase?.rect);
                Some((bounds.x, bounds.x + bounds.width))
            })
            .unwrap_or((bar.x, bar.x));
        Some(Self {
            bar,
            dock: dock?.rect,
            middle: middle?.rect,
            clearance: clearance?.rect,
            asked: asked?.rect,
            metrics,
            scene: input.placement_scene,
            held,
            phase,
        })
    }

    fn strip_top(&self) -> f32 {
        self.clearance.y + self.clearance.height
    }

    fn dock_bottom(&self) -> f32 {
        let bottom = self.dock.y + self.dock.height;
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

    /// A control may reach a few points past the band, where the table's card piles do too.
    fn middle(&self, left: f32, width: f32, height: f32) -> Option<Rect> {
        let (phase_left, phase_right) = self.phase;
        let gap = self.metrics.gap;
        (!self.held
            && height <= self.middle.height + 2.0 * self.metrics.overlap + 1.0
            && left >= self.middle.x
            && (left + width + gap <= phase_left || left - gap >= phase_right))
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

impl PillPlace {
    fn beside_controls(held: bool, over: f32, left: f32, right: f32) -> Self {
        if held {
            Self::Over(over)
        } else if (left > 0.0) == (right > 0.0) {
            // Buttons at both ends can leave too little room to wrap the pill without covering a row.
            Self::Middle
        } else {
            Self::Beside { left, right }
        }
    }
}

/// The pill that says what a prompt asks, `asks`, at `place`: a fixed width, so that a long
/// question is laid out on as many lines as it takes. It lets the pointer through, so it never
/// covers a card the prompt picks.
fn pill(asks: &str, place: PillPlace, slots: &PromptSlots<'_>) -> Element {
    let style = TypeStyle {
        size: slots.metrics.pill_text,
        ..type_style::TITLE
    };
    let padding = slots.metrics.pill_padding;
    let height = style.size * style.line_height + padding;
    let room = |taken: f32| {
        if taken > 0.0 {
            taken + slots.metrics.gap
        } else {
            0.0
        }
    };
    let (left, right) = match place {
        PillPlace::Middle => (0.0, 0.0),
        PillPlace::Beside { left, right } => (room(left), room(right)),
        PillPlace::Over(_) => (0.0, room(slots.metrics.button_min_width)),
    };
    let area = if matches!(place, PillPlace::Over(_)) {
        slots.asked
    } else {
        slots.bar
    };
    let width = area.width - left - right;
    let x = area.x + left;
    let line = slots.middle.y + (slots.middle.height - height) / 2.0;
    let top = match place {
        PillPlace::Over(bottom) => bottom - height,
        _ => line,
    };
    let mut bounds = rect(x, top, width, height);
    if let Some(scene) = slots.scene {
        if matches!(place, PillPlace::Over(_)) {
            scene.place_named("prompt pill", Vec::new(), [("prompt.asked", bounds.into())]);
        } else {
            bounds = pill_slot(scene, slots, bounds);
        }
    }
    let mut pill = text(asks)
        .style(style)
        .text_align(TextAlign::Center)
        .width(bounds.width)
        .min_height(height)
        .background(color::PANEL)
        .radius(height / 2.0)
        .passes(true);
    pill.style.padding = [padding / 2.0, padding, padding / 2.0, padding];
    pill.anchor = Some(match place {
        PillPlace::Over(over) => Anchor::Point(Vec2::new(x, over), Vec2::Y),
        _ => Anchor::Point(Vec2::new(bounds.x, bounds.y), Vec2::ZERO),
    });
    pill
}

/// Tries the full prompt width at the nearest quiet height. When cards fill the middle of
/// the board from top to bottom, the pill can use the space to either side of those cards.
fn pill_slot(scene: &PlacementScene, slots: &PromptSlots, preferred: Rect) -> Rect {
    let gap = slots.metrics.asked_gap;
    let height = preferred.height;
    let cards: Vec<_> = [Group::YourRows, Group::TheirRows, Group::Stack]
        .into_iter()
        .flat_map(|group| scene.group(group))
        .collect();
    let mut tops = vec![preferred.y];
    tops.extend(
        cards
            .iter()
            .flat_map(|card| [card.y + card.height + gap, card.y - gap - height])
            .filter(|&top| top >= slots.clearance.y && top + height <= slots.strip_top()),
    );
    tops.sort_by(|a, b| (a - preferred.y).abs().total_cmp(&(b - preferred.y).abs()));
    let full_width = tops.into_iter().map(|y| Candidate {
        bounds: Rect { y, ..preferred },
        clearance: rect(slots.bar.x, y - gap, slots.bar.width, height + 2.0 * gap),
    });

    let end = preferred.x + preferred.width;
    // A narrower pill can wrap. Reserve an extra line when finding its horizontal slot.
    let tall = 2.0 * height - slots.metrics.pill_padding;
    let (left, right) = cards
        .iter()
        .filter(|card| {
            card.y < preferred.y + tall + gap && card.y + card.height > preferred.y - gap
        })
        .fold((end, preferred.x), |(left, right), card| {
            (left.min(card.x), right.max(card.x + card.width))
        });
    let start = preferred.x.max(slots.phase.1 + gap);
    let room = [((right + gap).max(start), end), (start, left - gap)];
    let beside = room
        .into_iter()
        .filter(|(left, right)| right - left >= slots.metrics.button_min_width)
        .map(|(left, right)| {
            let bounds = rect(left, preferred.y, right - left, height);
            let clearance = inset(
                Rect {
                    height: tall,
                    ..bounds
                },
                -gap,
            );
            Candidate {
                bounds,
                clearance: clearance.clearance,
            }
        });
    scene
        .place_named(
            "prompt pill",
            table_rows(),
            full_width
                .chain(beside)
                .map(|candidate| ("prompt.bar", candidate)),
        )
        .map_or(preferred, |(_, bounds)| bounds)
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
/// disabled button saying "Opponent's Turn", as the web's dock shows with no spinner under the
/// opponent's orb. The opponent acts while they decide, or while their turn plays on without
/// the viewer, as the web's `opponentActing` says. `None` at any other time.
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
        .fold(slots.phase.1, f32::max);
    let right = slots.dock.x + slots.dock.width;
    let room = right - occupied_right - slots.metrics.gap;
    let narrow = (slots.metrics.button_min_width..wide)
        .contains(&room)
        .then_some(room);
    let (placed, slot) = slots.dock(OPPONENTS_TURN, wide, narrow);
    let mut label = slots.sized(label);
    label.style.width = narrow.filter(|_| slot == 2);
    label.anchor = Some(Anchor::Point(
        Vec2::new(right, placed.y + placed.height / 2.0),
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
