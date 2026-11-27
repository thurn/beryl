//! The interface checks that read frames alone (docs/beryl-3d.md, "Interface checks"). Each is a
//! function from a frame, or two for "lines hold", to the faults it finds. They know nothing
//! about any card or screen. The thresholds are `data/client/audit.json`.

use std::collections::BTreeMap;

use glam::{Vec2, Vec4};

use super::fault::{Context, Fault, Rule, Subject};
use super::tree::locator_of;
use kit_frame::frame::{
    AnchorBox, Draw, Frame, Missing, NodeRef, Place, Ring, Role, ShapeAnchor, StateBits,
    outline_corners,
};
use kit_scene::hit::{
    Overlaps, bounds, contains, covering, has_free_rect, hit, intersection, is_within,
    painted_over, visible_bounds,
};
use kit_ui::input::{Rect, Size};

/// The most covering elements a "reachable" fault names.
const MOST_COVERING: usize = 6;

/// Every check that needs one frame, in the doc's order.
#[must_use]
pub fn check(frame: &Frame, context: &Context) -> Vec<Fault> {
    let mut faults = on_screen(frame, context);
    faults.extend(overlap(frame, context));
    faults.extend(super::card_marks::card_marks(frame, context));
    faults.extend(reachable(frame, context));
    faults.extend(duplicates(frame, context));
    faults.extend(text_fits(frame, context));
    faults.extend(super::text_overlap::text_overlap(frame, context));
    faults.extend(super::covers::covers(frame, context));
    faults.extend(drawn(frame, context));
    faults.extend(named(frame, context));
    faults.extend(clips(frame, context));
    faults.extend(layer_drawn(frame, context));
    faults.extend(relations_hold(frame, context));
    faults.extend(minimum_sizes(frame, context));
    faults
}

/// Checks that each selected keep-clear slot respects its declared resting obstacles. The
/// obstacles are the boxes that layout read, so a card passing through a slot is allowed.
#[must_use]
pub fn keep_clear(frame: &Frame, context: &Context) -> Vec<Fault> {
    frame
        .clearances
        .iter()
        .flat_map(|placement| {
            placement.avoided.iter().filter_map(move |&other| {
                intersection(placement.rect, other)?;
                let control = Subject {
                    locator: placement.name.to_owned(),
                    rect: placement.rect,
                };
                let covered = Subject {
                    locator: "resting node".to_owned(),
                    rect: other,
                };
                Some(fault(
                    Rule::RelationsHold,
                    context,
                    format!("{} covers {}", control.describe(), covered.describe()),
                    vec![control, covered],
                ))
            })
        })
        .collect()
}

/// Checks the drawn box of each anchored node against its target's current box.
#[must_use]
pub fn anchors_hold(frame: &Frame, context: &Context) -> Vec<Fault> {
    frame
        .anchors
        .iter()
        .filter_map(|placement| {
            let element = &frame.resolved[placement.element as usize];
            let rect = bounds(element);
            let actual = Vec2::new(rect.x, rect.y);
            let target = frame.anchor_bounds(&placement.anchor.to);
            let expected = target.map(|target| placement.corner(target));
            if expected.is_some_and(|corner| {
                (actual - corner).abs().max_element() <= context.audit.anchor_points
            }) {
                return None;
            }
            let detail = expected.map_or_else(
                || "has no anchor target".to_owned(),
                |corner| format!("should start at ({:.2}, {:.2})", corner.x, corner.y),
            );
            let subject = Subject {
                locator: locator_of(frame, placement.element),
                rect,
            };
            Some(fault(
                Rule::RelationsHold,
                context,
                format!("{} {detail}", subject.describe()),
                vec![subject],
            ))
        })
        .collect()
}

