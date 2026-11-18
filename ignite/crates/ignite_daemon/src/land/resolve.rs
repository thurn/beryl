//! Settling `pushing` records: landings whose push may or may not have reached the remote.
//!
//! A landing that lost its connection during the push leaves its record `pushing`. The next
//! `ignite land` of the same ticket settles it, but the person may never run one (the session
//! ended, or the ticket was released), and the ticket would stay open although its commit is on
//! the integration branch. The daemon's sender settles them too: a record whose commit is in the
//! history of the remote tip becomes `pushed` and is told to the hub; one whose commit is not,
//! and that is old enough that its push is not still in flight, is dropped.

use std::time::Duration;

use ignite_core::time::Timestamp;

use super::Landing;
use super::records::{LandingRecord, RecordState};
use crate::server::Context;

/// How old a `pushing` record must be before the sender looks at it, so that a landing whose
/// push is still in flight finishes by itself.
pub(super) const SETTLE_AFTER: Duration = Duration::from_secs(60);
/// How old a `pushing` record must be before it is dropped when its commit is not on the remote.
pub(super) const DROP_AFTER: Duration = Duration::from_mins(10);

/// How many seconds ago the record was written; a record with no time counts as very old.
fn age_s(record: &LandingRecord, now: Timestamp) -> f64 {
    record
        .written_at
        .map_or(f64::INFINITY, |at| now.seconds_since(at))
}

impl Landing {
    /// Settles every `pushing` record the sender can, as described in the module's header.
    pub(super) fn resolve_pushing(&self, ctx: &Context) {
        let records = match self.records.all() {
            Ok(records) => records,
            Err(error) => {
                eprintln!("ignite: landing records not read: {error}");
                return;
            }
        };
        let now = ctx.clock.now();
        let mut tip = None;
        for mut record in records {
            if record.state != RecordState::Pushing
                || age_s(&record, now) < SETTLE_AFTER.as_secs_f64()
            {
                continue;
            }
            if tip.is_none() {
                match ctx.git.tip(&ctx.config.repository.staging) {
                    Ok(commit) => tip = Some(commit),
                    // Without the tip, nothing can be said about any record.
                    Err(_) => return,
                }
            }
            let Some(tip) = &tip else { return };
            match self.trees.is_ancestor(&ctx.repo, &record.commit, tip) {
                Ok(true) => {
                    record.state = RecordState::Pushed;
                    if let Err(error) = self.records.write(&record) {
                        eprintln!(
                            "ignite: landing of {} not settled: {error}",
                            record.landed.ticket
                        );
                    }
                }
                Ok(false) if age_s(&record, now) >= DROP_AFTER.as_secs_f64() => {
                    eprintln!(
                        "ignite: dropping the landing record of {}: its commit never reached {}",
                        record.landed.ticket, ctx.config.repository.staging
                    );
                    if let Err(error) = self
                        .records
                        .remove_if(&record.landed.ticket, &record.commit)
                    {
                        eprintln!("ignite: record not dropped: {error}");
                    }
                }
                Ok(false) => {}
                Err(error) => eprintln!(
                    "ignite: landing of {} not settled: {error}",
                    record.landed.ticket
                ),
            }
        }
    }
}
