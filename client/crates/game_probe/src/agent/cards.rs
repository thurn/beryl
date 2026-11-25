//! Checks that visible, named game cards have their own faces, and that a pile that holds cards
//! shows a card on top.

use kit_frame::frame::{Frame, Role, StateBits};
use kit_probe::agent::{Context, Fault, Rule, Subject, tree::locator_of};
use kit_scene::hit::bounds;

/// The cards whose face the viewer can see and whose name the viewer knows.
pub(super) fn face_up(frame: &Frame) -> impl Iterator<Item = u32> + '_ {
    (0..frame.resolved.len() as u32).filter(|&index| {
        let element = &frame.resolved[index as usize];
        element.role == Role::Card
            && element.opacity > 0.0
            && !element.state.has(StateBits::FACE_DOWN)
            && !frame.name(element.name).is_empty()
    })
}

/// The piles that hold more cards than the top two that are drawn as cards (`lay_pile` in
/// `game_table`). A pile writes its element only while it has a top card, and its `count` is
/// how many cards it holds.
pub(super) fn piles(frame: &Frame) -> impl Iterator<Item = u32> + '_ {
    (0..frame.resolved.len() as u32).filter(|&index| {
        let element = &frame.resolved[index as usize];
        element.role == Role::Pile && element.count > 2
    })
}

/// Whether some visible card covers the middle of the pile at `pile`. Without one the pile shows
/// the dark rims of the cards beneath its top two with nothing on them, a black card where its
/// top card should be, or nothing at all on the phone, which draws no rims. The card need not
/// be the pile's own: a card on its way out of the pile still covers it.
fn shows_a_top_card(frame: &Frame, pile: u32) -> bool {
    let area = bounds(&frame.resolved[pile as usize]);
    let middle = (area.x + area.width / 2.0, area.y + area.height / 2.0);
    frame.resolved.iter().any(|card| {
        let at = bounds(card);
        card.role == Role::Card
            && card.opacity > 0.0
            && (at.x..=at.x + at.width).contains(&middle.0)
            && (at.y..=at.y + at.height).contains(&middle.1)
    })
}

/// Checks for cards drawn face up with no face of their own, and for piles that hold cards but
/// show none on top.
#[must_use]
pub fn blank_cards(frame: &Frame, context: &Context) -> Vec<Fault> {
    let fault = |index: u32, what: &str| {
        let subject = Subject {
            locator: locator_of(frame, index),
            rect: bounds(&frame.resolved[index as usize]),
        };
        let message = format!("{} {what}", subject.describe());
        Fault {
            rule: Rule::BlankCard,
            elements: vec![subject],
            at: context.at,
            message,
        }
    };
    let faceless = face_up(frame)
        .filter(|&index| frame.resolved[index as usize].face.is_none())
        .map(|index| fault(index, "is drawn face up with no face of its own"));
    let bare = piles(frame)
        .filter(|&index| !shows_a_top_card(frame, index))
        .map(|index| fault(index, "holds cards but no card is drawn on top of it"));
    faceless.chain(bare).collect()
}
