//! Reading the old systems' data into the hub's database at the switch. Every importer only
//! reads its source, so a failed switch can be undone.

pub mod compile;
pub mod runlog;
pub mod tickets;

/// What one importer did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub rows: u64,
    pub skipped: u64,
    /// Rows that could not be imported, each with the reason.
    pub problems: Vec<String>,
}
