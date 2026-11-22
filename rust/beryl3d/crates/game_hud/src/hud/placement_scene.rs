//! Placement of phone controls against the projected resting boxes of the table.

use std::cell::RefCell;
use std::sync::Arc;

use glam::Vec2;
use kit_base::{geometry::Rect, pose::Pose};
use kit_frame::frame::{Clearance, ElementId, Role};
use kit_scene::hit::intersection;
use kit_scene::{Band, Candidate, KeepClear, Node, NodeQuery, NodeRef, SolvePhase, Space, Tree};

const YOUR_ROWS: ElementId = ElementId(u32::MAX);
const THEIR_ROWS: ElementId = ElementId(u32::MAX - 1);
const STACK: ElementId = ElementId(u32::MAX - 2);
const HAND: ElementId = ElementId(u32::MAX - 3);
const PHASE: ElementId = ElementId(u32::MAX - 4);

/// A group of table nodes whose occupied boxes can be queried by a control.
#[derive(Clone, Copy)]
pub enum Group {
    YourRows,
    TheirRows,
    Stack,
    Hand,
}

impl Group {
    fn id(self) -> ElementId {
        match self {
            Self::YourRows => YOUR_ROWS,
            Self::TheirRows => THEIR_ROWS,
            Self::Stack => STACK,
            Self::Hand => HAND,
        }
    }

    #[must_use]
    pub fn children(self) -> NodeQuery {
        NodeQuery::Children(NodeRef::Id(self.id()))
    }
}

/// One slot tried by a keep-clear control.
#[derive(Clone, Debug)]
pub struct Attempt {
    pub control: &'static str,
    pub slot: &'static str,
    pub bounds: Rect,
    pub selected: bool,
    pub blocked: Vec<Obstacle>,
}

/// A scene node that the slot would cover.
#[derive(Clone, Debug)]
pub struct Obstacle {
    pub id: ElementId,
    pub group: &'static str,
    pub bounds: Rect,
}

/// The phone's arranged table and the screen-space relations solved as its HUD is built.
#[derive(Debug)]
pub struct PlacementScene {
    tree: RefCell<Tree>,
    clearances: RefCell<Vec<Clearance>>,
    attempts: Option<RefCell<Vec<Attempt>>>,
}

impl Default for PlacementScene {
    fn default() -> Self {
        let mut tree = Tree::default();
        for (id, name) in [
            (YOUR_ROWS, "viewer rows"),
            (THEIR_ROWS, "opponent rows"),
            (STACK, "stack"),
            (HAND, "viewer hand"),
        ] {
            tree.push(Node::screen(id, name, Rect::default()));
        }
        Self {
            tree: RefCell::new(tree),
            clearances: RefCell::default(),
            attempts: None,
        }
    }
}

impl PlacementScene {
    /// Registers an object at its laid-out pose, with the bounds projected by the table camera.
    pub fn object(
        &mut self,
        id: ElementId,
        role: Role,
        group: Group,
        pose: Pose,
        corners: [Vec2; 4],
    ) {
        let low = corners.into_iter().reduce(Vec2::min).unwrap_or_default();
        let high = corners.into_iter().reduce(Vec2::max).unwrap_or_default();
        let tree = self.tree.get_mut();
        let mut node = Node::screen(id, "table object", Rect::default());
        node.parent = tree.resolve(&NodeRef::Id(group.id()));
        node.role = role;
        node.space = Space::Table {
            pose,
            bounds: rect(low.x, low.y, high.x - low.x, high.y - low.y),
        };
        node.band = Band::Table;
        tree.push(node);
    }

    /// Registers the phase icon where the HUD displays it.
    pub fn phase(&mut self, bounds: Rect) {
        self.tree
            .get_mut()
            .push(Node::screen(PHASE, "phase", bounds));
    }

    #[must_use]
    pub fn phase_query() -> NodeQuery {
        NodeQuery::Node(NodeRef::Id(PHASE))
    }

    #[must_use]
    pub fn bounds(&self, id: ElementId) -> Option<Rect> {
        let tree = self.tree.borrow();
        let index = tree.resolve(&NodeRef::Id(id))?;
        Some(tree.nodes()[index as usize].space.screen_bounds())
    }

    #[must_use]
    pub fn group(&self, group: Group) -> Vec<Rect> {
        self.tree
            .borrow()
            .avoided(u32::MAX, &[group.children()])
            .collect()
    }

    /// Declares a keep-clear node and returns the index of its first clear slot and its box.
    /// Callers may use a modal or compact fallback if no slot fits.
    ///
    /// # Panics
    ///
    /// If the scene contains too many nodes for its identifiers.
    pub fn place(
        &self,
        name: &'static str,
        of: Vec<NodeQuery>,
        candidates: impl IntoIterator<Item = Candidate>,
    ) -> Option<(usize, Rect)> {
        self.place_named(
            name,
            of,
            candidates.into_iter().map(|candidate| (name, candidate)),
        )
    }

