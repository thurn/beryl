//! The manual table's selection (docs/manual-multiplayer.md, "Selection"), as the web's
//! `registerSelection` (`ui/src/table/select.ts`) and `Selection` (`ui/src/table/Selection.tsx`)
//! make it: a click with Cmd (Ctrl off the Mac) adds a card to the selection or takes it out,
//! and a drag over the empty table sweeps a rectangle that selects every card it touches. A
//! command given to a selected card acts on every selected card (`commands::actions_for`). Each
//! selected card wears the `selected` mark (`Mark::Selected`), and Escape empties the selection.
//! Above the highest selected permanent stands a summary of the selected permanents: how many
//! there are, and their total power and total toughness.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use beryl_protocol::script::{Mark, Subject};
use glam::Vec2;

use game_play::light::{Lit, MarkSpan, Span};
use game_play::verbs::Interval;
use kit_base::data::space::flat::table::{selection as summary_space, sweep};
use kit_base::tokens::{color, type_style};
use kit_frame::frame::{Frame, Place, Role, StateBits};
use kit_ui::element::{Align, Anchor, Element, row, text};
use kit_ui::input::Rect;

/// The name of the rectangle a sweep draws.
pub const SWEEP: &str = "selection-rectangle";

/// The name of the summary over the selected permanents, the web's `aria-label`.
pub const SUMMARY: &str = "Selection";

/// Adds `slot` to `selection`, or takes it out if it is there, as a Cmd-click does.
pub fn toggle(selection: &mut Vec<ObjectId>, slot: ObjectId) {
    if let Some(at) = selection.iter().position(|&selected| selected == slot) {
        selection.remove(at);
    } else {
        selection.push(slot);
    }
}

/// The rectangle a sweep from `from` to `to` covers, in logical points.
#[must_use]
pub fn sweep_box(from: Vec2, to: Vec2) -> Rect {
    let (low, high) = (from.min(to), from.max(to));
    Rect {
        x: low.x,
        y: low.y,
        width: high.x - low.x,
        height: high.y - low.y,
    }
}

/// The cards a sweep over `rect` selects in `frame`: each card on the battlefield, on the stack
/// or in the viewer's hand that the table can act on at `view` and whose box on the screen
/// touches the rectangle, in the order the frame drew them. A card in a pile, a hidden card and
/// a held card are left out, as the web leaves out piles and hidden cards.
#[must_use]
pub fn swept(frame: &Frame, rect: Rect, view: &PlayerView) -> Vec<ObjectId> {
    let mut selected = Vec::new();
    for element in &frame.resolved {
        let Place::Card { zone, side, .. } = element.place else {
            continue;
        };
        let reachable = match zone {
            ZoneName::Battlefield | ZoneName::Stack => true,
            ZoneName::Hand => side == Side::You,
            _ => false,
        };
        if element.role != Role::Card
            || !reachable
            || element.state.0 & StateBits::HELD != 0
            || !touches(rect, &element.corners)
        {
            continue;
        }
        let Ok(slot) = u16::try_from(element.id.0).map(ObjectId) else {
            continue;
        };
        if actionable(view, slot) && !selected.contains(&slot) {
            selected.push(slot);
        }
    }
    selected
}

/// Whether the table can act on the card in `slot` at `view`: whether its table gives the card
/// a reference.
#[must_use]
pub fn actionable(view: &PlayerView, slot: ObjectId) -> bool {
    view.table.as_ref().is_some_and(|table| {
        (table.object_actions.iter()).any(|action| action.subject == Subject::Object { slot })
    })
}

/// Takes out of `selection` every card the table can no longer act on at `view`, as the web's
/// selection keeps only cards with an action.
pub fn keep(selection: &mut Vec<ObjectId>, view: &PlayerView) {
    selection.retain(|&slot| actionable(view, slot));
}

/// Whether the box round `corners` touches `rect`, edges included (`touches` in select.ts).
fn touches(rect: Rect, corners: &[Vec2; 4]) -> bool {
    let low = corners
        .iter()
        .fold(Vec2::INFINITY, |low, &corner| low.min(corner));
    let high = (corners.iter()).fold(Vec2::NEG_INFINITY, |high, &corner| high.max(corner));
    low.x <= rect.x + rect.width
        && high.x >= rect.x
        && low.y <= rect.y + rect.height
        && high.y >= rect.y
}

