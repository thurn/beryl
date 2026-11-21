//! Scene nodes, their cross-space relations, and the order in which they are placed.

use std::borrow::Cow;

use glam::Vec2;
use kit_base::{geometry::Rect, pose::Pose};
use kit_frame::frame::{ElementId, Frame, Resolved, Role};

pub use kit_frame::frame::{NodeRef, PaintBand as Band, Show};
use kit_ui::layout::Layer;

use crate::hit::{bounds, intersection};

/// A node's geometry in either table or screen space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Space {
    Table {
        /// The pose in the table's coordinates, before projection through the camera.
        pose: Pose,
        /// The projected bounding box in logical points.
        bounds: Rect,
    },
    Screen {
        /// The laid-out box in logical points.
        bounds: Rect,
    },
}

impl Space {
    /// The box that either space occupies on screen, in logical points.
    #[must_use]
    pub const fn screen_bounds(self) -> Rect {
        match self {
            Self::Table { bounds, .. } | Self::Screen { bounds } => bounds,
        }
    }
}

/// A way to select nodes whose screen boxes a keep-clear placement must avoid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeQuery {
    Node(NodeRef),
    Children(NodeRef),
    Role(Role),
}

/// An identifier for a candidate placement supplied by screen layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRef(pub u32);

/// A candidate screen box and the portion tested against the avoided nodes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub bounds: Rect,
    pub clearance: Rect,
}

impl From<Rect> for Candidate {
    fn from(bounds: Rect) -> Self {
        Self {
            bounds,
            clearance: bounds,
        }
    }
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

/// Selects the first candidate slot that covers none of the queried nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeepClear {
    pub of: Vec<NodeQuery>,
    /// Candidate slots in descending order of preference.
    pub places: Vec<SlotRef>,
}

/// A named thing in the scene, with its space, paint band and placement relations.
#[derive(Clone, Debug, PartialEq)]
pub struct Node<'a> {
    pub id: ElementId,
    pub role: Role,
    pub name: Cow<'a, str>,
    /// The parent's index in the current tree. Use a `NodeRef` across rebuilds.
    pub parent: Option<u32>,
    pub space: Space,
    pub band: Band,
    pub anchor: Option<Anchor>,
    pub keep_clear: Option<KeepClear>,
    pub show: Option<Show>,
}

impl<'a> Node<'a> {
    /// A screen region with no parent or relations yet.
    #[must_use]
    pub fn screen(id: ElementId, name: &'a str, bounds: Rect) -> Self {
        Self {
            id,
            role: Role::Region,
            name: Cow::Borrowed(name),
            parent: None,
            space: Space::Screen { bounds },
            band: Band::ScreenPanels,
            anchor: None,
            keep_clear: None,
            show: None,
        }
    }

    /// Reads a node registered by the frame builder. The frame owns its name.
    #[must_use]
    fn from_resolved(frame: &'a Frame, index: u32, element: &Resolved) -> Self {
        let bounds = bounds(element);
        let space = match element.pose {
            Some(pose) => Space::Table { pose, bounds },
            None => Space::Screen { bounds },
        };
        Self {
            id: element.id,
            role: element.role,
            name: Cow::Borrowed(frame.name(element.name)),
            parent: (element.parent != u32::MAX).then_some(element.parent),
            space,
            band: Band::of(element),
            anchor: None,
            keep_clear: None,
            show: frame.shown_by(index).map(Show::object),
        }
    }
}

/// When the client solves the scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolvePhase {
    /// Layout in `handle` can allocate.
    Handle,
    /// Placement in `frame` allocates nothing.
    Frame,
}

/// One pass through the scene. Each pass can read the results of the passes before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolveStep {
    /// Arrange table containers and project the table nodes through the camera.
    Arrange,
    /// Lay out the screen with the projected table boxes available.
    Layout,
    /// Choose the first clear placement of each node with a keep-clear relation.
    KeepClear,
    /// Place anchored nodes from the current boxes of their targets.
    Anchor,
}

/// The scene's nodes in registration order, with parents before their children.
/// A declared tree owns its nodes. A finished frame supplies the same tree as
/// a view of the nodes already registered by the frame builder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree<Nodes = Vec<Node<'static>>> {
    nodes: Nodes,
    slots: Vec<Candidate>,
    selected: Vec<Option<SlotRef>>,
}

impl<'a> Tree<&'a Frame> {
    /// Views the registered nodes of a frame without copying their storage.
    #[must_use]
    pub const fn from_frame(frame: &'a Frame) -> Self {
        Self {
            nodes: frame,
            slots: Vec::new(),
            selected: Vec::new(),
        }
    }

    /// All registered nodes, in the same order as `Frame::resolved`.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = Node<'a>> + '_ {
        self.nodes
            .resolved
            .iter()
            .enumerate()
            .map(|(index, element)| Node::from_resolved(self.nodes, index as u32, element))
    }

    /// The node at this index in registration order.
    #[must_use]
    pub fn node(&self, index: u32) -> Option<Node<'a>> {
        let element = self.nodes.resolved.get(index as usize)?;
        Some(Node::from_resolved(self.nodes, index, element))
    }
}

impl<'a> Tree<Vec<Node<'a>>> {
    /// All nodes, in registration order.
    #[must_use]
    pub fn nodes(&self) -> &[Node<'a>] {
        &self.nodes
    }

