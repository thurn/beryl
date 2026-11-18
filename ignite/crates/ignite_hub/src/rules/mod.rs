//! Improvement rules: queries over the records that find a source of waste, name a subject and
//! estimate a cost and a saving per week. The hub files a ticket for each finding that clears the
//! minimum and has no ticket closed for its subject within 30 days.
//!
//! Stub. The ticket Thrh4.21 (Improvement rules) owns this folder
//! and adds a module per rule.

use rusqlite::Connection;

use ignite_core::time::Timestamp;

/// What one rule found.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub rule: String,
    pub subject: String,
    /// `dollars`, `agent-hours` or `core-hours`.
    pub unit: String,
    /// The subject's cost per week now.
    pub cost: f64,
    /// The estimated saving per week.
    pub saving: f64,
    /// The ticket's title and body.
    pub title: String,
    pub body: String,
}

pub trait Rule: Send + Sync {
    fn name(&self) -> &str;
    /// A report-only rule shows on the dashboard and files nothing.
    fn report_only(&self) -> bool;
    fn find(&self, conn: &Connection, now: Timestamp) -> rusqlite::Result<Vec<Finding>>;
}

/// Every rule.
pub fn all() -> Vec<Box<dyn Rule>> {
    Vec::new()
}
