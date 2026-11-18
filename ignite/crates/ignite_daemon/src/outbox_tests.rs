use super::*;
use ignite_core::protocol::client::Exit;
use ignite_store::local::open_in_memory;

use crate::hubclient::tests::{FakeHttpHub, record, stopped_hub};
use crate::server::error_reply;

fn verbs(body: &str) -> Vec<String> {
    let batch: ignite_core::protocol::hub::RecordBatch = serde_json::from_str(body).unwrap();
    batch.records.into_iter().map(|r| r.verb).collect()
}

#[test]
fn records_queued_while_the_hub_is_down_arrive_in_order_once_it_returns() {
    let conn = open_in_memory().unwrap();
    push_record(&conn, &record("test"), Timestamp::EPOCH).unwrap();
    push_record(&conn, &record("land"), Timestamp::EPOCH).unwrap();

    let down = stopped_hub();
    let error = flush(&conn, &down).unwrap_err();
    assert!(matches!(error, Error::Unreachable(_)));
    assert_eq!(error_reply(&error).exit, Exit::Unreachable);
    assert_eq!(store::peek(&conn, 10).unwrap().len(), 2);

    let hub = FakeHttpHub::start(|_, _, _| (200, r#"{"ids": [1, 2]}"#.to_owned()));
    assert_eq!(flush(&conn, &hub.client()).unwrap(), 2);
    let seen = hub.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].url, "/test/api/records");
    assert_eq!(verbs(&seen[0].body), ["test", "land"]);
    assert!(store::peek(&conn, 10).unwrap().is_empty());
    assert_eq!(flush(&conn, &hub.client()).unwrap(), 0);
}

#[test]
fn records_and_results_keep_their_order_across_kinds() {
    let conn = open_in_memory().unwrap();
    let result: NewResult = serde_json::from_value(serde_json::json!({
        "key": {"check": "rust:a", "hash": "0".repeat(64), "platform": null},
        "outcome": "pass", "machine": "m-1", "record": 1, "output": null
    }))
    .unwrap();
    push_record(&conn, &record("one"), Timestamp::EPOCH).unwrap();
    push_result(&conn, &result, Timestamp::EPOCH).unwrap();
    push_record(&conn, &record("two"), Timestamp::EPOCH).unwrap();

    let hub = FakeHttpHub::start(|_, _, _| (200, "{}".to_owned()));
    assert_eq!(flush(&conn, &hub.client()).unwrap(), 3);
    let urls: Vec<String> = hub.seen().into_iter().map(|s| s.url).collect();
    assert_eq!(
        urls,
        [
            "/test/api/records",
            "/test/api/results/store",
            "/test/api/records"
        ]
    );
}

#[test]
fn an_entry_the_hub_rejects_does_not_hold_back_the_rest() {
    let conn = open_in_memory().unwrap();
    store::push(&conn, "result", "not json", "t").unwrap();
    push_record(&conn, &record("test"), Timestamp::EPOCH).unwrap();
    let hub = FakeHttpHub::start(|_, _, _| (200, "{}".to_owned()));
    assert_eq!(flush(&conn, &hub.client()).unwrap(), 1);
    assert!(store::peek(&conn, 10).unwrap().is_empty());
}

#[test]
fn a_refused_token_keeps_the_queue_for_when_the_machine_is_registered_again() {
    let conn = open_in_memory().unwrap();
    push_record(&conn, &record("test"), Timestamp::EPOCH).unwrap();
    push_record(&conn, &record("land"), Timestamp::EPOCH).unwrap();

    let refusing = FakeHttpHub::start(|_, _, _| (401, "{}".to_owned()));
    assert!(matches!(
        flush(&conn, &refusing.client()),
        Err(Error::Unreachable(_))
    ));
    assert_eq!(store::peek(&conn, 10).unwrap().len(), 2);

    let hub = FakeHttpHub::start(|_, _, _| (200, "{}".to_owned()));
    assert_eq!(flush(&conn, &hub.client()).unwrap(), 2);
}

