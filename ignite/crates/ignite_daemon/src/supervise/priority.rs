//! Finds the ticket a command is doing work for before it enters the core queue, and from it
//! the command's place in the queue: agent priority for a ticket that holds master back, and
//! [`Urgency`] for an emergency's ticket or a rushed session (see [`crate::rush`]).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ignite_core::dispatch::JUSTICIAR;
use ignite_core::ids::TicketId;
use ignite_core::protocol::Class;
use ignite_core::protocol::tickets::{Status, Ticket, WholeTicket};
use ignite_core::traits::{Clock, CommandSpec, Hub};
use ignite_hub::upkeep::filing::SIGNATURE_KEY;
use ignite_hub::upkeep::justiciar::MASTER_LAG_SUBJECT;
use ignite_hub::upkeep::signature::Signature;
use ignite_store::{local::trees, mirror};
use rusqlite::{Connection, OpenFlags};

use super::SupervisedLauncher;
use crate::handles::lock;
use crate::sched::Urgency;

pub(super) struct AgentPriority {
    trees: Mutex<Connection>,
    mirror_path: PathBuf,
    mirror: Mutex<Option<Connection>>,
    hub: Arc<dyn Hub>,
    clock: Arc<dyn Clock>,
    master_lag_signature: String,
}

/// The ticket that held master back, if any, and the urgency, for one command.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Placing {
    pub(super) priority_ticket: Option<TicketId>,
    pub(super) urgency: Option<Urgency>,
}

/// The labels of the tickets whose commands go ahead of every class but tip builds.
const EMERGENCY_LABELS: [&str; 2] = ["incident", JUSTICIAR];

impl SupervisedLauncher {
    /// Looks up the ticket a command names or the ticket that leases its tree. Read the local
    /// mirror first so a command can usually enter the queue without a hub round trip.
    #[must_use]
    pub fn with_agent_priority(
        mut self,
        trees: Connection,
        mirror_path: PathBuf,
        hub: Arc<dyn Hub>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        self.agent_priority = Some(AgentPriority {
            trees: Mutex::new(trees),
            mirror_path,
            mirror: Mutex::new(None),
            hub,
            clock,
            master_lag_signature: Signature::of_subject(MASTER_LAG_SUBJECT, JUSTICIAR).key(),
        });
        self
    }
}

