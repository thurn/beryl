//! A CLI worker's transcript digest: the hub stores what a machine's daemon appends, including
//! tool results linked to their calls, the session page under the ticket carries it with a box
//! that tells the worker something, a ticket with several sessions links each, a reader can wait
//! for the next line with a long poll, and the public read-only listener serves neither the
//! route nor the page, because a transcript can hold secrets.
#![allow(clippy::format_collect)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use ignite_core::config::Config;
use ignite_core::ids::MachineId;
use ignite_core::protocol::cli::{TranscriptKind, TranscriptLine, TranscriptSession};
use ignite_core::time::Timestamp;
use ignite_hub::dashboard::serve::{Site, serve_with};
use ignite_hub::routes::Request;
use ignite_hub::server::HubServer;
use ignite_store::hub::{self, tokens, transcripts};

const CONFIG: &str = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = "trees"
checks = "checks"

[hub]
url = "http://127.0.0.1:0"
project = "pj"

[gate]
threshold = 0.004
cap = 400

[machine.m-1]
os = "macos"
ticket_trees = 1
landing_trees = 1
trees_disk_gb = 10
floor = { batch = 1, tip = 1 }
"#;

const SESSION: &str = "cli-claude-0a1b2c3d";
const NOW: &str = "2026-11-20T12:00:00.000Z";

fn lines() -> Vec<TranscriptLine> {
    let line = |turn, index, kind, text: &str| TranscriptLine {
        session: SESSION.parse().unwrap(),
        ticket: Some("TT1".parse().unwrap()),
        turn,
        index,
        kind,
        text: text.to_owned(),
        at: None,
        call: None,
    };
    vec![
        line(1, 0, TranscriptKind::Prompt, "Run `ignite start TT1`"),
        line(1, 1, TranscriptKind::Tool, "Bash: ignite status <now>"),
        TranscriptLine {
            call: Some(1),
            ..line(1, 2, TranscriptKind::ToolResult, "</script> ticket TT1")
        },
        line(1, 3, TranscriptKind::Assistant, "Reading the files"),
        line(1, 4, TranscriptKind::Result, "ended cleanly"),
    ]
}

/// A database with one CLI worker holding a ticket and a few digest lines.
fn seeded() -> rusqlite::Connection {
    let conn = held();
    let now = Timestamp::parse(NOW).unwrap();
    transcripts::append(&conn, &MachineId::new("m-1").unwrap(), &lines(), now).unwrap();
    conn
}

/// A database with one CLI worker holding a ticket, and no digest lines.
fn held() -> rusqlite::Connection {
    let conn = hub::open_in_memory().unwrap();
    conn.execute_batch(
        "INSERT INTO machines (id, name, os, cores, memory_mb, version, last_seen)
         VALUES ('m-1', 'jarvis', 'macos', 8, 16000, 'abc', '2026-11-20T11:59:00.000Z');
         INSERT INTO tickets (id, title, status, labels, body, created_at, updated_at, created_by, version)
         VALUES ('TT1', 'Held ticket', 'in_progress', '[\"tier:sonnet\"]', '',
                 '2026-11-20T10:00:00.000Z', '2026-11-20T10:00:00.000Z', 'Derek', 5);
         INSERT INTO workers (machine, session, app, role, state, ticket)
         VALUES ('m-1', 'cli-claude-0a1b2c3d', 'claude', 'worker', 'working', 'TT1');",
    )
    .unwrap();
    conn
}

fn page(conn: &rusqlite::Connection, path: &str, private: bool, editable: bool) -> (u16, String) {
    let (status, _, body) = reply(conn, path, private, editable);
    (status, body)
}

/// The status, the `Location` header and the body of a `GET` of `path`.
fn reply(
    conn: &rusqlite::Connection,
    path: &str,
    private: bool,
    editable: bool,
) -> (u16, Option<String>, String) {
    let request = Request {
        method: "GET".to_owned(),
        path: path.to_owned(),
        query: Vec::new(),
        writer: None,
        body: Vec::new(),
        headers: Vec::new(),
    };
    let site = Site {
        project: "pj",
        wordmark: None,
        editable,
        private,
        pool: None,
    };
    let reply = serve_with(conn, &request, &site, Timestamp::parse(NOW).unwrap()).unwrap();
    let location = reply
        .headers
        .iter()
        .find(|(name, _)| *name == "Location")
        .map(|(_, value)| value.clone());
    (
        reply.status,
        location,
        String::from_utf8(reply.body).unwrap(),
    )
}

