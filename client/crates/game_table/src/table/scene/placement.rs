//! The table's placement rules and work counters.

use beryl_protocol::protocol::{ObjectId, PlayerView, RowName, Side};

use super::Table;
use crate::table::place::{
    Group, PileRules, Placement, Rules, Zones, Zooms, place_group, place_reusing, place_zones,
    rezoom, solve_zones, zooms,
};
use crate::table::rows::NameIds;
use crate::table::{HandFits, Layout, TableShape};

impl Table {
    fn layout_for(&self, shared: bool) -> Layout {
        let shape = if shared {
            TableShape::SharedPiles
        } else {
            TableShape::PerPlayerPiles
        };
        Layout {
            shape,
            ..self.layout
        }
    }

    fn rules<'a>(&'a self, layout: &'a Layout, ids: Option<&'a NameIds<'a>>) -> Rules<'a> {
        (
            layout,
            &self.camera,
            &self.scroll,
            &self.row_fits,
            ids,
            &self.data.verbs.lift,
        )
    }

    /// Every object's resting place in `view`, whose stack's abilities `abilities` numbers,
    /// bottom first, for this window.
    #[must_use]
    pub fn placement(&self, view: &PlayerView, abilities: &[u32]) -> Placement {
        let layout = self.layout_for(view.shared.is_some());
        let placement = place_zones(&Zones::of_view(view, abilities), self.rules(&layout, None));
        self.placed.add(placement.count());
        placement
    }

    /// Runs the placement solver with its optional decision side table enabled.
    pub fn placement_for_probe(&self, view: &PlayerView, abilities: &[u32]) -> Placement {
        let layout = self.layout_for(view.shared.is_some());
        solve_zones(
            &Zones::of_view(view, abilities),
            self.rules(&layout, None),
            Some(Box::default()),
        )
    }

    /// Every object's resting place in `zones`, for this window.
    pub fn placement_of(&self, zones: &Zones) -> Placement {
        let layout = self.layout_for(zones.shared);
        let placement = place_zones(zones, self.rules(&layout, None));
        self.placed.add(placement.count());
        placement
    }

    /// Every object's resting place in `zones`, for this window, taking each group's from
    /// `known`, this window's placement of `known_zones`, where that group holds the same objects
    /// in both.
    pub fn placement_reusing(&self, zones: &Zones, known: (&Zones, &Placement)) -> Placement {
        let layout = self.layout_for(zones.shared);
        let (placement, laid) = place_reusing(zones, known, self.rules(&layout, None));
        self.placed.add(laid);
        placement
    }

    /// How far each battlefield row of `zones` is zoomed out for this window (`place::zooms`).
    /// `ids` numbers the names of `zones`' objects, if the caller keeps them.
    pub fn zooms(&self, zones: &Zones, piles: &PileRules, ids: Option<&NameIds<'_>>) -> Zooms {
        let layout = self.layout_for(zones.shared);
        zooms(zones, piles, self.rules(&layout, ids))
    }

    /// How far each battlefield row of `zones` is zoomed out for this window, measuring again
    /// only the rows `changed` names and taking the others' fits from `known` (`place::rezoom`).
    pub fn rezoom(
        &self,
        known: &Zooms,
        (zones, piles, ids): (&Zones, &PileRules, &NameIds<'_>),
        changed: impl Fn(Side, RowName) -> bool,
    ) -> Zooms {
        let layout = self.layout_for(zones.shared);
        rezoom(known, zones, piles, self.rules(&layout, Some(ids)), changed)
    }

    /// Places the objects of `group` in `zones` into `placement` for this window, and leaves the
    /// slots it placed in `placed`. `ids` numbers the names of `zones`' objects.
    pub fn place_group(
        &self,
        (zones, ids, piles): (&Zones, &NameIds<'_>, &PileRules),
        group: Group,
        output: (&mut Placement, &mut HandFits),
        placed: &mut Vec<ObjectId>,
    ) {
        let layout = self.layout_for(zones.shared);
        place_group(
            (zones, piles),
            group,
            self.rules(&layout, Some(ids)),
            output,
            placed,
        );
        self.placed.add(placed.len());
    }
}
