//! The ticket routes. They keep the ticket service's contract, so the shapes here match its
//! JSON: `GET /tickets`, `GET /tickets/<id>`, `GET /graph`, `POST /batch`, `POST /leases/<name>`,
//! `GET /leases`, `GET /export` and `POST /import`. The change feed is a long poll,
//! `GET /feed?since=<seq>&wait=<seconds>`, instead of a WebSocket, and events are gone.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::ids::{Commit, MachineId, Seq, SessionId, TicketId, TreeId};
use crate::time::Timestamp;

/// A ticket's type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TicketType {
    #[default]
    Task,
    Epic,
    Bug,
    Feature,
    Chore,
    Decision,
    Spike,
    Story,
    Milestone,
}

/// A ticket's status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Open,
    InProgress,
    Blocked,
    Deferred,
    Closed,
}

/// What one ticket landed, which the next ticket's `ignite start` prints.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandedWork {
    pub commits: Vec<Commit>,
    pub files: Vec<String>,
    pub note: String,
}

/// An improvement ticket's estimated saving.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saving {
    /// The rule that filed the ticket.
    pub rule: String,
    pub subject: String,
    /// `dollars`, `agent-hours` or `core-hours`, per week.
    pub unit: String,
    pub amount: f64,
}

/// The fields a client may set, on create or update. Every field is optional on update.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TicketFields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub parent: Option<Option<TicketId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub ticket_type: Option<TicketType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub assignee: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub defer_until: Option<Option<Timestamp>>,
    /// The epic's target number of workers.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub workers: Option<Option<u32>>,
    /// A hard cap on the epic's workers.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_workers: Option<Option<u32>>,
    /// A planner's figure for the cores the ticket will use.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub cores: Option<Option<f64>>,
}

/// Reads a field that is present, so that `null` means "clear it" and absence (through
/// `#[serde(default)]`) means "leave it". The ticket service's update has exactly these three
/// cases, so the nested option is the wire format, not an accident.
#[allow(clippy::option_option)]
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Reads a field that must be present but may be `null`.
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
}

/// A ticket as the hub stores it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ticket {
    pub id: TicketId,
    pub parent: Option<TicketId>,
    pub next_child: u32,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub acceptance: String,
    #[serde(default)]
    pub design: String,
    #[serde(default)]
    pub notes: String,
    #[serde(rename = "type")]
    pub ticket_type: TicketType,
    pub status: Status,
    pub priority: u8,
    pub assignee: Option<String>,
    #[serde(default)]
    pub last: u8,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
    pub defer_until: Option<Timestamp>,
    pub close_reason: Option<String>,
    pub created_at: Timestamp,
    pub created_by: String,
    pub updated_at: Timestamp,
    pub closed_at: Option<Timestamp>,
    /// The feed position of the last change.
    pub version: Seq,
    #[serde(default)]
    pub workers: Option<u32>,
    #[serde(default)]
    pub max_workers: Option<u32>,
    #[serde(default)]
    pub cores: Option<f64>,
    #[serde(default)]
    pub machine: Option<MachineId>,
    #[serde(default)]
    pub session: Option<SessionId>,
    #[serde(default)]
    pub tree: Option<TreeId>,
    #[serde(default)]
    pub claimed_at: Option<Timestamp>,
    #[serde(default)]
    pub landed_at: Option<Timestamp>,
    #[serde(default)]
    pub landed: Option<LandedWork>,
    #[serde(default)]
    pub saving: Option<Saving>,
}

/// A dependency edge's kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeKind {
    #[default]
    Blocks,
    ConditionalBlocks,
    WaitsFor,
    Related,
    DiscoveredFrom,
}

/// `src` depends on `dst`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub src: TicketId,
    pub dst: TicketId,
    #[serde(default)]
    pub kind: EdgeKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub ticket: TicketId,
    pub n: u32,
    pub author: String,
    pub at: Timestamp,
    pub text: String,
    #[serde(default)]
    pub legacy_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct History {
    pub ticket: TicketId,
    pub seq: Seq,
    pub actor: String,
    pub at: Timestamp,
    /// `{field: [old, new], ...}`, or an imported event array.
    pub change: serde_json::Value,
}

