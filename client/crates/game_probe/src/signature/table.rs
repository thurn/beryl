//! The card groups placement lays out, before fitting them to a window.

use beryl_protocol::protocol::{ObjectView, PlayerView};

use game_table::table::{
    Beneath, Group, PileRules, TableShape, Zones, grouped, prepare_row, zone_members,
};

/// A group of cards laid out together, with the cards tucked to either side of it.
pub(super) struct CardGroup<'a> {
    pub(super) pile: Vec<&'a ObjectView>,
    pub(super) left: Vec<&'a ObjectView>,
    pub(super) right: Vec<&'a ObjectView>,
}

/// The zones of the view's table shape in `Group::ALL` order, with the placement order of each
/// pile and row.
#[must_use]
pub(super) fn zone_groups(view: &PlayerView) -> Vec<Vec<CardGroup<'_>>> {
    let shape = TableShape::of_view(view);
    let zones = Zones::of_view(view, &[]);
    let rules = PileRules::of_view(view);
    Group::ALL
        .into_iter()
        .zip(zone_members(&zones))
        .filter(|(group, _)| group.in_shape(shape))
        .map(|(group, cards)| {
            let Group::Row { row, .. } = group else {
                return vec![CardGroup {
                    pile: cards,
                    left: Vec::new(),
                    right: Vec::new(),
                }];
            };
            let (cards, under, names) = prepare_row(&cards, row, None);
            let beneath = Beneath::new(under, &cards);
            let groups = grouped((&cards, &beneath), (names, None, rules.for_group(group)));
            groups
                .piles()
                .map(|pile| {
                    let host = pile[0].slot;
                    CardGroup {
                        pile: pile.to_vec(),
                        left: beneath.of(host, true).collect(),
                        right: beneath.of(host, false).collect(),
                    }
                })
                .collect()
        })
        .collect()
}
