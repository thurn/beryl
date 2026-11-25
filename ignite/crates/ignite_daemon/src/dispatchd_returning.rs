//! What the daemon does with the hub's answers about tickets it lost while this machine was
//! away (see `ignite_core::protocol::returning`).
//!
//! When the hub releases a silent machine's claims, the machine's CLI workers keep their trees
//! and, after a reboot, their slots. Each heartbeat lists the ticket each worker holds a tree
//! for, and the reply says, for each ticket the hub released from that worker, whether the hub
//! gave it back or someone else took, closed or changed it meanwhile. A ticket given back needs
//! nothing here: the worker's slot and tree never changed. For a lost ticket, the daemon saves
//! the tree's work on the ticket's saved branch and frees the tree, ends the worker's turn and
//! frees its slot, and says on the ticket where the work went.

use std::collections::BTreeMap;

use ignite_core::protocol::returning::{RELEASED_OFFLINE_KEY, Reconciled, ReturnOutcome};
use ignite_core::protocol::tickets::{BatchRequest, Operation};

use super::HeadlessDispatch;
use crate::server::Context;

impl HeadlessDispatch {
    /// Acts on the `reconciled` list of a heartbeat's reply. A lost ticket whose tree cannot be
    /// saved is left as it is, and the next heartbeat reports it again.
    pub fn reconciled(&self, ctx: &Context, answers: &[Reconciled]) {
        for answer in answers {
            let (session, ticket) = (&answer.session, &answer.ticket);
            if answer.outcome == ReturnOutcome::Reclaimed {
                eprintln!(
                    "ignite: the hub gave {ticket} back to {session} after this machine was away"
                );
                continue;
            }
            let branch = match self.host.free_lost(session, ticket) {
                Ok(branch) => branch,
                Err(error) => {
                    eprintln!(
                        "ignite: {session} lost {ticket}, but its tree was not saved: {error}"
                    );
                    continue;
                }
            };
            let retired = self
                .cli
                .get()
                .map_or(Ok(false), |cli| cli.pool.retire(session));
            if let Err(error) = retired {
                eprintln!("ignite: {session} lost {ticket}, but its slot was not freed: {error}");
            }
            self.state().held.remove(session);
            let saved = branch.map_or_else(
                || "It held no tree here, so there was no work to save.".to_owned(),
                |branch| format!("Its work is saved as {branch}."),
            );
            let text = format!(
                "Machine {} reported again after the hub released its claims for silence, but \
                 this ticket had been taken, closed or changed meanwhile, so the returning \
                 worker {session} stopped and freed its tree. {saved}",
                ctx.machine
            );
            let request = BatchRequest {
                preconditions: Vec::new(),
                ops: vec![
                    Operation::Comment {
                        id: ticket.to_string(),
                        text,
                    },
                    Operation::Update {
                        id: ticket.to_string(),
                        set: None,
                        add_labels: Vec::new(),
                        remove_labels: Vec::new(),
                        set_meta: BTreeMap::new(),
                        unset_meta: vec![RELEASED_OFFLINE_KEY.to_owned()],
                    },
                ],
            };
            if let Err(error) = ctx.hub.batch(&request) {
                eprintln!("ignite: the loss of {ticket} by {session} was not recorded: {error}");
            }
        }
        self.save();
    }
}
