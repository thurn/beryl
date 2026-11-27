//! Registers a run on the express reserve and lends it extra cores that are free now.

use ignite_core::protocol::Class;

use super::{Grant, GrantRequest, Scheduler};

impl Scheduler {
    /// Reserves the express lane's own cores and lends extra shared or idle tip cores, without
    /// waiting. Release the grant at the end even if it carries zero extra cores.
    pub fn try_borrow(&self, wanted: u32) -> Option<Grant> {
        let mut state = self.lock();
        let request = (wanted > 0).then_some(GrantRequest {
            class: Class::Land,
            cores: wanted,
            resources: Vec::new(),
            memory_mb: None,
            shrinks: false,
            grows: false,
            cores_file: None,
            reserve: None,
            priority_ticket: None,
            urgency: None,
        });
        let id = state.ledger.begin_express(request, self.clock.now());
        self.advance(&mut state);
        let shared = state.ready.remove(&id);
        let extra = wanted.saturating_sub(shared.as_ref().map_or(0, |grant| grant.cores));
        let cores = state.ledger.finish_express(id, extra);
        self.advance(&mut state);
        cores.map(|cores| Grant {
            id,
            cores,
            queued_s: shared.map_or(0, |grant| grant.queued_s),
            resources: Vec::new(),
            cores_file: None,
            priority_ticket: None,
            urgency: None,
        })
    }
}