/// Why a ticket is not ready, by the readiness rule that holds it: 1 the ticket itself is not
/// open or is deferred, 2 its own dependency, 3 an ancestor's dependency, 4 a sibling that must
/// finish before a `last` ticket, 5 a live child. See `crate::ready`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blocker {
    pub id: TicketId,
    pub rule: u8,
}

/// `GET /tickets/<id>`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TicketDetail {
    #[serde(flatten)]
    pub ticket: Ticket,
    /// Edges in both directions.
    pub edges: Vec<Edge>,
    pub children: Vec<TicketId>,
    pub ready: bool,
    pub blocked_by: Vec<Blocker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comments: Option<Vec<Comment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<History>>,
}

/// The query of `GET /tickets`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketQuery {
    pub status: Option<Status>,
    #[serde(rename = "type")]
    pub ticket_type: Option<TicketType>,
    pub parent: Option<TicketId>,
    pub ancestor: Option<TicketId>,
    #[serde(default)]
    pub label: Vec<String>,
    #[serde(default)]
    pub label_any: Vec<String>,
    pub search: Option<String>,
    pub assignee: Option<String>,
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub all: bool,
    pub limit: Option<u32>,
}

/// A ticket in `GET /graph`, without its long text fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphTicket {
    #[serde(flatten)]
    pub ticket: Ticket,
    pub ready: bool,
    pub blocked_by: Vec<Blocker>,
}

/// `GET /graph`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub tickets: Vec<GraphTicket>,
    pub edges: Vec<Edge>,
    pub seq: Seq,
}

/// A condition every operation of a batch depends on. Each is an object of exactly the keys
/// shown, as in the ticket service.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Precondition {
    Version {
        id: TicketId,
        version: Seq,
    },
    Assignee {
        id: TicketId,
        #[serde(deserialize_with = "nullable")]
        assignee: Option<String>,
    },
    Status {
        id: TicketId,
        status: Status,
    },
    Seq {
        seq: Seq,
    },
}

/// A dependency named on create: the ticket the new one depends on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    /// A ticket ID, or `$<index>` for a ticket created earlier in the same batch.
    pub on: String,
    #[serde(default)]
    pub kind: Option<EdgeKind>,
}

/// One operation of a batch. `id` may be `$<index>`, a ticket created earlier in the batch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    Create {
        fields: TicketFields,
        #[serde(default)]
        parent: Option<String>,
        #[serde(default)]
        deps: Vec<Dependency>,
        #[serde(default)]
        request_key: Option<String>,
    },
    Update {
        id: String,
        #[serde(default)]
        set: Option<TicketFields>,
        #[serde(default)]
        add_labels: Vec<String>,
        #[serde(default)]
        remove_labels: Vec<String>,
        #[serde(default)]
        set_meta: BTreeMap<String, serde_json::Value>,
        #[serde(default)]
        unset_meta: Vec<String>,
    },
    Claim {
        id: String,
    },
    Close {
        id: String,
        reason: String,
        #[serde(default)]
        include_ready: bool,
    },
    Reopen {
        id: String,
    },
    Defer {
        id: String,
        until: Timestamp,
    },
    Comment {
        id: String,
        text: String,
    },
    DepAdd {
        id: String,
        on: String,
        #[serde(default)]
        kind: Option<EdgeKind>,
    },
    DepRemove {
        id: String,
        on: String,
        #[serde(default)]
        kind: Option<EdgeKind>,
    },
}

/// `POST /batch`: every operation succeeds or none does.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BatchRequest {
    #[serde(rename = "if", default)]
    pub preconditions: Vec<Precondition>,
    pub ops: Vec<Operation>,
}

/// One operation's result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationResult {
    pub id: TicketId,
    /// Ancestors closed automatically by a close.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub closed: Vec<TicketId>,
    /// Ready tickets, for a close with `include_ready`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready: Option<Vec<TicketId>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchReply {
    pub seq: Seq,
    pub results: Vec<OperationResult>,
}

/// A 409 from `POST /batch`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BatchConflict {
    /// The zero-based index of the precondition or operation that failed.
    pub failed: usize,
    pub current: Option<Ticket>,
}

/// One session waiting for a lease, in arrival order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseWaiter {
    pub holder: String,
    #[serde(default)]
    pub machine: Option<String>,
    pub since: Timestamp,
}

