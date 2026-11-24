//! Retained layout provenance read on demand by the inspection driver.

use super::{Layer, Node};
use crate::element::{Element, FloatId};
use kit_frame::frame::ElementId;

/// The original element, its layout parent and transitions still in progress.
pub struct NodeStyle<'a> {
    pub element: &'a Element,
    pub parent: Option<&'a Element>,
    pub owner: Option<&'static str>,
    pub transitions: Vec<&'static str>,
    pub float: Option<FloatId>,
}

impl Layer {
    /// Reads the retained node that wrote a resolved element.
    #[must_use]
    pub fn node_style(&self, id: ElementId) -> Option<NodeStyle<'_>> {
        self.screen
            .iter()
            .chain(&self.departing)
            .find_map(|node| visit(node, id, None, None))
            .or_else(|| {
                self.floats.iter().find_map(|float| {
                    let mut node = visit(&float.node, id, None, None)?;
                    node.float = Some(float.id);
                    Some(node)
                })
            })
    }
}

fn visit<'a>(
    node: &'a Node,
    id: ElementId,
    parent: Option<&'a Element>,
    owner: Option<&'static str>,
) -> Option<NodeStyle<'a>> {
    let element = &node.element;
    let owner = element.probe_owner.or(owner);
    if id == node.id {
        return Some(NodeStyle {
            element,
            parent,
            owner,
            float: None,
            transitions: node.tracks.active_properties().collect(),
        });
    }
    node.children
        .iter()
        .chain(&node.departing)
        .find_map(|child| visit(child, id, Some(element), owner))
}
