//! The interface checks that read frames alone (docs/beryl-3d.md, "Interface checks"). Each is a
//! function from a frame, or two for "lines hold", to the faults it finds. They know nothing
//! about any card or screen. The thresholds are the `audit` object of `data/beryl3d/gates.json`.

use std::collections::BTreeMap;

use glam::Vec2;

use super::fault::{Context, Fault, Layout, Rule, Subject};
use super::hit::{
    Overlaps, bounds, contains, has_free_rect, hit, intersection, is_within, visible_bounds,
};
use super::tree::locator_of;
use crate::data::audit;
use crate::frame::{Frame, Place, REVEALED_FACE, Role, StateBits};
use crate::input::{Rect, Size};

/// Every check that needs one frame, in the doc's order.
#[must_use]
pub fn check(frame: &Frame, context: &Context) -> Vec<Fault> {
    let mut faults = on_screen(frame, context);
    faults.extend(overlap(frame, context));
    faults.extend(reachable(frame, context));
    faults.extend(duplicates(frame, context));
    faults.extend(text_fits(frame, context));
    faults.extend(named(frame, context));
    faults.extend(blank_cards(frame, context));
    faults.extend(clips(frame, context));
    faults.extend(phone_sizes(frame, context));
    faults
}

/// On screen: every interactive element lies inside the window's safe area. A card needs only a
/// square `TARGET_POINTS` on a side (`PHONE_TARGET_POINTS` on the phone layout), or as much of it
/// as a card turned edge-on has, inside it, because a hand is held partly past the bottom edge,
/// as Arena's and the web interface's are. A card the pointer holds is not at rest, and goes
/// where the pointer takes it.
#[must_use]
pub fn on_screen(frame: &Frame, context: &Context) -> Vec<Fault> {
    let side = target_side(context);
    let inset = context.safe_area;
    let safe = Rect {
        x: inset.left,
        y: inset.top,
        width: context.size.width - inset.left - inset.right,
        height: context.size.height - inset.top - inset.bottom,
    };
    interactive(frame)
        .filter(|&index| {
            let element = &frame.resolved[index as usize];
            let rect = bounds(element);
            if element.state.has(StateBits::HELD) {
                false
            } else if element.role == Role::Card {
                !intersection(safe, rect).is_some_and(|in_safe| {
                    in_safe.width >= side.min(rect.width) && in_safe.height >= side.min(rect.height)
                })
            } else {
                !contains(safe, rect)
            }
        })
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

/// The side of the square of a card that must be on screen and uncovered, in logical points.
fn target_side(context: &Context) -> f32 {
    match context.layout {
        Layout::Desktop => audit::TARGET_POINTS,
        Layout::Phone => audit::PHONE_TARGET_POINTS,
    }
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

/// Reachable: a hit test at the centre of the visible part of an interactive flat-layer element
/// (its box cut to its clip) returns that element (or something inside it). Every interactive card at rest has an uncovered area at
/// least `TARGET_POINTS` square (`PHONE_TARGET_POINTS` on the phone layout) where a hit test
/// returns it. Cards are judged only on a frame where nothing on the table moves: a card passing
/// over another covers it for a moment, which leaves no card out of reach. A hand card the
/// pointer has up out of the fan is judged where it rests in the fan (`Frame::lifted`), as the
/// pointer's hit test finds it: a press on a card the raised card covers moves the pointer
/// there first, which lowers the raised card and raises the one beneath.
#[must_use]
pub fn reachable(frame: &Frame, context: &Context) -> Vec<Fault> {
    let resting;
    let frame = if frame.lifted.is_empty() {
        frame
    } else {
        let mut copy = frame.clone();
        copy.swap_lifted();
        resting = copy;
        &resting
    };
    let side = target_side(context);
    let mut faults = Vec::new();
    // Built on the first card at rest, so a frame with none builds no index.
    let mut overlaps: Option<Overlaps> = None;
    let still = !frame
        .resolved
        .iter()
        .any(|element| element.state.has(StateBits::MOVING) || element.state.has(StateBits::HELD));
    for index in interactive(frame) {
        let element = &frame.resolved[index as usize];
        if element.role == Role::Card {
            if !still {
                continue;
            }
            let min = Size {
                width: side,
                height: side,
            };
            let overlaps = overlaps.get_or_insert_with(|| Overlaps::new(frame));
            if !has_free_rect(frame, overlaps, index, min) {
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
        // A row scrolled half out of its list is pressed on the part the list still shows.
        let rect = visible_bounds(element).unwrap_or_else(|| bounds(element));
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

/// The kinds of element whose ids `duplicates` counts apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Layer,
    Card,
    /// A card's face in the Revealed Cards pile.
    Revealed,
}

/// No duplicates: no two elements share an id, so no game object has two cards, and no two
/// sibling elements of the flat layer share a role and a name. A card's id is its object's
/// slot and a flat-layer element's is given by the layer, so the two kinds are counted apart.
/// A card id at or above `REVEALED_FACE` is the face of the card in slot `id - REVEALED_FACE`
/// in the Revealed Cards pile, which a card held up to be read has beside itself: it is no
/// second card of that object, and is counted apart from the cards.
/// Arrows are not held to the sibling rule: a spell with two targets of one kind has two arrows
/// named alike, told apart by their ids.
#[must_use]
pub fn duplicates(frame: &Frame, context: &Context) -> Vec<Fault> {
    let mut faults = Vec::new();
    let mut ids: BTreeMap<(Kind, u32), u32> = BTreeMap::new();
    let mut siblings: BTreeMap<(u32, &str, &str), u32> = BTreeMap::new();
    for (index, element) in frame.resolved.iter().enumerate() {
        let index = index as u32;
        let key = match element.role {
            Role::Card if element.id.0 >= REVEALED_FACE => (Kind::Revealed, element.id.0),
            Role::Card => (Kind::Card, element.id.0),
            _ => (Kind::Layer, element.id.0),
        };
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
        if element.place != Place::Layer || element.role == Role::Arrow || name.is_empty() {
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

/// Text fits: no laid-out line of text reaches outside its element's box, as a line wider than
/// the box, or lines that together are taller than it, would.
#[must_use]
pub fn text_fits(frame: &Frame, context: &Context) -> Vec<Fault> {
    frame
        .lines
        .iter()
        .filter(|line| !contains(bounds(&frame.resolved[line.element as usize]), line.rect))
        .map(|line| {
            let subject = subject(frame, line.element);
            fault(
                Rule::TextFits,
                context,
                format!(
                    "a line of {}'s text ({}) reaches outside its box",
                    subject.describe(),
                    super::fault::rect_text(line.rect)
                ),
                vec![subject],
            )
        })
        .collect()
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

/// Phone sizes: on the phone layout, every interactive element of the flat layer is at least
/// `PHONE_TARGET_POINTS` square, a size a finger can press, and no line of text is smaller than
/// `PHONE_TEXT_POINTS`, as the web interface's `phoneFaults` (`ui/scripts/phone-faults.mjs`)
/// holds its phone shots. Cards are left to "reachable", which holds a card's uncovered part to
/// the same square. The desktop layout has no such limits.
#[must_use]
pub fn phone_sizes(frame: &Frame, context: &Context) -> Vec<Fault> {
    if context.layout != Layout::Phone {
        return Vec::new();
    }
    let side = audit::PHONE_TARGET_POINTS;
    let small_targets = interactive(frame).filter_map(|index| {
        let element = &frame.resolved[index as usize];
        let rect = bounds(element);
        (element.place == Place::Layer
            && element.role != Role::Card
            && (rect.width < side || rect.height < side))
            .then(|| {
                let subject = subject(frame, index);
                fault(
                    Rule::PhoneSizes,
                    context,
                    format!(
                        "{} is smaller than {side} by {side} points",
                        subject.describe()
                    ),
                    vec![subject],
                )
            })
    });
    let small_text = (frame.lines.iter())
        .filter(|line| line.size < audit::PHONE_TEXT_POINTS)
        .map(|line| {
            let subject = subject(frame, line.element);
            fault(
                Rule::PhoneSizes,
                context,
                format!(
                    "a line of {}'s text is {} points, under {}",
                    subject.describe(),
                    line.size,
                    audit::PHONE_TEXT_POINTS
                ),
                vec![subject],
            )
        });
    small_targets.chain(small_text).collect()
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

/// Clips fit: the frame asked for no more clip rectangles than `Frame::clips` holds. A clip
/// asked for past the last entry is dropped, and what it should have cut off is drawn whole.
#[must_use]
pub fn clips(frame: &Frame, context: &Context) -> Vec<Fault> {
    if !frame.clips_full {
        return Vec::new();
    }
    vec![fault(
        Rule::Clips,
        context,
        format!(
            "the frame asked for more than {} clip rectangles",
            crate::frame::CLIPS - 1
        ),
        Vec::new(),
    )]
}

/// An arrow as one frame drew it, kept for "lines hold" to compare with the next frame's, since
/// a `Frame` is reused for the next and cannot be cloned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeptArrow {
    pub id: u32,
    /// The control points of its curve on the screen (`Role::Arrow`).
    pub corners: [Vec2; 4],
    pub moving: bool,
}

/// Replaces `kept` with the arrows of `frame`. Allocates only when `frame` has more arrows than
/// `kept` has ever held.
pub fn keep_arrows(frame: &Frame, kept: &mut Vec<KeptArrow>) {
    kept.clear();
    kept.extend(
        frame
            .resolved
            .iter()
            .filter(|element| element.role == Role::Arrow)
            .map(|element| KeptArrow {
                id: element.id.0,
                corners: element.corners,
                moving: element.state.has(StateBits::MOVING),
            }),
    );
}

/// How many arrows of `after` the frame before drew too, which "lines hold" compares.
#[must_use]
pub fn arrows_held(before: &[KeptArrow], after: &Frame) -> usize {
    after
        .resolved
        .iter()
        .filter(|element| {
            element.role == Role::Arrow && before.iter().any(|was| was.id == element.id.0)
        })
        .count()
}

/// Lines hold: an arrow whose two ends are at rest does not change shape. An arrow is matched
/// across two consecutive frames by its id, `before` holding the earlier frame's arrows
/// (`keep_arrows`). Its ends are at rest when neither of its end points moved more than
/// `LINE_END_POINTS` and it is moving in neither frame, and it changed shape when either control
/// point moved `LINE_RESHAPE_POINTS` or more. `context` is the later frame's.
#[must_use]
pub fn lines_hold(before: &[KeptArrow], after: &Frame, context: &Context) -> Vec<Fault> {
    let mut faults = Vec::new();
    for (index, now) in after.resolved.iter().enumerate() {
        if now.role != Role::Arrow {
            continue;
        }
        let Some(was) = before.iter().find(|was| was.id == now.id.0) else {
            continue;
        };
        if was.moving || now.state.has(StateBits::MOVING) {
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