#[test]
fn the_session_page_shows_the_lines_and_the_tell_box() {
    let conn = seeded();
    let (status, html) = page(&conn, &format!("/TT1/session/{SESSION}"), true, true);
    assert_eq!(status, 200, "{html}");
    for expected in [
        "Run `ignite start TT1`",
        "Bash: ignite status \\u003cnow\\u003e",
        "\"kind\":\"tool_result\"",
        "\"call\":1",
        "Reading the files",
        "ended cleanly",
        "sonnet",
        "data-tell",
        "name=\"now\"",
        "data-ticket=\"TT1\"",
        "data-after=\"5\"",
    ] {
        assert!(
            html.to_lowercase().contains(&expected.to_lowercase()),
            "{expected}: {html}"
        );
    }
    // The last line is the turn's end, so the header says the worker waits between turns,
    // with a muted dot, though its worker row still says `working`.
    assert!(html.contains("turn 1 ended"), "{html}");
    assert!(html.contains("waiting") && !html.contains(">working<"), "{html}");
    // While a turn runs, the header shows the worker's own state.
    let running = held();
    transcripts::append(
        &running,
        &MachineId::new("m-1").unwrap(),
        &lines()[..4],
        Timestamp::parse(NOW).unwrap(),
    )
    .unwrap();
    let (_, html) = page(&running, &format!("/TT1/session/{SESSION}"), true, true);
    assert!(html.contains(">working<") && !html.contains("turn 1 ended"), "{html}");
    // A line's text cannot end the element that carries the lines as JSON.
    assert!(!html.contains("</script> ticket"), "{html}");
    // The script renders the lines, so the page has no copy of them as HTML rows.
    assert!(!html.contains("class=\"c-transcript-line\""));
    // Without edit controls the page shows the lines but no box.
    let (status, html) = page(&conn, &format!("/TT1/session/{SESSION}"), true, false);
    assert_eq!(status, 200);
    assert!(html.contains("Reading the files") && !html.contains("data-tell"));
    // `/<ticket>/session` is the ticket's latest session.
    let (status, html) = page(&conn, "/TT1/session", true, true);
    assert_eq!(status, 200);
    assert!(html.contains("Reading the files") && html.contains(SESSION));
    // A session that never worked the ticket, and a ticket without sessions, are 404.
    assert_eq!(
        page(&conn, "/TT1/session/cli-claude-ffffffff", true, true).0,
        404
    );
    assert_eq!(page(&conn, "/TT2/session", true, true).0, 404);
}

#[test]
fn the_session_page_shows_the_model_and_effort_the_daemon_reported() {
    let conn = seeded();
    // Before any report, the ticket's tier stands in for the model.
    let (_, html) = page(&conn, "/TT1/session", true, true);
    assert!(html.contains("class=\"c-session-model\">sonnet<"), "{html}");
    assert!(html.contains("effort unknown"), "{html}");
    let run = TranscriptSession {
        session: SESSION.parse().unwrap(),
        model: Some("claude-opus-5-5".to_owned()),
        effort: Some("high".to_owned()),
    };
    transcripts::record_sessions(&conn, &[run], Timestamp::parse(NOW).unwrap()).unwrap();
    let (_, html) = page(&conn, "/TT1/session", true, true);
    assert!(
        html.contains("class=\"c-session-model\">claude-opus-5-5<"),
        "{html}"
    );
    assert!(html.contains("high effort"), "{html}");
}

#[test]
fn the_first_address_of_a_session_redirects_to_its_ticket() {
    let conn = seeded();
    let (status, location, _) = reply(&conn, &format!("/pool/session/{SESSION}"), true, true);
    assert_eq!(status, 308);
    assert_eq!(
        location.as_deref(),
        Some(format!("/TT1/session/{SESSION}").as_str())
    );
    // Another name for the ticket redirects to its own.
    let (status, location, _) = reply(&conn, "/tt1/session/", true, true);
    assert_eq!(status, 308);
    assert_eq!(location.as_deref(), Some("/TT1/session"));
    assert_eq!(
        page(&conn, "/pool/session/cli-claude-ffffffff", true, true).0,
        404
    );
}

