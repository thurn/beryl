//! Opens library sorts from authenticated table actions.

use beryl_protocol::protocol::TableAction;
use beryl_table::action::Flow;
use beryl_table::assist::{self, Assist, text};
use beryl_table::{Action, Entry, Refusal, Seat};

use super::{PromptState, PublishedTable};

impl PublishedTable {
    pub(crate) fn dispatch_action(
        &mut self,
        seat: Seat,
        action: TableAction,
        save: impl FnOnce(&Entry) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        let Action::Flow {
            seat: library_owner,
            flow,
            count,
            source,
        } = action
        else {
            return self.apply_client_checked(seat, action, save).map(|_| ());
        };
        if library_owner != seat || !assist::enabled(&self.table, seat, Assist::LibraryFlows) {
            return Err(Refusal::Permission);
        }
        if let Some(source) = source {
            let publication = self.publication(seat).ok_or(Refusal::Stale)?;
            let slot = publication.refs.card_slot(&source).ok_or(Refusal::Stale)?;
            let card = publication
                .view
                .table
                .as_ref()
                .and_then(|table| table.card_commands.get(&slot));
            if !card.is_some_and(|card| {
                card.flows
                    .iter()
                    .any(|row| row.kind == flow && row.count == count)
            }) {
                return Err(Refusal::INVALID);
            }
        }
        let title = match flow {
            Flow::Scry => "Scry",
            Flow::Surveil => "Surveil",
            Flow::LookAtTop => "Look at top cards",
        };
        let prompt = PromptState::library_sort(&self.table, seat, title.into(), flow, count)?;
        self.open_prompt(
            seat,
            prompt,
            move |state| {
                let PromptState::Sort(sort) = state else {
                    return Err(Refusal::INVALID);
                };
                text::flow_result(text::Flow::new(flow, count), sort.cards(), &sort.choice())
            },
            save,
        )
    }
}
