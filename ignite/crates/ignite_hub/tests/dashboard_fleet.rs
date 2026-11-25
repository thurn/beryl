//! The fleet pages and the home page's attention section, rendered from a seeded hub database
//! at a fixed time.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};

use rusqlite::Connection;

use ignite_core::config::Config;
use ignite_core::fleet::{
    FleetSettings, FleetUpdate, FleetView, MachineSetting, MachineState, ProviderSample,
    ProviderSetting, TreeSettings, TreeSpace,
};
use ignite_core::time::Timestamp;
use ignite_hub::dashboard::serve::{Site, serve, serve_with};
use ignite_hub::routes::Request;
use ignite_hub::server::HubServer;
use ignite_store::hub;

const NOW: &str = "2026-11-17T12:00:00.000Z";
const FLEET_CONFIG: &str = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = "trees"
checks = "checks"

[hub]
url = "http://127.0.0.1:0"
project = "proj"

[gate]
threshold = 0.004
cap = 400

[machine.fixture]
os = "macos"
ticket_trees = 1
landing_trees = 1
floor = { batch = 1, tip = 1 }
"#;

fn get(conn: &Connection, path: &str) -> (u16, String) {
    render(conn, path, false)
}

fn render(conn: &Connection, path: &str, private: bool) -> (u16, String) {
    let request = Request {
        method: "GET".to_owned(),
        path: path.to_owned(),
        query: Vec::new(),
        writer: None,
        body: Vec::new(),
        headers: Vec::new(),
    };
    let site = Site {
        project: "proj",
        private,
        ..Site::default()
    };
    let response = serve_with(conn, &request, &site, Timestamp::parse(NOW).unwrap())
        .unwrap_or_else(|| panic!("no dashboard route for {path}"));
    (response.status, String::from_utf8(response.body).unwrap())
}

fn page(conn: &Connection, path: &str) -> String {
    let (status, body) = get(conn, path);
    assert_eq!(status, 200, "{path}");
    body
}

/// Jarvis has three workers. Cygnus runs checks only, and Multivac has no configured provider.
fn seeded() -> Connection {
    let mut conn = hub::open_in_memory().unwrap();
    seed(&conn);
    hub::fleet_settings::set_machine_state(
        &mut conn,
        "cygnus",
        MachineState::ChecksOnly,
        "operator",
        Timestamp::parse(NOW).unwrap(),
    )
    .unwrap();
    conn
}

fn seed(conn: &Connection) {
    conn.execute_batch(
        "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen) VALUES
           ('m-j', 'jarvis', 'macos', 18, 65536, '[]', 'abc1234', '2026-11-17T11:59:30.000Z'),
           ('m-c', 'cygnus', 'windows', 16, 32768, '[]', 'abc1234', '2026-11-17T11:59:00.000Z'),
           ('m-m', 'multivac', 'macos', 12, 32768, '[]', 'abc1234', '2026-11-17T11:58:00.000Z');
         INSERT INTO workers (machine, session, app, role, state, ticket, command, command_since, command_running) VALUES
           ('m-j', 'local_d', 'claude', 'dispatcher', 'idle', NULL, NULL, NULL, 0),
           ('m-j', 'cli-claude-long', 'claude', 'worker', 'working', 'e1.1', 'test', '2026-11-17T11:49:00.000Z', 1),
           ('m-j', 'local_short', 'claude', 'worker', 'working', 'e1.2', 'test', '2026-11-17T11:51:00.000Z', 1),
           ('m-j', 'local_quiet', 'claude', 'worker', 'working', 'e1.3', 'land', '2026-11-17T11:40:00.000Z', 0),
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
           ('o1', NULL, 'Pinned elsewhere', 'task', 'open', '[\"machine:plan9\"]', '', NULL, NULL, NULL, NULL,
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
         INSERT INTO regressions (check_name, ticket, first_bad, last_good, state) VALUES ('kernel-test', 'r1', 'bbbbbbbbbbbb', 'aaaaaaaaaaaa', 'open');
         INSERT INTO checks VALUES ('slow-oracle', 'oracle', 'regression', 'off', 'oracle');
         INSERT INTO tips (commit_sha, seen, verified, state) VALUES
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
        ("/fleet", "jarvis"),
        ("/pool", "cli-claude-long"),
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
    let fleet = page(&conn, "/fleet");
    assert!(fleet.contains("agent 3 (2 core-minutes)"));
    assert!(fleet.contains("tree-01"));
    // A trailing slash is the same page; an unknown name is looked up as a
    // ticket and is a 404, and a path with several segments is no dashboard route.
    assert!(page(&conn, "/pool/").contains("cli-claude-long"));
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
        "/fleet",
        "/pool",
        "/epics",
        "/landings",
        "/tips",
        "/finds",
        "/",
    ] {
        page(&conn, path);
    }
    let home = page(&conn, "/");
    assert!(!home.contains("id=\"attention\""));
    assert!(!home.contains("href=\"#attention\""));
    assert!(home.contains("href=\"/pool\""));
}

#[test]
fn the_old_machines_address_redirects_to_fleet() {
    let conn = seeded();
    for path in ["/machines", "/machines/"] {
        let request = Request {
            method: "GET".to_owned(),
            path: path.to_owned(),
            query: vec![("tab".to_owned(), "current work".to_owned())],
            writer: None,
            body: Vec::new(),
            headers: Vec::new(),
        };
        let reply = serve(&conn, &request, "proj", Timestamp::parse(NOW).unwrap()).unwrap();
        assert_eq!(reply.status, 308);
        assert_eq!(
            reply.headers.first(),
            Some(&("Location", "/fleet?tab=current%20work".to_owned()))
        );
    }
}

fn browser_request(address: SocketAddr, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(address).unwrap();
    let marker = if method == "PUT" {
        "X-Ignite-Dashboard: 1\r\n"
    } else {
        ""
    };
    write!(
        stream,
        "{method} {path} HTTP/1.0\r\nHost: hub\r\nConnection: close\r\n\
         Tailscale-User-Login: operator@example.com\r\n{marker}\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let (headers, body) = reply.split_once("\r\n\r\n").unwrap();
    (
        headers.split(' ').nth(1).unwrap().parse().unwrap(),
        body.to_owned(),
    )
}

fn machine_settings<'a>(page: &'a str, name: &str) -> &'a str {
    let start = page
        .find(&format!("data-fleet-machine=\"{name}\""))
        .unwrap();
    let end = page[start..].find("</dl>").unwrap();
    &page[start..start + end]
}

