use ignite_core::capacity::BYTES_PER_GIB;
use ignite_core::config::Os;
use ignite_core::protocol::machine_state::{MachineSnapshot, ReclaimableDisk, StartBlock};
use ignite_core::protocol::tickets::Status;
use ignite_core::time::Timestamp;
use ignite_store::hub::{self, machine_snapshots};
use rusqlite::{Connection, params};

use super::Upkeep;
use super::tests::{all_tickets, comment_count, ticket};

fn at(minute: u32) -> Timestamp {
    Timestamp::EPOCH.plus_seconds(60.0 * f64::from(minute))
}

fn machine(conn: &Connection, id: &str, os: Os) {
    conn.execute(
        "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen)
         VALUES (?1, ?1, ?2, 8, 16000, '[]', 'v1', ?3)",
        params![id, os.as_str(), at(0).to_string()],
    )
    .unwrap();
}

fn healthy() -> MachineSnapshot {
    MachineSnapshot {
        at: at(0),
        free_disk_gib: Some(200),
        reclaimable: None,
        last_disk_cleanup_at: None,
        next_ticket: Some("Tabcd".parse().unwrap()),
        next_ticket_disk_gib: Some(106),
        start_block: None,
        start_reason: None,
        cpu_percent: Some(30),
        queued_cores: 0,
        swap_used_mb: Some(0),
        idle_workers: 0,
        working_workers: 8,
        stalled_workers: 0,
        down_workers: 0,
        ready_tickets: 1,
    }
}

fn disk_blocked() -> MachineSnapshot {
    MachineSnapshot {
        free_disk_gib: Some(65),
        start_block: Some(StartBlock::Disk),
        start_reason: Some("Starts blocked on disk".to_owned()),
        ..healthy()
    }
}

fn record(
    conn: &Connection,
    machine: &str,
    minutes: std::ops::RangeInclusive<u32>,
    mut snapshot: MachineSnapshot,
) {
    for minute in minutes {
        snapshot.at = at(minute);
        machine_snapshots::record(conn, machine, &snapshot, None).unwrap();
    }
}

#[test]
fn reclaimable_disk_with_thirteen_idle_workers_files_one_ticket_and_clears() {
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    machine(&conn, "m-disk", Os::Macos);
    let blocked = MachineSnapshot {
        reclaimable: Some(ReclaimableDisk {
            bytes: 800_000_000_000,
            measured_at: at(0),
        }),
        idle_workers: 13,
        ready_tickets: 4,
        ..disk_blocked()
    };
    record(&conn, "m-disk", 0..=14, blocked.clone());
    assert!(
        upkeep
            .pass(&mut conn, at(14))
            .unwrap()
            .tickets_filed
            .is_empty()
    );

    record(&conn, "m-disk", 15..=15, blocked.clone());
    let first = upkeep.pass(&mut conn, at(15)).unwrap();
    assert_eq!(first.tickets_filed.len(), 1);
    let id = &first.tickets_filed[0];
    let filed = ticket(&conn, id);
    assert_eq!(filed.priority, 0);
    assert_eq!(filed.status, Status::Open);
    assert!(filed.labels.contains(&"justiciar".to_owned()));
    assert!(filed.title.contains("Disk handling stalled"));
    for reading in [
        "13 idle",
        "Ready tickets: 4",
        "65 GiB free",
        "745.1 GiB reclaimable",
        "needs 106 GiB free",
        "root cause",
    ] {
        assert!(filed.body.contains(reading), "{reading}: {}", filed.body);
    }
    assert!(
        upkeep
            .pass(&mut conn, at(15))
            .unwrap()
            .tickets_filed
            .is_empty()
    );

    record(&conn, "m-disk", 16..=30, blocked);
    assert_eq!(
        upkeep
            .pass(&mut conn, at(30))
            .unwrap()
            .tickets_filed
            .as_slice(),
        std::slice::from_ref(id)
    );
    assert_eq!(all_tickets(&conn, at(30)).len(), 1);
    assert_eq!(comment_count(&conn, id), 1);

    // Silence and a short healthy run on either side of a gap do not count as an hour.
    assert!(
        upkeep
            .pass(&mut conn, at(90))
            .unwrap()
            .tickets_closed
            .is_empty()
    );
    record(&conn, "m-disk", 91..=120, healthy());
    record(&conn, "m-disk", 125..=184, healthy());
    assert!(
        upkeep
            .pass(&mut conn, at(184))
            .unwrap()
            .tickets_closed
            .is_empty()
    );
    record(&conn, "m-disk", 185..=185, healthy());
    assert_eq!(
        upkeep
            .pass(&mut conn, at(185))
            .unwrap()
            .tickets_closed
            .as_slice(),
        std::slice::from_ref(id)
    );
    assert_eq!(ticket(&conn, id).status, Status::Closed);
}