/// The rectangle a sweep draws over `rect`, as the web's `.selection-rectangle`: a thin gold
/// line round a faint gold wash, which the pointer goes through.
#[must_use]
pub fn rectangle(rect: Rect) -> Element {
    let mut element = Element::new(Role::Region)
        .name(SWEEP)
        .width(rect.width)
        .height(rect.height)
        .background(color::SWEEP)
        .border((color::SWEEP_RIM, sweep::RIM))
        .passes(true);
    element.anchor = Some(Anchor::Point(Vec2::new(rect.x, rect.y), Vec2::ZERO));
    element
}

/// Makes the `selected` marks in `lit` those of `selection` at `now`: a card that has left the
/// selection loses its mark, and one that has joined it gains one, both at once, as the web's
/// `SelectionMark` comes and goes with the selection. Allocates nothing while `lit` has room.
pub fn mark(lit: &mut Lit, selection: &[ObjectId], now: u64) {
    let at = Interval { from: now, to: now };
    for index in 0..lit.open().len() {
        let open = lit.open()[index];
        if let Span::Mark(MarkSpan {
            mark: Mark::Selected,
            on: Subject::Object { slot },
        }) = open.span
            && open.ends.is_none()
            && !selection.contains(&slot)
        {
            lit.end(&open.span, at);
        }
    }
    for &slot in selection {
        let span = Span::Mark(MarkSpan {
            mark: Mark::Selected,
            on: Subject::Object { slot },
        });
        let shown = (lit.open().iter()).any(|open| open.span == span && open.ends.is_none());
        if !shown {
            lit.begin(span, at);
        }
    }
}

/// The selected permanent whose box stands highest on the window in `frame`, which the summary
/// stands over, as the web's `Selection` anchors its summary to the selected permanent with the
/// least `y`. `None` when no selected card is on the battlefield.
#[must_use]
pub fn highest(frame: &Frame, selection: &[ObjectId]) -> Option<ObjectId> {
    if selection.is_empty() {
        return None;
    }
    let mut best: Option<(f32, ObjectId)> = None;
    for element in &frame.resolved {
        let Place::Card {
            zone: ZoneName::Battlefield,
            ..
        } = element.place
        else {
            continue;
        };
        let Ok(slot) = u16::try_from(element.id.0).map(ObjectId) else {
            continue;
        };
        if element.role != Role::Card || !selection.contains(&slot) {
            continue;
        }
        let top = (element.corners.iter()).fold(f32::INFINITY, |top, corner| top.min(corner.y));
        if best.is_none_or(|(least, _)| top < least) {
            best = Some((top, slot));
        }
    }
    best.map(|(_, slot)| slot)
}

/// The summary of the selected permanents at `view`, standing over the permanent `over`, as
/// the web's `.selection-summary`: their count and their total power over their total
/// toughness, on a dark panel with a faint gold line round it. Only cards on the battlefield
/// count, and a permanent with no power and toughness adds nothing to the totals. `None` with
/// no permanent to stand over.
#[must_use]
pub fn summary(
    view: &PlayerView,
    selection: &[ObjectId],
    over: Option<ObjectId>,
) -> Option<Element> {
    let over = over?;
    let (mut count, mut power, mut toughness) = (0, 0, 0);
    for object in &view.objects {
        if object.zone != ZoneName::Battlefield || !selection.contains(&object.slot) {
            continue;
        }
        count += 1;
        if let Some(pt) = object.pt {
            power += pt.power;
            toughness += pt.toughness;
        }
    }
    if count == 0 {
        return None;
    }
    let cards = if count == 1 { "card" } else { "cards" };
    let label = |words: &str| {
        text(words)
            .style(type_style::MENU)
            .color(color::SELECTION_SUMMARY_INK)
    };
    let mut panel = row()
        .name(SUMMARY)
        .gap(summary_space::GAP)
        .align_items(Align::Center)
        .background(color::SELECTION_SUMMARY)
        .border((color::SELECTION_SUMMARY_RIM, summary_space::RIM))
        .radius(summary_space::RADIUS)
        .passes(true)
        .child(label(&format!("{count} {cards}")))
        .child(label(&format!("{power}/{toughness}")));
    panel.style.padding = [
        summary_space::PADDING_Y,
        summary_space::PADDING_X,
        summary_space::PADDING_Y,
        summary_space::PADDING_X,
    ];
    panel.anchor = Some(Anchor::Above(
        Subject::Object { slot: over },
        summary_space::ABOVE,
    ));
    Some(panel)
}