fn provider_row<'a>(page: &'a str, name: &str) -> &'a str {
    let (_, row) = page
        .split_once(&format!("data-fleet-provider=\"{name}\""))
        .unwrap();
    row.split("</section>")
        .next()
        .unwrap()
        .split("data-fleet-provider=")
        .next()
        .unwrap()
}

fn configure_providers(settings: &mut FleetSettings) {
    // Claude stops at 90% of its 5-hour window and 95% of its weekly ones.
    for (name, enabled, share, stop_percent, five_hour_stop_percent) in [
        ("claude", true, 12, 95, Some(90)),
        ("codex", true, 12, 95, None),
        ("disabled", false, 100, 99, None),
    ] {
        settings.providers.insert(
            name.to_owned(),
            ProviderSetting {
                enabled,
                share,
                stop_percent,
                five_hour_stop_percent,
                ignore_usage_threshold: name == "codex",
            },
        );
    }
}

fn assert_provider_controls(body: &str) {
    let codex = provider_row(body, "codex");
    for expected in [
        r#"aria-label="codex enabled" checked"#,
        r#"data-fleet-field="share" aria-label="codex share""#,
        r#"data-fleet-field="stop_percent" aria-label="codex weekly stop percentage""#,
        // A provider saved before the 5-hour window had its own percentage uses the weekly one.
        r#"data-fleet-field="five_hour_stop_percent" aria-label="codex 5-hour stop percentage" min="1" max="100" step="1" required value="95""#,
        r#"aria-label="codex continue past usage threshold" checked"#,
        ">50%<",
    ] {
        assert!(codex.contains(expected), "expected {expected} in {codex}");
    }
    let claude = provider_row(body, "claude");
    assert!(claude.contains(r#"aria-label="claude continue past usage threshold">"#));
}

fn assert_machine_controls(body: &str) {
    for (name, state) in [
        ("jarvis", "active"),
        ("cygnus", "checks-only"),
        ("multivac", "off"),
    ] {
        let section = machine_settings(body, name);
        assert_eq!(section.matches(" checked").count(), 1, "{name}");
        assert!(
            section.contains(&format!(r#"value="{state}" checked"#)),
            "{name}"
        );
        assert!(!section.contains("Disk cap"), "{name}");
    }
}

#[test]
fn fleet_renders_provider_shares_usage_holds_and_workers() {
    let mut conn = seeded();
    let now = Timestamp::parse(NOW).unwrap();
    conn.execute_batch(
        r#"UPDATE machines SET cli_apps = '["claude","unshared"]' WHERE name = 'jarvis';
           UPDATE tickets SET provider = 'claude' WHERE id IN ('e1.1', 'e1.5');
           UPDATE tickets SET session = 'local_short' WHERE id = 'e1.2';"#,
    )
    .unwrap();
    let mut settings = hub::fleet_settings::settings(&conn).unwrap();
    configure_providers(&mut settings);
    hub::fleet_settings::put(&mut conn, settings.version, settings, "operator", now).unwrap();
    let reset = "2026-11-17T14:00:00.000Z";
    for (ago, window, percent_used) in [
        (60.0, "5-hour", 91.0),
        (10.0, "5-hour", 92.0),
        (20.0, "Weekly · all models", 40.0),
    ] {
        hub::provider_usage::record(
            &conn,
            &ProviderSample {
                machine: "m-j".parse().unwrap(),
                provider: "claude".to_owned(),
                at: now.plus_seconds(-ago),
                status: "ok".to_owned(),
                label: Some(window.to_owned()),
                percent_used: Some(percent_used),
                resets_at: Some(reset.to_owned()),
                note: None,
            },
        )
        .unwrap();
    }

    let body = page(&conn, "/fleet");
    assert_eq!(body.matches("data-fleet-provider=").count(), 4);
    let claude = provider_row(&body, "claude");
    for expected in [
        "50%",
        "90%",
        "2 of 1 by share",
        "Held until",
        "5-hour at 92%",
        "datetime=\"2026-11-17T14:00:00.000Z\"",
        "Share can go to codex.",
        r#"aria-label="5-hour usage, stop at 90%""#,
        r#"aria-label="Weekly · all models usage, stop at 95%""#,
        "width:92%",
        "Weekly · all models",
        "Not reported",
    ] {
        assert!(claude.contains(expected), "expected {expected} in {claude}");
    }
    assert!(!claude.contains("91%"));
    assert!(provider_row(&body, "codex").contains("0 of 1 by share"));
    assert!(provider_row(&body, "codex").contains("Continue past threshold"));
    assert!(provider_row(&body, "disabled").contains(">Off<"));
    assert!(provider_row(&body, "unshared").contains(">Off<"));
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
    assert!(row("cli-claude-long").contains("Running over 10 minutes"));
    assert!(!row("local_short").contains("over 10 minutes"));
    // Holds a ticket and last ran a command 20 minutes ago: marked as silent.
    assert!(row("local_quiet").contains("No command for over 10 minutes"));
    // Idle workers with no ticket are never marked.
    assert!(!row("local_mw").contains("minutes"));
    assert!(row("cli-claude-long").contains("ignite test"));
}

#[test]
fn the_pool_links_cli_transcripts_on_the_private_listener() {
    let conn = seeded();
    let link = "href=\"/e1.1/session/cli-claude-long\"";
    let (status, private) = render(&conn, "/pool", true);
    assert_eq!(status, 200);
    assert!(private.contains(link));
    assert!(!page(&conn, "/pool").contains(link));
    assert!(!private.contains("local_d"));
    assert!(!private.contains("dispatcher"));
}

#[test]
fn the_home_page_shows_attention_cards_above_active() {
    let conn = seeded();
    let body = page(&conn, "/");
    let start = body.find("id=\"attention\"").expect("an attention section");
    assert!(start < body.find("id=\"active\"").unwrap());
    assert!(body.find("href=\"#attention\"").unwrap() < body.find("href=\"#active\"").unwrap());
    let section = body[start..].split("</section>").next().unwrap();
    assert!(section.contains("<h2 class=\"c-part-title\">Needs a person"));
    assert!(section.contains("<div class=\"c-grid\">"));
    let (_, ticket) = section
        .split_once("<a class=\"c-card\" href=\"/p1\"")
        .expect("a linked attention ticket card");
    let ticket = ticket.split("</a>").next().unwrap();
    assert!(ticket.contains("<h3 class=\"c-card-title\">Choose a vendor</h3>"));
    assert!(ticket.contains("Pick one by Friday"));
    assert!(section.contains("class=\"c-card c-card-static\""));
    assert!(section.contains("cygnus is checks-only (set by operator)"));
    assert!(section.contains("The regression in kernel-test"));
    assert!(section.contains("href=\"/r1\""));
    assert!(section.contains("No machine can run"));
    assert!(section.contains("pinned to plan9"));
}

#[test]
fn a_reporting_cli_pool_can_start_workers_for_a_regression() {
    let mut conn = seeded();
    let now = Timestamp::parse(NOW).unwrap();
    let mut beat = ignite_core::protocol::hub::Heartbeat {
        tree_volumes: None,
        sample: None,
        capacity: None,
        machine: "m-m".parse().unwrap(),
        version: "abc1234".to_owned(),
        workers: Vec::new(),
        cli_apps: vec!["codex".to_owned()],
        families: None,
    };
    hub::fleet::heartbeat(&mut conn, &beat, now).unwrap();

    let pool = |conn: &Connection| {
        let body = page(conn, "/pool");
        let machine = &body[body.find(">multivac").unwrap()..];
        machine[..machine.find("</section>").unwrap()].to_owned()
    };
    let body = page(&conn, "/");
    assert!(!body.contains("The regression in kernel-test"));
    assert!(body.contains("cygnus is checks-only (set by operator)"));
    assert!(pool(&conn).contains("codex CLI"));
    assert!(pool(&conn).contains("No workers."));

    hub::fleet_settings::set_machine_state(
        &mut conn,
        "m-m",
        MachineState::ChecksOnly,
        "operator",
        now,
    )
    .unwrap();
    let home = page(&conn, "/");
    assert!(home.contains("multivac is checks-only (set by operator)"));
    assert!(!home.contains("The regression in kernel-test"));
    assert!(pool(&conn).contains("checks-only (set by operator)"));

    hub::fleet::heartbeat(&mut conn, &beat, now.plus_seconds(-601.0)).unwrap();
    let home = page(&conn, "/");
    assert!(home.contains("multivac has not reported"));
    assert!(home.contains("The regression in kernel-test"));

    hub::fleet::heartbeat(&mut conn, &beat, now).unwrap();
    hub::fleet_settings::set_machine_state(
        &mut conn,
        "m-m",
        MachineState::Off,
        "another operator",
        now,
    )
    .unwrap();
    let home = page(&conn, "/");
    assert!(home.contains("multivac is off (set by another operator)"));
    assert!(home.contains("The regression in kernel-test"));

    beat.cli_apps.clear();
    hub::fleet::heartbeat(&mut conn, &beat, now).unwrap();
    hub::fleet_settings::set_machine_state(&mut conn, "m-m", MachineState::Active, "operator", now)
        .unwrap();
    let home = page(&conn, "/");
    assert!(home.contains("The regression in kernel-test"));
    assert!(!home.contains("has no dispatcher"));
    assert!(pool(&conn).contains("active (set by operator)"));
    assert!(!pool(&conn).contains("codex CLI"));
}

#[test]
fn fleet_limits_name_the_tickets_holding_master_back() {
    let mut conn = seeded();
    let now = Timestamp::parse(NOW).unwrap();
    let attention = |conn: &Connection| {
        let body = page(conn, "/");
        let first = &body[body.find("id=\"attention\"").expect("an attention section")..];
        first[..first.find("</section>").unwrap()].to_owned()
    };
    let first = attention(&conn);
    assert!(first.contains("Tickets holding master back"), "{first}");
    assert!(first.contains("href=\"/r1\""), "{first}");
    // A pause of every machine hands those tickets to nobody.
    hub::fleet::set_paused_everywhere(&conn, true, now).unwrap();
    let first = attention(&conn);
    assert!(
        first.contains("Hand-offs are paused on every machine, so nobody takes the tickets"),
        "{first}"
    );
    hub::fleet::set_paused_everywhere(&conn, false, now).unwrap();
    hub::fleet_settings::set_machine_state(
        &mut conn,
        "cygnus",
        MachineState::Active,
        "operator",
        now,
    )
    .unwrap();
    assert!(!attention(&conn).contains("Tickets holding master back"));
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
    assert!(dormant.contains("Inherited"));
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

/// The dashboard names the provider, window, percentage and reset for fleet holds.
#[test]
fn the_first_line_names_a_provider_on_a_usage_hold() {
    let conn = seeded();
    let sample = |at: &str, window: &str, percent: f64, resets: &str| {
        hub::provider_usage::record(
            &conn,
            &ignite_core::fleet::ProviderSample {
                machine: "m-j".parse().unwrap(),
                provider: "codex".to_owned(),
                at: at.parse().unwrap(),
                status: "ok".to_owned(),
                label: Some(window.to_owned()),
                percent_used: Some(percent),
                resets_at: Some(resets.to_owned()),
                note: None,
            },
        )
        .unwrap();
    };
    sample(
        "2026-11-17T11:00:00Z",
        "5-hour",
        99.4,
        "2026-11-17T14:00:00Z",
    );
    sample(
        "2026-11-17T11:00:00Z",
        "Weekly · all models",
        40.0,
        "2026-11-20T00:00:00Z",
    );
    sample(
        "2026-11-16T11:00:00Z",
        "Weekly · Fable",
        100.0,
        "2026-11-17T08:00:00Z",
    );
    let body = page(&conn, "/");
    assert!(
        body.contains("codex is on a usage hold (5-hour at 99%, resets 2026-11-17T14:00:00Z)"),
        "{body}"
    );
    assert!(!body.contains("Fable at"), "{body}");

    sample(
        "2026-11-17T11:30:00Z",
        "5-hour",
        20.0,
        "2026-11-17T14:00:00Z",
    );
    assert!(!page(&conn, "/").contains("usage hold"));
}

#[path = "dashboard_fleet/machine_snapshots.rs"]
mod machine_snapshots;

#[path = "dashboard_fleet/machine_settings.rs"]
mod machine_settings_tests;
