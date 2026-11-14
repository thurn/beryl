use crate::action::Selection;
use crate::state::Zone;
use crate::{Action, Seat, Table};

/// Untaps the seat's tapped permanents, except those flagged as staying tapped.
pub fn untap_all(table: &Table, seat: Seat) -> Action {
    let objects = table
        .battlefield
        .iter()
        .copied()
        .filter(|id| {
            let card = &table.objects[id];
            card.controller == seat && card.tapped && !card.doesnt_untap
        })
        .collect();
    Action::Tap {
        selection: Selection {
            objects,
            zone: Zone::Battlefield,
        },
        tapped: false,
    }
}
