//! The `available` page, rendered from a seeded hub database at a fixed time: what workers hold,
//! the ready tickets in the order the pool offers them with why each waits, and the ready
//! tickets the pool is never offered.

use rusqlite::Connection;

use ignite_core::capacity::BYTES_PER_GIB;
use ignite_core::config::Os;
use ignite_core::dispatch_pressure::{DispatchPressure, TreeCap};
use ignite_core::ids::{MachineId, SessionId};
use ignite_core::protocol::hub::{IdleWorker, MachineFree};
use ignite_core::time::Timestamp;
use ignite_hub::dashboard::serve::serve;
use ignite_hub::routes::Request;
use ignite_store::hub::{self, machine_asks};

const NOW: &str = "2026-11-17T12:00:00.000Z";

fn page(conn: &Connection, path: &str) -> String {
    let request = Request {
        method: "GET".to_owned(),
        path: path.to_owned(),
        query: Vec::new(),
        writer: None,
        body: Vec::new(),
        headers: Vec::new(),
    };
    let response = serve(conn, &request, "proj", Timestamp::parse(NOW).unwrap())
        .unwrap_or_else(|| panic!("no dashboard route for {path}"));
    assert_eq!(response.status, 200, "{path}");
    String::from_utf8(response.body).unwrap()
}

fn free(machine: &str, idle: &[&str], trees: u32) -> MachineFree {
    MachineFree {
        machine: MachineId::new(machine).unwrap(),
        os: Os::Macos,
        idle_workers: idle
            .iter()
            .map(|session| IdleWorker {
                session: SessionId::new(*session).unwrap(),
                app: "claude".to_owned(),
                models: vec!["opus".to_owned(), "sonnet".to_owned()],
                model_ids: std::collections::BTreeMap::new(),
                default_model: None,
            })
            .collect(),
        free_ticket_trees: trees,
        free_cores: 8.0,
        admit_cores: Some(8.0),
        pressure: None,
        families: Vec::new(),
        name: None,
        justiciar_only: false,
        master_only: false,
        explain: true,
        room: None,
    }
}

/// `jarvis` has one worker on `e1.1` and one idle; `cygnus` reports but its only worker is
/// busy; `multivac` reports but has never asked for work. Under the active epic `e1`, `e1.2`
/// is small and `e1.3` expects more cores than `jarvis` admits. `e2.1` is under an epic with no
/// allocation and `p1` is set aside for a person.
fn seeded() -> Connection {
    let conn = hub::open_in_memory().unwrap();
    conn.execute_batch(
        "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen) VALUES
           ('m-j', 'jarvis', 'macos', 18, 65536, '[]', 'abc1234', '2026-11-17T11:59:30.000Z'),
           ('m-c', 'cygnus', 'macos', 16, 32768, '[]', 'abc1234', '2026-11-17T11:59:00.000Z'),
           ('m-m', 'multivac', 'macos', 12, 32768, '[]', 'abc1234', '2026-11-17T11:59:00.000Z');
         INSERT INTO workers (machine, session, app, role, state, ticket) VALUES
           ('m-j', 'local_busy', 'claude', 'worker', 'working', 'e1.1'),
           ('m-j', 'local_idle', 'claude', 'worker', 'idle', NULL),
           ('m-c', 'local_c', 'claude', 'worker', 'working', NULL);
         INSERT INTO tickets (id, parent, title, type, status, priority, labels, workers, cores,
                              machine, claimed_at, created_at, updated_at, created_by, version) VALUES
           ('e1', NULL, 'Active epic', 'epic', 'open', 2, '[]', 3, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e1.1', 'e1', 'Being worked', 'task', 'in_progress', 2, '[]', NULL, 1.0, 'm-j',
            '2026-11-17T11:00:00.000Z', '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e1.2', 'e1', 'Small next task', 'task', 'open', 1, '[]', NULL, 1.0, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e1.3', 'e1', 'Huge build', 'task', 'open', 2, '[]', NULL, 20.0, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e2', NULL, 'Dormant epic', 'epic', 'open', 2, '[]', 0, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e2.1', 'e2', 'Waits for its epic', 'task', 'open', 2, '[]', NULL, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('p1', NULL, 'Choose a vendor', 'task', 'open', 2, '[\"needs:person\"]', NULL, NULL,
            NULL, NULL, '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1);",
    )
    .unwrap();
    let asked = Timestamp::parse("2026-11-17T11:58:00.000Z").unwrap();
    machine_asks::record(&conn, &free("m-j", &["local_idle"], 3), Ok("e1.1"), asked).unwrap();
    machine_asks::record(&conn, &free("m-c", &[], 2), Err("nothing"), asked).unwrap();
    conn
}

/// The text of the page's section with `id`.
fn section<'a>(body: &'a str, id: &str) -> &'a str {
    let start = body
        .find(&format!("id=\"{id}\""))
        .unwrap_or_else(|| panic!("no section {id}"));
    let rest = &body[start..];
    &rest[..rest.find("</section>").unwrap_or(rest.len())]
}

