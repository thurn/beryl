use std::sync::Mutex;
use std::time::Duration;

use ignite_core::fakes::FakeClock;
use ignite_core::time::Timestamp;
use ignite_store::local::{handle_stops, handles as handle_store, open_in_memory, rows};

use super::*;

/// Records the handles it was asked to end.
#[derive(Default)]
struct FakeEnder(Mutex<Vec<String>>);

impl EndVerbProcess for FakeEnder {
    fn end(&self, handle: &Handle) -> Result<Option<String>, String> {
        self.0.lock().unwrap().push(handle.to_string());
        Ok(Some("its verb process 7".to_owned()))
    }
}

const WAIT: Duration = Duration::from_secs(60);

fn running(id: &str, pid: Option<i64>) -> rows::Handle {
    rows::Handle {
        id: id.to_owned(),
        verb: "check".to_owned(),
        session: Some("local_a".to_owned()),
        tree: None,
        state: "running".to_owned(),
        exit: None,
        reply: None,
        started_at: Timestamp::EPOCH.to_string(),
        finished_at: None,
        pid,
        executable: None,
        protocol: None,
    }
}

fn stop(handle: &str, asked: bool, refused: Option<&str>) -> rows::HandleStop {
    rows::HandleStop {
        handle: handle.to_owned(),
        asked,
        refused: refused.map(str::to_owned),
        at: Timestamp::EPOCH.to_string(),
    }
}

/// A database with one running verb process per `(handle, stop)`.
fn world(entries: &[(&str, rows::HandleStop)]) -> Mutex<Connection> {
    let conn = open_in_memory().unwrap();
    for (id, stop) in entries {
        handle_store::upsert(&conn, &running(id, Some(7))).unwrap();
        handle_stops::set(&conn, stop).unwrap();
    }
    Mutex::new(conn)
}

fn state(db: &Mutex<Connection>, id: &str) -> String {
    handle_store::get(&db.lock().unwrap(), id)
        .unwrap()
        .unwrap()
        .state
}

#[test]
fn a_verb_process_asked_to_stop_is_ended_once_a_minute_has_passed_and_not_before() {
    let db = world(&[("C-1", stop("C-1", true, None))]);
    let clock = FakeClock::new(Timestamp::EPOCH);
    let ender = FakeEnder::default();

    clock.advance(59.0);
    assert!(end_overdue(&db, &clock, &ender, WAIT).is_empty());
    assert_eq!(state(&db, "C-1"), "running");

    clock.advance(1.0);
    let log = end_overdue(&db, &clock, &ender, WAIT);
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(*ender.0.lock().unwrap(), ["C-1"]);
    let row = handle_store::get(&db.lock().unwrap(), "C-1")
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "stopped");
    assert_eq!(row.exit, Some(i64::from(Exit::Conflict.code())));

    // The row is finished, so a later round leaves it alone.
    clock.advance(60.0);
    assert!(end_overdue(&db, &clock, &ender, WAIT).is_empty());
    assert_eq!(ender.0.lock().unwrap().len(), 1);
}

#[test]
fn a_handle_that_refused_the_stop_is_never_ended() {
    let db = world(&[(
        "L-1",
        stop("L-1", false, Some("its push to staging has begun")),
    )]);
    let clock = FakeClock::new(Timestamp::EPOCH);
    let ender = FakeEnder::default();
    clock.advance(3600.0);
    assert!(end_overdue(&db, &clock, &ender, WAIT).is_empty());
    assert!(ender.0.lock().unwrap().is_empty());
    assert_eq!(state(&db, "L-1"), "running");
}

#[test]
fn a_handle_that_finished_by_itself_or_runs_in_the_daemon_is_not_touched() {
    let db = world(&[
        ("C-1", stop("C-1", true, None)),
        ("C-2", stop("C-2", true, None)),
    ]);
    {
        let conn = db.lock().unwrap();
        let mut done = running("C-1", Some(7));
        "done".clone_into(&mut done.state);
        handle_store::upsert(&conn, &done).unwrap();
        handle_store::upsert(&conn, &running("C-2", None)).unwrap();
    }
    let clock = FakeClock::new(Timestamp::EPOCH);
    let ender = FakeEnder::default();
    clock.advance(120.0);
    assert!(end_overdue(&db, &clock, &ender, WAIT).is_empty());
    assert!(ender.0.lock().unwrap().is_empty());
    assert_eq!(state(&db, "C-1"), "done");
    assert_eq!(state(&db, "C-2"), "running");
}