/// On screen: every interactive element lies inside the window's safe area. A card needs only a
/// square with the profile's target size on a side, or as much of it
/// as a card turned edge-on has, inside it, because a hand is held partly past the bottom edge,
/// as Arena's and the web interface's are. A card the pointer holds is not at rest, and goes
/// where the pointer takes it. An element a scrolling list cuts is judged by the part of it the
/// list shows, since only that part can be seen or pressed: a row half scrolled past a list
/// that ends inside the safe area is on screen. A hovered card is raised to be read, so its top
/// and sides must lie inside the safe area, though its foot may pass the bottom edge as the
/// hand's does: a card raised behind a laptop's camera notch cannot be read.
#[must_use]
pub fn on_screen(frame: &Frame, context: &Context) -> Vec<Fault> {
    let side = context.checks.target_points;
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
            let rect = visible_bounds(element).unwrap_or_else(|| bounds(element));
            if element.state.has(StateBits::HELD) {
                false
            } else if element.role == Role::Card && element.state.has(StateBits::HOVERED) {
                // Half a point of slack for rounding where a card meets the edge exactly.
                rect.y < safe.y - 0.5
                    || rect.x < safe.x - 0.5
                    || rect.x + rect.width > safe.x + safe.width + 0.5
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
/// No overlap: no two interactive elements of the flat layer intersect where they are seen
/// (each box cut to its clip), unless one contains the other in the tree. Cards are exempt, because rows and hands overlap them on purpose.
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
            // Only the parts a scrolling list shows can overlap: a row scrolled half under the
            // list's edge is hidden past it.
            let (Some(a), Some(b)) = (
                visible_bounds(&frame.resolved[first as usize]),
                visible_bounds(&frame.resolved[second as usize]),
            ) else {
                continue;
            };
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
/// (its box cut to its clip) returns that element (or something inside it), and no later
/// element of the flat layer is drawn on a panel over that centre, which a press would go
/// through but which would cover the element's look (`hit::painted_over`). Every interactive card at rest has an uncovered area at
/// least the profile's target size square where a hit test
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
    let side = context.checks.target_points;
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
                let over: Vec<String> = covering(frame, overlaps, index)
                    .into_iter()
                    .take(MOST_COVERING)
                    .map(|other| self::subject(frame, other).describe())
                    .collect();
                faults.push(fault(
                    Rule::Reachable,
                    context,
                    format!(
                        "{} has no uncovered area {side} by {side} points where a press reaches \
                         it; under {}",
                        subject.describe(),
                        if over.is_empty() {
                            "nothing".to_owned()
                        } else {
                            over.join(", ")
                        }
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
            if let Some(over) = painted_over(frame, index, center) {
                let subjects = vec![subject(frame, index), subject(frame, over)];
                let message = format!(
                    "{} is drawn under {}, which a press goes through",
                    subjects[0].describe(),
                    subjects[1].describe()
                );
                faults.push(fault(Rule::Reachable, context, message, subjects));
            }
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

/// Checks for reused IDs and flat-layer siblings with the same role and name. IDs of framed
/// surfaces have their own namespace. Line and outline siblings may share names.
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
        if element.place != Place::Layer
            || matches!(element.role, Role::Arrow | Role::Mark)
            || name.is_empty()
        {
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

/// Drawn: every glyph of every element's text, and every icon, has an atlas cell at its own key
/// (`Frame::undrawn`). A line of text whose glyphs the host never delivered, or delivered only
/// at another size, is laid out in full, so the tree lists its string and "text fits" passes it,
/// while the window shows letters missing or blurred; an icon with no cell shows nothing. It
/// judges the frame as the host left it, so it is meant for a host that has delivered every
/// glyph and icon the client asked for before the frame is checked, as the test driver's stub
/// atlas does (`driver::glyphs`).
#[must_use]
pub fn drawn(frame: &Frame, context: &Context) -> Vec<Fault> {
    (frame.undrawn.iter())
        .map(|undrawn| {
            let subject = subject(frame, undrawn.element);
            let what = match undrawn.missing {
                Missing::Glyphs(count) => format!(
                    "{count} glyph{} of {}'s text {} no atlas cell at {} own size",
                    if count == 1 { "" } else { "s" },
                    subject.describe(),
                    if count == 1 { "has" } else { "have" },
                    if count == 1 { "its" } else { "their" },
                ),
                Missing::Icon { icon, size } => format!(
                    "{}'s icon {icon} has no atlas cell at {size} points, so it is not drawn",
                    subject.describe()
                ),
            };
            fault(Rule::Drawn, context, what, vec![subject])
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

/// In a profile with a text minimum, check the flat layer's target and text sizes as the web
/// interface's `phoneFaults` does. Cards are checked by "reachable", which holds a card's
/// uncovered part to the same square. An element is measured from its
/// corners on the window, so one laid out at a fraction of a point measures a little less than
/// its size, as 295.3 - 251.3 is 43.999985 in `f32`; a shortfall under `SIZE_TOLERANCE` passes.
#[must_use]
pub fn minimum_sizes(frame: &Frame, context: &Context) -> Vec<Fault> {
    let Some(text_points) = context.checks.text_points else {
        return Vec::new();
    };
    let side = context.checks.target_points;
    let least = side - SIZE_TOLERANCE;
    let small_targets = interactive(frame).filter_map(|index| {
        let element = &frame.resolved[index as usize];
        let rect = bounds(element);
        (element.place == Place::Layer
            && element.role != Role::Card
            && (rect.width < least || rect.height < least))
            .then(|| {
                let subject = subject(frame, index);
                fault(
                    Rule::MinimumSizes,
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
        .filter(|line| line.size < text_points)
        .map(|line| {
            let subject = subject(frame, line.element);
            fault(
                Rule::MinimumSizes,
                context,
                format!(
                    "a line of {}'s text is {} points, under {}",
                    subject.describe(),
                    line.size,
                    text_points
                ),
                vec![subject],
            )
        });
    small_targets.chain(small_text).collect()
}

/// How much less than a size, in logical points, an element may measure and still have it:
/// far below a pixel on any screen, and far above `f32`'s error at a window's coordinates.
const SIZE_TOLERANCE: f32 = 0.01;

/// Layer drawn: a frame whose flat layer has records draws at least one of them inside the
/// window. Each record is placed for the table's camera (`Camera::layer_transform`) and the
/// renderer projects it with `Frame::view_projection`, so a frame that was not given that
/// camera draws its whole layer off screen and shows a black window.
#[must_use]
pub fn layer_drawn(frame: &Frame, context: &Context) -> Vec<Fault> {
    let layer = frame.draws.get(frame.layer as usize..).unwrap_or_default();
    let projection = frame.view_projection;
    if layer.is_empty() || layer.iter().any(|draw| in_window(draw, projection)) {
        return Vec::new();
    }
    vec![fault(
        Rule::LayerDrawn,
        context,
        format!(
            "none of the flat layer's {} records is drawn inside the window",
            layer.len()
        ),
        Vec::new(),
    )]
}

/// Checks placement, shape and clearance relations against the frame's current nodes.
#[must_use]
pub fn relations_hold(frame: &Frame, context: &Context) -> Vec<Fault> {
    let mut faults = keep_clear(frame, context);
    faults.extend(anchors_hold(frame, context));
    for placement in &frame.decorations {
        let node = &frame.resolved[placement.element as usize];
        let problem = match placement.anchor {
            ShapeAnchor::Outline(id) => {
                let target = frame.anchor_bounds(&NodeRef::Id(id));
                let ring = frame
                    .rings
                    .iter()
                    .find(|ring| ring.element == placement.element);
                match (target, ring) {
                    (Some(target), Some(ring)) => {
                        let drawn = ring_rect(ring, frame.view_projection, context.size);
                        outline_problem(drawn, target, context.audit.mark_points)
                    }
                    (None, _) => Some("has no outline target".to_owned()),
                    (Some(target), None) => {
                        outline_problem(bounds(node), target, context.audit.mark_points)
                    }
                }
            }
            ShapeAnchor::Symbol(id) => frame.anchor_bounds(&NodeRef::Id(id)).map_or_else(
                || Some("has no symbol target".to_owned()),
                |target| {
                    (rect_centre(bounds(node)).distance(rect_centre(target))
                        > context.audit.mark_points)
                        .then(|| "is off the centre of its target".to_owned())
                },
            ),
            ShapeAnchor::Line(ends) => [node.corners[0], node.corners[3]]
                .into_iter()
                .zip(ends)
                .enumerate()
                .find_map(|(end, (actual, relation))| {
                    let target = frame.endpoint(relation.target);
                    let holds = target.is_some_and(|target| {
                        actual.distance(target.centre + relation.offset)
                            <= context.audit.anchor_points
                    });
                    (!holds).then(|| {
                        format!(
                            "has a detached {} end",
                            if end == 0 { "first" } else { "second" }
                        )
                    })
                }),
        };
        if let Some(problem) = problem {
            let subject = subject(frame, placement.element);
            faults.push(fault(
                Rule::RelationsHold,
                context,
                format!("{} {problem}", subject.describe()),
                vec![subject],
            ));
        }
    }
    faults
}

/// An outline may grow around its target. Its centre and enclosed box still have to follow it.
fn outline_problem(line: Rect, target: Rect, tolerance: f32) -> Option<String> {
    let offset = rect_centre(line) - rect_centre(target);
    if offset.length() > tolerance {
        return Some(format!(
            "is centred {:.1},{:.1} points from its target",
            offset.x, offset.y
        ));
    }
    let around = Rect {
        x: line.x - tolerance,
        y: line.y - tolerance,
        width: line.width + 2.0 * tolerance,
        height: line.height + 2.0 * tolerance,
    };
    (!contains(around, target)).then(|| "does not go round its target".to_owned())
}

/// The rectangle on the window, in logical points, that bounds the line `ring`'s outline is
/// drawn along, through `projection`, in a window of `size`.
#[must_use]
pub fn ring_rect(ring: &Ring, projection: glam::Mat4, size: Size) -> Rect {
    ring_reach(ring, projection, size, ring.line)
}

/// The rectangle on the window, in logical points, outside which `ring`'s outline and its glow
/// draw nothing (`Ring::outer`), through `projection`, in a window of `size`.
#[must_use]
pub fn ring_outer(ring: &Ring, projection: glam::Mat4, size: Size) -> Rect {
    ring_reach(ring, projection, size, ring.outer)
}

/// The rectangle on the window that bounds the rectangle `half` across and up from the middle
/// of `ring`'s quad, in the quad's own units, projected as the renderer draws it.
fn ring_reach(ring: &Ring, projection: glam::Mat4, size: Size, half: [f32; 2]) -> Rect {
    AnchorBox::around(outline_corners(ring.model, half, projection, size)).rect()
}

/// The centre of `rect`.
fn rect_centre(rect: Rect) -> Vec2 {
    Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// Whether the centre of `draw` projects inside the window through `projection`.
fn in_window(draw: &Draw, projection: glam::Mat4) -> bool {
    let m = draw.model;
    let clip = projection * Vec4::new(m[3], m[7], m[11], 1.0);
    clip.w > 0.0 && clip.x.abs() <= clip.w && clip.y.abs() <= clip.w
}

/// Clips fit: the frame asked for no more clip rectangles than `Frame::clips` holds, and no
/// more gradients than its paint table holds (`Paints`). A clip asked for past the last entry is
/// dropped, and what it should have cut off is drawn whole; a gradient past the last is not
/// drawn.
#[must_use]
pub fn clips(frame: &Frame, context: &Context) -> Vec<Fault> {
    let mut faults = Vec::new();
    if frame.clips_full {
        faults.push(fault(
            Rule::Clips,
            context,
            format!(
                "the frame asked for more than {} clip rectangles",
                kit_frame::frame::CLIPS - 1
            ),
            Vec::new(),
        ));
    }
    if frame.paints.full {
        faults.push(fault(
            Rule::Clips,
            context,
            format!(
                "the frame asked for more than {} gradients",
                kit_frame::frame::GRADIENTS
            ),
            Vec::new(),
        ));
    }
    faults
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
        let still =
            moved(0) <= context.audit.line_end_points && moved(3) <= context.audit.line_end_points;
        let reshaped = moved(1) >= context.audit.line_reshape_points
            || moved(2) >= context.audit.line_reshape_points;
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
pub(super) fn interactive(frame: &Frame) -> impl Iterator<Item = u32> + '_ {
    frame
        .resolved
        .iter()
        .enumerate()
        .filter(|(_, element)| element.interactive)
        .map(|(index, _)| index as u32)
}

pub(super) fn subject(frame: &Frame, index: u32) -> Subject {
    Subject {
        locator: locator_of(frame, index),
        rect: bounds(&frame.resolved[index as usize]),
    }
}

pub(super) fn fault(
    rule: Rule,
    context: &Context,
    message: String,
    elements: Vec<Subject>,
) -> Fault {
    Fault {
        rule,
        elements,
        at: context.at,
        message,
    }
}
