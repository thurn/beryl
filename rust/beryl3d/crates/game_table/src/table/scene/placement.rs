//! The table's placement rules and work counters.

use beryl_protocol::protocol::{ObjectId, PlayerView, RowName, Side};

use super::Table;
use crate::table::HandFits;
use crate::table::place::{
    Group, PileRules, Placement, Rules, Zones, Zooms, place_group, place_reusing, place_zones,
    rezoom, zooms,
};
use crate::table::rows::NameIds;

impl Table {
    fn placement_rules<'a>(&'a self, ids: Option<&'a NameIds<'a>>) -> Rules<'a> {
        (
            &self.layout,
            &self.camera,
            &self.scroll,
            &self.row_fits,
            ids,
        )
    }

    /// Every object's resting place in `view`, whose stack's abilities `abilities` numbers,
    /// bottom first, for this window.
    #[must_use]
    pub fn placement(&self, view: &PlayerView, abilities: &[u32]) -> Placement {
        self.placement_of(&Zones::of_view(view, abilities))
    }

    /// Every object's resting place in `zones`, for this window.
    pub fn placement_of(&self, zones: &Zones) -> Placement {
        let placement = place_zones(zones, self.placement_rules(None));
        self.placed.add(placement.count());
        placement
    }

    /// Every object's resting place in `zones`, for this window, taking each group's from
    /// `known`, this window's placement of `known_zones`, where that group holds the same objects
    /// in both.
    pub fn placement_reusing(&self, zones: &Zones, known: (&Zones, &Placement)) -> Placement {
        let (placement, laid) = place_reusing(zones, known, self.placement_rules(None));
        self.placed.add(laid);
        placement
    }

    /// How far each battlefield row of `zones` is zoomed out for this window (`place::zooms`).
    /// `ids` numbers the names of `zones`' objects, if the caller keeps them.
    pub fn zooms(&self, zones: &Zones, piles: &PileRules, ids: Option<&NameIds<'_>>) -> Zooms {
        zooms(zones, piles, self.placement_rules(ids))
    }

    /// How far each battlefield row of `zones` is zoomed out for this window, measuring again
    /// only the rows `changed` names and taking the others' fits from `known` (`place::rezoom`).
    pub fn rezoom(
        &self,
        known: &Zooms,
        (zones, piles, ids): (&Zones, &PileRules, &NameIds<'_>),
        changed: impl Fn(Side, RowName) -> bool,
    ) -> Zooms {
        rezoom(
            known,
            zones,
            piles,
            self.placement_rules(Some(ids)),
            changed,
        )
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
        let rules = self.placement_rules(Some(ids));
        place_group((zones, piles), group, rules, output, placed);
        self.placed.add(placed.len());
    }
}
