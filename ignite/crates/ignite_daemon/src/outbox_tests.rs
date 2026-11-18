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
