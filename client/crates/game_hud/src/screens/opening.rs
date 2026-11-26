//! The opening hand and the mulligan, shown on the game screen before the first turn: the web
//! interface's `Opening` (`ui/src/screens/Opening.tsx`) with the rules of `.opening` in
//! `ui/src/screens/styles/opening.css`. A dimmed table, whether the viewer is on the play, a
//! title, the viewer's hand fanned across the desktop or in rows on the phone, and, while putting
//! cards on the bottom of the library, a row of slots under it. The open prompt's buttons stand in
//! the prompt bar, which `screens::game` adds after this so that the backdrop is drawn under
//! them. The life orbs and the step stops stand under the backdrop, which dims them, as the
//! web's overlay is drawn over its `Hud`.
//!
//! The cards are the hand's scene objects (`Show`): the table moves each from the hand
//! into its box and back (`table::shown`), so no card is drawn at the hand while the opening is
//! shown. A put-on-bottom pick clicks the cards in the row (`Click::On`); the session marks each
//! one picked as selected, and the row shows it in a slot.
//!
//! The opening coin spins before it lands beside the play-or-draw label. In-game coin flips
//! use the separate `announce` notice.
//!
//! When the opponent won the flip, the opening shows the flip for 1.5 seconds however soon
//! they chose, as the web's coin spins (`HudIn::spinning`): the title says who won, the hand
//! is dealt out of sight and no button is offered. Until a button is offered, three gold dots
//! under the title say that the game is waiting, as the web's `.opening-waiting` does.
//!
//! Play or draw, asked before anyone has started, shows its buttons side by side under the title
//! (`choice`, the web's `.opening-choice`), and the game screen leaves the prompt bar out then
//! (`choosing_start`). The other opening buttons stand in the prompt bar.
//!
//! Not built yet: the web's face-down hand while the viewer has declared a mulligan.

pub mod coin;

use beryl_protocol::PlayerView;
use beryl_protocol::prompt::{PromptButton, PromptView, Purpose};
use beryl_protocol::protocol::{CardHandling, Declaration, ObjectId, Side, Status, ZoneName};
use beryl_protocol::script::{Mark, Subject};
use glam::Vec2;

use crate::hud::HudIn;
use game_table::profiles::Profile;
use game_table::table::ScreenProjection;
use kit_base::data::Card;
use kit_base::data::hud::OpeningStyle;
use kit_base::data::profiles::{ScreenMetrics, SingleRow, WrappedHand};
use kit_base::geometry::Rect;
use kit_base::paint::Shadow;
use kit_base::tokens::{color, gradient, shadow, size, type_style};
use kit_frame::frame::{ElementId, Role};
use kit_scene::{Show, grid::grid_element};
use kit_space::arrange::Grid;
use kit_text::text::TypeStyle;
use kit_ui::element::{Action, Align, Anchor, Element, Justify, Overflow, column, row, text};
use kit_ui::widgets::screen::backdrop;
use kit_ui::widgets::{button, primary};

/// The source of the opening overlay, for the client map.
pub const SOURCE: &str = file!();

/// The name of the element that holds the whole opening, its backdrop and its group, which
/// "every step shown" looks for (`game_probe::agent::steps::sample`).
pub const SCREEN_NAME: &str = "opening screen";

/// The starting hand size ([CR 103.5]): "Each player draws a number of cards equal to their
/// starting hand size, which is normally seven."
///
/// [CR 103.5]: https://yawgatog.com/resources/magic-rules/#R1035
const STARTING_HAND: usize = 7;

/// The title, `.opening-title`: 56 points of the serif face, bold.
const TITLE: TypeStyle = type_style::OPENING_TITLE;

/// Whether the viewer is on the play, `.opening-coin`: 22 points of the serif face.
const COIN: TypeStyle = type_style::OPENING_COIN;

/// Whether the game is in its opening, as the web's `GameScreen` decides it
/// (`ui/src/screens/Game.tsx`): before the first turn, while the game is being decided or
/// played rather than free, over or stopped.
pub fn in_opening(view: &PlayerView) -> bool {
    view.turn.number == 0
        && !matches!(
            view.status,
            Status::Free | Status::Over { .. } | Status::Stopped
        )
}

/// The view the opening shows: the latest whose script has played out, while it is in the
/// opening. The web's `Opening` is drawn from that view, so that its title and hand change only
/// as the table finishes playing the deal, a mulligan or the new hand, never ahead of it.
fn opening_view<'a>(input: &HudIn<'a>) -> Option<&'a PlayerView> {
    input.played.filter(|view| in_opening(view))
}

