//! Relations that screen elements can declare for the scene tree.

use super::{ElementId, FrameBuilder, Place, Resolved, Role};
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
    /// Optional window bounds that the placed box is clamped inside.
    pub clamp: Option<Rect>,
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
            clamp: None,
        }
    }

    /// Clamps the placed box inside `bounds` so it does not reach past the window's edges.
    #[must_use]
    pub const fn clamped(mut self, bounds: Rect) -> Self {
        self.clamp = Some(bounds);
        self
    }

    /// The top left corner of a box of `size` placed against `target`.
    #[must_use]
    pub fn corner(&self, target: Rect, size: Vec2) -> Vec2 {
        let mut corner = Vec2::new(target.x, target.y)
            + self.at * Vec2::new(target.width, target.height)
            + self.offset
            - self.align * size;
        if let Some(bounds) = self.clamp {
            let max_x = (bounds.x + bounds.width - size.x).max(bounds.x);
            let max_y = (bounds.y + bounds.height - size.y).max(bounds.y);
            corner.x = corner.x.clamp(bounds.x, max_x);
            corner.y = corner.y.clamp(bounds.y, max_y);
        }
        corner
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

/// What an end of a line follows. A line aimed by the player can follow the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Endpoint {
    Node(ElementId),
    Pointer(Vec2),
}

/// The position of a line end relative to its target's box centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineEnd {
    pub target: Endpoint,
    pub offset: Vec2,
}

/// The geometry a decoration takes from other nodes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeAnchor {
    Outline(ElementId),
    Symbol(ElementId),
    Line([LineEnd; 2]),
}

/// One decoration and the relation declared by its owner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decoration {
    pub element: u32,
    pub anchor: ShapeAnchor,
}

/// A box that a line can use in either space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorBox {
    pub centre: Vec2,
    pub half: Vec2,
}

impl AnchorBox {
    /// The box that encloses the given projected corners.
    #[must_use]
    pub fn around(corners: [Vec2; 4]) -> Self {
        let low = corners.into_iter().reduce(Vec2::min).unwrap_or_default();
        let high = corners.into_iter().reduce(Vec2::max).unwrap_or_default();
        Self {
            centre: (low + high) / 2.0,
            half: (high - low) / 2.0,
        }
    }

    #[must_use]
    pub fn rect(self) -> Rect {
        let low = self.centre - self.half;
        Rect {
            x: low.x,
            y: low.y,
            width: self.half.x * 2.0,
            height: self.half.y * 2.0,
        }
    }
}

impl From<Rect> for AnchorBox {
    fn from(rect: Rect) -> Self {
        let half = Vec2::new(rect.width, rect.height) / 2.0;
        Self {
            centre: Vec2::new(rect.x, rect.y) + half,
            half,
        }
    }
}

/// Displays a table object in this node's box, cut to the node's clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Show {
    object: ElementId,
    turn: f32,
}

impl Show {
    /// Shows a table object with a stable id, including while the object is out of view.
    #[must_use]
    pub const fn object(id: ElementId) -> Self {
        Self {
            object: id,
            turn: 0.0,
        }
    }

    /// Turns the card clockwise by `turn` radians around the center of its box.
    #[must_use]
    pub const fn with_turn(self, turn: f32) -> Self {
        Self { turn, ..self }
    }

    /// The stable id of the table object.
    #[must_use]
    pub const fn object_id(self) -> ElementId {
        self.object
    }

    /// The clockwise screen rotation in radians.
    #[must_use]
    pub const fn turn(self) -> f32 {
        self.turn
    }
}

/// The frame-time geometry of a screen element that presents a table object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShowPlacement {
    pub object: ElementId,
    pub bounds: Rect,
    /// Clockwise rotation in radians around the center of `bounds`.
    pub turn: f32,
    pub clip: Option<super::ScreenClip>,
}

impl ShowPlacement {
    /// Validates ownership before changing the table's presentation.
    ///
    /// # Panics
    ///
    /// If two placements present the same object.
    pub fn assert_unique(placements: &[Self]) {
        for (index, placement) in placements.iter().enumerate() {
            assert!(
                !placements[..index]
                    .iter()
                    .any(|earlier| earlier.object == placement.object),
                "table object {:?} has multiple Show hosts",
                placement.object
            );
        }
    }
}

impl FrameBuilder<'_> {
    /// Records the Show relation declared by the screen element.
    ///
    /// # Panics
    ///
    /// If the host is not a registered screen element or already has a relation, or
    /// another host already presents the object.
    pub fn shows(&mut self, element: u32, object: ElementId) {
        assert_eq!(self.frame.resolved[element as usize].place, Place::Layer);
        assert!(
            self.frame
                .shows
                .last()
                .is_none_or(|&(host, _)| host < element),
            "Show hosts must be registered once, in scene order"
        );
        assert!(
            !self.frame.shows.iter().any(|&(_, shown)| shown == object),
            "table object {object:?} has multiple Show hosts"
        );
        self.frame.shows.push((element, object));
    }

    /// Registers the box of a layout node that has no draw records of its own.
    pub fn anchor_node(&mut self, id: ElementId, rect: Rect) {
        self.frame.anchor_nodes.push((id, rect));
    }

    /// The target of a relation, among the nodes registered so far.
    #[must_use]
    pub fn anchor_bounds(&self, target: &NodeRef) -> Option<Rect> {
        self.frame.anchor_bounds(target)
    }

    /// The resolved scene element and its index, among the nodes registered so far.
    #[must_use]
    pub fn anchor_element(&self, target: &NodeRef) -> Option<(u32, Resolved)> {
        self.frame.anchor_element(target)
    }

    /// Resolves the target of a line before its owner shapes the curve.
    #[must_use]
    pub fn endpoint(&self, target: Endpoint) -> Option<AnchorBox> {
        self.frame.endpoint(target)
    }

    /// Records a shape relation for the node just drawn.
    pub fn decorate(&mut self, element: u32, anchor: ShapeAnchor) {
        self.frame.decorations.push(Decoration { element, anchor });
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
        let corners = self.anchor_element(reference)?.1.corners;
        let low = corners.into_iter().reduce(Vec2::min)?;
        let high = corners.into_iter().reduce(Vec2::max)?;
        Some(Rect {
            x: low.x,
            y: low.y,
            width: high.x - low.x,
            height: high.y - low.y,
        })
    }

    /// Resolves a relation to a drawn element and its index in this frame.
    #[must_use]
    pub fn anchor_element(&self, reference: &NodeRef) -> Option<(u32, Resolved)> {
        let index = self.anchor_index(reference)?;
        Some((index, self.resolved[index as usize]))
    }

    /// Resolves an endpoint against the current nodes or the pointer it follows.
    #[must_use]
    pub fn endpoint(&self, target: Endpoint) -> Option<AnchorBox> {
        match target {
            Endpoint::Node(id) => self.anchor_bounds(&NodeRef::Id(id)).map(AnchorBox::from),
            Endpoint::Pointer(centre) => Some(AnchorBox {
                centre,
                half: Vec2::ZERO,
            }),
        }
    }

    /// The shape relation declared by the element at this frame index.
    #[must_use]
    pub fn decoration_of(&self, index: u32) -> Option<ShapeAnchor> {
        let at = self
            .decorations
            .binary_search_by_key(&index, |item| item.element)
            .ok()?;
        Some(self.decorations[at].anchor)
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
