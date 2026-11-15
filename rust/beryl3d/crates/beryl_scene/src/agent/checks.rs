//! The interface checks that read frames alone (docs/beryl-3d.md, "Interface checks"). Each is a
//! function from a frame, or two for "lines hold", to the faults it finds. They know nothing
//! about any card or screen. The thresholds are the `audit` object of `data/beryl3d/gates.json`.

use std::collections::BTreeMap;

use glam::Vec2;

use super::fault::{Context, Fault, Layout, Rule, Subject};
use super::hit::{bounds, contains, free_rect, hit, intersection, is_within};
use super::tree::locator_of;
use crate::data::audit;
use crate::frame::{Frame, Place, Role, StateBits};
use crate::input::{Rect, Size};

/// Every check that needs one frame, in the doc's order.
#[must_use]
pub fn check(frame: &Frame, context: &Context) -> Vec<Fault> {
    let mut faults = on_screen(frame, context);
    faults.extend(overlap(frame, context));
    faults.extend(reachable(frame, context));
    faults.extend(duplicates(frame, context));
    faults.extend(named(frame, context));
    faults.extend(blank_cards(frame, context));
    faults
}

/// On screen: every interactive element lies inside the window's safe area.
#[must_use]
pub fn on_screen(frame: &Frame, context: &Context) -> Vec<Fault> {
    let inset = context.safe_area;
    let safe = Rect {
        x: inset.left,
        y: inset.top,
        width: context.size.width - inset.left - inset.right,
        height: context.size.height - inset.top - inset.bottom,
    };
    interactive(frame)
        .filter(|&index| !contains(safe, bounds(&frame.resolved[index as usize])))
        .map(|index| {
            let subject = subject(frame, index);
            fault(
                Rule::OnScreen,
                context,
                format!(
                    "{} is not inside the safe area ({})",
                    subject.describe(),
                    super::fault::rect_text(safe)
                ),
                vec![subject],
            )
        })
        .collect()
}

/// No overlap: no two interactive elements of the flat layer intersect, unless one contains
/// the other in the tree. Cards are exempt, because rows and hands overlap them on purpose.
#[must_use]
pub fn overlap(frame: &Frame, context: &Context) -> Vec<Fault> {
    let elements: Vec<u32> = interactive(frame)
        .filter(|&index| {
            let element = &frame.resolved[index as usize];
            element.place == Place::Layer && element.role != Role::Card
        })
        .collect();
    let mut faults = Vec::new();
    for (at, &first) in elements.iter().enumerate() {
        for &second in &elements[at + 1..] {
            if is_within(frame, first, second) || is_within(frame, second, first) {
                continue;
            }
            let (a, b) = (
                bounds(&frame.resolved[first as usize]),
                bounds(&frame.resolved[second as usize]),
            );
            if intersection(a, b).is_none() {
                continue;
            }
            let (a, b) = (subject(frame, first), subject(frame, second));
            faults.push(fault(
                Rule::Overlap,
                context,
                format!("{} overlaps {}", a.describe(), b.describe()),
                vec![a, b],
            ));
        }
    }
    faults
}

/// Reachable: a hit test at the centre of an interactive flat-layer element returns that
/// element (or something inside it). Every interactive card at rest has an uncovered area at
/// least `TARGET_POINTS` square (`PHONE_TARGET_POINTS` on the phone layout) where a hit test
/// returns it.
#[must_use]
pub fn reachable(frame: &Frame, context: &Context) -> Vec<Fault> {
    let side = match context.layout {
        Layout::Desktop => audit::TARGET_POINTS,
        Layout::Phone => audit::PHONE_TARGET_POINTS,
    };
    let mut faults = Vec::new();
    for index in interactive(frame) {
        let element = &frame.resolved[index as usize];
        if element.role == Role::Card {
            if element.state.has(StateBits::MOVING) || element.state.has(StateBits::HELD) {
                continue;
            }
            let min = Size {
                width: side,
                height: side,
            };
            if free_rect(frame, index, min).is_none() {
                let subject = subject(frame, index);
                faults.push(fault(
                    Rule::Reachable,
                    context,
                    format!(
                        "{} has no uncovered area {side} by {side} points where a press reaches it",
                        subject.describe()
                    ),
                    vec![subject],
                ));
            }
            continue;
        }
        if element.place != Place::Layer {
            continue;
        }
        let rect = bounds(element);
        let center = Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        let reached = hit(frame, center);
        if reached.is_some_and(|at| is_within(frame, at, index)) {
            continue;
        }
        let mut subjects = vec![subject(frame, index)];
        let message = match reached {
            Some(other) => {
                subjects.push(subject(frame, other));
                format!(
                    "a press at the centre of {} reaches {}",
                    subjects[0].describe(),
                    subjects[1].describe()
                )
            }
            None => format!(
                "a press at the centre of {} reaches nothing",
                subjects[0].describe()
            ),
        };
        faults.push(fault(Rule::Reachable, context, message, subjects));
    }
    faults
}