/// Whether the opening shows the open prompt's buttons itself, side by side under the title,
/// instead of the prompt bar: play or draw, asked before anyone has started, on the desktop
/// layout (`choosingStart` in `ui/src/screens/Opening.tsx`). While it does, the game screen
/// leaves the prompt out.
#[must_use]
pub(crate) fn choosing_start(input: &HudIn) -> bool {
    input.profile.data().hud.opening.start_choice
        && !input.spinning
        && opening_view(input).is_some_and(|view| {
            view.turn.starting.is_none()
                && input.view.is_some_and(|latest| std::ptr::eq(latest, view))
        })
        && input
            .prompt
            .is_some_and(|prompt| !prompt.buttons.is_empty())
}

/// Whether the game screen shows the opening.
#[must_use]
pub fn shown(input: &HudIn) -> bool {
    opening_view(input).is_some()
}

/// What the game screen shows in the opening, or none once it is over.
#[must_use]
pub fn opening(input: &HudIn) -> Option<Element> {
    let view = opening_view(input)?;
    let you = &view.players.you;
    // While the coin spins, the opening does not show who starts, as the web's `Opening` holds
    // it back; the client offers no decision meanwhile.
    let starting = view.turn.starting.filter(|_| !input.spinning);
    // The open prompt belongs to the latest view, so while scripts before it still play the
    // opening offers no decision, as the web's offers none until its view has played out.
    let caught_up = input.view.is_some_and(|latest| std::ptr::eq(latest, view));
    let prompt = input.prompt.filter(|_| caught_up);
    let picking = bottom_prompt(prompt);
    let hand: Vec<ObjectId> = (view.objects.iter())
        .filter(|object| object.zone == ZoneName::Hand && object.owner == Side::You)
        .map(|object| object.slot)
        .collect();
    let picked = |slot: ObjectId| picking.is_some_and(|prompt| selected(prompt, slot));
    let bottom: Vec<ObjectId> = hand.iter().copied().filter(|&slot| picked(slot)).collect();
    // One card per mulligan goes on the bottom, as the web's `toBottom` counts them.
    let to_bottom = if picking.is_some() {
        hand.len()
            .saturating_sub(STARTING_HAND - usize::from(you.mulligans))
            .max(1)
    } else {
        0
    };
    let title = title(view, starting, picking.is_some());
    let actions = if picking.is_some() {
        1
    } else {
        prompt.map_or(0, |prompt| prompt.buttons.len())
    };
    let shown: Vec<ObjectId> = (hand.iter().copied())
        .filter(|&slot| !picked(slot))
        .collect();
    // The opponent choosing who starts shows on the coin, and the opponent considering their
    // hand on their side of the table, so neither shows the dots (`Opening`'s
    // `choosingFirst` and `considering`).
    let choosing_first = opponent_choosing(view, input.spinning);
    let considering = matches!(view.handling, Some(CardHandling::Hand { .. }));
    Some(present(
        &Presentation {
            view,
            starting,
            coin: Some(starting == Some(Side::You) || choosing_first),
            title: &title,
            hand: &shown,
            hand_hidden: starting.is_none(),
            rows: hand.len(),
            bottom: &bottom,
            slots: to_bottom.max(bottom.len()),
            actions,
            waiting: actions == 0 && !choosing_first && !considering,
            press: None,
            choice: if choosing_start(input) {
                prompt.map_or(&[], |prompt| &prompt.buttons)
            } else {
                &[]
            },
        },
        input.profile,
        input.data.hud.opening,
    ))
}

/// Whether the coin rocks between the seats while the opponent chooses who starts.
#[must_use]
pub fn opponent_choosing(view: &PlayerView, spinning: bool) -> bool {
    view.turn.starting.is_none() && view.turn.flip_winner == Some(Side::Opponent) && !spinning
}

/// Putting a mulligan card on the bottom uses the opening hand's slots.
fn bottom_prompt(prompt: Option<&PromptView>) -> Option<&PromptView> {
    prompt.filter(|prompt| prompt.is(Purpose::PutOnBottom))
}

