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

/// The rules that decide which battlefield cards can share a pile.
#[derive(Default)]
pub struct PileRules {
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
        rules.lands = piled_lands(self.battlefield.iter().filter_map(|&slot| self.get(slot)));
        self.stack_names(&mut rules.named);
    }

    fn stack_names(&self, names: &mut Vec<ObjectId>) {
        match self.stack_items {
            StackItems::View(stack) => stack_names(names, stack.iter()),
            StackItems::Played(stack) => stack_names(names, stack.iter().map(|entry| &entry.item)),
        }
    }
}

impl PileRules {
    /// Reads the land counts and stack references that affect piling in `view`.
    pub fn of_view(view: &PlayerView) -> Self {
        let permanents = view
            .objects
            .iter()
            .filter(|object| object.zone == ZoneName::Battlefield);
        let mut rules = Self {
            lands: piled_lands(permanents),
            ..Self::default()
        };
        stack_names(&mut rules.named, view.stack.iter());
        rules
    }

    /// Applies the replay's new pile rules and marks any additional rows that need placement.
    pub fn update(
        &mut self,
        zones: &Zones,
        dirty: &mut [bool; Group::ALL.len()],
        lands_may_change: bool,
    ) {
        if lands_may_change {
            let lands = piled_lands(zones.battlefield.iter().filter_map(|&slot| zones.get(slot)));
            for side in [Side::You, Side::Opponent] {
                if self.lands[side_index(side)] != lands[side_index(side)] {
                    dirty[Group::Row {
                        side,
                        row: RowName::Lands,
                    }
                    .index()] = true;
                }
            }
            self.lands = lands;
        }
        if dirty[Group::Stack.index()] {
            zones.stack_names(&mut self.scratch);
            self.mark_named_changes(zones, dirty);
            std::mem::swap(&mut self.scratch, &mut self.named);
        }
    }

    /// The sorted slots that stand alone in a row whose twins can pile.
    pub fn for_group(&self, group: Group) -> Option<&[ObjectId]> {
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

    /// Re-place a named permanent's row when it must leave or may rejoin a pile.
    fn mark_named_changes(&self, zones: &Zones, dirty: &mut [bool; Group::ALL.len()]) {
        if self.named == self.scratch {
            return;
        }
        for (was, now) in [(&self.named, &self.scratch), (&self.scratch, &self.named)] {
            for &slot in was.iter().filter(|slot| now.binary_search(slot).is_err()) {
                let Some(object) = zones.get(slot) else {
                    continue;
                };
                let Some(group @ Group::Row { .. }) = Group::of(object) else {
                    continue;
                };
                if dirty[group.index()] || self.for_group(group).is_none() {
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

fn piled_lands<'a>(permanents: impl Iterator<Item = &'a ObjectView>) -> [bool; 2] {
    let mut lands = [0; 2];
    for object in permanents {
        if object.types.contains(&CardType::Land) {
            lands[side_index(object.controller)] += 1;
            if lands.iter().all(|&count| count >= LANDS_PILED) {
                break;
            }
        }
    }
    lands.map(|count| count >= LANDS_PILED)
}

fn stack_names<'a>(names: &mut Vec<ObjectId>, stack: impl Iterator<Item = &'a StackItem>) {
    names.clear();
    names.extend(stack.flat_map(|item| {
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
    names.sort_unstable();
    names.dedup();
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