/// A named, expiring lease.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lease {
    pub name: String,
    pub holder: String,
    pub machine: String,
    pub expires_at: Timestamp,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default)]
    pub waiters: Vec<LeaseWaiter>,
}

/// The body of `POST /leases/<name>`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum LeaseOp {
    Acquire {
        holder: String,
        ttl_s: u64,
        #[serde(default)]
        payload: Option<serde_json::Value>,
        #[serde(default)]
        no_queue: bool,
    },
    Renew {
        holder: String,
        ttl_s: u64,
        #[serde(default)]
        payload: Option<serde_json::Value>,
        #[serde(default)]
        if_revision: Option<String>,
    },
    Release {
        holder: String,
        #[serde(default)]
        if_revision: Option<String>,
    },
}

/// Who holds a lease that could not be taken.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseBusy {
    pub holder: Option<String>,
    pub expires_at: Option<Timestamp>,
    #[serde(default)]
    pub position: Option<u32>,
    #[serde(default)]
    pub seq: Option<Seq>,
}

/// The answer to a lease operation: the lease (200) or who holds it (409).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeaseReply {
    Held(Lease),
    Busy(LeaseBusy),
}

/// One change in the feed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedChange {
    pub seq: Seq,
    /// `ticket`, `lease`, `machine`, `result` or another table's name.
    pub kind: String,
    #[serde(default)]
    pub ticket: Option<TicketId>,
    #[serde(default)]
    pub name: Option<String>,
}

/// `GET /feed?since=<seq>&wait=<seconds>`: empty when the wait ended with nothing new.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedBatch {
    /// The newest position the hub knows.
    pub seq: Seq,
    pub changes: Vec<FeedChange>,
}

/// A ticket with everything attached, as `GET /export` writes and `POST /import` reads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WholeTicket {
    #[serde(flatten)]
    pub ticket: Ticket,
    /// Outgoing edges.
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub history: Vec<History>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// `POST /import`: up to 200 tickets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportRequest {
    pub tickets: Vec<WholeTicket>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReply {
    pub seq: Seq,
    pub imported: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_matches_the_ticket_service_json() {
        let batch: BatchRequest = serde_json::from_str(
            r#"{"if": [{"id": "a.1", "version": 7}, {"seq": 9}, {"id": "a.1", "status": "open"}],
                "ops": [
                  {"op": "create", "fields": {"title": "Split", "priority": 2}, "parent": "a",
                   "deps": [{"on": "a.1"}]},
                  {"op": "dep_add", "id": "$0", "on": "a.2", "kind": "waits-for"},
                  {"op": "close", "id": "a.1", "reason": "done"}
                ]}"#,
        )
        .unwrap();
        assert_eq!(
            batch.preconditions[0],
            Precondition::Version {
                id: "a.1".parse().unwrap(),
                version: Seq(7)
            }
        );
        assert_eq!(batch.preconditions[1], Precondition::Seq { seq: Seq(9) });
        assert!(matches!(
            batch.preconditions[2],
            Precondition::Status { .. }
        ));
        assert!(matches!(
            &batch.ops[1],
            Operation::DepAdd {
                kind: Some(EdgeKind::WaitsFor),
                ..
            }
        ));
        let text = serde_json::to_string(&batch.ops[2]).unwrap();
        assert!(text.contains(r#""op":"close""#), "{text}");
    }

    #[test]
    fn lease_ops_are_tagged_by_op() {
        let op: LeaseOp =
            serde_json::from_str(r#"{"op": "acquire", "holder": "s1", "ttl_s": 60}"#).unwrap();
        assert!(matches!(
            op,
            LeaseOp::Acquire {
                no_queue: false,
                ..
            }
        ));
    }

    #[test]
    fn update_fields_distinguish_absent_from_null() {
        let fields: TicketFields =
            serde_json::from_str(r#"{"assignee": null, "title": "x"}"#).unwrap();
        assert_eq!(fields.assignee, Some(None));
        assert_eq!(fields.parent, None);
        let back = serde_json::to_value(&fields).unwrap();
        assert_eq!(back, serde_json::json!({"title": "x", "assignee": null}));
    }
}
