//! The part of the "no overlap" and "text overlaps nothing" interface checks (docs/beryl-3d.md,
//! "Interface checks") that compares the flat layer with what lies under it. "No overlap"
//! compares interactive elements of the flat layer with each other, and "text overlaps nothing"
//! compares lines of text with those elements. Neither sees a button or a line of text drawn
//! over a pile on the table, or a standalone icon, such as a phase indicator, drawn over a card
//! or over a line of text, because none of these is an interactive element of the flat layer.

use std::collections::BTreeSet;

use glam::Vec2;

use super::checks::{fault, subject};
use super::fault::{Context, Fault, Rule};
use kit_frame::frame::{Frame, Place, Role, StateBits};
use kit_scene::hit::{intersection, is_within, painted_over, visible_bounds};
use kit_ui::input::Rect;

/// How far two boxes may reach into each other and not count as overlapping, in logical
/// points, as in "text overlaps nothing": a line's box is as tall as its line, leading and all.
const SLACK: f32 = 1.0;

/// How the first element of a pair meets the second, which names the rule a fault breaks.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum How {
    /// A control or an icon covers a pile, a card or a control: "no overlap".
    Covers,
    /// A line of the first element's text covers a pile: "text overlaps nothing".
    TextCovers,
    /// An icon covers a line of the second element's text: "text overlaps nothing".
    OverText,
}

/// What a flat-layer element is, for this check.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// An interactive element that is not a card, such as a button.
    Control,
    /// A line of the element's text.
    Line,
    /// An icon drawn on a panel of its own, such as a disc, that stands alone, not inside an
    /// interactive element. A bare glyph lets most of what lies under it show between its
    /// strokes, so it is left out.
    Icon,
}

/// Covers nothing: no interactive control, line of text or standalone icon on a panel of its
/// own (`Kind::Icon`) of the flat layer covers a pile on the table, and no such icon meets a
/// card at rest, a control or a line of another element's text. A pile is the stack of cards a library or graveyard is drawn as:
/// a control or a line of text over it hides how many cards it holds and the card on top, and a
/// player expects a touch there to open it. A standalone icon passes the pointer, so nothing
/// else notices it, but it hides what is under it, or is hidden by a control drawn over it.
/// Cards other than piles are left out for controls and text, as they are of "no overlap",
/// because the flat layer may lie over a hand or a row on purpose. A pair is not judged when one
/// element holds the other, when either is no more opaque than `context.audit.transparent`, or
/// when a third panel drawn over both covers the middle of the part where they meet, as a sheet
/// covers the table (`hidden`). One fault names each pair once.
#[must_use]
pub fn covers(frame: &Frame, context: &Context) -> Vec<Fault> {
    let seen = |index: u32| frame.resolved[index as usize].opacity > context.audit.transparent;
    let lines = lines(frame, &seen);
    let mut over = flat(frame, &seen);
    over.extend(lines.iter().map(|&(index, rect)| (index, rect, Kind::Line)));
    let under = table(frame, &seen);
    let controls: Vec<(u32, Rect)> = over
        .iter()
        .filter(|&&(_, _, kind)| kind == Kind::Control)
        .map(|&(index, rect, _)| (index, rect))
        .collect();
    let mut pairs = BTreeSet::new();
    for &(first, a, kind) in &over {
        let icon = kind == Kind::Icon;
        // Each element `first` is compared with, whether the pair is a covering (`Overlap`)
        // rather than text over an icon, and which of the two is drawn over the other.
        let piles = under
            .iter()
            .filter(|&&(_, _, pile)| pile || icon)
            .map(|&(index, rect, _)| (index, rect, true, first));
        let texts = lines
            .iter()
            .filter(|_| icon)
            .map(|&(index, rect)| (index, rect, false, first));
        let pressed = controls
            .iter()
            .filter(|_| icon)
            .map(|&(index, rect)| (index, rect, true, index));
        for (second, b, covering, top) in piles.chain(texts).chain(pressed) {
            if first == second || is_within(frame, first, second) || is_within(frame, second, first)
            {
                continue;
            }
            let Some(shared) = intersection(a, b) else {
                continue;
            };
            // What a control or an icon covers is hidden by it, so only a third panel drawn
            // over both excuses the pair. An opaque line of text over an icon hides the icon.
            if hidden(frame, (first, second), shared, covering.then_some(top)) {
                continue;
            }
            let how = match (kind, covering) {
                (Kind::Line, _) => How::TextCovers,
                (_, true) => How::Covers,
                (_, false) => How::OverText,
            };
            pairs.insert((first, second, how));
        }
    }
    pairs
        .into_iter()
        .map(|(first, second, how)| {
            let (a, b) = (subject(frame, first), subject(frame, second));
            let (rule, message) = match how {
                How::Covers => (
                    Rule::Overlap,
                    format!("{} covers {}", a.describe(), b.describe()),
                ),
                How::TextCovers => (
                    Rule::TextOverlap,
                    format!("text of {} overlaps {}", a.describe(), b.describe()),
                ),
                How::OverText => (
                    Rule::TextOverlap,
                    format!("{} overlaps text of {}", a.describe(), b.describe()),
                ),
            };
            fault(rule, context, message, vec![a, b])
        })
        .collect()
}

