//! The zones of the table a script plays over for one step, read from what the script's player
//! keeps between steps: the group of the object in each slot, and the table's objects by slot.

use beryl_protocol::protocol::{ObjectId, ObjectView, PlayerView, RowName, Side, ZoneName};
use beryl_protocol::script::{Piles, Table as Played};
use std::borrow::Cow;

use super::{Group, OnStack, Zones, row_of, side_of};

impl<'a> Zones<'a> {
    /// The zones of the table a script plays over, as `of_table` gives them, where `groups`
    /// holds the `Group::number` of the object in each slot (`Group::of`), or `NO_GROUP`: a
    /// battlefield row then finds its cards by comparing one number each, without reading each
    /// card's side and types.
    pub(crate) fn of_table_grouped(
        table: &'a Played,
        (groups, tucked): (&'a [u8], &'a [ObjectId]),
    ) -> Self {
        let mut zones = Self::of_table(table);
        zones.groups = Some(groups);
        zones.tucked = Some(tucked);
        zones
    }

    /// The zones of the table a script plays over, as `of_table_grouped` gives them, where
    /// `slots` already holds the table's objects by slot, as equal objects, and `stack` its
    /// stack's items as `on_stack` lists them.
    pub(crate) fn of_table_indexed(
        table: &'a Played,
        (slots, stack): (&'a [Option<&'a ObjectView>], &'a [OnStack]),
        (groups, tucked): (&'a [u8], &'a [ObjectId]),
    ) -> Self {
        let piles = |piles: &'a Piles| [&piles.library[..], &piles.graveyard[..]];
        Self {
            in_view_order: None,
            slots: Cow::Borrowed(slots),
            groups: Some(groups),
            tucked: Some(tucked),
            battlefield: &table.battlefield,
            stack: Cow::Borrowed(stack),
            piles: [piles(&table.you), piles(&table.opponent)],
            handling: None,
            arrival: None,
        }
    }

    /// The same zones, whose hands and exile piles list their cards in the order `arrival` gives
    /// by slot (`arrival`), where the table a script plays over would list them by slot.
    pub(crate) fn arrived(self, arrival: &'a [u32]) -> Self {
        Self {
            arrival: Some(arrival),
            ..self
        }
    }

    /// Puts `objects`, the cards of a hand or an exile pile, in the order they arrived there,
    /// where this is the table a script plays over and knows it; a card it does not know comes
    /// last, by slot. The sort is stable, so cards already in order stay as they are.
    pub(super) fn in_arrival_order(&self, objects: &mut [&'a ObjectView]) {
        if let Some(arrival) = self.arrival {
            objects.sort_by_key(|object| {
                let slot = usize::from(object.slot.0);
                (arrival.get(slot).copied().unwrap_or(u32::MAX), slot)
            });
        }
    }

    /// The permanents of one side's `row`, in the order they entered, but for those that lie
    /// under another permanent (`host`), which its row lists with the cards under it.
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
        found.retain(|object| object.attached_to.is_none() || self.host(object).is_none());
        found
    }
}

/// The order in which the hands and exile piles of the table a script plays over list their cards,
/// as a number for each slot, `u32::MAX` for a card in neither: the order of the view the script
/// leads to, where its cards rest at the end, with each card that only the view it starts from
/// holds (one that leaves during the script) just after the card it follows there. The table
/// keeps its objects by slot, which is not the order a view lists a hand in once cards have been
/// drawn into it from a library whose slots are lower, as on the manual table, so a card would
/// fly to one place in the hand and rest in another.
#[must_use]
pub(crate) fn arrival(before: &PlayerView, after: &PlayerView) -> Vec<u32> {
    /// Room after each card of the view the script leads to for the cards that follow it only
    /// in the view it starts from.
    const ROOM: u32 = 1 << 12;
    let listed = |view: &'_ PlayerView| {
        view.objects
            .iter()
            .filter(|object| matches!(object.zone, ZoneName::Hand | ZoneName::Exile))
            .map(|object| usize::from(object.slot.0))
            .collect::<Vec<usize>>()
    };
    let (first, then) = (listed(before), listed(after));
    let end = first.iter().chain(&then).max().map_or(0, |&slot| slot + 1);
    let mut arrival = vec![u32::MAX; end];
    let mut next = 0;
    for slot in then {
        if arrival[slot] == u32::MAX {
            next += ROOM;
            arrival[slot] = next;
        }
    }
    let mut follows = 0;
    for slot in first {
        if arrival[slot] == u32::MAX {
            follows += 1;
            arrival[slot] = follows;
        } else {
            follows = arrival[slot];
        }
    }
    arrival
}
