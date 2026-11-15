//! Converts refusals between the table, the peer link and the shared IPC reply.

use beryl_link::wire::RefusalReason;
use beryl_session::ipc::TableActReply;
use beryl_table::Refusal;

pub fn to_wire(refusal: Refusal) -> RefusalReason {
    match refusal {
        Refusal::Permission => RefusalReason::Permission,
        Refusal::Stale => RefusalReason::Stale,
        Refusal::Invalid { blocking } => RefusalReason::Invalid { blocking },
    }
}

pub fn reply_from_wire(reason: RefusalReason) -> TableActReply {
    TableActReply::from(Err(from_wire(reason)))
}

pub(crate) fn from_wire(reason: RefusalReason) -> Refusal {
    match reason {
        RefusalReason::Permission => Refusal::Permission,
        RefusalReason::Stale => Refusal::Stale,
        RefusalReason::Invalid { blocking } => Refusal::Invalid { blocking },
    }
}