/// What an opening shows: whether the viewer is on the play (none while that is still being
/// chosen), the title, the cards still in the hand row and those on their way to the bottom,
/// how many cards the hand holds in all (which sizes the phone's rows), how many slots the
/// bottom row has, how many buttons the dock holds, and what a press on a card does, if
/// anything. The engine's game and the manual table each fill it from their own prompt.
///
/// `hand_hidden` lays the hand row out but draws none of its cards, as the web keeps the hand
/// under the table until it knows who starts, so that the cards are dealt out of sight and then
/// stand in their places at once. `waiting` shows the dots under the title.
pub struct Presentation<'a> {
    pub view: &'a PlayerView,
    pub starting: Option<Side>,
    /// Whether to draw the coin and whether it bears the starting player's 1.
    pub coin: Option<bool>,
    pub title: &'a str,
    pub hand: &'a [ObjectId],
    pub hand_hidden: bool,
    pub rows: usize,
    pub bottom: &'a [ObjectId],
    pub slots: usize,
    pub actions: usize,
    pub waiting: bool,
    pub press: Option<fn(ObjectId) -> Action>,
    /// The buttons shown side by side under the title (`.opening-choice`): the first is
    /// primary and stands rightmost, as in Arena.
    pub choice: &'a [PromptButton],
}

/// The opening `shown` describes, over the dimmed table: the coin, the title, the hand and the
/// bottom slots, as the web's `OpeningPresentation` lays them out.
pub fn present(shown: &Presentation, profile: Profile, style: OpeningStyle) -> Element {
    let window = profile.window();
    let Presentation {
        view,
        starting,
        slots,
        ..
    } = *shown;
    let sizes = Sizes::for_profile(shown, profile);
    let card = |slot: ObjectId, width: f32| {
        let mut card = card_box(view, slot, (width, profile.card()));
        card.press = shown.press.map(|press| press(slot));
        card
    };
    let mut coin_row = if shown.coin.is_some() && sizes.fan.is_none() {
        column()
    } else {
        row()
    }
    .name("coin")
    .height(sizes.coin_height)
    .align_items(Align::Center);
    coin_row.style.margin = [0.0, 0.0, sizes.coin_gap, 0.0];
    if let Some(first) = shown.coin {
        coin_row = coin_row
            .gap(sizes.coin_label_gap)
            .child(coin::disc(coin::diameter(profile), first));
    }
    if let Some(starting) = starting {
        let words = if starting == Side::You {
            "On the play"
        } else {
            "On the draw"
        };
        let mut label = text(words)
            .style(sizes.coin)
            .color(color::GOLD)
            .passes(true);
        if let Some(scale) = sizes.header_scale {
            label = label.drop_shadow(scaled_shadows(&shadow::OPENING_COIN_LABEL, scale));
        }
        if shown.coin.is_none() {
            label = label.align_self(sizes.coin_align);
        }
        coin_row = coin_row.child(label);
    }
    let mut heading = text(shown.title)
        .style(sizes.title)
        .color(color::TEXT)
        .passes(true);
    if let Some(scale) = sizes.header_scale {
        heading = heading
            .color(color::OPENING_TITLE)
            .drop_shadow(scaled_shadows(&shadow::OPENING_TITLE, scale));
    }
    heading.style.margin = [0.0, 0.0, sizes.title_gap, 0.0];
    let hand_row = hand_grid(shown, &sizes, profile.card(), card);
    let dots = (shown.waiting && profile.data().hud.opening.waiting_dots).then(|| waiting(style));
    let slots_row = (slots > 0).then(|| {
        let boxes = (0..slots).map(|at| match shown.bottom.get(at) {
            Some(&slot) => card(slot, sizes.slot),
            None => empty_slot(at, (sizes.slot, profile.card())),
        });
        let mut slots_row = row()
            .name("Bottom of your library")
            .gap(sizes.slot_gap)
            .children(boxes);
        slots_row.style.margin = [sizes.slots_gap, 0.0, 0.0, 0.0];
        slots_row
    });
    // The pointer goes through to the table's cards, unless the cards here take presses of
    // their own, which they would inherit the passing from.
    let passes = shown.press.is_none() && shown.choice.is_empty();
    let mut screen = column()
        .name("opening")
        .passes(passes)
        .align_items(Align::Center)
        .child(coin_row)
        .child(heading)
        .child(hand_row)
        .children(slots_row)
        .children(dots)
        .children(choice(shown.choice, style));
    // The flat layer's root puts a screen in its bottom right corner, so the group is anchored
    // below the window's top middle instead, as the web pads `.opening` down from the top.
    let top = Vec2::new(window.size.width / 2.0, sizes.top);
    screen.anchor = Some(Anchor::Point(top, Vec2::new(0.5, 0.0)));
    row()
        .name(SCREEN_NAME)
        .passes(passes)
        // The opening's backdrop is a gradient that leaves the opponent's side of the table
        // clear, in place of the backdrop's flat color.
        .child(
            backdrop(window.size)
                .background([0; 4])
                .background_image(vec![gradient::OPENING_BACKDROP]),
        )
        .child(screen)
}

