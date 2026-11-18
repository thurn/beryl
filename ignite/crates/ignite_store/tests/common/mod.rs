//! A hub database with helpers that speak the ticket service's JSON, so the ported tests read
//! like the ones they came from.
#![allow(dead_code)]

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::protocol::tickets::{
    BatchConflict, BatchReply, BatchRequest, Blocker, Ticket, TicketDetail, TicketQuery,
};
use ignite_core::time::Timestamp;
use ignite_store::StoreError;
use ignite_store::hub::{self, batch, tickets};

pub const NOW: &str = "2026-11-17T12:00:00.000Z";

pub struct Hub {
    pub conn: Connection,
    pub now: Timestamp,
}

impl Hub {
    pub fn new() -> Self {
        Hub {
            conn: hub::open_in_memory().unwrap(),
            now: Timestamp::parse(NOW).unwrap(),
        }
    }

    /// Applies a batch given as the ticket service's JSON, as `actor`.
    pub fn post_as(
        &mut self,
        actor: &str,
        ops: Value,
        conditions: Value,
    ) -> Result<batch::BatchOutcome, StoreError> {
        let request: BatchRequest =
            serde_json::from_value(json!({"if": conditions, "ops": ops})).unwrap();
        batch::apply(&mut self.conn, &request, actor, "m-test", self.now)
    }

    pub fn post(&mut self, ops: Value) -> Result<batch::BatchOutcome, StoreError> {
        self.post_as("worker", ops, json!([]))
    }

    /// A batch that must apply.
    pub fn batch(&mut self, ops: Value) -> BatchReply {
        self.batch_if(ops, json!([]))
    }

    pub fn batch_if(&mut self, ops: Value, conditions: Value) -> BatchReply {
        match self.post_as("worker", ops, conditions).unwrap() {
            batch::BatchOutcome::Applied(reply) => reply,
            batch::BatchOutcome::Conflict(conflict) => panic!("conflict: {conflict:?}"),
        }
    }

    /// A batch that must conflict.
    pub fn conflict(&mut self, ops: Value, conditions: Value) -> BatchConflict {
        match self.post_as("worker", ops, conditions).unwrap() {
            batch::BatchOutcome::Applied(reply) => panic!("applied: {reply:?}"),
            batch::BatchOutcome::Conflict(conflict) => *conflict,
        }
    }

    /// The IDs a batch's operations returned.
    pub fn ids(&mut self, ops: Value) -> Vec<String> {
        self.batch(ops)
            .results
            .into_iter()
            .map(|r| r.id.to_string())
            .collect()
    }

    pub fn show(&self, id: &str) -> TicketDetail {
        tickets::get(&self.conn, id, true, true, self.now)
            .unwrap()
            .unwrap_or_else(|| panic!("no ticket {id}"))
    }

    pub fn blocked_by(&self, id: &str) -> Vec<(String, u8)> {
        pairs(&self.show(id).blocked_by)
    }

    pub fn list(&self, query: Value) -> Vec<String> {
        self.tickets(query)
            .into_iter()
            .map(|t| t.id.to_string())
            .collect()
    }

    pub fn tickets(&self, query: Value) -> Vec<Ticket> {
        let query: TicketQuery = serde_json::from_value(query).unwrap();
        tickets::list(&self.conn, &query, self.now).unwrap()
    }
}

pub fn pairs(blockers: &[Blocker]) -> Vec<(String, u8)> {
    blockers
        .iter()
        .map(|b| (b.id.to_string(), b.rule))
        .collect()
}

pub fn pair(id: &str, rule: u8) -> (String, u8) {
    (id.to_owned(), rule)
}

/// A create operation, as the ticket service's tests wrote it.
pub fn create(title: &str, parent: Option<&str>, fields: Value) -> Value {
    let mut fields = fields;
    fields["title"] = json!(title);
    json!({"op": "create", "fields": fields, "parent": parent})
}

pub fn create_with(title: &str, parent: Option<&str>, fields: Value, extra: Value) -> Value {
    let mut op = create(title, parent, fields);
    for (key, value) in extra.as_object().unwrap() {
        op[key] = value.clone();
    }
    op
}
