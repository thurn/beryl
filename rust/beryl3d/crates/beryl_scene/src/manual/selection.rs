//! The manual table's selection (docs/manual-multiplayer.md, "Selection"), as the web's
//! `registerSelection` (`ui/src/table/select.ts`) and `Selection` (`ui/src/table/Selection.tsx`)
//! make it: a click with Cmd (Ctrl off the Mac) adds a card to the selection or takes it out,
//! and a drag over the empty table sweeps a rectangle that selects every card it touches. A
//! command given to a selected card acts on every selected card (`commands::actions_for`). Each
//! selected card wears the `selected` mark (`Mark::Selected`), and Escape empties the selection.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ObjectId, Side, ZoneName};
use beryl_protocol::script::{Mark, Subject};
use glam::Vec2;

use crate::data::space::flat::table::sweep;
use crate::element::{Anchor, Element};
use crate::frame::{Frame, Place, Role, StateBits};
use crate::input::Rect;
use crate::light::{Lit, MarkSpan, Span};
use crate::tokens::color;
use crate::verbs::Interval;

/// The name of the rectangle a sweep draws.
pub const SWEEP: &str = "selection-rectangle";

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