/// Converts a desktop opening shadow's stage lengths to window points.
fn scaled_shadows(shadows: &[Shadow], scale: f32) -> Vec<Shadow> {
    shadows
        .iter()
        .map(|shadow| Shadow {
            x: shadow.x * scale,
            y: shadow.y * scale,
            blur: shadow.blur * scale,
            spread: shadow.spread * scale,
            ..*shadow
        })
        .collect()
}

/// Shows the hand's scene objects in the opening grid, keeping their boxes while
/// the coin hides the cards.
fn hand_grid(
    shown: &Presentation,
    sizes: &Sizes,
    shape: Card,
    card: impl Fn(ObjectId, f32) -> Element,
) -> Element {
    let item = Vec2::new(sizes.card, sizes.card * shape.height / shape.width);
    let mut hand: Vec<_> = shown
        .hand
        .iter()
        .map(|&slot| card(slot, sizes.card))
        .collect();
    if let Some(fan) = sizes.fan.filter(|_| !shown.hand_hidden) {
        fan_hand(&mut hand, item.y, fan);
    }
    let columns = sizes.hand_columns.unwrap_or(hand.len());
    let grid = Grid::new(columns, item, Vec2::splat(sizes.card_gap));
    let width = grid.size(sizes.hand_columns.unwrap_or(hand.len())).x;
    let mut hand_row = grid_element(row().name("hand"), hand, grid, width, true);
    if let Some(height) = sizes.hand_height {
        hand_row = hand_row.max_height(height).overflow(Overflow::ScrollY);
    }
    if shown.hand_hidden {
        // The cards' boxes lie outside this empty strip's clip, so the table draws none of
        // them through their Show relations, and the strip takes no room, as the web's empty
        // `.opening-hand` takes none.
        hand_row = row()
            .name("hand held")
            .height(0.0)
            .overflow(Overflow::Clip)
            .passes(true)
            .child(hand_row);
    }
    hand_row
}

/// How far one step away from the middle of the hand turns and lowers a card.
#[derive(Clone, Copy)]
struct Fan {
    turn: f32,
    drop: f32,
}

/// Adds the web's `.opening-fan` transform without changing the grid's layout.
#[expect(
    clippy::cast_precision_loss,
    reason = "an opening hand holds only a few cards"
)]
fn fan_hand(cards: &mut [Element], height: f32, fan: Fan) {
    let middle = (cards.len() as f32 - 1.0) / 2.0;
    let from_pivot = Vec2::new(0.0, -height / 2.0);
    for (index, card) in cards.iter_mut().enumerate() {
        let offset = index as f32 - middle;
        let turn = offset * fan.turn;
        // The web rotates around the bottom center. Move the box's center by the same
        // rotation about that pivot, then apply the web's vertical offset.
        card.style.translation = Vec2::from_angle(turn).rotate(from_pivot) - from_pivot
            + Vec2::new(0.0, offset * offset * fan.drop);
        card.show = card.show.map(|show| show.with_turn(turn));
    }
}

/// The play or draw buttons, `.opening-choice`: a row reversed so that the first button stands
/// on the right, 40 points apart (so the children are laid out last to first), each at least
/// `--button-width` wide, 40 points under what precedes them.
fn choice(buttons: &[PromptButton], metrics: OpeningStyle) -> Option<Element> {
    if buttons.is_empty() {
        return None;
    }
    let mut choice = row()
        .name("choice")
        .gap(metrics.choice_gap)
        .justify_content(Justify::Center);
    choice.style.margin = [metrics.choice_top, 0.0, 0.0, 0.0];
    Some(
        choice.children(buttons.iter().enumerate().rev().map(|(at, view)| {
            let widget = if at == 0 { primary } else { button };
            widget(&view.label)
                .min_width(size::BUTTON_WIDTH)
                .enabled(view.enabled)
                .on_press(crate::prompts::bar::Message::Button(view.id))
        })),
    )
}

