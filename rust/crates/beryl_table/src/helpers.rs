use beryl_card_store::{Record, STORE, Side};

use crate::action::Selection;
use crate::state::{Card, Face, Object, Zone};
use crate::{Action, Seat, Table};

/// Untaps the seat's tapped permanents, except those flagged as staying tapped.
pub fn untap_all(table: &Table, seat: Seat) -> Action {
    Action::Tap {
        selection: Selection {
            objects: untap_targets(table, seat).map(|card| card.id).collect(),
            zone: Zone::Battlefield,
        },
        tapped: false,
    }
}

pub(crate) fn untap_targets(table: &Table, seat: Seat) -> impl Iterator<Item = &Object> {
    table
        .battlefield
        .iter()
        .map(|id| &table.objects[id])
        .filter(move |card| card.controller == seat && card.tapped && !card.doesnt_untap)
}

pub(crate) fn card_record(card: &Card) -> Option<Record> {
    match card {
        Card::Named(name) => STORE
            .card_ignoring_case(name)
            .or_else(|| STORE.tokens(name).into_iter().next()),
        Card::Hidden | Card::Custom { .. } => None,
    }
}

pub(crate) fn printed_face(record: &Record, face: Face) -> Option<&beryl_card_store::Face> {
    match face {
        Face::Front => record.faces.first(),
        Face::Back => record.faces.iter().find(|face| face.side == Side::Back),
        Face::Down => None,
    }
}
