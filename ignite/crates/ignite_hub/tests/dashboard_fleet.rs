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
           ('o1', NULL, 'Owned elsewhere', 'task', 'open', '[]', '', NULL, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1),
           ('r1', NULL, 'Fix kernel-test', 'bug', 'open', '[]', '', NULL, NULL, NULL, NULL,
            '2026-11-16T00:00:00.000Z', '2026-11-16T00:00:00.000Z', 'x', 1);
         UPDATE tickets SET owner = 'plan9/claude' WHERE id = 'o1';
         INSERT INTO meta (key, value) VALUES ('routing', '{\"machines\":[
           {\"name\":\"jarvis\",\"os\":\"macos\"},
           {\"name\":\"cygnus\",\"os\":\"windows\"}],
           \"apps\":[\"claude\",\"codex\"]}');
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
            body.contains("href=\"/pool\""),
            "{path} lacks the page links"
        );
    }
    // A trailing slash is the same page; an unknown name is looked up as a
    // ticket and is a 404, and a path with several segments is no dashboard route.
    assert!(page(&conn, "/pool/").contains("local_long"));
    assert_eq!(get(&conn, "/nowhere").0, 404);
    let request = Request {
        method: "GET".to_owned(),
        path: "/nowhere/deeper".to_owned(),
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
    assert!(first.contains("plan9/claude"));
}

#[test]
fn a_paused_pool_names_the_tickets_holding_master_back() {
    let conn = seeded();
    let attention = |conn: &Connection| {
        let body = page(conn, "/");
        let first = &body[body.find("id=\"attention\"").expect("an attention section")..];
        first[..first.find("</section>").unwrap()].to_owned()
    };
    // cygnus's pool is paused and the regression in kernel-test, ticket r1, is open.
    let first = attention(&conn);
    assert!(
        first.contains("master is held back while cygnus's pool is paused"),
        "{first}"
    );
    assert!(first.contains("one worker at a time"), "{first}");
    assert!(first.contains("href=\"/r1\""), "{first}");
    // A pause of every machine hands those tickets to nobody.
    conn.execute(
        "INSERT INTO pauses (machine, at) VALUES ('*', '2026-11-17T11:00:00.000Z')",
        [],
    )
    .unwrap();
    let first = attention(&conn);
    assert!(
        first.contains("Hand-offs are paused on every machine, so nobody takes the tickets"),
        "{first}"
    );
    // With no pool paused, the line is gone.
    conn.execute_batch(
        "DELETE FROM pauses; UPDATE workers SET state = 'idle' WHERE state = 'paused';",
    )
    .unwrap();
    assert!(!attention(&conn).contains("held back"));
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
    assert!(active.contains("Allocation 3"));
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

#[test]
fn tips_show_where_the_last_rounds_time_went() {
    use ignite_core::ids::{Commit, MachineId};
    use ignite_core::tipround::TipRound;
    let conn = seeded();
    assert!(page(&conn, "/tips").contains("No round has been recorded."));
    let round = TipRound {
        commit: Commit::new("3333333333").unwrap(),
        machine: MachineId::new("jarvis").unwrap(),
        at: Timestamp::parse(NOW).unwrap(),
        wall_s: 600.0,
        ran: 12,
        cached: 30,
        checks_s: 400.0,
        listing_s: 180.0,
        listing_alone_s: None,
        building_s: 0.0,
        waiting_s: 20.0,
        budget: None,
    };
    hub::tip_rounds::record(&conn, &round).unwrap();
    let body = page(&conn, "/tips");
    assert!(body.contains("Recent rounds"), "{body}");
    assert!(body.contains("333333333"), "{body}");
    assert!(body.contains("12 checks run, 30 cached"), "{body}");
    assert!(body.contains("3.0 listing"), "{body}");
    assert!(body.contains("slow"), "{body}");
}

/// A check a landing found failing before tip verification did is counted by family.
#[test]
fn tips_count_the_checks_tip_verification_missed_by_family() {
    use ignite_store::hub::rows::TipMiss;
    let conn = seeded();
    assert!(page(&conn, "/tips").contains("Tip verification missed no check this week."));
    let miss = |check: &str, family: &str, at: &str| TipMiss {
        check_name: check.to_owned(),
        family: family.to_owned(),
        tip: "4444444444".to_owned(),
        kind: "skipped".to_owned(),
        reason: format!("{check} was past the tip budget."),
        ticket: "Tabc1".to_owned(),
        at: at.to_owned(),
    };
    for (check, family, at) in [
        ("source:a", "source", "2026-11-17T10:00:00.000Z"),
        ("source:b", "source", "2026-11-17T11:00:00.000Z"),
        ("unit:a", "unit", "2026-11-17T11:30:00.000Z"),
        // Older than seven days: not counted.
        ("unit:old", "unit", "2026-11-01T11:30:00.000Z"),
    ] {
        hub::tip_misses::record(&conn, &miss(check, family, at)).unwrap();
    }
    let body = page(&conn, "/tips");
    assert!(
        body.contains("Missed by tip verification (7 days)"),
        "{body}"
    );
    assert!(body.contains("2 misses"), "{body}");
    assert!(body.contains("source:b was past the tip budget."), "{body}");
    assert!(body.contains("1 miss "), "{body}");
    assert!(!body.contains("unit:old"), "{body}");
}

/// A machine whose newest sample of a plan window holds hand-offs is named on the first line
/// until the window resets or a newer sample of that window does not hold.
#[test]
fn the_first_line_names_a_machine_on_a_usage_hold() {
    let conn = seeded();
    let sample = |at: &str, window: &str, percent: f64, holds: bool, resets: &str| {
        let data = serde_json::json!({
            "holds": holds, "at": at, "status": "ok", "window": window,
            "percent_used": percent, "resets_at": resets, "note": null,
        });
        conn.execute(
            "INSERT INTO metrics (machine, source, data, at) VALUES ('m-j', 'plan_usage', ?1, ?2)",
            rusqlite::params![data.to_string(), at],
        )
        .unwrap();
    };
    sample(
        "2026-11-17T11:00:00.000Z",
        "5-hour",
        99.4,
        true,
        "2026-11-17T14:00:00Z",
    );
    sample(
        "2026-11-17T11:00:00.000Z",
        "Weekly · all models",
        40.0,
        false,
        "2026-11-20T00:00:00Z",
    );
    // A window that held but has since reset.
    sample(
        "2026-11-16T11:00:00.000Z",
        "Weekly · Fable",
        100.0,
        true,
        "2026-11-17T08:00:00Z",
    );
    let body = page(&conn, "/");
    assert!(
        body.contains("jarvis is on a usage hold (5-hour at 99%, resets 2026-11-17T14:00:00Z)"),
        "{body}"
    );
    assert!(!body.contains("Fable at"), "{body}");

    // A newer sample of the window is lower, so the hold has ended.
    sample(
        "2026-11-17T11:30:00.000Z",
        "5-hour",
        20.0,
        false,
        "2026-11-17T14:00:00Z",
    );
    assert!(!page(&conn, "/").contains("usage hold"));
}
