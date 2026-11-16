//! When battlefield twins can share a pile, as in the web interface's `sideGroups`.

use beryl_protocol::protocol::{
    CardType, ObjectId, ObjectView, PlayerView, RowName, Side, StackItem, TargetRef, ZoneName,
};
use beryl_protocol::script::Entry;

use super::super::rows::{plain, twins};
use super::{Group, Zones, side_index};

/// Each controller needs this many lands before twins share piles in their lands row.
const LANDS_PILED: usize = 7;

pub(super) enum StackItems<'a> {
    View(&'a [StackItem]),
    Played(&'a [Entry]),
}

#[derive(Default)]
pub(crate) struct PileRules {
    lands: [bool; 2],
    named: Vec<ObjectId>,
    // Reused for the next replay step, while the rules in force keep their names.
    scratch: Vec<ObjectId>,
}

impl Zones<'_> {
    pub(super) fn pile_rules(&self) -> PileRules {
        let mut rules = PileRules::default();
        self.fill_pile_rules(&mut rules);
        rules
    }

    fn fill_pile_rules(&self, rules: &mut PileRules) {
        let objects = self.battlefield.iter().filter_map(|&slot| self.get(slot));
        match self.stack_items {
            StackItems::View(stack) => rules.fill(objects, stack.iter()),
            StackItems::Played(stack) => {
                rules.fill(objects, stack.iter().map(|entry| &entry.item));
            }
        }
    }
}

impl PileRules {
    pub(crate) fn of_view(view: &PlayerView) -> Self {
        let permanents = view
            .objects
            .iter()
            .filter(|object| object.zone == ZoneName::Battlefield);
        let mut rules = Self::default();
        rules.fill(permanents, view.stack.iter());
        rules
    }

    fn fill<'a>(
        &mut self,
        permanents: impl Iterator<Item = &'a ObjectView>,
        stack: impl Iterator<Item = &'a StackItem>,
    ) {
        let mut lands = [0; 2];
        for object in permanents {
            if object.types.contains(&CardType::Land) {
                lands[side_index(object.controller)] += 1;
                if lands.iter().all(|&count| count >= LANDS_PILED) {
                    break;
                }
            }
        }
        self.lands = lands.map(|count| count >= LANDS_PILED);
        self.named.clear();
        self.named.extend(stack.flat_map(|item| {
            let (source, targets) = match item {
                StackItem::Spell { targets, .. } => (None, targets),
                StackItem::Ability {
                    source, targets, ..
                } => (*source, targets),
            };
            source
                .into_iter()
                .chain(targets.iter().filter_map(|target| match target {
                    TargetRef::Object { slot } => Some(*slot),
                    _ => None,
                }))
        }));
        self.named.sort_unstable();
        self.named.dedup();
    }

    /// Applies the replay's new pile rules and marks any additional rows that need placement.
    pub(crate) fn update(&mut self, zones: &Zones, dirty: &mut [bool; Group::ALL.len()]) {
        let mut after = Self {
            named: std::mem::take(&mut self.scratch),
            ..Self::default()
        };
        zones.fill_pile_rules(&mut after);
        self.mark_changes(&after, zones, dirty);
        after.scratch = std::mem::take(&mut self.named);
        *self = after;
    }

    /// The sorted slots that stand alone in a row whose twins can pile.
    pub(crate) fn for_group(&self, group: Group) -> Option<&[ObjectId]> {
        match group {
            Group::Row { side, row } if row != RowName::Lands || self.lands[side_index(side)] => {
                Some(&self.named)
            }
            _ => None,
        }
    }

    /// Whether both sets of rules give these objects the same opportunity to pile.
    pub(super) fn same_for_row(&self, known: &Self, group: Group, objects: &[&ObjectView]) -> bool {
        match (self.for_group(group), known.for_group(group)) {
            (Some(named), Some(was)) if named != was => ![(named, was), (was, named)]
                .into_iter()
                .any(|(named, free)| {
                    objects.iter().copied().any(|object| {
                        named.binary_search(&object.slot).is_ok()
                            && free.binary_search(&object.slot).is_err()
                            && pile_twin(object, objects.iter().copied(), free)
                    })
                }),
            (named, was) => named == was,
        }
    }

    /// Re-place a lands row when its controller crosses the threshold. Re-place a named
    /// permanent's row when it must leave or may rejoin a pile, even if its own view is unchanged.
    fn mark_changes(&self, after: &Self, zones: &Zones, dirty: &mut [bool; Group::ALL.len()]) {
        for side in [Side::You, Side::Opponent] {
            if self.lands[side_index(side)] != after.lands[side_index(side)] {
                dirty[Group::Row {
                    side,
                    row: RowName::Lands,
                }
                .index()] = true;
            }
        }
        for (was, now) in [(&self.named, &after.named), (&after.named, &self.named)] {
            for &slot in was.iter().filter(|slot| now.binary_search(slot).is_err()) {
                let Some(object) = zones.get(slot) else {
                    continue;
                };
                let Some(group @ Group::Row { .. }) = Group::of(object) else {
                    continue;
                };
                if dirty[group.index()]
                    || (self.for_group(group).is_none() && after.for_group(group).is_none())
                {
                    continue;
                }
                let row = zones
                    .battlefield
                    .iter()
                    .filter_map(|&slot| zones.get(slot))
                    .filter(|other| Group::of(other) == Some(group));
                if pile_twin(object, row, now) {
                    dirty[group.index()] = true;
                }
            }
        }
    }
}

fn pile_twin<'a>(
    object: &ObjectView,
    mut row: impl Iterator<Item = &'a ObjectView>,
    named: &[ObjectId],
) -> bool {
    plain(object)
        && row.any(|other| {
            twins(object, other) && plain(other) && named.binary_search(&other.slot).is_err()
        })
}
