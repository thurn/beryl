//! Combined copiable characteristics of a split card outside the stack.
//!
//! Implements: CR 709.4

use std::hash::{Hash, Hasher};

use beryl_card::definition::Shape;
use beryl_card::{Colors, Face, FaceIx, ManaCost, Supertypes, Types};

use super::Copiable;
use crate::layout::Layout;
use crate::table::GameTable;

// A spell or permanent's copiable record hashes as before. Only the values read
// off the stack can mark a split card as having both halves.
impl<L: Layout> Hash for Copiable<L> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.card.hash(h);
        self.face.hash(h);
        self.additions.hash(h);
        self.read.hash(h);
        self.component_face.hash(h);
        if self.off_stack {
            self.off_stack.hash(h);
        }
    }
}

impl<L: Layout> Copiable<L> {
    /// The values to read off the stack.
    #[inline]
    pub(crate) const fn with_other_half(mut self) -> Self {
        self.off_stack = true;
        self
    }

    /// The other half when both exist ([CR 709.4]): "In every zone except the stack,
    /// the characteristics of a split card are those of its two halves combined."
    ///
    /// [CR 709.4]: https://yawgatog.com/resources/magic-rules/#R7094
    #[inline]
    pub(super) fn other_half(self, t: &GameTable) -> Option<&'static Face> {
        (self.off_stack
            && self.face != FaceIx::DOWN
            && matches!(self.shape(t), Shape::Split { .. }))
        .then(|| t.def(self.card).face(FaceIx::BACK))
    }

    /// The printed card types of the values, including both halves where they exist.
    pub fn types(self, t: &GameTable) -> Types {
        let types = self.face(t).types;
        self.other_half(t)
            .map_or(types, |half| types.union(half.types))
    }

    /// The printed supertypes of the values, including both halves where they exist.
    pub fn supertypes(self, t: &GameTable) -> Supertypes {
        let types = self.face(t).supertypes;
        self.other_half(t)
            .map_or(types, |half| types.union(half.supertypes))
    }

    /// The mana cost of the copiable characteristics before any value of X is applied.
    pub fn mana_cost(self, t: &GameTable) -> ManaCost {
        let cost = self.face(t).cost;
        self.other_half(t).map_or(cost, |half| cost.plus(half.cost))
    }

    /// The names the object has. [CR 709.4a]: "An object has the chosen name if one
    /// of its names is the chosen name."
    ///
    /// [CR 709.4a]: https://yawgatog.com/resources/magic-rules/#R7094a
    #[must_use]
    pub fn names(self, t: &GameTable) -> [Option<&'static str>; 2] {
        let first = self.name(t);
        [
            (!first.is_empty()).then_some(first),
            self.other_half(t).map(|half| half.name),
        ]
    }

    /// Whether the object has a particular name.
    pub fn has_name(self, t: &GameTable, name: &str) -> bool {
        self.names(t)
            .into_iter()
            .flatten()
            .any(|actual| actual == name)
    }

    pub(super) fn printed_colors(self, t: &GameTable) -> Colors {
        let colors = self.face(t).colors;
        self.other_half(t)
            .map_or(colors, |half| colors.union(half.colors))
    }
}