/// The controls and standalone icons of the flat layer that are `seen`, each with its visible
/// box and what it is.
fn flat(frame: &Frame, seen: &impl Fn(u32) -> bool) -> Vec<(u32, Rect, Kind)> {
    let mut over = Vec::new();
    for (index, element) in frame.resolved.iter().enumerate() {
        let index = index as u32;
        if element.place != Place::Layer || !seen(index) {
            continue;
        }
        let kind = if element.interactive && element.role != Role::Card {
            Kind::Control
        } else if element.role == Role::Icon
            && element.state.has(StateBits::PAINTED)
            && !inside_interactive(frame, index)
        {
            Kind::Icon
        } else {
            continue;
        };
        if let Some(rect) = visible_bounds(element).and_then(shrunk) {
            over.push((index, rect, kind));
        }
    }
    over
}

/// Each line of text whose element is `seen`, by the part its element's clip shows.
fn lines(frame: &Frame, seen: &impl Fn(u32) -> bool) -> Vec<(u32, Rect)> {
    frame
        .lines
        .iter()
        .filter(|line| seen(line.element))
        .filter_map(|line| {
            let element = &frame.resolved[line.element as usize];
            let rect = match element.clip {
                Some(clip) => intersection(line.rect, clip)?,
                None => line.rect,
            };
            Some((line.element, shrunk(rect)?))
        })
        .collect()
}

/// The piles and cards on the table that are `seen` and at rest, each with its visible box and
/// whether it is a pile. A card that moves, is held or is raised to be read is not at rest.
fn table(frame: &Frame, seen: &impl Fn(u32) -> bool) -> Vec<(u32, Rect, bool)> {
    let still = |index: u32| {
        let state = frame.resolved[index as usize].state;
        !state.has(StateBits::MOVING)
            && !state.has(StateBits::HELD)
            && !state.has(StateBits::HOVERED)
    };
    frame
        .resolved
        .iter()
        .enumerate()
        .filter_map(|(index, element)| {
            let index = index as u32;
            let pile = element.role == Role::Pile;
            let card = element.role == Role::Card && element.place != Place::Layer;
            ((pile || card) && seen(index) && still(index))
                .then(|| Some((index, shrunk(visible_bounds(element)?)?, pile)))
                .flatten()
        })
        .collect()
}

/// Whether element `index` is an interactive element or lies inside one, as a button's glyph
/// lies inside the button.
fn inside_interactive(frame: &Frame, index: u32) -> bool {
    let mut at = index;
    while at != u32::MAX {
        let element = &frame.resolved[at as usize];
        if element.interactive {
            return true;
        }
        at = element.parent;
    }
    false
}

/// Whether a panel drawn after the earlier of the two elements of `pair` in the paint order
/// covers the middle of `shared`, the part where the two meet: a sheet over the table hides the
/// pile under it, a banner drawn across the table hides an icon under it, and an opaque label
/// drawn over an icon hides the icon, as "text overlaps nothing" allows. A panel that is `top`,
/// the element that covers the other, or lies inside it does not count, because that panel is
/// what covers the other element.
fn hidden(frame: &Frame, (first, second): (u32, u32), shared: Rect, top: Option<u32>) -> bool {
    let middle = Vec2::new(
        shared.x + shared.width / 2.0,
        shared.y + shared.height / 2.0,
    );
    let earlier = frame
        .paint_order()
        .find(|&index| index == first || index == second)
        .unwrap_or(first);
    painted_over(frame, earlier, middle)
        .is_some_and(|panel| top.is_none_or(|top| !is_within(frame, panel, top)))
}

/// `rect` less `SLACK` on each side, or `None` when nothing is left.
fn shrunk(rect: Rect) -> Option<Rect> {
    let (width, height) = (rect.width - 2.0 * SLACK, rect.height - 2.0 * SLACK);
    (width > 0.0 && height > 0.0).then_some(Rect {
        x: rect.x + SLACK,
        y: rect.y + SLACK,
        width,
        height,
    })
}
