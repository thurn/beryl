//! The efficiency section of ticket pages against a seeded hub database: the placeholder on the
//! page, and the strips, hovercards and views of the fragment that fills it.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::time::Timestamp;
use ignite_hub::dashboard::serve::{Site, serve_with};
use ignite_hub::routes::Request;
use ignite_store::hub::{self, ticket_spend};

const NOW: &str = "2026-11-17T12:00:00.000Z";

fn seeded() -> Connection {
    let conn = hub::open_in_memory().unwrap();
    conn.execute_batch(
        "INSERT INTO tickets (id, parent, title, type, status, created_at, updated_at, created_by, version)
         VALUES ('TEPIC', NULL, 'An epic', 'epic', 'open', '2026-11-16T10:00:00Z', '2026-11-16T10:00:00Z', 'Derek', 1),
                ('TEPIC.1', 'TEPIC', 'First child', 'task', 'closed', '2026-11-16T10:00:00Z', '2026-11-16T10:00:00Z', 'Derek', 1),
                ('TEPIC.2', 'TEPIC', 'Second child', 'task', 'open', '2026-11-16T10:00:00Z', '2026-11-16T10:00:00Z', 'Derek', 1),
                ('TNONE', NULL, 'No sessions yet', 'task', 'open', '2026-11-16T10:00:00Z', '2026-11-16T10:00:00Z', 'Derek', 1);",
    )
    .unwrap();
    let now = Timestamp::parse(NOW).unwrap();
    let window = |ticket: &str, start: &str, command: &str| -> Value {
        json!({
            "ticket": ticket, "window": format!("claude:{ticket}:{start}"), "provider": "claude",
            "session": format!("local_{ticket}"), "start": start, "end": "2026-11-16T12:00:00Z",
            "time": [
                {"c": "tool", "k": "rust tests", "g": "checks", "p": "compile", "t": 0.0, "s": 120.0, "n": 1, "x": command},
                {"c": "model", "k": "", "g": "model", "t": 120.0, "s": 60.0},
                {"c": "person", "k": "question", "g": "person", "t": 180.0, "s": 3600.0},
                {"c": "tool", "k": "read and search", "g": "tools", "t": 3780.0, "s": 30.0, "n": 4}
            ],
            "dollars": [
                {"c": "tool", "k": "rust tests", "g": "checks", "t": 0.0, "d": 1.5, "r": 2,
                 "tk": [10, 100, 200_000, 400], "cd": [0.0, 0.1, 1.0, 0.4], "m": "claude-test"},
                {"c": "model", "k": "reply", "g": "model", "t": 120.0, "d": 0.5, "r": 1,
                 "tk": [1, 10, 50_000, 100], "cd": [0.0, 0.0, 0.4, 0.1], "m": "claude-test"}
            ]
        })
    };
    ticket_spend::record(
        &conn,
        "m-one",
        &window(
            "TEPIC.1",
            "2026-11-16T10:00:00Z",
            "cargo test --secret-path",
        ),
        now,
    )
    .unwrap();
    ticket_spend::record(
        &conn,
        "m-two",
        &window("TEPIC.2", "2026-11-16T11:00:00Z", "cargo test"),
        now,
    )
    .unwrap();
    conn
}

fn get(conn: &Connection, target: &str, editable: bool) -> String {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let request = Request {
        method: "GET".to_owned(),
        path: path.to_owned(),
        query: query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
        writer: None,
        body: Vec::new(),
        headers: Vec::new(),
    };
    let site = Site {
        project: "proj",
        wordmark: None,
        editable,
    };
    let response = serve_with(conn, &request, &site, Timestamp::parse(NOW).unwrap()).unwrap();
    assert_eq!(response.status, 200, "{target}");
    String::from_utf8(response.body).unwrap()
}

#[test]
fn a_ticket_with_windows_has_an_efficiency_section_between_comments_and_changelog() {
    let conn = seeded();
    let page = get(&conn, "/TEPIC.1", false);
    let section = page
        .find("id=\"efficiency\"")
        .expect("an efficiency section");
    assert!(section < page.find("id=\"changelog\"").unwrap());
    assert!(page.contains("data-include=\"/TEPIC.1?fragment=spend\""));
    assert!(page.contains("c-spend-skeleton"));
    // The epic rolls up its children; a ticket with no windows has no section.
    assert!(get(&conn, "/TEPIC", false).contains("id=\"efficiency\""));
    assert!(!get(&conn, "/TNONE", false).contains("id=\"efficiency\""));
}

#[test]
fn the_fragment_draws_both_strips_with_figures_hovercards_and_views() {
    let conn = seeded();
    let fragment = get(&conn, "/TEPIC.1?fragment=spend", false);
    for expected in [
        "data-side=\"left\"",
        "data-side=\"right\"",
        "Agent time",
        "3m 30s",
        "$2.00",
        "Waited on a person",
        "1h 00m",
        "data-group=\"checks\"",
        "rust tests · compile",
        "Cache read",
        "200K tokens",
        "data-view=\"timeline\"",
        "data-view=\"kind\"",
        "c-spend-hatch",
        "Thinking",
        "Requests that called rust tests",
        "thinking included",
    ] {
        assert!(fragment.contains(expected), "missing {expected}");
    }
    // A single ticket has no merged view and its segments are not links to tickets.
    assert!(!fragment.contains("data-view=\"merged\""));
    assert!(!fragment.contains("<a class=\"c-spend-bar"));
}

#[test]
fn commands_show_only_on_the_private_listener() {
    let conn = seeded();
    assert!(!get(&conn, "/TEPIC.1?fragment=spend", false).contains("--secret-path"));
    assert!(get(&conn, "/TEPIC.1?fragment=spend", true).contains("cargo test --secret-path"));
}

#[test]
fn an_epic_stacks_a_band_per_child_and_offers_a_merged_view() {
    let conn = seeded();
    let fragment = get(&conn, "/TEPIC?fragment=spend", false);
    assert!(fragment.contains("data-view=\"merged\""));
    assert!(fragment.contains("<a class=\"c-spend-bar"));
    assert!(fragment.contains("href=\"/TEPIC.2\""));
    assert!(fragment.contains("Second child"));
    // Both children's agent time and dollars add up.
    assert!(fragment.contains("7m 00s"));
    assert!(fragment.contains("$4.00"));
}