#[test]
fn a_ticket_with_two_sessions_links_both_and_shows_each_ones_own_lines() {
    let conn = held();
    // An earlier session gave TT1 up; the current one goes on to TT2 along a lane later.
    let earlier = "cli-claude-99999999";
    let line = |session: &str, ticket: &str, index, text: &str| TranscriptLine {
        session: session.parse().unwrap(),
        ticket: Some(ticket.parse().unwrap()),
        turn: 1,
        index,
        kind: TranscriptKind::Assistant,
        text: text.to_owned(),
        at: None,
        call: None,
    };
    let now = Timestamp::parse(NOW).unwrap();
    let machine = MachineId::new("m-1").unwrap();
    transcripts::append(
        &conn,
        &machine,
        &[line(earlier, "TT1", 0, "Giving it up")],
        now,
    )
    .unwrap();
    transcripts::append(&conn, &machine, &lines(), now).unwrap();
    transcripts::append(
        &conn,
        &machine,
        &[line(SESSION, "TT2", 9, "On the next ticket")],
        now,
    )
    .unwrap();

    let (status, latest) = page(&conn, "/TT1/session", true, true);
    assert_eq!(status, 200);
    assert!(latest.contains("Reading the files"), "{latest}");
    assert!(!latest.contains("Giving it up") && !latest.contains("On the next ticket"));
    assert!(
        latest.contains(&format!("href=\"/TT1/session/{earlier}\"")),
        "{latest}"
    );
    let (status, first) = page(&conn, &format!("/TT1/session/{earlier}"), true, true);
    assert_eq!(status, 200);
    assert!(first.contains("Giving it up") && !first.contains("Reading the files"));
    // The earlier session no longer holds the ticket, so there is no box to tell it something.
    assert!(!first.contains("data-tell"));
    assert!(first.contains(&format!("href=\"/TT1/session/{SESSION}\"")));

    // The ticket's sidebar links its sessions on the private listener only.
    let (_, ticket) = page(&conn, "/TT1", true, true);
    assert!(ticket.contains("href=\"/TT1/session\""), "{ticket}");
    assert!(ticket.contains("2 sessions"), "{ticket}");
    let (_, public) = page(&conn, "/TT1", false, false);
    assert!(!public.contains("/TT1/session"));
}

#[test]
fn the_pool_page_links_a_cli_worker_on_the_private_listener_only() {
    let conn = seeded();
    let link = format!("/TT1/session/{SESSION}");
    assert!(page(&conn, "/pool", true, true).1.contains(&link));
    assert!(!page(&conn, "/pool", false, false).1.contains(&link));
}

#[test]
fn the_session_page_is_a_404_when_the_listener_is_not_private() {
    let conn = seeded();
    for path in [
        format!("/TT1/session/{SESSION}"),
        "/TT1/session".to_owned(),
        format!("/pool/session/{SESSION}"),
    ] {
        let (status, html) = page(&conn, &path, false, false);
        assert_eq!(status, 404, "{path}");
        assert!(!html.contains("Reading the files"));
    }
}

