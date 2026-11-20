//! Keeping the hub's attachment files within their budget (`[hub] attachment_budget_mb`). Each
//! hour, upkeep:
//!
//! 1. removes the file of every attachment whose ticket closed more than
//!    [`KEEP_CLOSED_DAYS`] days ago;
//! 2. if the kept files still total more than the budget, removes the files of closed tickets,
//!    the ticket closed longest ago first, until the total is under 90% of the budget;
//! 3. never removes an open ticket's attachment, and keeps on disk a file that any kept
//!    attachment still names, whichever ticket it belongs to;
//! 4. deletes each file in the directory that no kept attachment names once it is an hour old:
//!    a removed attachment's file, or one left by an upload that failed before its row was
//!    written.
//!
//! When the files of open tickets alone are over the budget, uploads are refused with 507 and
//! upkeep files one ticket saying attachment storage is full ([`CATEGORY`]). The ticket records
//! the budget it was filed under, and `clearing` closes it once the kept files are back under
//! that budget.
//!
//! Attachments are evidence for working a ticket, not a record to keep forever, so a removed
//! file only leaves its row, which says the image was removed and when.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde_json::Value;

use ignite_core::protocol::attachments::size_text;
use ignite_core::protocol::tickets::{Ticket, TicketFields, TicketType};
use ignite_core::time::Timestamp;
use ignite_store::hub::attachments;
use ignite_store::{Result, StoreError};

use super::clearing_kinds::Verdict;
use super::filing::{self, Filed, Filing, UPKEEP};
use super::signature::Signature;
use crate::attachments::AttachmentFiles;

/// How long after its ticket closes an attachment's file is kept.
pub const KEEP_CLOSED_DAYS: f64 = 90.0;

/// How old a file that no kept attachment names must be before it is deleted, so an upload in
/// flight, which writes its file before its row, is never caught between the two.
pub const UNNAMED_SECONDS: f64 = 3_600.0;

/// How often upkeep tidies the files.
pub const TIDY_SECONDS: f64 = 3_600.0;

/// The category of the ticket upkeep files when attachment storage is full.
pub const CATEGORY: &str = "attachments-full";

/// The metadata key of that ticket holding the budget, in bytes, it was filed under.
pub const BUDGET_KEY: &str = "ignite_attachment_budget";

/// What one tidy did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tidied {
    /// Tickets whose attachments' files were removed.
    pub removed: Vec<String>,
    /// Files deleted from disk.
    pub deleted: Vec<String>,
    /// The storage-full ticket, when the files of open tickets alone are over the budget.
    pub filed: Option<Filed>,
}

fn io_error(what: &str, error: &std::io::Error) -> StoreError {
    StoreError::Invalid(format!("{what}: {error}"))
}

/// One tidy of `files` at `now`, in the order the module's header lists.
pub fn tidy(conn: &mut Connection, files: &AttachmentFiles, now: Timestamp) -> Result<Tidied> {
    let mut tidied = Tidied::default();
    let budget = files.budget();
    for (ticket, closed_at) in attachments::closed_with_files(conn)? {
        if now.seconds_since(closed_at) > KEEP_CLOSED_DAYS * 86_400.0 {
            attachments::remove_all(conn, &ticket, now)?;
            tidied.removed.push(ticket.to_string());
        }
    }
    if attachments::kept_bytes(conn, false)? > budget {
        let target = budget / 10 * 9;
        for (ticket, _) in attachments::closed_with_files(conn)? {
            if attachments::kept_bytes(conn, false)? < target {
                break;
            }
            attachments::remove_all(conn, &ticket, now)?;
            tidied.removed.push(ticket.to_string());
        }
    }
    let kept = attachments::kept_files(conn)?;
    for file in files
        .files()
        .map_err(|e| io_error("listing attachment files", &e))?
    {
        if !kept.contains(&file.name) && now.seconds_since(file.modified) >= UNNAMED_SECONDS {
            files
                .delete(&file.name)
                .map_err(|e| io_error("deleting an attachment file", &e))?;
            tidied.deleted.push(file.name);
        }
    }
    let total = attachments::kept_bytes(conn, false)?;
    if total > budget {
        tidied.filed = Some(file_full(conn, total, budget, now)?);
    }
    Ok(tidied)
}

/// Files, or comments once a day on, the ticket saying attachment storage is full.
fn file_full(conn: &mut Connection, total: u64, budget: u64, now: Timestamp) -> Result<Filed> {
    let body = format!(
        "Attachment storage is full: open tickets keep {} of images and the budget is {}, so \
         the hub refuses new attachments (HTTP 507). Upkeep removes only the files of closed \
         tickets, so it cannot make room.\n\nRaise `[hub] attachment_budget_mb` in \
         `ignite.toml`, or close or finish the open tickets that hold the largest images (each \
         ticket's page lists its attachments). Ignite closes this ticket once the kept files \
         are back under the budget.",
        size_text(total),
        size_text(budget)
    );
    let mut metadata = BTreeMap::new();
    metadata.insert(BUDGET_KEY.to_owned(), Value::from(budget));
    let filing = Filing {
        signature: Signature::of_subject("attachments", CATEGORY),
        // At most one comment a day while storage stays full.
        evidence: now.day(),
        fields: TicketFields {
            title: Some("Attachment storage is full".to_owned()),
            body: Some(body),
            ticket_type: Some(TicketType::Bug),
            priority: Some(1),
            labels: Some(vec!["fleet".to_owned()]),
            metadata: Some(metadata),
            ..TicketFields::default()
        },
        deps: Vec::new(),
        released: true,
    };
    filing::file(conn, UPKEEP, &filing, now)
}

/// Whether storage is still full for the storage-full ticket `ticket`: the kept files are over
/// the budget it records. A ticket without one is left to people.
pub fn verdict(conn: &Connection, ticket: &Ticket) -> Result<Verdict> {
    let Some(budget) = ticket.metadata.get(BUDGET_KEY).and_then(Value::as_u64) else {
        return Ok(Verdict::Unwatched);
    };
    let total = attachments::kept_bytes(conn, false)?;
    Ok(if total > budget {
        Verdict::Present
    } else {
        Verdict::Cleared(format!(
            "the kept attachment files total {}, within the budget of {}.",
            size_text(total),
            size_text(budget)
        ))
    })
}
