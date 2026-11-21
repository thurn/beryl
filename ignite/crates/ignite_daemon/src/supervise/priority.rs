//! Finds the ticket an agent command is doing work for before it enters the core queue.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ignite_core::dispatch::JUSTICIAR;
use ignite_core::ids::TicketId;
use ignite_core::protocol::Class;
use ignite_core::protocol::tickets::{Status, Ticket, WholeTicket};
use ignite_core::traits::{CommandSpec, Hub};
use ignite_hub::upkeep::filing::SIGNATURE_KEY;
use ignite_hub::upkeep::justiciar::MASTER_LAG_SUBJECT;
use ignite_hub::upkeep::signature::Signature;
use ignite_store::{local::trees, mirror};
use rusqlite::{Connection, OpenFlags};

use super::SupervisedLauncher;
use crate::handles::lock;

pub(super) struct AgentPriority {
    trees: Mutex<Connection>,
    mirror_path: PathBuf,
    mirror: Mutex<Option<Connection>>,
    hub: Arc<dyn Hub>,
    master_lag_signature: String,
}

impl SupervisedLauncher {
    /// Looks up the ticket a command names or the ticket that leases its tree. Read the local
    /// mirror first so a command can usually enter the queue without a hub round trip.
    #[must_use]
    pub fn with_agent_priority(
        mut self,
        trees: Connection,
        mirror_path: PathBuf,
        hub: Arc<dyn Hub>,
    ) -> Self {
        self.agent_priority = Some(AgentPriority {
            trees: Mutex::new(trees),
            mirror_path,
            mirror: Mutex::new(None),
            hub,
            master_lag_signature: Signature::of_subject(MASTER_LAG_SUBJECT, JUSTICIAR).key(),
        });
        self
    }
}

impl AgentPriority {
    pub(super) fn ticket(&self, spec: &CommandSpec) -> Option<TicketId> {
        if spec.class != Class::Agent {
            return None;
        }
        let id = spec.ticket.clone().or_else(|| {
            let row = trees::get(&lock(&self.trees), spec.tree.as_ref()?.as_str()).ok()??;
            TicketId::new(row.ticket?).ok()
        })?;
        let ticket = self
            .mirrored(&id)
            .or_else(|| self.hub.ticket(&id).ok().map(|detail| detail.ticket))?;
        (ticket.status != Status::Closed && holds_master(&ticket, &self.master_lag_signature))
            .then_some(id)
    }

    fn mirrored(&self, id: &TicketId) -> Option<Ticket> {
        let mut mirror = lock(&self.mirror);
        if mirror.is_none() {
            *mirror =
                Connection::open_with_flags(&self.mirror_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .ok();
        }
        let row = mirror::latest(mirror.as_ref()?, id.as_str()).ok()??;
        let whole: WholeTicket = serde_json::from_str(&row.whole_json).ok()?;
        Some(whole.ticket)
    }
}

fn holds_master(ticket: &Ticket, master_lag_signature: &str) -> bool {
    ticket.labels.iter().any(|label| label == "regression")
        || (ticket.labels.iter().any(|label| label == JUSTICIAR)
            && ticket
                .metadata
                .get(SIGNATURE_KEY)
                .and_then(serde_json::Value::as_str)
                == Some(master_lag_signature))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::fakes::FakeHub;
    use ignite_core::ids::TreeId;
    use serde_json::json;

    #[test]
    fn a_leased_tree_supplies_the_ticket_and_only_master_blockers_get_priority() {
        let db = ignite_store::local::open_in_memory().unwrap();
        trees::upsert(
            &db,
            &ignite_store::local::rows::Tree {
                id: "tree-01".to_owned(),
                path: "/trees/tree-01".to_owned(),
                kind: "ticket".to_owned(),
                state: "leased".to_owned(),
                ticket: Some("FIX".to_owned()),
                session: None,
                branch: None,
                built_commit: None,
                leased_at: None,
                last_used: None,
                owner: None,
            },
        )
        .unwrap();
        let hub = Arc::new(FakeHub::default());
        let launcher = SupervisedLauncher::with_command(Vec::new(), PathBuf::new())
            .with_agent_priority(db, PathBuf::new(), hub.clone());
        let priority = launcher.agent_priority.as_ref().unwrap();
        let spec = CommandSpec {
            args: vec!["test".to_owned()],
            cwd: PathBuf::from("/trees/tree-01"),
            env: Vec::new(),
            class: Class::Agent,
            cores: 4,
            resources: Vec::new(),
            grows: false,
            tree: Some(TreeId::new("tree-01").unwrap()),
            ticket: None,
            hang_limit_s: None,
            reserve: None,
        };
        let id = TicketId::new("FIX").unwrap();
        let ticket = |labels: &[&str], metadata| {
            serde_json::from_value(json!({
                "id": id, "parent": null, "next_child": 1, "title": "Fix",
                "type": "task", "status": "in_progress", "priority": 0, "assignee": null,
                "labels": labels, "metadata": metadata, "defer_until": null, "close_reason": null,
                "created_at": "2026-11-21T00:00:00.000Z", "created_by": "test",
                "updated_at": "2026-11-21T00:00:00.000Z", "closed_at": null, "version": 1,
                "edges": [], "children": [], "ready": false, "blocked_by": [],
            }))
            .unwrap()
        };
        hub.insert_ticket(ticket(&["regression"], json!({})));
        assert_eq!(priority.ticket(&spec), Some(id.clone()));
        hub.insert_ticket(ticket(&[JUSTICIAR], json!({})));
        assert_eq!(priority.ticket(&spec), None);
        hub.insert_ticket(ticket(
            &[JUSTICIAR],
            json!({SIGNATURE_KEY: priority.master_lag_signature}),
        ));
        assert_eq!(priority.ticket(&spec), Some(id));
    }
}
