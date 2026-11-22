use super::*;

#[test]
fn fleet_shows_the_latest_machine_snapshot() {
    use ignite_core::capacity::{BuildSpaceReport, CapacityReport, ProviderCapacity};
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
        last_disk_cleanup_at: None,
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
        admit_cores: 15.0,
        free_cores: 4.0,
        outside_cores: Some(6),
        running_workers: Some(3),
        worker_room: 2,
        free_memory_mb: Some(9000),
        free_disk_gb: Some(67),
        build_space: Some(BuildSpaceReport {
            slots: 6,
            free: 2,
            ceiling_gb: 100,
            largest_tree_bytes: Some(84_500_000_000),
        }),
        queue_seconds: 25.5,
        free_trees: vec![vec![], vec![]],
        leased_trees: [("work".parse().unwrap(), vec![])].into(),
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
        "Capacity",
        "3 running, room to start 2",
        "15 admitted, 4 free, 6 busy outside Ignite",
        "Free memory",
        "9000 MB",
        "6 build slots, 2 free, 100 GB tree ceiling, largest tree 84.5 GB",
        "1 leased, 2 free",
        "Longest wait for cores",
        "25.5 s",
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

#[test]
fn fleet_greys_stale_capacity_and_shows_the_last_dispatch_reason() {
    use ignite_core::capacity::CapacityReport;
    use ignite_core::config::Os;
    use ignite_core::protocol::hub::{MachineFree, NextReply};
    use ignite_core::protocol::machine_state::MachineSnapshot;
    use ignite_hub::{auth::Writer, routes::dispatch};

    let mut conn = seeded();
    let now: Timestamp = NOW.parse().unwrap();
    let machine = "m-c".parse().unwrap();
    let mut snapshot: MachineSnapshot = serde_json::from_value(serde_json::json!({
        "at": now.plus_seconds(-61.0),
        "queued_cores": 0,
        "idle_workers": 0,
        "working_workers": 0,
        "stalled_workers": 0,
        "down_workers": 0,
        "ready_tickets": 0,
    }))
    .unwrap();
    let report = CapacityReport::default();
    hub::machine_snapshots::record(&conn, "m-c", &snapshot, Some(&report)).unwrap();
    snapshot.at = now.plus_seconds(-4.0);
    hub::machine_snapshots::record(&conn, "m-c", &snapshot, None).unwrap();
    hub::fleet_settings::set_machine_state(&mut conn, "cygnus", MachineState::Off, "operator", now)
        .unwrap();

    let free = MachineFree {
        machine,
        os: Os::Windows,
        idle_workers: Vec::new(),
        free_ticket_trees: 1,
        free_cores: 2.0,
        admit_cores: None,
        pressure: None,
        families: Vec::new(),
        name: None,
        justiciar_only: false,
        master_only: false,
        explain: true,
        room: None,
    };
    let request = Request {
        method: "POST".into(),
        path: "/next".into(),
        query: vec![],
        writer: Some(Writer {
            machine: free.machine.clone(),
            actor: "test".into(),
        }),
        body: serde_json::to_vec(&free).unwrap(),
        headers: vec![],
    };
    let response = dispatch::handle(&mut conn, &request, now, &[]).unwrap();
    assert_eq!(response.status, 200);
    let answer: NextReply = serde_json::from_slice(&response.body).unwrap();
    let NextReply::Nothing(Some(reason)) = answer else {
        panic!("expected a dispatch reason for the off machine, received {answer:?}");
    };

    let body = page(&conn, "/fleet");
    assert!(body.contains("Stale · sampled 61 s ago"));
    assert!(body.contains(r#"<span class="c-muted">Build slots unreported</span>"#));
    assert!(body.contains(&format!("{reason} (asked 0 s ago)")));
    assert!(hub::capacity::latest(&conn, now).unwrap().is_empty());
}
