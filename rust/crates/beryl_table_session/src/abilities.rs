//! Publishes printed ability rows and applies them using the host's visible source.

use beryl_protocol::protocol::{TableAbilityOption, TableObjectRef};
use beryl_table::assist::text::{self, AbilityRow, LoyaltyCost};
use beryl_table::assist::{self, Assist};
use beryl_table::state::Zone;
use beryl_table::{Action, Refusal, Seat};

use crate::view::cards::CardProjection;

use super::PublishedTable;

pub(crate) fn menu_option(row: &AbilityRow) -> TableAbilityOption {
    let label = match row {
        AbilityRow::Activated { cost, text } => format!("{cost}: {text}"),
        AbilityRow::Loyalty { cost, text } => {
            let cost = match cost {
                LoyaltyCost::Plus(amount) => format!("+{amount}"),
                LoyaltyCost::Minus(amount) => format!("−{amount}"),
                LoyaltyCost::MinusX => "−X".into(),
                LoyaltyCost::Zero => "0".into(),
            };
            format!("{cost}: {text}")
        }
    };
    TableAbilityOption {
        label,
        requires_x: needs_x(row),
    }
}

fn needs_x(row: &AbilityRow) -> bool {
    matches!(
        row,
        AbilityRow::Loyalty {
            cost: LoyaltyCost::MinusX,
            ..
        }
    )
}

impl PublishedTable {
    pub(super) fn ability_action(
        &self,
        seat: Seat,
        source: &TableObjectRef,
        row: usize,
        x: Option<u16>,
    ) -> Result<Action, Refusal> {
        if !assist::enabled(&self.table, seat, Assist::AbilityMenus) {
            return Err(Refusal::Permission);
        }
        let publication = self.publication(seat).ok_or(Refusal::Stale)?;
        let (id, slot) = publication
            .refs
            .resolve_card(source, Zone::Battlefield)
            .ok_or(Refusal::Stale)?;
        let shown = publication
            .view
            .table
            .as_ref()
            .and_then(|table| table.card_commands.get(&slot))
            .and_then(|card| card.abilities.get(row))
            .ok_or(Refusal::INVALID)?;
        if shown.requires_x != x.is_some() {
            return Err(Refusal::INVALID);
        }
        let card = self.table.objects.get(&id).ok_or(Refusal::Stale)?;
        let rows = CardProjection::new(card).abilities();
        let row = rows.get(row).ok_or(Refusal::Stale)?;
        Ok(text::ability_action(id, row, x))
    }
}