    /// Places a control with profile slot names for its candidates.
    pub fn place_named(
        &self,
        name: &'static str,
        of: Vec<NodeQuery>,
        candidates: impl IntoIterator<Item = (&'static str, Candidate)>,
    ) -> Option<(usize, Rect)> {
        let mut tree = self.tree.borrow_mut();
        let candidates: Vec<_> = candidates.into_iter().collect();
        let places: Vec<_> = candidates
            .iter()
            .map(|&(_, candidate)| tree.add_slot(candidate))
            .collect();
        let index = u32::try_from(tree.nodes().len()).expect("nodes fit u32 indexes");
        let mut node = Node::screen(ElementId(PHASE.0 - 1 - index), name, Rect::default());
        node.keep_clear = Some(KeepClear {
            of,
            places: places.clone(),
        });
        let index = tree.push(node);
        tree.solve(SolvePhase::Handle, |_, _| {});
        let selected = tree.selected_slot(index);
        let relation = tree.nodes()[index as usize].keep_clear.as_ref()?;
        if let Some(attempts) = &self.attempts {
            let mut attempts = attempts.borrow_mut();
            for (&slot, &(slot_name, candidate)) in places.iter().zip(&candidates) {
                let chosen = selected == Some(slot);
                attempts.push(Attempt {
                    control: name,
                    slot: slot_name,
                    bounds: candidate.bounds,
                    selected: chosen,
                    blocked: obstacles(&tree, index, &relation.of, candidate.clearance),
                });
                if chosen {
                    break;
                }
            }
        }
        let choice = places.iter().position(|&slot| Some(slot) == selected)?;
        let (_, candidate) = candidates[choice];
        self.clearances.borrow_mut().push(Clearance {
            name,
            rect: candidate.clearance,
            avoided: tree.avoided(index, &relation.of).collect(),
        });
        Some((choice, candidate.bounds))
    }

    /// The bounds, ids and groups of the table and phase nodes a prompt can query.
    /// Control placement does not change this source geometry.
    pub(crate) fn prompt_geometry(&self) -> Vec<(ElementId, Option<u32>, Rect)> {
        self.tree
            .borrow()
            .nodes()
            .iter()
            .filter(|node| node.keep_clear.is_none())
            .map(|node| (node.id, node.parent, node.space.screen_bounds()))
            .collect()
    }

    /// A prompt solves its controls against a private copy of the table.
    #[must_use]
    pub(crate) fn prompt_scene(&self) -> Self {
        let mut tree = Tree::default();
        for node in self
            .tree
            .borrow()
            .nodes()
            .iter()
            .filter(|node| node.keep_clear.is_none())
        {
            tree.push(node.clone());
        }
        Self {
            tree: RefCell::new(tree),
            clearances: RefCell::default(),
            attempts: self.recording().then(RefCell::default),
        }
    }

    pub fn record(&mut self) {
        self.attempts = Some(RefCell::default());
    }

    pub(crate) fn recording(&self) -> bool {
        self.attempts.is_some()
    }

    pub(crate) fn raise_dock(&self, control: &'static str, bounds: Rect) {
        if let Some(attempts) = &self.attempts {
            attempts.borrow_mut().push(Attempt {
                control,
                slot: "prompt.dock",
                bounds,
                selected: false,
                blocked: obstacles(
                    &self.tree.borrow(),
                    u32::MAX,
                    &[Group::Hand.children()],
                    bounds,
                ),
            });
        }
    }

    pub fn take_attempts(&self) -> Vec<Attempt> {
        self.attempts.as_ref().map_or_else(Vec::new, RefCell::take)
    }

    pub(crate) fn add_attempts(&self, recorded: &[Attempt]) {
        if let Some(attempts) = &self.attempts {
            attempts.borrow_mut().extend_from_slice(recorded);
        }
    }

    pub(crate) fn take_clearances(self) -> Vec<Clearance> {
        self.clearances.into_inner()
    }

    pub(crate) fn add_clearances(&self, clearances: &[Clearance]) {
        self.clearances.borrow_mut().extend_from_slice(clearances);
    }

    /// The solved relations, shared with frames without copying their boxes.
    #[must_use]
    pub fn into_clearances(self) -> Arc<[Clearance]> {
        self.clearances.into_inner().into()
    }
}

/// A box in window points.
#[must_use]
pub const fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Insets the tested box by `amount` on each side. A negative inset leaves a gap.
#[must_use]
pub fn inset(bounds: Rect, amount: f32) -> Candidate {
    Candidate {
        bounds,
        clearance: rect(
            bounds.x + amount,
            bounds.y + amount,
            (bounds.width - 2.0 * amount).max(0.0),
            (bounds.height - 2.0 * amount).max(0.0),
        ),
    }
}

fn obstacles(tree: &Tree, index: u32, of: &[NodeQuery], candidate: Rect) -> Vec<Obstacle> {
    tree.obstacles(index, of)
        .filter_map(|node| {
            let bounds = node.space.screen_bounds();
            intersection(candidate, bounds)?;
            let group = node.parent.map_or(node.name.as_ref(), |parent| {
                tree.nodes()[parent as usize].name.as_ref()
            });
            Some(Obstacle {
                id: node.id,
                group: match group {
                    "viewer rows" => "viewer rows",
                    "opponent rows" => "opponent rows",
                    "viewer hand" => "viewer hand",
                    "stack" => "stack",
                    _ => "phase",
                },
                bounds,
            })
        })
        .collect()
}