impl AgentPriority {
    /// Where `spec`'s command goes in the core queue. Only `agent` and `land` commands, the
    /// work of a ticket's own session and its landing, are placed: tip rounds, experiments and
    /// idle work keep their class. An emergency ranks above a rush, so a rushed command still
    /// reads its ticket's labels.
    pub(super) fn place(&self, spec: &CommandSpec) -> Placing {
        if !matches!(spec.class, Class::Agent | Class::Land) {
            return Placing::default();
        }
        let row = spec
            .tree
            .as_ref()
            .and_then(|tree| trees::get(&lock(&self.trees), tree.as_str()).ok().flatten());
        let Some(id) = spec
            .ticket
            .clone()
            .or_else(|| TicketId::new(row.as_ref()?.ticket.clone()?).ok())
        else {
            return Placing::default();
        };
        let session = row.as_ref().and_then(|row| row.session.as_deref());
        let rushed = crate::rush::rushed(&lock(&self.trees), self.clock.now(), Some(&id), session);
        let ticket = self
            .mirrored(&id)
            .or_else(|| self.hub.ticket(&id).ok().map(|detail| detail.ticket))
            .filter(|ticket| ticket.status != Status::Closed);
        let emergency = ticket.as_ref().is_some_and(|ticket| {
            ticket
                .labels
                .iter()
                .any(|label| EMERGENCY_LABELS.contains(&label.as_str()))
        });
        let holds = spec.class == Class::Agent
            && ticket
                .as_ref()
                .is_some_and(|ticket| holds_master(ticket, &self.master_lag_signature));
        Placing {
            priority_ticket: holds.then_some(id),
            urgency: if emergency {
                Some(Urgency::Emergency)
            } else if rushed {
                Some(Urgency::Rush)
            } else {
                None
            },
        }
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
    use ignite_core::time::Timestamp;
    use serde_json::json;

    /// A launcher whose local database leases `tree-01` to `FIX`, and the spec of an agent
    /// command in that tree.
    fn launcher(hub: &Arc<FakeHub>) -> (SupervisedLauncher, CommandSpec) {
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
        let clock = Arc::new(ignite_core::fakes::FakeClock::new(Timestamp::EPOCH));
        let launcher = SupervisedLauncher::with_command(Vec::new(), PathBuf::new())
            .with_agent_priority(db, PathBuf::new(), hub.clone(), clock);
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
        (launcher, spec)
    }

    fn fix() -> TicketId {
        TicketId::new("FIX").unwrap()
    }

    fn ticket(
        labels: &[&str],
        metadata: &serde_json::Value,
    ) -> ignite_core::protocol::tickets::TicketDetail {
        serde_json::from_value(json!({
            "id": fix(), "parent": null, "next_child": 1, "title": "Fix",
            "type": "task", "status": "in_progress", "priority": 0, "assignee": null,
            "labels": labels, "metadata": metadata, "defer_until": null, "close_reason": null,
            "created_at": "2026-11-21T00:00:00.000Z", "created_by": "test",
            "updated_at": "2026-11-21T00:00:00.000Z", "closed_at": null, "version": 1,
            "edges": [], "children": [], "ready": false, "blocked_by": [],
        }))
        .unwrap()
    }

    fn placing(priority_ticket: Option<TicketId>, urgency: Option<Urgency>) -> Placing {
        Placing {
            priority_ticket,
            urgency,
        }
    }

    #[test]
    fn a_leased_tree_supplies_the_ticket_and_only_master_blockers_get_priority() {
        let hub = Arc::new(FakeHub::default());
        let (launcher, spec) = launcher(&hub);
        let priority = launcher.agent_priority.as_ref().unwrap();
        hub.insert_ticket(ticket(&[], &json!({})));
        assert_eq!(priority.place(&spec), placing(None, None));
        hub.insert_ticket(ticket(&["regression"], &json!({})));
        assert_eq!(priority.place(&spec), placing(Some(fix()), None));
        // Every justiciar ticket is an emergency; only a master-lag one holds master back.
        let emergency = Some(Urgency::Emergency);
        hub.insert_ticket(ticket(&[JUSTICIAR], &json!({})));
        assert_eq!(priority.place(&spec), placing(None, emergency));
        let master_lag = json!({SIGNATURE_KEY: priority.master_lag_signature});
        hub.insert_ticket(ticket(&[JUSTICIAR], &master_lag));
        assert_eq!(priority.place(&spec), placing(Some(fix()), emergency));
        hub.insert_ticket(ticket(&["incident"], &json!({})));
        assert_eq!(priority.place(&spec), placing(None, emergency));
        // Tip rounds keep their class whatever their ticket.
        let tip = CommandSpec {
            class: Class::Tip,
            ..spec
        };
        assert_eq!(priority.place(&tip), placing(None, None));
    }

    /// A rush on the tree's ticket places its commands, and its landing's, as a rush.
    #[test]
    fn a_rushed_tickets_commands_and_landing_are_rushed() {
        let hub = Arc::new(FakeHub::default());
        let (launcher, spec) = launcher(&hub);
        let priority = launcher.agent_priority.as_ref().unwrap();
        hub.insert_ticket(ticket(&[], &json!({})));
        let session = ignite_core::ids::SessionId::new("local_r").unwrap();
        crate::rush::start(
            &lock(&priority.trees),
            Timestamp::EPOCH,
            &session,
            Some(fix()),
        )
        .unwrap();
        assert_eq!(priority.place(&spec), placing(None, Some(Urgency::Rush)));
        let landing = CommandSpec {
            class: Class::Land,
            tree: None,
            ticket: Some(fix()),
            ..spec
        };
        assert_eq!(priority.place(&landing), placing(None, Some(Urgency::Rush)));
    }
}