#[test]
fn disk_that_stays_blocked_after_cleanup_files_with_its_timestamp() {
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    machine(&conn, "m-cleanup", Os::Macos);
    record(
        &conn,
        "m-cleanup",
        0..=14,
        MachineSnapshot {
            // One GiB is insufficient to close the 41 GiB shortfall.
            reclaimable: Some(ReclaimableDisk {
                bytes: BYTES_PER_GIB,
                measured_at: at(0),
            }),
            ..disk_blocked()
        },
    );
    assert!(
        upkeep
            .pass(&mut conn, at(14))
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    record(&conn, "m-cleanup", 15..=15, disk_blocked());
    assert!(
        upkeep
            .pass(&mut conn, at(15))
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    record(
        &conn,
        "m-cleanup",
        16..=16,
        MachineSnapshot {
            last_disk_cleanup_at: Some(at(16)),
            ..disk_blocked()
        },
    );
    let filed = upkeep.pass(&mut conn, at(16)).unwrap().tickets_filed;
    assert_eq!(filed.len(), 1);
    let filed = ticket(&conn, &filed[0]);
    assert!(filed.title.contains("Disk handling stalled"));
    assert!(filed.body.contains(&format!("after cleanup at {}", at(16))));
}

#[test]
fn idle_with_work_ignores_pauses_and_ignites_own_emergency_tickets() {
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    for id in ["m-a", "m-b"] {
        machine(&conn, id, Os::Macos);
        let idle = MachineSnapshot {
            idle_workers: 1,
            ..healthy()
        };
        record(&conn, id, 0..=9, idle.clone());
        record(
            &conn,
            id,
            10..=20,
            MachineSnapshot {
                start_block: Some(StartBlock::Paused),
                ..idle.clone()
            },
        );
        record(
            &conn,
            id,
            21..=31,
            MachineSnapshot {
                next_ticket: Some("Jabcd".parse().unwrap()),
                next_ticket_disk_gib: None,
                ..idle.clone()
            },
        );
        record(&conn, id, 32..=41, idle);
    }
    for minute in [9, 20, 31, 41] {
        assert!(
            upkeep
                .pass(&mut conn, at(minute))
                .unwrap()
                .tickets_filed
                .is_empty(),
            "filed at minute {minute}"
        );
    }
    for id in ["m-a", "m-b"] {
        record(
            &conn,
            id,
            42..=42,
            MachineSnapshot {
                idle_workers: 1,
                ..healthy()
            },
        );
    }
    let filed = upkeep.pass(&mut conn, at(42)).unwrap().tickets_filed;
    assert_eq!(filed.len(), 2);
    assert_ne!(filed[0], filed[1]);
    assert_eq!(
        ticket(&conn, &filed[0]).title,
        "Workers idle with work on m-a"
    );
    assert_eq!(
        ticket(&conn, &filed[1]).title,
        "Workers idle with work on m-b"
    );
    for id in ["m-a", "m-b"] {
        record(&conn, id, 43..=103, healthy());
    }
    let closed = upkeep.pass(&mut conn, at(103)).unwrap().tickets_closed;
    assert_eq!(closed.len(), 2);
    assert!(filed.iter().all(|id| closed.contains(id)));
}

#[test]
fn corroborated_pacing_and_memory_blocks_file_only_a_pressure_ticket() {
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    machine(&conn, "m-pressure", Os::Macos);
    let idle = MachineSnapshot {
        idle_workers: 13,
        ready_tickets: 37,
        ..healthy()
    };
    record(
        &conn,
        "m-pressure",
        0..=14,
        MachineSnapshot {
            start_block: Some(StartBlock::Pacing),
            queued_cores: 9,
            ..idle.clone()
        },
    );
    let memory = MachineSnapshot {
        start_block: Some(StartBlock::Memory),
        swap_used_mb: Some(12_288),
        ..idle
    };
    record(&conn, "m-pressure", 15..=29, memory.clone());
    for minute in [10, 29] {
        assert!(
            upkeep
                .pass(&mut conn, at(minute))
                .unwrap()
                .tickets_filed
                .is_empty()
        );
    }

    record(&conn, "m-pressure", 30..=30, memory);
    let filed = upkeep.pass(&mut conn, at(30)).unwrap().tickets_filed;
    assert_eq!(filed.len(), 1);
    assert_eq!(
        ticket(&conn, &filed[0]).title,
        "Oversubscription persisting on m-pressure"
    );
    assert_eq!(all_tickets(&conn, at(30)).len(), 1);
}

#[test]
fn a_full_build_quota_explains_idle_workers_while_swap_can_still_escalate() {
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    machine(&conn, "m-capped", Os::Macos);
    let capped = MachineSnapshot {
        idle_workers: 5,
        ready_tickets: 10,
        start_block: Some(StartBlock::Pacing),
        start_reason: Some(
            "Tabcd waits for shared build cores on m-capped: 3 build tickets held, limit 3"
                .to_owned(),
        ),
        ..healthy()
    };
    record(&conn, "m-capped", 0..=30, capped.clone());
    assert!(
        upkeep
            .pass(&mut conn, at(30))
            .unwrap()
            .tickets_filed
            .is_empty()
    );

    record(
        &conn,
        "m-capped",
        31..=61,
        MachineSnapshot {
            swap_used_mb: Some(12_288),
            ..capped
        },
    );
    let filed = upkeep.pass(&mut conn, at(61)).unwrap().tickets_filed;
    assert_eq!(filed.len(), 1);
    assert_eq!(
        ticket(&conn, &filed[0]).title,
        "Oversubscription persisting on m-capped"
    );
}

#[test]
fn an_idle_escalation_needs_healthy_dispatch_samples_to_clear() {
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    machine(&conn, "m-idle", Os::Macos);
    let idle = MachineSnapshot {
        start_block: Some(StartBlock::Pacing),
        queued_cores: 8,
        swap_used_mb: Some(4_096),
        idle_workers: 1,
        ..healthy()
    };
    record(&conn, "m-idle", 0..=10, idle.clone());
    let filed = upkeep.pass(&mut conn, at(10)).unwrap().tickets_filed;
    assert_eq!(filed.len(), 1);
    assert_eq!(
        ticket(&conn, &filed[0]).title,
        "Workers idle with work on m-idle"
    );

    record(
        &conn,
        "m-idle",
        11..=71,
        MachineSnapshot {
            queued_cores: 9,
            ..idle
        },
    );
    let report = upkeep.pass(&mut conn, at(71)).unwrap();
    assert_eq!(report.tickets_filed.len(), 1);
    assert_eq!(
        ticket(&conn, &report.tickets_filed[0]).title,
        "Oversubscription persisting on m-idle"
    );
    assert_eq!(ticket(&conn, &filed[0]).status, Status::Open);

    record(&conn, "m-idle", 72..=132, healthy());
    let closed = upkeep.pass(&mut conn, at(132)).unwrap().tickets_closed;
    assert!(closed.contains(&filed[0]));
    assert_eq!(ticket(&conn, &filed[0]).status, Status::Closed);
}

#[test]
fn queue_and_swap_pressure_escalate_only_when_sustained_and_need_observed_recovery() {
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    machine(&conn, "m-pressure", Os::Macos);
    record(
        &conn,
        "m-pressure",
        0..=14,
        MachineSnapshot {
            queued_cores: 9,
            ..healthy()
        },
    );
    let swapping = MachineSnapshot {
        swap_used_mb: Some(4_097),
        ..healthy()
    };
    record(&conn, "m-pressure", 15..=29, swapping.clone());
    assert!(
        upkeep
            .pass(&mut conn, at(29))
            .unwrap()
            .tickets_filed
            .is_empty()
    );
    record(&conn, "m-pressure", 30..=30, swapping);
    let filed = upkeep.pass(&mut conn, at(30)).unwrap().tickets_filed;
    assert_eq!(filed.len(), 1);
    assert!(ticket(&conn, &filed[0]).body.contains("4097 MB swap"));

    record(
        &conn,
        "m-pressure",
        31..=91,
        MachineSnapshot {
            swap_used_mb: None,
            ..healthy()
        },
    );
    assert!(
        upkeep
            .pass(&mut conn, at(91))
            .unwrap()
            .tickets_closed
            .is_empty()
    );
    record(&conn, "m-pressure", 92..=152, healthy());
    assert_eq!(
        upkeep.pass(&mut conn, at(152)).unwrap().tickets_closed,
        filed
    );
}

#[test]
fn queue_pressure_clears_on_hosts_that_do_not_report_swap() {
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    machine(&conn, "m-windows", Os::Windows);
    let normal = MachineSnapshot {
        swap_used_mb: None,
        ..healthy()
    };
    record(
        &conn,
        "m-windows",
        0..=30,
        MachineSnapshot {
            queued_cores: 9,
            ..normal.clone()
        },
    );
    let filed = upkeep.pass(&mut conn, at(30)).unwrap().tickets_filed;
    assert_eq!(filed.len(), 1);

    record(&conn, "m-windows", 31..=91, normal);
    assert_eq!(
        upkeep.pass(&mut conn, at(91)).unwrap().tickets_closed,
        filed
    );
}

#[test]
fn cleanup_with_slot_based_admission_uses_observed_free_disk() {
    let mut conn = hub::open_in_memory().unwrap();
    let mut upkeep = Upkeep::new(None).unwrap();
    machine(&conn, "m-slots", Os::Macos);
    let blocked = MachineSnapshot {
        free_disk_gib: Some(10),
        next_ticket_disk_gib: None,
        start_block: Some(StartBlock::Trees),
        start_reason: Some("Waiting for a build slot".to_owned()),
        ..healthy()
    };
    record(&conn, "m-slots", 0..=4, blocked.clone());
    record(
        &conn,
        "m-slots",
        5..=15,
        MachineSnapshot {
            last_disk_cleanup_at: Some(at(5)),
            ..blocked
        },
    );
    let filed = upkeep.pass(&mut conn, at(15)).unwrap().tickets_filed;
    assert_eq!(filed.len(), 1);
    let report = ticket(&conn, &filed[0]);
    assert!(report.body.contains("free disk stayed at or below 10 GiB"));
    assert!(report.body.contains("Waiting for a build slot"));

    record(
        &conn,
        "m-slots",
        16..=76,
        MachineSnapshot {
            next_ticket_disk_gib: None,
            ..healthy()
        },
    );
    assert_eq!(
        upkeep.pass(&mut conn, at(76)).unwrap().tickets_closed,
        filed
    );
}