    /// Registers a node and returns its index for its children to use.
    ///
    /// # Panics
    ///
    /// When its parent has yet to be registered or the index does not fit a `u32`.
    pub fn push(&mut self, node: Node<'a>) -> u32 {
        let index = u32::try_from(self.nodes.len()).expect("nodes fit u32 indexes");
        assert!(node.parent.is_none_or(|parent| parent < index));
        self.nodes.push(node);
        self.selected.push(None);
        index
    }

    /// Looks up a reference in the current tree. Missing and ambiguous references
    /// both resolve to `None`.
    #[must_use]
    pub fn resolve(&self, reference: &NodeRef) -> Option<u32> {
        match reference {
            NodeRef::Id(id) => self.unique(|node| node.id == *id),
            NodeRef::Named { parent, role, name } => {
                let parent = match parent {
                    Some(parent) => Some(self.resolve(parent)?),
                    None => None,
                };
                self.unique(|node| {
                    node.parent == parent && node.role == *role && node.name == *name
                })
            }
        }
    }

    fn unique(&self, matches: impl Fn(&Node<'a>) -> bool) -> Option<u32> {
        let mut found = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| matches(node));
        let (index, _) = found.next()?;
        if found.next().is_some() {
            return None;
        }
        u32::try_from(index).ok()
    }

    /// Runs the placement passes once, in dependency order. In `handle`, table
    /// arrangement precedes screen layout and the choice of keep-clear slots. In
    /// `frame`, only anchoring runs, from the boxes at the current animation time.
    /// The anchor pass and its callback must allocate nothing.
    pub fn solve(&mut self, phase: SolvePhase, mut run: impl FnMut(SolveStep, &mut Self)) {
        use SolveStep::{Anchor, Arrange, KeepClear, Layout};

        let steps: &[_] = match phase {
            SolvePhase::Handle => &[Arrange, Layout, KeepClear],
            SolvePhase::Frame => &[Anchor],
        };
        for &step in steps {
            run(step, self);
            if step == KeepClear {
                self.place_clear_nodes();
            }
        }
    }

    /// Registers a screen slot for a subsequent keep-clear pass.
    ///
    /// # Panics
    ///
    /// If the number of slots exceeds the range of `SlotRef`.
    pub fn add_slot(&mut self, candidate: Candidate) -> SlotRef {
        let index = u32::try_from(self.slots.len()).expect("slots fit u32 indexes");
        self.slots.push(candidate);
        SlotRef(index)
    }

    /// The first clear slot selected for a node, or none if every candidate was blocked.
    #[must_use]
    pub fn selected_slot(&self, index: u32) -> Option<SlotRef> {
        self.selected.get(index as usize).copied().flatten()
    }

    /// The boxes selected by a relation's queries, excluding the node itself.
    pub fn avoided(&self, index: u32, queries: &[NodeQuery]) -> impl Iterator<Item = Rect> + '_ {
        let matches: Vec<_> = queries
            .iter()
            .map(|query| match query {
                NodeQuery::Node(reference) => (self.resolve(reference), None, None),
                NodeQuery::Children(reference) => (None, self.resolve(reference), None),
                NodeQuery::Role(role) => (None, None, Some(*role)),
            })
            .collect();
        self.nodes
            .iter()
            .enumerate()
            .filter(move |&(at, node)| {
                at != index as usize
                    && matches.iter().any(|&(target, parent, role)| {
                        target.is_some_and(|target| target as usize == at)
                            || parent.is_some_and(|parent| node.parent == Some(parent))
                            || role == Some(node.role)
                    })
            })
            .map(|(_, node)| node.space.screen_bounds())
    }

    fn place_clear_nodes(&mut self) {
        for index in 0..self.nodes.len() {
            let Some(relation) = &self.nodes[index].keep_clear else {
                continue;
            };
            let avoided: Vec<_> = self
                .avoided(u32::try_from(index).expect("node index"), &relation.of)
                .collect();
            let choice = relation.places.iter().find_map(|&slot| {
                let candidate = self.slots.get(slot.0 as usize)?;
                avoided
                    .iter()
                    .all(|&other| intersection(candidate.clearance, other).is_none())
                    .then_some((slot, candidate.bounds))
            });
            self.selected[index] = choice.map(|(slot, _)| slot);
            if let Some((_, bounds)) = choice {
                self.nodes[index].space = Space::Screen { bounds };
            }
        }
    }

    /// Updates a node's geometry after arrangement, projection or placement.
    ///
    /// # Panics
    ///
    /// When the index is outside the current tree.
    pub fn set_space(&mut self, index: u32, space: Space) {
        self.nodes[index as usize].space = space;
    }
}

/// A Show relation resolved against the active screen layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShowPlacement {
    pub object: ElementId,
    pub bounds: Rect,
    pub clip: Option<Rect>,
}

impl Tree {
    /// Resolves the active screen's Show declarations after layout or scrolling. The object
    /// keeps its resting pose, even when the declaring element is currently outside its clip.
    pub fn screen_shows(layer: &Layer, placements: &mut Vec<ShowPlacement>) {
        placements.clear();
        layer.visit_shows(|show, bounds, clip| {
            if let Some(object) = show.object_id() {
                placements.push(ShowPlacement {
                    object,
                    bounds,
                    clip,
                });
            }
        });
    }
}