#[test]
fn only_a_malformed_answer_drops_entries_and_other_refusals_keep_them() {
    for status in [401, 403, 408, 429] {
        let conn = open_in_memory().unwrap();
        push_record(&conn, &record("test"), Timestamp::EPOCH).unwrap();
        let hub = FakeHttpHub::start(move |_, _, _| (status, "{}".to_owned()));
        assert!(
            matches!(flush(&conn, &hub.client()), Err(Error::Unreachable(_))),
            "{status}"
        );
        assert_eq!(store::peek(&conn, 10).unwrap().len(), 1, "{status}");
    }
    for status in [400, 422] {
        let conn = open_in_memory().unwrap();
        push_record(&conn, &record("test"), Timestamp::EPOCH).unwrap();
        let hub = FakeHttpHub::start(move |_, _, _| (status, "{}".to_owned()));
        assert_eq!(flush(&conn, &hub.client()).unwrap(), 0, "{status}");
        assert!(store::peek(&conn, 10).unwrap().is_empty(), "{status}");
    }
}

/// A file database in a fresh directory of its own, removed when dropped.
struct FileDatabase {
    dir: std::path::PathBuf,
    conn: Connection,
}

impl FileDatabase {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ignite-outbox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = ignite_store::local::open(&dir.join("daemon.db")).unwrap();
        FileDatabase { dir, conn }
    }
}

impl Drop for FileDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn an_entry_too_large_for_the_hub_is_moved_aside_and_the_rest_are_sent() {
    let db = FileDatabase::new("too-large");
    push_record(&db.conn, &record("before"), Timestamp::EPOCH).unwrap();
    push_record(&db.conn, &record("huge"), Timestamp::EPOCH).unwrap();
    push_record(&db.conn, &record("after"), Timestamp::EPOCH).unwrap();
    let huge = store::peek(&db.conn, 10).unwrap()[1].clone();

    // The hub refuses any request that carries the huge record, as it would refuse a body over
    // its limit.
    let hub = FakeHttpHub::start(|_, _, body| {
        if verbs(body).iter().any(|verb| verb == "huge") {
            (
                413,
                r#"{"error": "the request body is too large"}"#.to_owned(),
            )
        } else {
            (200, "{}".to_owned())
        }
    });
    assert_eq!(flush(&db.conn, &hub.client()).unwrap(), 2);
    assert!(store::peek(&db.conn, 10).unwrap().is_empty());

    let accepted: Vec<Vec<String>> = hub
        .seen()
        .iter()
        .map(|seen| verbs(&seen.body))
        .filter(|verbs| !verbs.iter().any(|verb| verb == "huge"))
        .collect();
    assert_eq!(accepted, [vec!["before"], vec!["after"]]);

    let bad = db
        .dir
        .join("outbox")
        .join(format!("{}-record.bad", huge.seq));
    assert_eq!(std::fs::read_to_string(bad).unwrap(), huge.body);
}

#[test]
fn a_run_too_large_as_a_whole_is_sent_in_halves() {
    let conn = open_in_memory().unwrap();
    for n in 0..8 {
        push_record(&conn, &record(&format!("r{n}")), Timestamp::EPOCH).unwrap();
    }
    // The hub takes at most three records a request.
    let hub = FakeHttpHub::start(|_, _, body| {
        if verbs(body).len() > 3 {
            (413, "{}".to_owned())
        } else {
            (200, "{}".to_owned())
        }
    });
    assert_eq!(flush(&conn, &hub.client()).unwrap(), 8);
    let sent: Vec<String> = hub
        .seen()
        .iter()
        .map(|seen| verbs(&seen.body))
        .filter(|verbs| verbs.len() <= 3)
        .flatten()
        .collect();
    let expected: Vec<String> = (0..8).map(|n| format!("r{n}")).collect();
    assert_eq!(sent, expected, "every record arrives once, in order");
}