#[test]
fn the_landing_page_links_to_the_available_page() {
    let conn = seeded();
    assert!(page(&conn, "/").contains("href=\"/available\""));
}

#[test]
fn running_tickets_come_first_and_waiting_ones_follow_in_dispatch_order() {
    let conn = seeded();
    let body = page(&conn, "/available");
    assert!(section(&body, "running").contains("Being worked"));
    let waiting = section(&body, "waiting");
    let small = waiting.find("Small next task").expect("e1.2 waits");
    let huge = waiting.find("Huge build").expect("e1.3 waits");
    assert!(small < huge, "the higher priority ticket goes first");
    assert!(waiting.contains("#1") && waiting.contains("#2"));
    assert!(waiting.contains("Next on jarvis"));
}

#[test]
fn a_waiting_ticket_says_which_limit_keeps_it_from_each_machine() {
    let conn = seeded();
    let body = page(&conn, "/available");
    // The chip names the reason; its card lists every machine's.
    assert!(
        body.contains("data-hovercard=\"why-E1.3\"")
            || body.contains("data-hovercard=\"why-e1.3\"")
    );
    let card = &body[body
        .find("<template id=\"why-e1.3\"")
        .or_else(|| body.find("<template id=\"why-E1.3\""))
        .unwrap()..];
    let card = &card[..card.find("</template>").unwrap()];
    assert!(card.contains("Why it is not running"), "{card}");
    assert!(card.contains("needs 20.0 cores"), "{card}");
    assert!(card.contains("every worker is busy"), "{card}");
    assert!(
        body.contains("has no workers"),
        "multivac, which has never asked and has no workers, says so"
    );
}

#[test]
fn ready_tickets_the_pool_is_never_offered_say_why() {
    let conn = seeded();
    let body = page(&conn, "/available");
    let held = section(&body, "not-offered");
    assert!(held.contains("Waits for its epic"));
    assert!(held.contains("Its epic has no allocation"));
    assert!(held.contains("Choose a vendor"));
    assert!(held.contains("Set aside for a person"));
}

#[test]
fn a_machine_that_never_asked_because_every_worker_is_busy_says_so() {
    let conn = seeded();
    conn.execute_batch(
        "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen) VALUES
           ('m-b', 'busybox', 'macos', 8, 16384, '[]', 'abc1234', '2026-11-17T11:59:30.000Z');
         INSERT INTO workers (machine, session, app, role, state, ticket) VALUES
           ('m-b', 'local_b', 'claude', 'worker', 'working', NULL);",
    )
    .unwrap();
    let body = page(&conn, "/available");
    let card = &body[body.find("<template id=\"why-e1.3\"").unwrap()..];
    let card = &card[..card.find("</template>").unwrap()];
    assert!(card.contains("busybox"), "{card}");
    assert!(body.contains("so its free trees and cores are not known"));
}

#[test]
fn a_machine_whose_build_queue_is_backed_up_says_only_master_tickets_start() {
    let conn = seeded();
    let mut backed_up = free("m-j", &["local_idle"], 3);
    backed_up.master_only = true;
    backed_up.pressure = Some(ignite_core::dispatch_pressure::DispatchPressure {
        queue_seconds: 217,
        ..Default::default()
    });
    let asked = Timestamp::parse("2026-11-17T11:59:00.000Z").unwrap();
    machine_asks::record(&conn, &backed_up, Err("nothing"), asked).unwrap();
    let body = page(&conn, "/available");
    let card = &body[body.find("<template id=\"why-e1.2\"").unwrap()..];
    let card = &card[..card.find("</template>").unwrap()];
    assert!(
        card.contains(
            "commands have waited 217 s for cores, so only tickets that hold master back start"
        ),
        "{card}"
    );
    assert!(!section(&body, "waiting").contains("Next on jarvis"));
}

