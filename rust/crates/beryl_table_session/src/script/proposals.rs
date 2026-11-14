use std::collections::BTreeSet;

use beryl_session::protocol::{ObjectId, PlayerView, ZoneName};
use beryl_table::Action;
use beryl_table::state::{ObjectId as HostId, Proposal};

use crate::view::Slots;

/// Projects the affected cards in the viewer's enabled proposal lines to their
/// stage slots. Pass only the proposals sent to this viewer.
#[must_use]
pub fn proposal_marks(proposals: &[Proposal], view: &PlayerView, slots: &Slots) -> Vec<ObjectId> {
    let mut ids = BTreeSet::new();
    for action in proposals
        .iter()
        .flat_map(|proposal| &proposal.lines)
        .filter(|line| line.on)
        .flat_map(|line| &line.actions)
    {
        collect_marks(action, &mut ids);
    }
    let affected: BTreeSet<_> = ids.into_iter().filter_map(|id| slots.get(id)).collect();
    view.objects
        .iter()
        .filter_map(|card| {
            // The viewer can see which face-down permanent is being discussed.
            // In a hidden pile, only mark cards whose identity is already visible.
            let hidden_pile = matches!(
                card.zone,
                ZoneName::Library | ZoneName::Hand | ZoneName::Sideboard
            );
            (affected.contains(&card.slot) && (!hidden_pile || card.name.is_some()))
                .then_some(card.slot)
        })
        .collect()
}

fn collect_marks(action: &Action, ids: &mut BTreeSet<HostId>) {
    match action {
        Action::Move { selection, .. }
        | Action::Tap { selection, .. }
        | Action::Reorder { selection, .. }
        | Action::Pin { selection, .. }
        | Action::Give { selection, .. }
        | Action::Face { selection, .. }
        | Action::Flag { selection, .. }
        | Action::Counter { selection, .. }
        | Action::Stats { selection, .. }
        | Action::Damage { selection, .. }
        | Action::Note { selection, .. }
        | Action::Attach { selection, .. }
        | Action::Unattach { selection }
        | Action::Remove { selection } => {
            ids.extend(&selection.objects);
        }
        Action::Batch { actions } => {
            for action in actions {
                collect_marks(action, ids);
            }
        }
        _ => {}
    }
}