/// No duplicates: no two elements share an id, so no game object has two cards, and no two
/// sibling elements of the flat layer share a role and a name. A card's id is its object's
/// slot and a flat-layer element's is given by the layer, so the two kinds are counted apart.
#[must_use]
pub fn duplicates(frame: &Frame, context: &Context) -> Vec<Fault> {
    let mut faults = Vec::new();
    let mut ids: BTreeMap<(bool, u32), u32> = BTreeMap::new();
    let mut siblings: BTreeMap<(u32, &str, &str), u32> = BTreeMap::new();
    for (index, element) in frame.resolved.iter().enumerate() {
        let index = index as u32;
        let key = (element.role == Role::Card, element.id.0);
        if let Some(&first) = ids.get(&key) {
            let (a, b) = (subject(frame, first), subject(frame, index));
            faults.push(fault(
                Rule::Duplicate,
                context,
                format!("{} and {} share an id", a.describe(), b.describe()),
                vec![a, b],
            ));
        } else {
            ids.insert(key, index);
        }
        let name = frame.name(element.name);
        if element.place != Place::Layer || name.is_empty() {
            continue;
        }
        let key = (element.parent, element.role.word(), name);
        if let Some(&first) = siblings.get(&key) {
            let (a, b) = (subject(frame, first), subject(frame, index));
            faults.push(fault(
                Rule::Duplicate,
                context,
                format!(
                    "{} and {} are siblings with the same role and name",
                    a.describe(),
                    b.describe()
                ),
                vec![a, b],
            ));
        } else {
            siblings.insert(key, index);
        }
    }
    faults
}

/// Named: every interactive element has a non-empty name. (Every element has a role, because
/// `Role` has no value for none.)
#[must_use]
pub fn named(frame: &Frame, context: &Context) -> Vec<Fault> {
    interactive(frame)
        .filter(|&index| frame.name(frame.resolved[index as usize].name).is_empty())
        .map(|index| {
            let subject = subject(frame, index);
            fault(
                Rule::Named,
                context,
                format!("{} is interactive and has no name", subject.describe()),
                vec![subject],
            )
        })
        .collect()
}

/// No blank card: every card drawn face up, whose name the viewer knows, has a face layer of
/// its own.
#[must_use]
pub fn blank_cards(frame: &Frame, context: &Context) -> Vec<Fault> {
    (0..frame.resolved.len() as u32)
        .filter(|&index| {
            let element = &frame.resolved[index as usize];
            element.role == Role::Card
                && element.opacity > 0.0
                && !element.state.has(StateBits::FACE_DOWN)
                && !frame.name(element.name).is_empty()
                && element.face.is_none()
        })
        .map(|index| {
            let subject = subject(frame, index);
            fault(
                Rule::BlankCard,
                context,
                format!(
                    "{} is drawn face up with no face of its own",
                    subject.describe()
                ),
                vec![subject],
            )
        })
        .collect()
}

/// Lines hold: an arrow whose two ends are at rest does not change shape. An arrow is matched
/// across the frames by its id. Its ends are at rest when neither of its end points moved more
/// than `LINE_END_POINTS` and it is moving in neither frame, and it changed shape when either
/// control point moved `LINE_RESHAPE_POINTS` or more. `context` is the later frame's.
#[must_use]
pub fn lines_hold(before: &Frame, after: &Frame, context: &Context) -> Vec<Fault> {
    let arrows = |frame: &Frame| {
        frame
            .resolved
            .iter()
            .enumerate()
            .filter(|(_, element)| element.role == Role::Arrow)
            .map(|(index, element)| (element.id.0, index as u32))
            .collect::<BTreeMap<u32, u32>>()
    };
    let earlier = arrows(before);
    let mut faults = Vec::new();
    for (index, now) in after.resolved.iter().enumerate() {
        if now.role != Role::Arrow {
            continue;
        }
        let Some(&was_index) = earlier.get(&now.id.0) else {
            continue;
        };
        let was = &before.resolved[was_index as usize];
        if was.state.has(StateBits::MOVING) || now.state.has(StateBits::MOVING) {
            continue;
        }
        let moved = |corner: usize| was.corners[corner].distance(now.corners[corner]);
        let still = moved(0) <= audit::LINE_END_POINTS && moved(3) <= audit::LINE_END_POINTS;
        let reshaped =
            moved(1) >= audit::LINE_RESHAPE_POINTS || moved(2) >= audit::LINE_RESHAPE_POINTS;
        if still && reshaped {
            let subject = subject(after, index as u32);
            faults.push(fault(
                Rule::LinesHold,
                context,
                format!(
                    "{} changed shape while both its ends were at rest",
                    subject.describe()
                ),
                vec![subject],
            ));
        }
    }
    faults
}

/// The indexes of the interactive elements.
fn interactive(frame: &Frame) -> impl Iterator<Item = u32> + '_ {
    frame
        .resolved
        .iter()
        .enumerate()
        .filter(|(_, element)| element.interactive)
        .map(|(index, _)| index as u32)
}

fn subject(frame: &Frame, index: u32) -> Subject {
    Subject {
        locator: locator_of(frame, index),
        rect: bounds(&frame.resolved[index as usize]),
    }
}

fn fault(rule: Rule, context: &Context, message: String, elements: Vec<Subject>) -> Fault {
    Fault {
        rule,
        elements,
        at: context.at,
        message,
    }
}
