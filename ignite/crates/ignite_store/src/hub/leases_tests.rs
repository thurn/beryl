//! The ticket service's lease behavior (`project-state.ts` and its tests), ported.

use serde_json::json;

use super::*;
use crate::hub::{feed, open_in_memory};

const START: Timestamp = Timestamp::from_millis(1_800_000_000_000);

fn acquire(holder: &str, ttl_s: u64) -> LeaseOp {
    LeaseOp::Acquire {
        holder: holder.to_owned(),
        ttl_s,
        payload: None,
        no_queue: false,
    }
}

fn release(holder: &str) -> LeaseOp {
    LeaseOp::Release {
        holder: holder.to_owned(),
        if_revision: None,
    }
}

fn held(reply: LeaseReply) -> Lease {
    match reply {
        LeaseReply::Held(lease) => lease,
        LeaseReply::Busy(busy) => panic!("busy: {busy:?}"),
    }
}

fn busy(reply: LeaseReply) -> LeaseBusy {
    match reply {
        LeaseReply::Busy(busy) => busy,
        LeaseReply::Held(lease) => panic!("held: {lease:?}"),
    }
}

#[test]
fn a_holder_acquires_renews_and_releases() {
    let mut conn = open_in_memory().unwrap();
    let lease = held(apply(&mut conn, "staging", &acquire("s1", 60), "m-1", START).unwrap());
    assert_eq!(lease.expires_at, START.plus_seconds(60.0));
    assert_eq!(lease.payload, json!({}));
    let renew = LeaseOp::Renew {
        holder: "s1".to_owned(),
        ttl_s: 120,
        payload: Some(json!({"revision": "r2"})),
        if_revision: None,
    };
    let later = START.plus_seconds(30.0);
    let renewed = held(apply(&mut conn, "staging", &renew, "m-1", later).unwrap());
    assert_eq!(renewed.expires_at, later.plus_seconds(120.0));
    assert_eq!(list(&conn, later).unwrap(), std::slice::from_ref(&renewed));
    let released = held(apply(&mut conn, "staging", &release("s1"), "m-1", later).unwrap());
    assert_eq!(released.expires_at, later);
    assert!(list(&conn, later).unwrap().is_empty());
}

#[test]
fn another_holder_queues_in_order_and_the_head_takes_the_released_lease() {
    let mut conn = open_in_memory().unwrap();
    held(apply(&mut conn, "staging", &acquire("owner", 300), "m-1", START).unwrap());
    let first = busy(apply(&mut conn, "staging", &acquire("a", 300), "m-2", START).unwrap());
    assert_eq!(first.holder.as_deref(), Some("owner"));
    assert_eq!(first.position, Some(1));
    let second = busy(apply(&mut conn, "staging", &acquire("b", 300), "m-2", START).unwrap());
    assert_eq!(second.position, Some(2));
    let again = busy(apply(&mut conn, "staging", &acquire("a", 300), "m-2", START).unwrap());
    assert_eq!(again.position, Some(1), "a retry keeps its place");

    // The owner releases with people waiting: the lease stays, expired, for the head.
    held(apply(&mut conn, "staging", &release("owner"), "m-1", START).unwrap());
    let jumped = busy(apply(&mut conn, "staging", &acquire("b", 300), "m-2", START).unwrap());
    assert_eq!(
        jumped.position,
        Some(2),
        "only the head may take a free lease"
    );
    let taken = held(apply(&mut conn, "staging", &acquire("a", 300), "m-2", START).unwrap());
    assert_eq!(taken.holder, "a");
    let waiting: Vec<&str> = taken.waiters.iter().map(|w| w.holder.as_str()).collect();
    assert_eq!(waiting, ["b"]);
}

#[test]
fn no_queue_answers_busy_without_joining() {
    let mut conn = open_in_memory().unwrap();
    held(apply(&mut conn, "tip", &acquire("owner", 60), "m-1", START).unwrap());
    let op = LeaseOp::Acquire {
        holder: "other".to_owned(),
        ttl_s: 60,
        payload: None,
        no_queue: true,
    };
    let answer = busy(apply(&mut conn, "tip", &op, "m-2", START).unwrap());
    assert_eq!(
        (answer.position, answer.seq),
        (None, Some(feed::current(&conn).unwrap()))
    );
    assert!(list(&conn, START).unwrap()[0].waiters.is_empty());
}

