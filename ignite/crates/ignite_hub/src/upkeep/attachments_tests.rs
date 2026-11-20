//! Upkeep keeps the attachment files within their budget: it removes the files of tickets
//! closed long ago, then those of the tickets closed longest ago while over the budget, never an
//! open ticket's, keeps a file another attachment still names, deletes files no attachment
//! names once they are an hour old, and files a ticket while open tickets alone fill the budget.
//!
//! The clock starts at the time the directory's first file was written, so the hour a file must
//! wait is measured from its real modification time without comparing against the wall clock.

use std::path::PathBuf;

use rusqlite::Connection;

use ignite_core::fakes::FakeClock;
use ignite_core::ids::TicketId;
use ignite_core::protocol::attachments::sniff;
use ignite_core::protocol::tickets::{Operation, Status, TicketType};
use ignite_core::traits::Clock;
use ignite_store::hub::{self, attachments};

use super::attachments::{BUDGET_KEY, CATEGORY, tidy};
use super::filing::{Change, FAULT_KEY};
use super::tests::{apply, create, ticket};
use super::{Upkeep, clearing::HOLD_SECONDS};
use crate::attachments::AttachmentFiles;

const DAY: f64 = 86_400.0;

struct Fixture {
    conn: Connection,
    files: AttachmentFiles,
    clock: FakeClock,
    dir: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str, budget: u64) -> Fixture {
    let dir = ignite_core::test_files::temp_dir().join(format!("attachments-upkeep-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    let files = AttachmentFiles::new(dir.clone(), budget);
    let probe = files.write(b"probe").unwrap();
    let written = files.files().unwrap()[0].modified;
    files.delete(&probe).unwrap();
    Fixture {
        conn: hub::open_in_memory().unwrap(),
        files,
        clock: FakeClock::new(written),
        dir,
    }
}

impl Fixture {
    fn ticket(&mut self, title: &str) -> String {
        let now = self.clock.now();
        apply(
            &mut self.conn,
            vec![create(title, TicketType::Task, None, &[])],
            now,
        )
        .remove(0)
    }

    fn close(&mut self, id: &str) {
        let now = self.clock.now();
        let close = Operation::Close {
            id: id.to_owned(),
            reason: "done".to_owned(),
            include_ready: false,
        };
        apply(&mut self.conn, vec![close], now);
    }

    /// Attaches an image of `size` bytes whose content is told apart by `tag`, and returns the
    /// file's name.
    fn attach(&mut self, id: &str, name: &str, tag: u8, size: usize) -> String {
        let mut bytes = b"GIF89a\x01\x00\x01\x00".to_vec();
        bytes.resize(size, tag);
        let sha256 = self.files.write(&bytes).unwrap();
        let ticket = TicketId::new(id).unwrap();
        let upload = attachments::Upload {
            ticket: &ticket,
            name,
            sha256: &sha256,
            image: sniff(&bytes).unwrap(),
            bytes: size as u64,
            caption: None,
        };
        attachments::insert(&mut self.conn, &upload, "tester", self.clock.now()).unwrap();
        sha256
    }

    fn tidy(&mut self) -> super::attachments::Tidied {
        let now = self.clock.now();
        tidy(&mut self.conn, &self.files, now).unwrap()
    }

    fn on_disk(&self) -> Vec<String> {
        self.files
            .files()
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect()
    }

    fn removed(&self, id: &str, name: &str) -> bool {
        attachments::get(&self.conn, id, name)
            .unwrap()
            .unwrap()
            .removed_at
            .is_some()
    }
}

#[test]
fn a_closed_tickets_files_go_after_ninety_days_and_an_open_tickets_stay() {
    let mut f = fixture("ninety", 1 << 30);
    let closed = f.ticket("closed");
    let open = f.ticket("open");
    let old = f.attach(&closed, "old.gif", 1, 100);
    let kept = f.attach(&open, "kept.gif", 2, 100);
    f.close(&closed);
    f.clock.advance(89.0 * DAY);
    assert!(f.tidy().removed.is_empty());
    f.clock.advance(2.0 * DAY);
    let tidied = f.tidy();
    assert_eq!(tidied.removed, [closed.as_str()]);
    assert_eq!(tidied.deleted, [old]);
    assert!(f.removed(&closed, "old.gif"));
    assert!(!f.removed(&open, "kept.gif"));
    assert_eq!(f.on_disk(), [kept]);
}

#[test]
fn over_the_budget_the_tickets_closed_longest_ago_lose_their_files_first() {
    let mut f = fixture("budget", 100);
    let open = f.ticket("open");
    let first = f.ticket("closed first");
    let second = f.ticket("closed second");
    f.attach(&open, "a.gif", 1, 30);
    f.attach(&first, "b.gif", 2, 40);
    f.attach(&second, "c.gif", 3, 40);
    f.close(&first);
    f.clock.advance(60.0);
    f.close(&second);
    // 110 bytes are kept against a budget of 100: removing the first closed ticket's 40 brings
    // the total to 70, under 90% of the budget, so the second keeps its file.
    let tidied = f.tidy();
    assert_eq!(tidied.removed, [first.as_str()]);
    assert!(f.removed(&first, "b.gif"));
    assert!(!f.removed(&second, "c.gif"));
    assert!(!f.removed(&open, "a.gif"));
    assert_eq!(attachments::kept_bytes(&f.conn, false).unwrap(), 70);
    assert!(tidied.filed.is_none());
}

#[test]
fn a_file_another_attachment_still_names_stays_on_disk() {
    let mut f = fixture("shared", 1 << 30);
    let closed = f.ticket("closed");
    let open = f.ticket("open");
    let shared = f.attach(&closed, "same.gif", 7, 64);
    assert_eq!(f.attach(&open, "same.gif", 7, 64), shared);
    f.close(&closed);
    f.clock.advance(91.0 * DAY);
    let tidied = f.tidy();
    assert_eq!(tidied.removed, [closed.as_str()]);
    assert!(tidied.deleted.is_empty());
    assert_eq!(f.on_disk(), [shared.as_str()]);
    assert!(
        attachments::kept_media_type(&f.conn, &shared)
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_file_no_attachment_names_is_deleted_once_it_is_an_hour_old() {
    let mut f = fixture("unnamed", 1 << 30);
    let left = f.files.write(b"GIF89a left by a failed upload").unwrap();
    f.clock.advance(1_800.0);
    assert!(f.tidy().deleted.is_empty());
    f.clock.advance(1_860.0);
    assert_eq!(f.tidy().deleted, [left]);
    assert!(f.on_disk().is_empty());
}

#[test]
fn storage_full_of_open_tickets_files_a_ticket_that_clears_itself() {
    let mut f = fixture("full", 50);
    let open = f.ticket("open");
    f.attach(&open, "big.gif", 1, 60);
    let mut upkeep = Upkeep::new(None).unwrap().with_attachments(f.files.clone());
    let report = upkeep.pass(&mut f.conn, f.clock.now()).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    let full = report.tickets_filed[0].clone();
    let filed = ticket(&f.conn, &full);
    assert!(filed.id.as_str().starts_with('A'), "{}", filed.id);
    assert_eq!(filed.metadata[FAULT_KEY]["category"], CATEGORY);
    assert_eq!(filed.metadata[BUDGET_KEY], 50);
    // The open ticket's file is never removed to make room, and a second look the same day
    // adds nothing to the ticket.
    let again = f.tidy();
    assert!(again.removed.is_empty());
    assert_eq!(again.filed.unwrap().change, Change::Unchanged);
    // Once the ticket closes, its file goes over the budget's limit and storage is clear; the
    // full ticket closes after the fault has stayed clear for the hold.
    f.close(&open);
    let mut closed = Vec::new();
    for _ in 0..30 {
        f.clock.advance(600.0);
        closed.extend(
            upkeep
                .pass(&mut f.conn, f.clock.now())
                .unwrap()
                .tickets_closed,
        );
        if !closed.is_empty() {
            break;
        }
    }
    assert_eq!(closed, [full.as_str()]);
    assert!(f.removed(&open, "big.gif"));
    assert_eq!(ticket(&f.conn, &full).status, Status::Closed);
    assert!(f.clock.now().seconds_since(filed.created_at) >= HOLD_SECONDS);
}
