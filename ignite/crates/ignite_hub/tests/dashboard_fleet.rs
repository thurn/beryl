//! The fleet pages (machines, pool, epics, landings, tips and finds) and the line at the top of
//! the home page, rendered from a seeded hub database at a fixed time.

use rusqlite::Connection;

use ignite_core::time::Timestamp;
use ignite_hub::dashboard::serve::serve;
use ignite_hub::routes::Request;
use ignite_store::hub;

const NOW: &str = "2026-11-17T12:00:00.000Z";

fn get(conn: &Connection, path: &str) -> (u16, String) {
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
    (response.status, String::from_utf8(response.body).unwrap())
}

fn page(conn: &Connection, path: &str) -> String {
    let (status, body) = get(conn, path);
    assert_eq!(status, 200, "{path}");
    body
}

/// Two machines: `jarvis` (macOS) is reporting with an unpaused dispatcher and two workers;
/// `cygnus` (Windows) reports, but its pool is paused. `multivac` has no dispatcher.
fn seeded() -> Connection {
    let conn = hub::open_in_memory().unwrap();
    seed(&conn);
    conn
}

fn seed(conn: &Connection) {
    conn.execute_batch(
        "INSERT INTO machines VALUES
           ('m-j', 'jarvis', 'macos', 18, 65536, '[]', 'abc1234', '2026-11-17T11:59:30.000Z'),
           ('m-c', 'cygnus', 'windows', 16, 32768, '[]', 'abc1234', '2026-11-17T11:59:00.000Z'),
           ('m-m', 'multivac', 'macos', 12, 32768, '[]', 'abc1234', '2026-11-17T11:58:00.000Z');
         INSERT INTO workers (machine, session, app, role, state, ticket, command, command_since, command_running) VALUES
           ('m-j', 'local_d', 'claude', 'dispatcher', 'idle', NULL, NULL, NULL, 0),
           ('m-j', 'local_long', 'claude', 'worker', 'working', 'e1.1', 'test', '2026-11-17T11:49:00.000Z', 1),
           ('m-j', 'local_short', 'claude', 'worker', 'working', 'e1.2', 'test', '2026-11-17T11:51:00.000Z', 1),
           ('m-j', 'local_quiet', 'claude', 'worker', 'working', 'e1.3', 'land', '2026-11-17T11:40:00.000Z', 0),
           ('m-c', 'local_cd', 'codex', 'dispatcher', 'paused', NULL, NULL, NULL, 0),
           ('m-m', 'local_mw', 'claude', 'worker', 'idle', NULL, NULL, NULL, 0);
         INSERT INTO tickets (id, parent, title, type, status, labels, notes, workers, machine, tree,
                              claimed_at, created_at, updated_at, created_by, version) VALUES
           ('e1', NULL, 'Active epic', 'epic', 'open', '[]', '', 3, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e2', NULL, 'Dormant epic', 'epic', 'open', '[]', '', NULL, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e2.1', 'e2', 'Waits for its epic', 'task', 'open', '[]', '', NULL, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e1.1', 'e1', 'Long command', 'task', 'in_progress', '[]', '', NULL, 'm-j', 'tree-01',
            '2026-11-17T11:00:00.000Z', '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e1.2', 'e1', 'Short command', 'task', 'in_progress', '[]', '', NULL, 'm-j', 'tree-02',
            '2026-11-17T11:00:00.000Z', '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e1.3', 'e1', 'Quiet worker', 'task', 'in_progress', '[]', '', NULL, 'm-j', 'tree-03',
            '2026-11-17T11:00:00.000Z', '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e1.4', 'e1', 'Ready task', 'task', 'open', '[]', '', NULL, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('e1.5', 'e1', 'Landed task', 'task', 'closed', '[]', '', NULL, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('p1', NULL, 'Choose a vendor', 'task', 'open', '[\"needs:person\"]', 'Pick one by Friday',
            NULL, NULL, NULL, NULL, '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('o1', NULL, 'Needs another system', 'task', 'open', '[\"needs:linux\"]', '', NULL, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('r1', NULL, 'Fix kernel-test', 'bug', 'open', '[]', '', NULL, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1);
         UPDATE tickets SET landed_at = '2026-11-17T10:00:00.000Z',
           landed = '{\"commits\":[\"0123456789abcdef\"],\"files\":[],\"note\":\"done\"}',
           claimed_at = '2026-11-17T09:00:00.000Z' WHERE id = 'e1.5';
         INSERT INTO records (machine, ticket, verb, class, queued_s, wall_s, cpu_s, peak_mb, ignite, at) VALUES
           ('m-j', 'e1.5', 'test', 'agent', 0, 1, 0, 1, 'a', '2026-11-17T09:30:00.000Z'),
           ('m-j', 'e1.5', 'test', 'agent', 0, 5, 30, 1, 'a', '2026-11-17T09:31:00.000Z'),
           ('m-j', 'e1.5', 'land', 'agent', 0, 5, 60, 1, 'a', '2026-11-17T09:59:00.000Z');
         INSERT INTO regressions VALUES ('kernel-test', 'r1', 'bbbbbbbbbbbb', 'aaaaaaaaaaaa', 'open');
         INSERT INTO checks VALUES ('slow-oracle', 'oracle', 'regression', 'off', 'oracle');
         INSERT INTO tips VALUES
           ('1111111111111', '2026-11-17T11:00:00.000Z', '2026-11-17T11:10:00.000Z', 'verified'),
           ('2222222222222', '2026-11-17T11:50:00.000Z', NULL, 'verifying');
         INSERT INTO tip_claims VALUES ('2222222222222', 'rust', 1, 'm-j', '2026-11-17T12:10:00.000Z');
         INSERT INTO explore VALUES ('fuzz', '2026-11-16', 3, 4e11), ('exhaust', '2026-11-16', 0, 2e11);
         INSERT INTO rebuilds VALUES ('kernel-body', 'c1', 4.0e11, '{}'), ('kernel-body', 'c2', 4.1e11, '{}');
         INSERT INTO rejected_commands (machine, session, verb, args, kind, at) VALUES
           ('m-j', 's-1', 'ticket dep list', '[]', 'unknown subcommand', '2026-11-17T11:00:00.000Z'),
           ('m-j', 's-2', 'ticket dep list', '[]', 'unknown subcommand', '2026-11-17T11:30:00.000Z');",
    )
    .unwrap();
}

#[test]
fn every_fleet_page_renders_from_a_seeded_database() {
    let conn = seeded();
    for (path, expected) in [
        ("/machines", "jarvis"),
        ("/pool", "local_long"),
        ("/epics", "Active epic"),
        ("/landings", "Landed task"),
        ("/tips", "kernel-test"),
        ("/finds", "kernel-body"),
        ("/finds", "2 sessions"),
    ] {
        let body = page(&conn, path);
        assert!(body.contains(expected), "{path} lacks {expected}");
        assert!(
            body.contains("href=\"/proj/pool\""),
            "{path} lacks the page links"
        );
    }
    // A trailing slash is the same page; an unknown name is not a fleet page.
    assert!(page(&conn, "/pool/").contains("local_long"));
    let request = Request {
        method: "GET".to_owned(),
        path: "/nowhere".to_owned(),
        query: Vec::new(),
        writer: None,
        body: Vec::new(),
        headers: Vec::new(),
    };
    assert!(serve(&conn, &request, "proj", Timestamp::parse(NOW).unwrap()).is_none());
}

#[test]
fn pages_render_when_the_hub_is_empty() {
    let conn = hub::open_in_memory().unwrap();
    for path in [
        "/machines",
        "/pool",
        "/epics",
        "/landings",
        "/tips",
        "/finds",
        "/",
    ] {
        page(&conn, path);
    }
    assert!(page(&conn, "/").contains("Nothing needs a person."));
}

#[test]
fn the_pool_marks_a_command_over_ten_minutes_and_a_silent_worker() {
    let conn = seeded();
    let body = page(&conn, "/pool");
    let row = |session: &str| {
        let start = body
            .find(session)
            .unwrap_or_else(|| panic!("no row for {session}"));
        let end = body[start..].find("</li>").unwrap();
        &body[start..start + end]
    };
    // Began 11 minutes ago and still running: marked. Began 9 minutes ago: not.
    assert!(row("local_long").contains("Running over 10 minutes"));
    assert!(!row("local_short").contains("over 10 minutes"));
    // Holds a ticket and last ran a command 20 minutes ago: marked as silent.
    assert!(row("local_quiet").contains("No command for over 10 minutes"));
    // Idle workers with no ticket are never marked.
    assert!(!row("local_mw").contains("minutes"));
    assert!(row("local_long").contains("ignite test"));
}

#[test]
fn the_first_line_lists_what_needs_a_person() {
    let conn = seeded();
    assert!(page(&conn, "/").contains("cygnus's pool is paused"));
    // With every dispatcher down, no worker can take the open regression.
    conn.execute(
        "UPDATE workers SET state = 'down' WHERE role = 'dispatcher'",
        [],
    )
    .unwrap();
    let body = page(&conn, "/");
    let first = &body[body.find("id=\"attention\"").expect("an attention section")..];
    let first = &first[..first.find("</section>").unwrap()];
    assert!(first.contains("needs a person: Choose a vendor"), "{first}");
    assert!(first.contains("Pick one by Friday"));
    assert!(first.contains("multivac has no dispatcher"));
    assert!(first.contains("cygnus has no dispatcher"));
    assert!(first.contains("The regression in kernel-test"));
    assert!(first.contains("No machine can run"));
    assert!(first.contains("needs:linux"));
}

#[test]
fn epics_show_activity_shares_and_verification() {
    let conn = seeded();
    let body = page(&conn, "/epics");
    let row = |title: &str| {
        let start = body
            .find(title)
            .unwrap_or_else(|| panic!("no row for {title}"));
        let end = body[start..].find("</li>").unwrap();
        &body[start..start + end]
    };
    let active = row("Active epic");
    assert!(active.contains("3 workers"));
    assert!(active.contains("3 in progress"));
    // The landed ticket landed at 10:00 and the last verified tip is from 11:10.
    assert!(active.contains("landed work verified"));
    let dormant = row("Dormant epic");
    assert!(dormant.contains("Inactive"));
}

#[test]
fn landings_count_cached_and_run_checks_of_the_gate() {
    let conn = seeded();
    let body = page(&conn, "/landings");
    assert!(body.contains("1 cached, 2 run"), "{body}");
    assert!(body.contains("012345678"));
}

#[test]
fn tips_show_the_tip_the_verified_commit_regressions_and_flaky_checks() {
    let conn = seeded();
    let body = page(&conn, "/tips");
    assert!(body.contains("222222222"));
    assert!(body.contains("111111111"));
    assert!(body.contains("Open regressions"));
    assert!(body.contains("slow-oracle"));
    assert!(body.contains("batch 1"));
}
