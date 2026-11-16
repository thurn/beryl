//! The zones of the table a script plays over for one step, read from what the script's player
//! keeps between steps: the group of the object in each slot, and the table's objects by slot.

use beryl_protocol::protocol::{ObjectView, RowName, Side};
use beryl_protocol::script::{Piles, Table as Played};
use std::borrow::Cow;

use super::{Group, Zones, on_stack, row_of, side_of};

impl<'a> Zones<'a> {
    /// The zones of the table a script plays over, as `of_table` gives them, where `groups`
    /// holds the `Group::number` of the object in each slot (`Group::of`), or `NO_GROUP`: a
    /// battlefield row then finds its cards by comparing one number each, without reading each
    /// card's side and types.
    pub(crate) fn of_table_grouped(table: &'a Played, groups: &'a [u8]) -> Self {
        let mut zones = Self::of_table(table);
        zones.groups = Some(groups);
        zones
    }

    /// The zones of the table a script plays over, as `of_table_grouped` gives them, where
    /// `slots` already holds the table's objects by slot, as equal objects.
    pub(crate) fn of_table_indexed(
        table: &'a Played,
        slots: &'a [Option<&'a ObjectView>],
        groups: &'a [u8],
    ) -> Self {
        let piles = |piles: &'a Piles| [&piles.library[..], &piles.graveyard[..]];
        Self {
            in_view_order: None,
            slots: Cow::Borrowed(slots),
            groups: Some(groups),
            battlefield: &table.battlefield,
            stack: on_stack(
                table.stack.iter().map(|entry| (&entry.item, entry.id)),
                &[],
                table.objects.values(),
            ),
            piles: [piles(&table.you), piles(&table.opponent)],
            handling: None,
        }
    }

    /// The permanents of one side's `row`, in the order they entered.
    pub(super) fn in_row(&self, side: Side, row: RowName) -> Vec<&'a ObjectView> {
        let mut found = Vec::with_capacity(self.battlefield.len());
        match self.groups {
            None => found.extend(
                self.battlefield
                    .iter()
                    .filter_map(|&slot| self.get(slot))
                    .filter(|object| side_of(object) == side && row_of(object) == row),
            ),
            Some(groups) => {
                let group = Group::Row { side, row }.number();
                found.extend(
                    self.battlefield
                        .iter()
                        .filter(|slot| groups.get(usize::from(slot.0)) == Some(&group))
                        .filter_map(|&slot| self.get(slot)),
                );
            }
        }
        found
    }
}
