use super::*;

#[test]
fn fleet_fields_are_optional_on_the_wire() {
    let heartbeat = r#"{"machine":"m-1","version":"old","workers":[]}"#;
    let reply = r#"{"hub_version":"old"}"#;
    let free = r#"{"machine":"m-1","os":"macos","idle_workers":[],
        "free_ticket_trees":1,"free_cores":2.0,"families":[]}"#;
    let reservation = r#"{"ticket":"T.1","machine":"m-1","session":"cli-codex-slot-1",
        "expires_at":"2026-11-20T12:00:00Z","code":"c0de"}"#;

    let heartbeat: Heartbeat = serde_json::from_str(heartbeat).unwrap();
    let reply: HeartbeatReply = serde_json::from_str(reply).unwrap();
    let free: MachineFree = serde_json::from_str(free).unwrap();
    let reservation: Reservation = serde_json::from_str(reservation).unwrap();
    assert_eq!(heartbeat.capacity, None);
    assert_eq!(reply.fleet, None);
    assert_eq!(free.room, None);
    assert_eq!(reservation.provider, None);
    assert!(
        serde_json::to_value(heartbeat)
            .unwrap()
            .get("capacity")
            .is_none()
    );
    assert!(serde_json::to_value(reply).unwrap().get("fleet").is_none());
    assert!(serde_json::to_value(free).unwrap().get("room").is_none());
    assert!(
        serde_json::to_value(reservation)
            .unwrap()
            .get("provider")
            .is_none()
    );
}

#[test]
fn record_matches_the_documented_line() {
    let record: Record = serde_json::from_str(
        r#"{"machine": "m-7f3a", "ticket": "Tq44s.9.48", "session": "local_c8470108",
            "tree": "tree-07", "verb": "test", "class": "agent", "family": "b3d-test",
            "queued_s": 4.1, "wall_s": 58.2, "cpu_s": 301.0, "instructions": 1.21e12,
            "peak_mb": 2210, "outcome": "fail", "commit": "8c1f2ab", "ignite": "d97ed0e",
            "at": "2026-11-17T14:12:00.000Z"}"#,
    )
    .unwrap();
    assert_eq!(record.class, Class::Agent);
    assert_eq!(record.outcome, Some(Outcome::Fail));
    assert!(record.compile_units.is_empty());
}

#[test]
fn portable_result_keys_have_no_platform() {
    let key = ResultKey {
        check: "rust:a::b".parse().unwrap(),
        hash: "0".repeat(64).parse().unwrap(),
        platform: None,
    };
    let value = serde_json::to_value(&key).unwrap();
    assert_eq!(value["platform"], serde_json::Value::Null);
    assert_eq!(serde_json::from_value::<ResultKey>(value).unwrap(), key);
}
