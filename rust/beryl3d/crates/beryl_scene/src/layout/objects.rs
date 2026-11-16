//! The boxes of the screen's elements that show scene objects (`Shown::Object`), such as the
//! viewer's grid of a pile's cards, as the table needs them to place each object in its box
//! (`Table::show_in`).

use beryl_protocol::protocol::ObjectId;
use beryl_protocol::script::Subject;
use glam::Vec2;

use super::{Layer, Node, contains};
use crate::element::{Display, Overflow, Shown};
use crate::input::Rect;

impl Layer {
    /// Puts in `boxes` each card the screen shows in an element's box: its slot, the box on the
    /// window in logical points, scrolled as its scrolling ancestors are, and whether the box
    /// lies wholly inside every element that clips it, so that the card is drawn. A card
    /// drawn in 3D cannot be cut to a clip, so a box partly scrolled out shows nothing.
    pub fn shown_objects(&self, boxes: &mut Vec<(ObjectId, Rect, bool)>) {
        boxes.clear();
        if let Some(screen) = &self.screen {
            let origin = match screen.element.anchor {
                Some(_) => screen.corner().unwrap_or(Vec2::ZERO),
                None => Vec2::ZERO,
            };
            collect(screen, origin, None, boxes);
        }
    }
}

/// Adds the boxes under `node`, whose tree's top left corner stands at `origin`, inside `clip`.
fn collect(node: &Node, origin: Vec2, clip: Option<Rect>, boxes: &mut Vec<(ObjectId, Rect, bool)>) {
    if node.element.style.display == Display::None {
        return;
    }
    let at = origin + node.offset();
    let rect = Rect {
        x: node.rect.x + at.x,
        y: node.rect.y + at.y,
        ..node.rect
    };
    if let Some(Shown::Object(Subject::Object { slot })) = node.element.shows {
        let inside = clip.is_none_or(|clip| {
            let corners = [
                Vec2::new(rect.x + 0.5, rect.y + 0.5),
                Vec2::new(rect.x + rect.width - 0.5, rect.y + rect.height - 0.5),
            ];
            corners.iter().all(|&corner| contains(clip, corner))
        });
        boxes.push((slot, rect, inside));
    }
    let clip = match node.element.style.overflow {
        Overflow::Clip | Overflow::Scroll => Some(clip.map_or(rect, |clip| cut(clip, rect))),
        Overflow::Visible => clip,
    };
    // A node's own move or fade offsets it alone, not its children (`layout::motion`).
    let inside = origin - node.scroll;
    for child in &node.children {
        match child.element.anchor {
            Some(_) => {
                if let Some(corner) = child.corner() {
                    collect(child, corner, None, boxes);
                }
            }
            None => collect(child, inside, clip, boxes),
        }
    }
}

/// The part of `a` that `b` covers, empty where they do not meet.
fn cut(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Rect {
        x,
        y,
        width: ((a.x + a.width).min(b.x + b.width) - x).max(0.0),
        height: ((a.y + a.height).min(b.y + b.height) - y).max(0.0),
    }
}