#[test]
fn a_waiter_that_stops_retrying_drops_out_and_an_expired_lease_is_free() {
    let mut conn = open_in_memory().unwrap();
    held(apply(&mut conn, "staging", &acquire("owner", 300), "m-1", START).unwrap());
    busy(
        apply(
            &mut conn,
            "staging",
            &acquire("abandoned", 300),
            "m-2",
            START,
        )
        .unwrap(),
    );
    let later = START.plus_seconds(WAITER_SECONDS + 1.0);
    assert!(list(&conn, later).unwrap()[0].waiters.is_empty());
    let expired = START.plus_seconds(301.0);
    let taken = held(apply(&mut conn, "staging", &acquire("next", 300), "m-3", expired).unwrap());
    assert_eq!(taken.holder, "next");
}

#[test]
fn renew_and_release_need_the_holder_and_the_expected_revision() {
    let mut conn = open_in_memory().unwrap();
    let op = LeaseOp::Acquire {
        holder: "owner".to_owned(),
        ttl_s: 60,
        payload: Some(json!({"revision": "r1"})),
        no_queue: false,
    };
    held(apply(&mut conn, "run", &op, "m-1", START).unwrap());
    let stranger = busy(apply(&mut conn, "run", &release("stranger"), "m-1", START).unwrap());
    assert_eq!(stranger.holder.as_deref(), Some("owner"));
    let elsewhere = busy(apply(&mut conn, "run", &release("owner"), "m-2", START).unwrap());
    assert_eq!(
        elsewhere.holder.as_deref(),
        Some("owner"),
        "the machine is part of the holder"
    );
    let stale = LeaseOp::Release {
        holder: "owner".to_owned(),
        if_revision: Some("r0".to_owned()),
    };
    busy(apply(&mut conn, "run", &stale, "m-1", START).unwrap());
    let current = LeaseOp::Release {
        holder: "owner".to_owned(),
        if_revision: Some("r1".to_owned()),
    };
    held(apply(&mut conn, "run", &current, "m-1", START).unwrap());
    let gone = busy(apply(&mut conn, "run", &release("owner"), "m-1", START).unwrap());
    assert_eq!((gone.holder, gone.expires_at), (None, None));
}

#[test]
fn malformed_operations_are_rejected() {
    let mut conn = open_in_memory().unwrap();
    for op in [
        acquire("owner", 0),
        acquire("", 60),
        LeaseOp::Acquire {
            holder: "owner".to_owned(),
            ttl_s: 60,
            payload: Some(json!([1])),
            no_queue: false,
        },
    ] {
        let result = apply(&mut conn, "x", &op, "m-1", START);
        assert!(matches!(result, Err(StoreError::Rejected(_))), "{result:?}");
    }
}

#[test]
fn each_change_and_each_new_waiter_moves_the_feed_and_names_the_lease() {
    let mut conn = open_in_memory().unwrap();
    held(apply(&mut conn, "staging", &acquire("owner", 300), "m-1", START).unwrap());
    let before = feed::current(&conn).unwrap();
    let joined = busy(apply(&mut conn, "staging", &acquire("waiter", 300), "m-2", START).unwrap());
    assert_eq!(joined.seq, Some(Seq(before.0 + 1)));
    busy(apply(&mut conn, "staging", &acquire("waiter", 300), "m-2", START).unwrap());
    assert_eq!(
        feed::current(&conn).unwrap(),
        Seq(before.0 + 1),
        "a retry is not news"
    );
    held(apply(&mut conn, "staging", &release("owner"), "m-1", START).unwrap());
    let changes = feed::since(&conn, joined.seq.unwrap(), 10).unwrap().changes;
    assert_eq!(changes.len(), 1);
    assert_eq!(
        (changes[0].kind.as_str(), changes[0].name.as_deref()),
        ("lease", Some("staging"))
    );
}

use ignite_core::ids::Seq;
