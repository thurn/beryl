use super::*;

#[test]
fn fleet_shows_the_latest_machine_snapshot() {
    use ignite_core::capacity::{CapacityReport, ProviderCapacity};
    use ignite_core::protocol::machine_state::{MachineSnapshot, ReclaimableDisk, StartBlock};

    let conn = seeded();
    conn.execute(
        "UPDATE machines SET cli_apps = ?1 WHERE id = 'm-j'",
        [r#"["claude"]"#],
    )
    .unwrap();
    let now: Timestamp = NOW.parse().unwrap();
    let mut snapshot = MachineSnapshot {
        at: now.plus_seconds(-120.0),
        free_disk_gib: Some(23),
        reclaimable: Some(ReclaimableDisk {
            bytes: 40 << 30,
            measured_at: now.plus_seconds(-60.0),
        }),
        next_ticket: Some("work".parse().unwrap()),
        next_ticket_disk_gib: Some(129),
        start_block: Some(StartBlock::Disk),
        start_reason: Some("Waiting for disk space".into()),
        cpu_percent: Some(64),
        queued_cores: 12,
        swap_used_mb: Some(512),
        idle_workers: 5,
        working_workers: 3,
        stalled_workers: 2,
        down_workers: 1,
        ready_tickets: 8,
    };
    hub::machine_snapshots::record(&conn, "m-j", &snapshot, None).unwrap();
    snapshot.at = now.plus_seconds(-30.0);
    snapshot.free_disk_gib = Some(67);
    let report = CapacityReport {
        providers: vec![ProviderCapacity {
            name: "claude".into(),
            tiers: vec!["opus".into()],
            probed: true,
            hold_reason: None,
            model_ids: std::collections::BTreeMap::new(),
            default_model: None,
        }],
        ..CapacityReport::default()
    };
    hub::machine_snapshots::record(&conn, "m-j", &snapshot, Some(&report)).unwrap();
    let page = page(&conn, "/fleet");
    for expected in [
        "Latest machine snapshot",
        "67 GiB free",
        "40.0 GiB",
        "64% CPU",
        "12 queued cores",
        "512 MB swap",
        "5 idle, 3 working, 2 stalled, 1 down",
        "Ready tickets",
        "work needs 129 GiB free",
        "Waiting for disk space",
        "Sampled 30 s ago",
        "CLI answered",
    ] {
        assert!(page.contains(expected), "missing {expected}");
    }
    assert!(!page.contains("23 GiB free"));
}