/// The three gold dots under the title while the game waits for the session or the opponent,
/// `.opening-buttons` holding `.working-dots.opening-waiting`: 34-point type whose dots are
/// 0.26em wide, 0.22em apart, 0.3em in from the row's start. The web pulses them; these stand
/// still.
fn waiting(metrics: OpeningStyle) -> Element {
    let dot = |at: usize| {
        row()
            .name(&format!("dot {}", at + 1))
            .width(metrics.dot)
            .height(metrics.dot)
            .radius(metrics.dot / 2.0)
            .background(color::GOLD)
            .passes(true)
    };
    let mut dots = row()
        .name("working")
        .gap(metrics.dot_gap)
        .passes(true)
        .children((0..3).map(dot));
    dots.style.margin = [0.0, 0.0, 0.0, metrics.dot_inset];
    let mut buttons = row()
        .name("waiting")
        .min_height(metrics.waiting_height)
        .align_items(Align::Center)
        .passes(true)
        .child(dots);
    buttons.style.margin = [metrics.waiting_gap, 0.0, 0.0, 0.0];
    buttons
}

/// The title, as the web's `Opening` words it, given who starts as the opening shows it.
fn title(view: &PlayerView, starting: Option<Side>, picking: bool) -> String {
    let you = &view.players.you;
    match starting {
        None if view.turn.flip_winner == Some(Side::You) => "You won the flip".to_owned(),
        None => "Opponent won the flip".to_owned(),
        Some(_) if picking => "Put on the bottom".to_owned(),
        Some(_) if you.mulligan == Declaration::WillFreeMulligan => "Free Mulligan".to_owned(),
        Some(_) if you.mulligan == Declaration::WillMulligan => "Mulligan".to_owned(),
        Some(_) if you.mulligans == 0 => "Opening hand".to_owned(),
        Some(_) => format!(
            "Mulligan to {}",
            STARTING_HAND.saturating_sub(usize::from(you.mulligans))
        ),
    }
}

/// Whether the session marks the card in `slot` picked in `prompt`.
fn selected(prompt: &PromptView, slot: ObjectId) -> bool {
    (prompt.marks.iter())
        .any(|marked| marked.mark == Mark::Selected && marked.on == Subject::Object { slot })
}

/// The box `width` points wide that shows the scene object in `slot`, named for its card and
/// slot as a locator names a card ("Island #8"). The box takes no press: the card drawn in it
/// is the table's, and a click on it clicks the open prompt on the card (`Client::frame`).
fn card_box(view: &PlayerView, slot: ObjectId, (width, card): (f32, Card)) -> Element {
    let name = (view.objects.iter())
        .find(|object| object.slot == slot)
        .and_then(|object| object.name.as_deref())
        .unwrap_or("card");
    let mut element = Element::new(Role::Region)
        .name(&format!("{name} #{}", slot.0))
        .width(width)
        .height(width * card.height / card.width);
    element.show = Some(Show::object(ElementId(u32::from(slot.0))));
    element
}

/// An empty slot for a card still to be put on the bottom, `.opening-slot`.
fn empty_slot(at: usize, (width, card): (f32, Card)) -> Element {
    Element::new(Role::Region)
        .name(&format!("slot {}", at + 1))
        .width(width)
        .height(width * card.height / card.width)
        .background(color::BACKDROP)
        .passes(true)
}

/// How large the opening's pieces are and where it stands, on one layout.
struct Sizes {
    /// Window points per stage point when the desktop opening is projected.
    header_scale: Option<f32>,
    fan: Option<Fan>,
    /// How far the group's top stands below the window's top.
    top: f32,
    coin_height: f32,
    coin_gap: f32,
    coin_label_gap: f32,
    /// Where the words of the coin stand in its row.
    coin_align: Align,
    coin: TypeStyle,
    title: TypeStyle,
    title_gap: f32,
    /// The width of each card of the hand, and of each card on its way to the bottom.
    card: f32,
    slot: f32,
    card_gap: f32,
    slot_gap: f32,
    slots_gap: f32,
    /// A limit on the number of hand cards in one row, and the height available for them.
    hand_columns: Option<usize>,
    hand_height: Option<f32>,
}

impl Sizes {
    fn for_profile(shown: &Presentation, profile: Profile) -> Self {
        let opening = profile.data().hud.opening;
        let room = profile
            .slot("opening")
            .expect("validated opening slot")
            .rect;
        if let Some(wrapped) = opening.wrapped_hand {
            let screen = profile.data().screen.expect("validated wrapped opening");
            Self::wrapped(
                shown,
                (wrapped, profile.card()),
                room,
                profile.window().size.width,
                screen,
            )
        } else {
            Self::single_row(
                room.y,
                opening.single_row.expect("validated single-row opening"),
                profile.screen_projection(),
            )
        }
    }