/// One request's status code and body.
fn send(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &[&str],
    body: &str,
) -> (u16, String) {
    let extra: String = headers
        .iter()
        .map(|header| format!("{header}\r\n"))
        .collect();
    let message = format!(
        "{method} {path} HTTP/1.0\r\nHost: hub\r\nConnection: close\r\n{extra}\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(message.as_bytes()).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    let status = reply.split(' ').nth(1).unwrap().parse().unwrap();
    let body = reply.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    (status, body.to_owned())
}

/// An append of the digest lines with the session's model and effort.
fn append_body() -> String {
    json!({
        "machine": "m-1",
        "lines": lines(),
        "sessions": [{"session": SESSION, "model": "claude-opus-5-5", "effort": "high"}],
    })
    .to_string()
}

/// The model and effort stored for the session in the database at `path`.
fn stored_run(path: &std::path::Path) -> (String, String) {
    let run = transcripts::session_run(&hub::open(path).unwrap(), SESSION)
        .unwrap()
        .unwrap();
    (run.model.unwrap(), run.effort.unwrap())
}

#[test]
fn the_hub_stores_a_line_once_and_the_public_listener_answers_404() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-transcripts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let database = dir.join("hub.sqlite");
    let token = {
        let conn = hub::open(&database).unwrap();
        tokens::issue(&conn, &"m-1".parse().unwrap()).unwrap()
    };
    let server = HubServer {
        config: Config::parse("ignite.toml", CONFIG).unwrap(),
        listen: "127.0.0.1:0".to_owned(),
        database,
        wordmark: None,
    };
    let public = server.start_read_only().unwrap();
    let private = server.start().unwrap();
    let auth = [
        format!("Authorization: Bearer {token}"),
        "X-Actor: s1".to_owned(),
    ];
    let auth: Vec<&str> = auth.iter().map(String::as_str).collect();
    let body = append_body();

    let append = |headers: &[&str], body: &str| {
        send(
            private.address(),
            "POST",
            "/pj/api/transcripts/append",
            headers,
            body,
        )
    };
    // Appending needs a token, and the token's machine.
    assert_eq!(append(&[], &body).0, 401);
    let other = json!({"machine": "m-2", "lines": lines()}).to_string();
    assert_eq!(append(&auth, &other).0, 403);
    // The same lines twice are stored once.
    let (status, first) = append(&auth, &body);
    assert_eq!(status, 200, "{first}");
    assert_eq!(serde_json::from_str::<Value>(&first).unwrap()["stored"], 5);
    let (_, second) = append(&auth, &body);
    assert_eq!(serde_json::from_str::<Value>(&second).unwrap()["stored"], 0);
    // The session's model and effort are stored with the lines.
    assert_eq!(
        stored_run(&dir.join("hub.sqlite")),
        ("claude-opus-5-5".into(), "high".into())
    );

    let route = format!("/pj/api/transcripts/{SESSION}");
    let (status, read) = send(private.address(), "GET", &route, &[], "");
    assert_eq!(status, 200);
    let read: Value = serde_json::from_str(&read).unwrap();
    assert_eq!(read["lines"].as_array().unwrap().len(), 5);
    assert_eq!(read["lines"][2]["kind"], "tool_result");
    assert_eq!(read["lines"][2]["call"], 1);
    assert!(read["lines"][1].get("call").is_none());
    let seq = read["lines"][2]["seq"].as_i64().unwrap();
    let (_, later) = send(
        private.address(),
        "GET",
        &format!("{route}?after={seq}"),
        &[],
        "",
    );
    let later: Value = serde_json::from_str(&later).unwrap();
    assert_eq!(later["lines"].as_array().unwrap().len(), 2);

    // The public listener answers 404 for the route and the page, and refuses the append.
    assert_eq!(send(public.address(), "GET", &route, &[], "").0, 404);
    assert_eq!(
        send(
            public.address(),
            "GET",
            &format!("/TT1/session/{SESSION}"),
            &[],
            ""
        )
        .0,
        404
    );
    assert_eq!(
        send(
            public.address(),
            "POST",
            "/pj/api/transcripts/append",
            &auth,
            &body
        )
        .0,
        405
    );
    // The private listener sends the page's first address on to the session's ticket. The
    // ticket itself is not in this database, so the pages are covered by the tests above.
    let (status, _) = send(
        private.address(),
        "GET",
        &format!("/pool/session/{SESSION}"),
        &[],
        "",
    );
    assert_eq!(status, 308);

    public.stop();
    private.stop();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_waiting_reader_gets_the_next_line_as_soon_as_it_is_appended() {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-transcripts-wait-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let database = dir.join("hub.sqlite");
    let token = {
        let conn = hub::open(&database).unwrap();
        tokens::issue(&conn, &"m-1".parse().unwrap()).unwrap()
    };
    let server = HubServer {
        config: Config::parse("ignite.toml", CONFIG).unwrap(),
        listen: "127.0.0.1:0".to_owned(),
        database,
        wordmark: None,
    };
    let private = server.start().unwrap();
    let address = private.address();
    let route = format!("/pj/api/transcripts/{SESSION}");

    // With nothing after `after` and no wait, the answer is empty at once.
    let (status, empty) = send(address, "GET", &format!("{route}?after=0"), &[], "");
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&empty).unwrap()["lines"],
        json!([])
    );
    assert_eq!(
        send(address, "GET", &format!("{route}?after=0&wait=x"), &[], "").0,
        400
    );

    // A reader waits as long as it may (two minutes); the append ends its wait.
    let (reply, replied) = mpsc::channel();
    let waiting = format!("{route}?after=0&wait=120");
    let reader = thread::spawn(move || {
        let answer = send(address, "GET", &waiting, &[], "");
        reply.send(()).unwrap();
        answer
    });
    // The reader is still waiting while nothing is appended.
    assert!(replied.recv_timeout(Duration::from_millis(300)).is_err());
    let auth = [
        format!("Authorization: Bearer {token}"),
        "X-Actor: s1".to_owned(),
    ];
    let auth: Vec<&str> = auth.iter().map(String::as_str).collect();
    let body = json!({"machine": "m-1", "lines": &lines()[..1]}).to_string();
    let (status, _) = send(address, "POST", "/pj/api/transcripts/append", &auth, &body);
    assert_eq!(status, 200);
    let (status, read) = reader.join().unwrap();
    assert_eq!(status, 200);
    let read: Value = serde_json::from_str(&read).unwrap();
    assert_eq!(read["lines"][0]["text"], "Run `ignite start TT1`");

    private.stop();
    let _ = std::fs::remove_dir_all(dir);
}
