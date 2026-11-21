//! Relations that screen elements can declare for the scene tree.

use super::{ElementId, FrameBuilder, Place, Role};
use glam::Vec2;
use kit_base::geometry::Rect;

/// A reference that can be resolved again after a tree rebuild.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeRef {
    /// An id supplied by a node's owner, which must keep it across rebuilds.
    /// Legacy arrow locator ids are positions in their frame's list.
    Id(ElementId),
    /// An immediate child with this role and name, or a root if `parent` is absent.
    Named {
        parent: Option<Box<Self>>,
        role: Role,
        name: String,
    },
}

/// Places a node relative to another node's screen box.
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub to: NodeRef,
    /// The point of the target's box, as fractions from its top left.
    pub at: Vec2,
    /// The point of this node's box to align with `at`, as fractions from its top left.
    pub align: Vec2,
    /// An additional offset in logical points.
    pub offset: Vec2,
}

impl Anchor {
    /// Anchors to the stable id of a node.
    #[must_use]
    pub const fn new(id: ElementId, at: Vec2, align: Vec2, offset: Vec2) -> Self {
        Self {
            to: NodeRef::Id(id),
            at,
            align,
            offset,
        }
    }

    /// The top left corner of a box of `size` placed against `target`.
    #[must_use]
    pub fn corner(&self, target: Rect, size: Vec2) -> Vec2 {
        Vec2::new(target.x, target.y)
            + self.at * Vec2::new(target.width, target.height)
            + self.offset
            - self.align * size
    }
}

/// The anchor used to draw a node, with the size of its box before its paint scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Anchored {
    pub element: u32,
    pub anchor: Anchor,
    pub size: Vec2,
    pub scale: f32,
}

impl Anchored {
    /// The painted corner the relation requires, accounting for scaling about the box centre.
    #[must_use]
    pub fn corner(&self, target: Rect) -> Vec2 {
        self.anchor.corner(target, self.size) + self.size * ((1.0 - self.scale) / 2.0)
    }
}

/// Displays a table object in this node's box, cut to the node's clip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Show {
    pub object: NodeRef,
}

impl Show {
    /// Shows a table object with a stable id, including while the object is out of view.
    #[must_use]
    pub const fn object(id: ElementId) -> Self {
        Self {
            object: NodeRef::Id(id),
        }
    }

    /// The stable id of a table object shown by a screen element.
    #[must_use]
    pub const fn object_id(&self) -> Option<ElementId> {
        match &self.object {
            NodeRef::Id(id) => Some(*id),
            NodeRef::Named { .. } => None,
        }
    }
}

impl FrameBuilder<'_> {
    /// Registers the box of a layout node that has no draw records of its own.
    pub fn anchor_node(&mut self, id: ElementId, rect: Rect) {
        self.frame.anchor_nodes.push((id, rect));
    }

    /// The target of a relation, among the nodes registered so far.
    #[must_use]
    pub fn anchor_bounds(&self, target: &NodeRef) -> Option<Rect> {
        self.frame.anchor_bounds(target)
    }

    /// Records the placement relation of the node just drawn.
    pub fn anchored(&mut self, placement: Anchored) {
        self.frame.anchors.push(placement);
    }

    /// Finds or registers a plain scissor rectangle in physical pixels. Table objects and
    /// screen elements under the same clip share an entry. Masked clips own their entry.
    pub fn scissor(&mut self, rect: Rect) -> Option<u8> {
        (1..=self.clips)
            .find(|&index| {
                self.frame.clips[usize::from(index)] == rect && self.mask_of(index).is_none()
            })
            .or_else(|| self.clip(rect))
    }
}

impl super::Frame {
    /// The centre of the box of the table's element `id` (a card or an ability, not an element
    /// of the flat layer), on the window in logical points, if the frame holds it.
    #[must_use]
    pub fn centre(&self, id: ElementId) -> Option<Vec2> {
        let corners = self.corners(id)?;
        Some(corners.iter().copied().sum::<Vec2>() / 4.0)
    }

    /// The corners of the box of the table's element `id` (a card or an ability, not an
    /// element of the flat layer), on the window in logical points, if the frame holds it.
    #[must_use]
    pub fn corners(&self, id: ElementId) -> Option<[Vec2; 4]> {
        let element = (self.resolved.iter())
            .find(|element| element.id == id && element.place != Place::Layer)?;
        Some(element.corners)
    }

    /// Resolves an anchor target against this frame's geometry.
    #[must_use]
    pub fn anchor_bounds(&self, reference: &NodeRef) -> Option<Rect> {
        if let NodeRef::Id(id) = reference
            && let Some((_, rect)) = self.anchor_nodes.iter().find(|(node, _)| node == id)
        {
            return Some(*rect);
        }
        let corners = self.resolved[self.anchor_index(reference)? as usize].corners;
        let low = corners.into_iter().reduce(Vec2::min)?;
        let high = corners.into_iter().reduce(Vec2::max)?;
        Some(Rect {
            x: low.x,
            y: low.y,
            width: high.x - low.x,
            height: high.y - low.y,
        })
    }

    fn anchor_index(&self, reference: &NodeRef) -> Option<u32> {
        let mut matching = self.resolved.iter().enumerate();
        let index = match reference {
            NodeRef::Id(id) => matching.find(|(_, node)| node.id == *id)?.0,
            NodeRef::Named { parent, role, name } => {
                let parent = match parent {
                    Some(parent) => self.anchor_index(parent)?,
                    None => u32::MAX,
                };
                let mut matches = matching.filter(|(_, node)| {
                    node.parent == parent && node.role == *role && self.name(node.name) == name
                });
                let index = matches.next()?.0;
                if matches.next().is_some() {
                    return None;
                }
                index
            }
        };
        u32::try_from(index).ok()
    }

    /// The anchor declared by the element at this frame index.
    #[must_use]
    pub fn anchor_of(&self, index: u32) -> Option<&Anchor> {
        self.anchors
            .iter()
            .find(|item| item.element == index)
            .map(|item| &item.anchor)
    }

    /// The table object that the screen element `element` shows.
    #[must_use]
    pub fn shown_by(&self, element: u32) -> Option<ElementId> {
        let at = self
            .shows
            .binary_search_by_key(&element, |&(index, _)| index)
            .ok()?;
        Some(self.shows[at].1)
    }

    /// The screen element that presents a table object and has its own press action.
    #[must_use]
    pub fn show_host(&self, object: ElementId) -> Option<u32> {
        self.shows.iter().find_map(|&(index, shown)| {
            (shown == object && self.resolved[index as usize].interactive).then_some(index)
        })
    }
}

/// The tested portion of a placed screen slot and the resting boxes it must avoid.
#[derive(Clone, Debug, PartialEq)]
pub struct Clearance {
    pub name: &'static str,
    pub rect: Rect,
    pub avoided: Vec<Rect>,
}
