//! Ticket changes shared by landing and closing.

use std::collections::BTreeMap;

use ignite_core::protocol::tickets::{CULPRIT_AFFINITY_KEY, Operation, REVERTED_KEY, Ticket};

use super::SAVED_BRANCH;

/// The operations that close `ticket` with `reason` and forget its saved branch, which the
/// closing session deletes from the remote.
pub(super) fn closing(ticket: &Ticket, reason: &str) -> Vec<Operation> {
    let mut ops = Vec::new();
    // A landing applies the work again after a tip round reverted it, so `reverted` is done.
    let reverted = ticket.metadata.contains_key(REVERTED_KEY);
    let stale: Vec<String> = [SAVED_BRANCH, REVERTED_KEY]
        .into_iter()
        .chain(reverted.then_some(CULPRIT_AFFINITY_KEY))
        .filter(|key| ticket.metadata.contains_key(*key))
        .map(str::to_owned)
        .collect();
    if !stale.is_empty() {
        ops.push(Operation::Update {
            id: ticket.id.as_str().to_owned(),
            set: None,
            add_labels: Vec::new(),
            remove_labels: if reverted {
                crate::upkeep::culprit_affinity::added_label(ticket)
                    .into_iter()
                    .collect()
            } else {
                Vec::new()
            },
            set_meta: BTreeMap::new(),
            unset_meta: stale,
        });
    }
    ops.push(Operation::Close {
        id: ticket.id.as_str().to_owned(),
        reason: reason.to_owned(),
        include_ready: false,
    });
    ops
}
