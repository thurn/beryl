//! Checks that the cards of a battlefield row stand apart unless the row overlaps them on
//! purpose.

use game_table::table::labels::OVERLAPPED;
use glam::Vec2;
use kit_frame::frame::{Frame, Role, StateBits};
use kit_probe::agent::{Context, Fault, Rule, Subject, tree::locator_of};
use kit_scene::hit::bounds;

/// How far, in logical points, two cards' outlines may run into each other before they count
/// as overlapping. It allows for rounding where two cards meet edge to edge.
const SLACK: f32 = 1.0;

/// The cards of battlefield rows that the row lays out side by side: visible, at rest, not
/// shown in a box or raised to be read away from the row, and not marked as overlapped on
/// purpose (`OVERLAPPED`: a copy behind a pile's top card, a card tucked under its host, a row
/// so crowded that its cards overlap at their least size, or a card on its way along a
/// script's track).
pub(super) fn row_cards(frame: &Frame) -> impl Iterator<Item = u32> + '_ {
    (0..frame.resolved.len() as u32).filter(|&index| {
        let element = &frame.resolved[index as usize];
        element.role == Role::Card
            && element.opacity > 0.0
            && !element.state.has(OVERLAPPED)
            && !element.state.has(StateBits::HOVERED)
            && !element.state.has(StateBits::SHOWN)
            && !element.state.has(StateBits::RAISED)
            && is_row(element.place.label())
    })
}

/// Whether `label` names a battlefield row, such as `battlefield.yours.lands`.
fn is_row(label: &str) -> bool {
    label.starts_with("battlefield.") && label.matches('.').count() == 2
}

/// Row overlap: no two cards of one battlefield row overlap where they are seen, unless the
/// row marks one of them as overlapped on purpose. A crowded row shrinks its cards, down to
/// its least size, so that each stands apart with a gap, before it lays one over another.
/// Judged only on a frame where nothing moves, since a card passing along its row covers its
/// neighbours for a moment.
#[must_use]
pub fn row_overlaps(frame: &Frame, context: &Context) -> Vec<Fault> {
    let moving = frame
        .resolved
        .iter()
        .any(|element| element.state.has(StateBits::MOVING) || element.state.has(StateBits::HELD));
    if moving {
        return Vec::new();
    }
    let cards: Vec<u32> = row_cards(frame).collect();
    let mut faults = Vec::new();
    for (at, &first) in cards.iter().enumerate() {
        let a = &frame.resolved[first as usize];
        for &second in &cards[at + 1..] {
            let b = &frame.resolved[second as usize];
            if a.place.label() != b.place.label() || penetration(&a.corners, &b.corners) <= SLACK {
                continue;
            }
            let [a, b] = [first, second].map(|index| Subject {
                locator: locator_of(frame, index),
                rect: bounds(&frame.resolved[index as usize]),
            });
            faults.push(Fault {
                rule: Rule::RowOverlap,
                message: format!("{} overlaps {} in its row", a.describe(), b.describe()),
                elements: vec![a, b],
                at: context.at,
            });
        }
    }
    faults
}

/// How far two convex outlines run into each other: the least depth along any edge's normal,
/// 0 when some edge separates them.
fn penetration(a: &[Vec2; 4], b: &[Vec2; 4]) -> f32 {
    let mut least = f32::INFINITY;
    for quad in [a, b] {
        for index in 0..4 {
            let edge = quad[(index + 1) % 4] - quad[index];
            let Some(normal) = Vec2::new(-edge.y, edge.x).try_normalize() else {
                continue;
            };
            let span = |corners: &[Vec2; 4]| {
                corners
                    .iter()
                    .map(|corner| corner.dot(normal))
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), value| {
                        (low.min(value), high.max(value))
                    })
            };
            let ((a_low, a_high), (b_low, b_high)) = (span(a), span(b));
            least = least.min((a_high.min(b_high) - a_low.max(b_low)).max(0.0));
        }
    }
    least
}
