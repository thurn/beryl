//! The fake hub's whole tickets, which `export` and `import` read and write, kept apart from the
//! ticket details that `ticket`, `tickets` and `batch` use.

use super::{FakeHub, lock};
use crate::ids::{Seq, TicketId};
use crate::protocol::tickets::{ImportReply, ImportRequest, WholeTicket};
use crate::traits::{Error, Result};

/// The most tickets one export page or one import carries, as at the hub.
const PAGE: usize = 200;

impl FakeHub {
    /// Stores `whole` as the hub's copy of its ticket and records a `ticket` change in the feed
    /// for it, as any write to a ticket does. Returns the change's feed position.
    pub fn put_whole(&self, mut whole: WholeTicket) -> Seq {
        let seq = Seq(lock(&self.feed).last().map_or(0, |c| c.seq.0) + 1);
        whole.ticket.version = seq;
        let id = whole.ticket.id.clone();
        lock(&self.whole).insert(id.clone(), whole);
        self.push_change("ticket", Some(id))
    }

    /// The hub's copy of one whole ticket.
    pub fn whole(&self, id: &TicketId) -> Option<WholeTicket> {
        lock(&self.whole).get(id).cloned()
    }

    /// Every import received so far, in order.
    pub fn imports(&self) -> Vec<ImportRequest> {
        lock(&self.imports).clone()
    }

    /// Keeps only the first `kept` changes of the feed, as a hub restored from an older snapshot
    /// has: its feed position goes back below what its clients have read.
    pub fn restart_feed(&self, kept: usize) {
        lock(&self.feed).truncate(kept);
    }

    pub(super) fn export_whole(
        &self,
        after: Option<&TicketId>,
        ids: &[TicketId],
    ) -> Result<Vec<WholeTicket>> {
        self.reachable()?;
        if ids.len() > PAGE {
            return Err(Error::Invalid(format!("at most {PAGE} tickets at once")));
        }
        let whole = lock(&self.whole);
        if !ids.is_empty() {
            return Ok(ids.iter().filter_map(|id| whole.get(id).cloned()).collect());
        }
        Ok(whole
            .values()
            .filter(|w| after.is_none_or(|after| w.ticket.id.as_str() > after.as_str()))
            .take(PAGE)
            .cloned()
            .collect())
    }

    /// Replaces each ticket unless its stored copy was updated later and the import is not
    /// forced; each replaced ticket gets a feed change.
    pub(super) fn import_whole(&self, request: &ImportRequest) -> Result<ImportReply> {
        self.reachable()?;
        lock(&self.imports).push(request.clone());
        let mut imported = 0;
        let mut seq = lock(&self.feed).last().map_or(Seq(0), |c| c.seq);
        for whole in &request.tickets {
            let newer = lock(&self.whole)
                .get(&whole.ticket.id)
                .is_some_and(|stored| stored.ticket.updated_at > whole.ticket.updated_at);
            if newer && !request.force {
                continue;
            }
            seq = self.put_whole(whole.clone());
            imported += 1;
        }
        Ok(ImportReply { seq, imported })
    }
}
