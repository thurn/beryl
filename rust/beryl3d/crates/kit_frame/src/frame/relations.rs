//! Relations that screen elements can declare for the scene tree.

use super::{ElementId, FrameBuilder, Role};
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