    /// The desktop's opening hand uses the same stage units as the reference screen.
    fn single_row(top: f32, metrics: SingleRow, projection: ScreenProjection) -> Self {
        let scale = projection.scale;
        let text_style = |style: TypeStyle| TypeStyle {
            size: style.size * scale,
            letter_spacing: style.letter_spacing * scale,
            ..style
        };
        Self {
            header_scale: Some(scale),
            fan: Some(Fan {
                turn: metrics.fan_turn.to_radians(),
                drop: metrics.fan_drop * scale,
            }),
            top: projection.origin.y + top * scale,
            coin_height: metrics.coin_height * scale,
            coin_gap: metrics.coin_gap * scale,
            coin_label_gap: size::OPENING_COIN_LABEL_GAP * scale,
            coin_align: Align::Center,
            coin: text_style(COIN),
            title: text_style(TITLE),
            title_gap: metrics.title_gap * scale,
            card: metrics.card * scale,
            slot: metrics.slot * scale,
            card_gap: metrics.card_gap * scale,
            slot_gap: metrics.slot_gap * scale,
            slots_gap: metrics.slots_gap * scale,
            hand_columns: None,
            hand_height: None,
        }
    }

    /// The phone's in `window` (`.phone-opening`, `phoneCardWidth` in
    /// `ui/src/screens/Opening.tsx`): the hand in rows of four cards, as large as fit between
    /// the coin and the dock with `rows` rows of the hand, `slots` slots for cards going to the
    /// bottom and `actions` buttons in the dock.
    fn wrapped(
        shown: &Presentation,
        (wrapped, card): (WrappedHand, Card),
        room: Rect,
        width: f32,
        screen: ScreenMetrics,
    ) -> Self {
        let Presentation {
            rows,
            slots,
            actions,
            ..
        } = *shown;
        // Reserve space while cards move from the hand into the bottom slots.
        let rows = rows.div_ceil(wrapped.per_row.get());
        let narrow = width < wrapped.narrow_below;
        let coin = if narrow {
            wrapped.coin_narrow
        } else {
            wrapped.coin
        };
        let per_row = wrapped.per_row.get() as f32;
        let header =
            coin / 2.0 + wrapped.coin_line + wrapped.coin_gap + wrapped.title + wrapped.title_gap;
        let footer = wrapped.dock_bottom
            + if actions > 0 {
                actions as f32 * screen.action_height + (actions - 1) as f32 * screen.action_gap
            } else {
                wrapped.no_actions
            };
        let most = if narrow {
            wrapped.card_most_narrow
        } else {
            wrapped.card_most
        };
        let reserved_gap = if slots > 0 { wrapped.slots_gap } else { 0.0 };
        let card_rows = rows as f32 + if slots > 0 { wrapped.slot_scale } else { 0.0 };
        let aspect = card.height / card.width;
        let height = room.height - header - footer - reserved_gap;
        let row_gaps = rows.saturating_sub(1) as f32 * wrapped.gap;
        let card = Grid::fitting_width(
            Vec2::new(room.width, height - row_gaps),
            Vec2::new(per_row, card_rows),
            Vec2::new(wrapped.gap, 0.0),
            aspect,
        )
        .min(most)
        .max(0.0)
        .floor();
        let slot = if slots > 0 {
            let room = room.width - wrapped.slot_edge;
            (wrapped.slot_scale * card)
                .min((room - (slots - 1) as f32 * wrapped.gap) / slots as f32)
        } else {
            card
        };
        Self {
            header_scale: None,
            fan: None,
            top: room.y - coin / 2.0,
            coin_height: coin + wrapped.coin_line,
            coin_gap: wrapped.coin_gap,
            coin_label_gap: 0.0,
            coin_align: Align::End,
            coin: TypeStyle {
                size: wrapped.coin_line,
                ..COIN
            },
            title: TypeStyle {
                size: wrapped.title,
                ..TITLE
            },
            title_gap: wrapped.title_gap,
            card,
            slot,
            card_gap: wrapped.gap,
            slot_gap: wrapped.gap,
            slots_gap: wrapped.slots_gap,
            hand_columns: Some(wrapped.per_row.get()),
            hand_height: Some((height - if slots > 0 { slot * aspect } else { 0.0 }).max(0.0)),
        }
    }
}
