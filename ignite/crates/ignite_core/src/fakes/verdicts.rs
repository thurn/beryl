//! The verdicts and reverted landings of [`FakeHub`]: a verdict store kept in memory, and each
//! reverted landing reported.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::ids::TicketId;
use crate::protocol::hub::{RevertFiled, Reverted, Verdict, VerdictKey};
use crate::traits::Result;

use super::{FakeHub, lock};

/// What [`FakeHub`] keeps of verdicts and reverts.
#[derive(Debug, Default)]
pub(super) struct FakeVerdicts {
    verdicts: Mutex<HashMap<VerdictKey, Verdict>>,
    reverted: Mutex<Vec<Reverted>>,
}

impl FakeHub {
    /// Every verdict stored so far, in no particular order.
    pub fn stored_verdicts(&self) -> Vec<Verdict> {
        lock(&self.verdicts.verdicts).values().cloned().collect()
    }

    /// Every reverted landing reported so far, in order.
    pub fn reverted_landings(&self) -> Vec<Reverted> {
        lock(&self.verdicts.reverted).clone()
    }

    pub(super) fn fake_verdict(&self, key: &VerdictKey) -> Result<Option<Verdict>> {
        self.reachable()?;
        Ok(lock(&self.verdicts.verdicts).get(key).cloned())
    }

    pub(super) fn fake_store_verdict(&self, verdict: &Verdict) -> Result<()> {
        self.reachable()?;
        lock(&self.verdicts.verdicts).insert(verdict.key.clone(), verdict.clone());
        Ok(())
    }

    /// Reopens the landing's ticket, as the real hub does; commits no landing recorded get a
    /// new ticket, numbered by the order of the reports.
    pub(super) fn fake_reverted(&self, reverted: &Reverted) -> Result<RevertFiled> {
        self.reachable()?;
        let mut all = lock(&self.verdicts.reverted);
        all.push(reverted.clone());
        let ticket = reverted.ticket.clone().unwrap_or_else(|| {
            TicketId::new(format!("R.{}", all.len())).unwrap_or_else(|_| unreachable!())
        });
        Ok(RevertFiled { ticket })
    }
}
