//! The table's placement rules and work counters.

use beryl_protocol::protocol::{ObjectId, PlayerView};

use super::Table;
use crate::table::HandFits;
use crate::table::place::{
    Group, PileRules, Placement, Rules, Zones, place_group, place_reusing, place_zones,
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
    pub(crate) fn placement_of(&self, zones: &Zones) -> Placement {
        let placement = place_zones(zones, self.placement_rules(None));
        self.placed.add(placement.count());
        placement
    }

    /// Every object's resting place in `zones`, for this window, taking each group's from
    /// `known`, this window's placement of `known_zones`, where that group holds the same objects
    /// in both.
    pub(crate) fn placement_reusing(
        &self,
        zones: &Zones,
        known: (&Zones, &Placement),
    ) -> Placement {
        let (placement, laid) = place_reusing(zones, known, self.placement_rules(None));
        self.placed.add(laid);
        placement
    }

    /// Places the objects of `group` in `zones` into `placement` for this window, and leaves the
    /// slots it placed in `placed`. `ids` numbers the names of `zones`' objects.
    pub(crate) fn place_group(
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