#[test]
fn the_available_page_shows_a_fleet_cap_and_accounts_for_earlier_tickets() {
    let conn = seeded();
    conn.execute_batch(
        "UPDATE tickets SET cores = 1.0 WHERE id = 'e1.3';
         INSERT INTO workers (machine, session, app, role, state, ticket) VALUES
           ('m-j', 'local_idle2', 'claude', 'worker', 'idle', NULL);",
    )
    .unwrap();
    let mut machine = free("m-j", &["local_idle", "local_idle2"], 3);
    let asked = Timestamp::parse("2026-11-17T11:59:00.000Z").unwrap();
    for (limit_gb, expected) in [
        (90, "waits for the Fleet tree disk cap"),
        (120, "behind 1 earlier ticket"),
    ] {
        machine.pressure = Some(DispatchPressure {
            free_gb: Some(300),
            tree_cap: Some(TreeCap {
                limit_gb,
                used_bytes: 10 * BYTES_PER_GIB,
            }),
            ..DispatchPressure::default()
        });
        machine_asks::record(&conn, &machine, Err("nothing"), asked).unwrap();
        let body = page(&conn, "/available");
        assert!(body.contains(expected), "{expected}");
    }
}

#[test]
fn a_checks_only_machine_says_it_takes_only_tickets_that_hold_master_back() {
    let conn = seeded();
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('fleet', ?1)",
        [r#"{"version":1,"machines":{"cygnus":{"state":"checks-only","disk_cap_gb":60}},"providers":{}}"#],
    )
    .unwrap();
    machine_asks::record(
        &conn,
        &free("m-c", &["local_c2"], 2),
        Err("nothing"),
        Timestamp::parse("2026-11-17T11:59:00.000Z").unwrap(),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO workers (machine, session, app, role, state, ticket)
         VALUES ('m-c', 'local_c2', 'claude', 'worker', 'idle', NULL)",
        [],
    )
    .unwrap();
    let body = page(&conn, "/available");
    let card = &body[body.find("<template id=\"why-e1.3\"").unwrap()..];
    let card = &card[..card.find("</template>").unwrap()];
    assert!(
        card.contains("checks-only machine: only tickets that hold master back start here"),
        "{card}"
    );
}

#[test]
fn the_available_page_uses_worker_room_and_explains_provider_constraints() {
    let conn = seeded();
    let mut free = free("m-j", &[], 3);
    free.room = Some(
        serde_json::from_value(serde_json::json!({
            "start": 1,
            "providers": [{"name": "claude", "tiers": ["sonnet"]}]
        }))
        .unwrap(),
    );
    let now = Timestamp::parse(NOW).unwrap();
    machine_asks::record(&conn, &free, Err("nothing"), now).unwrap();
    let body = page(&conn, "/available");
    assert!(section(&body, "waiting").contains("Next on jarvis"));
    assert!(body.contains("room to start 1"));

    conn.execute(
        "UPDATE tickets SET labels = '[\"tier:opus\"]' WHERE id = 'e1.2'",
        [],
    )
    .unwrap();
    let body = page(&conn, "/available");
    assert!(body.contains("claude does not run tier opus"));
    assert!(!section(&body, "waiting").contains("Next on jarvis"));
}

#[test]
fn the_available_page_explains_why_a_legacy_worker_cannot_take_work() {
    let conn = seeded();
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('fleet', ?1)",
        [r#"{"version":1,"machines":{},"providers":{"claude":{"enabled":false,"share":1,"stop_percent":99}}}"#],
    )
    .unwrap();
    let body = page(&conn, "/available");
    assert!(body.contains("claude is off"));
    assert!(!section(&body, "waiting").contains("Next on jarvis"));
}

#[test]
fn a_chip_names_a_reporting_machines_reason_not_a_silent_machines() {
    let conn = seeded();
    // `multivac` never asked and has no workers. Renamed to sort before the machines that
    // reported, its line must still not become the chip.
    conn.execute("UPDATE machines SET name = 'aardvark' WHERE id = 'm-m'", [])
        .unwrap();
    let body = page(&conn, "/available");
    let waiting = section(&body, "waiting");
    assert!(!waiting.contains(">Has no workers<"), "{waiting}");
    assert!(
        waiting.contains(">Every worker is busy<") || waiting.contains(">Needs 20.0 cores<"),
        "{waiting}"
    );
    let card = &body[body.find("<template id=\"why-e1.3\"").unwrap()..];
    let card = &card[..card.find("</template>").unwrap()];
    assert!(
        card.contains("has no workers"),
        "the card still lists it: {card}"
    );
}

#[test]
fn a_ticket_pinned_to_a_silent_machine_names_that_machines_reason() {
    let conn = seeded();
    // `multivac` never asked and has no workers; pin `e1.3` to it.
    conn.execute(
        "UPDATE tickets SET labels = '[\"machine:multivac\"]' WHERE id = 'e1.3'",
        [],
    )
    .unwrap();
    let body = page(&conn, "/available");
    let waiting = section(&body, "waiting");
    assert!(waiting.contains(">Has no workers<"), "{waiting}");
    assert!(
        !waiting.contains(">Pinned to machine multivac<"),
        "{waiting}"
    );
}
