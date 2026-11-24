//! Screen boxes that present table objects. They are placed before the table is drawn.

use std::collections::BTreeSet;

use glam::Vec2;
use kit_base::geometry::plane::Zoom;
use kit_frame::frame::{ScreenClip, ShowPlacement};

use super::write::above_floats;
use super::{Layer, Node};
use crate::element::{Anchor, Display, Element};

/// A host must be placeable before the table. In particular, a card cannot be shown by
/// a descendant of a node whose position follows that card.
pub(super) fn validate_screen_shows(screen: &Element) -> usize {
    fn visit(element: &Element, independent: bool, objects: &mut BTreeSet<u32>) {
        if element.style.display == Display::None {
            return;
        }
        let independent = independent
            && !element.waits
            && matches!(element.anchor, None | Some(Anchor::Point(..)));
        if let Some(show) = element.show {
            assert!(independent, "a Show host must be laid out before the table");
            let id = show.object_id();
            assert!(
                objects.insert(id.0),
                "table object {id:?} has multiple Show hosts"
            );
        }
        for child in &element.children {
            visit(child, independent, objects);
        }
    }

    let mut objects = BTreeSet::new();
    visit(screen, true, &mut objects);
    objects.len()
}

pub(super) fn validate_float(element: &Element) {
    assert!(
        element.show.is_none(),
        "a Show host must be part of the screen"
    );
    for child in &element.children {
        validate_float(child);
    }
}

impl Layer {
    /// Visits the active screen's Show placements. A host outside its clip still owns the card.
    /// Uses the same transform and clip calculations as the frame writer.
    pub fn visit_shows(&self, mut visit: impl FnMut(ShowPlacement)) {
        if self.screen_shows == 0 {
            return;
        }
        let Some(root) = &self.screen else {
            return;
        };
        let origin = if root.element.anchor.is_some() {
            root.corner()
        } else {
            Some(Vec2::ZERO)
        };
        if let Some(origin) = origin {
            self.shows_in(root, true, origin, Zoom::IDENTITY, None, &mut visit);
        }
    }

    fn shows_in(
        &self,
        node: &Node,
        root: bool,
        origin: Vec2,
        zoom: Zoom,
        clip: Option<ScreenClip>,
        visit: &mut impl FnMut(ShowPlacement),
    ) {
        if node.element.style.display == Display::None {
            return;
        }
        let geometry = self.geometry(node, origin, zoom, self.state(node));
        let masked = geometry.mask(node.mask(), clip);
        if let Some(show) = node.element.show {
            visit(ShowPlacement {
                object: show.object_id(),
                bounds: geometry.painted,
                turn: show.turn(),
                clip: masked.or(clip),
            });
        }
        let inside = geometry.inside(node);
        let inside_clip = geometry.inside_clip(node, masked, clip);
        for child in &node.children {
            match child.element.anchor {
                None => self.shows_in(child, false, inside, geometry.zoom, inside_clip, visit),
                Some(Anchor::Point(..)) => {
                    let zoom = if root && above_floats(child) {
                        Zoom::IDENTITY
                    } else {
                        geometry.zoom
                    };
                    self.shows_in(
                        child,
                        false,
                        child.corner().expect("a point anchor has a corner"),
                        zoom,
                        None,
                        visit,
                    );
                }
                _ => {}
            }
        }
    }
}
