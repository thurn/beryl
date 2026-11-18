//! The ticket views against a seeded hub database: what each page shows, revalidation, redirects
//! and the status fragment.
#![allow(clippy::format_collect)]

use rusqlite::{Connection, params};
use serde_json::{Value, json};

use ignite_core::protocol::tickets::BatchRequest;
use ignite_core::time::Timestamp;
use ignite_hub::dashboard::serve::{alias_redirect, serve, static_file};
use ignite_hub::routes::{Request, Response};
use ignite_store::hub::{self, batch};

const NOW: &str = "2026-11-17T12:00:00.000Z";

struct Hub {
    conn: Connection,
}

fn request(method: &str, target: &str, tag: Option<&str>) -> Request {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    Request {
        method: method.to_owned(),
        path: path.strip_prefix("/proj").unwrap_or(path).to_owned(),
        query: query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
        writer: None,
        body: Vec::new(),
        headers: tag
            .map(|tag| ("if-none-match".to_owned(), tag.to_owned()))
            .into_iter()
            .collect(),
    }
}

fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

impl Hub {
    fn get(&self, target: &str) -> Response {
        self.request("GET", target, None)
    }

    fn request(&self, method: &str, target: &str, tag: Option<&str>) -> Response {
        let now = Timestamp::parse(NOW).unwrap();
        serve(&self.conn, &request(method, target, tag), "proj", now).expect("a dashboard route")
    }

    fn text(&self, target: &str) -> String {
        String::from_utf8(self.get(target).body).unwrap()
    }

    fn batch(&mut self, op: &Value) {
        let request: BatchRequest = serde_json::from_value(json!({"ops": [op]})).unwrap();
        let outcome = batch::apply(
            &mut self.conn,
            &request,
            "Derek",
            "m-test",
            Timestamp::parse(NOW).unwrap(),
        )
        .unwrap();
        assert!(matches!(outcome, batch::BatchOutcome::Applied(_)), "{op}");
    }
}

fn seeded() -> Hub {
    let conn = hub::open_in_memory().unwrap();
    conn.execute_batch(
        "WITH RECURSIVE numbers(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM numbers WHERE n < 4992)
         INSERT INTO tickets (id, parent, title, status, body, created_at, updated_at, created_by, version)
         SELECT CASE WHEN n <= 200 THEN printf('root%04d', n) ELSE printf('q44s.%d', n + 20) END,
                CASE WHEN n > 200 THEN 'q44s' END, printf('Fixture ticket %d', n),
                CASE WHEN n <= 20 THEN 'in_progress' WHEN n <= 200 THEN 'open' ELSE 'closed' END,
                'Fixture issue body', '2026-11-15T19:30:00.000Z', '2026-11-15T19:30:00.000Z', 'Sim actor', n
         FROM numbers;
         INSERT INTO tickets (id, parent, title, type, status, labels, body, created_at, updated_at, created_by, version)
         VALUES
          ('q44s', NULL, 'Dashboard epic', 'epic', 'open', '[]', 'Collect all dashboard work',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('q44s.7', 'q44s', 'Render milestone', 'milestone', 'open', '[]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('q44s.7.24', 'q44s.7', 'Render live tickets <safely>', 'task', 'in_progress', '[\"lane:ui\"]',
           'Read proj-q44s, q44s.7, and Terro for **context**. See `proj-path` and [proj-path](/proj/tickets/proj-path?ref=notes).',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('q44s.7.24.1', 'q44s.7.24', 'Render detail rows', 'task', 'open', '[]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('erro', NULL, 'Simulator failure', 'bug', 'blocked', '[\"role:steward\"]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('brpt', NULL, 'Filed from the app', 'bug', 'open', '[\"bug-report\"]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('wait', NULL, 'Wait for deployment', 'task', 'deferred', '[]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('path', NULL, 'Validate the old path', 'task', 'open', '[]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992);
         UPDATE tickets SET assignee = 'local_ff743711', session = 'local_ff743711', machine = 'm-one',
                            claimed_at = '2026-11-16T19:31:00.000Z' WHERE id = 'q44s.7.24';
         UPDATE tickets SET metadata = '{\"_enact_beads\":{\"owner\":\"Old owner\",\"started_at\":\"2026-11-16T12:00:00Z\",
           \"dependencies\":[{\"type\":\"blocks\"}]},\"enact_host\":\"test-mac\"}' WHERE id = 'wait';
         INSERT INTO edges VALUES ('q44s.7.24', 'erro', 'blocks'), ('path', 'q44s.7.24', 'waits-for');
         INSERT INTO aliases VALUES ('old', 'q44s.7.24');
         INSERT INTO comments (ticket, n, author, at, text)
           VALUES ('q44s.7.24', 1, 'Derek', '2026-11-16T20:00:00.000Z', 'See proj-path.');
         INSERT INTO history VALUES ('q44s.7.24', 4993, 'Derek', '2026-11-16T19:30:00.000Z',
           '{\"title\":[\"Render tickets\",\"Render live tickets <safely>\"]}');
         INSERT INTO history VALUES ('q44s.7.24', 0, 'Derek', '2026-11-16T19:30:00.000Z',
           '[{\"actor\":\"Derek\",\"created_at\":\"2026-11-15T19:30:00Z\",\"event_type\":\"created\"}]');",
    )
    .unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO meta VALUES ('seq', ?1)",
        params!["4993"],
    )
    .unwrap();
    Hub { conn }
}

fn count(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}

#[test]
fn the_home_page_lists_the_open_tickets_with_progress() {
    let hub = seeded();
    let reply = hub.get("/proj/");
    assert_eq!(reply.status, 200);
    let home = String::from_utf8(reply.body).unwrap();
    assert_eq!(count(&home, "class=\"c-card\""), 226);
    for id in [
        "active",
        "all",
        "epics",
        "tickets",
        "automatic",
        "bug-reports",
        "deferred",
    ] {
        assert!(home.contains(&format!("href=\"#{id}\"")), "{id}");
    }
    for expected in [
        "Slow CI failures",
        "href=\"/proj/tickets/Tq44s.7.24\"",
        "class=\"c-mono\">Tq44s</",
        "4792/4795",
        "<title>Tickets · Proj</title>",
        "<meta name=\"cirrus-root\" content=\"/proj/\">",
        "data-feed-seq=\"4993\"",
    ] {
        assert!(home.contains(expected), "{expected}");
    }
}

#[test]
fn a_ticket_page_shows_its_sections_links_and_changelog() {
    let hub = seeded();
    let page = hub.text("/proj/tickets/Tq44s.7.24");
    for expected in [
        "Description",
        "Comments",
        "Changelog",
        "Subtasks",
        "Blocked by",
        "Blocking",
        "Assignee",
        "Epic",
        "In progress",
        "Render live tickets &lt;safely&gt;",
        "session ff7437",
        "Validate the old path",
        "Simulator failure",
        "<strong>context</strong>",
        "href=\"/proj/tickets/Tq44s\"",
        "href=\"/proj/tickets/Tpath\"",
        "renamed it",
        "created this",
        "class=\"c-mono\">Tq44s.7.24</",
        "href=\"/proj/tickets/Tq44s.7\">Tq44s.7</",
        "class=\"c-row-id c-mono c-hide-narrow\">Tq44s.7.24.1</",
        "<code>Tpath</code>",
        "href=\"/proj/tickets/Tpath?ref=notes\">Tpath</a>",
    ] {
        assert!(page.contains(expected), "{expected}");
    }
    assert!(
        page.contains("href=\"/proj/tickets/Terro\"")
            && page.contains("<span class=\"c-mono\">Terro</span></a>")
    );
    assert!(!page.contains("proj-path") && !page.contains("proj-q44s"));
    assert!(
        page.contains("data-feed-tickets=\"q44s.7.24 q44s.7.24.1 path erro q44s.7 q44s\""),
        "{page}"
    );
}

#[test]
fn imported_tickets_show_their_owner_and_start_without_the_import_key() {
    let hub = seeded();
    let page = hub.text("/proj/tickets/Twait");
    assert!(page.contains("<dt>Owner</dt><dd>Old owner</dd>"));
    assert!(page.contains("<dt>Started</dt><dd><time"));
    assert!(page.contains("started work"));
    assert!(page.contains("enact_host"));
    assert!(!page.contains("_enact_beads"));
}

#[test]
fn pages_revalidate_only_when_their_content_changes() {
    let mut hub = seeded();
    let home = hub.get("/proj/");
    let detail = hub.get("/proj/tickets/Tq44s.7.24");
    let (home_tag, detail_tag) = (
        header(&home, "etag").unwrap().to_owned(),
        header(&detail, "etag").unwrap().to_owned(),
    );
    for (path, tag) in [
        ("/proj/", &home_tag),
        ("/proj/tickets/Tq44s.7.24", &detail_tag),
    ] {
        assert!(tag.starts_with("W/\""));
        let unchanged = hub.request("GET", path, Some(&format!("\"another\", {}", &tag[2..])));
        assert_eq!(unchanged.status, 304);
        assert!(unchanged.body.is_empty());
        assert_eq!(header(&unchanged, "etag"), Some(tag.as_str()));
    }

    hub.batch(&json!({"op": "comment", "id": "q44s.7.24", "text": "New dashboard comment"}));
    assert_eq!(hub.request("GET", "/proj/", Some(&home_tag)).status, 304);
    let commented = hub.request("GET", "/proj/tickets/Tq44s.7.24", Some(&detail_tag));
    assert_eq!(commented.status, 200);
    let body = String::from_utf8(commented.body.clone()).unwrap();
    assert!(body.contains("New dashboard comment"));
    assert!(body.contains("data-feed-seq=\"4994\""));

    let tag = header(&commented, "etag").unwrap().to_owned();
    hub.batch(&json!({"op": "update", "id": "erro", "set": {"title": "Renamed failure"}}));
    let changed = hub.request("GET", "/proj/tickets/Tq44s.7.24", Some(&tag));
    assert_eq!(changed.status, 200);
    assert!(
        String::from_utf8(changed.body)
            .unwrap()
            .contains("Renamed failure")
    );

    let before = header(&hub.get("/proj/"), "etag").unwrap().to_owned();
    hub.batch(&json!({"op": "reopen", "id": "q44s.999"}));
    let progressed = hub.request("GET", "/proj/", Some(&before));
    assert_eq!(progressed.status, 200);
    assert!(
        String::from_utf8(progressed.body)
            .unwrap()
            .contains("4791/4795")
    );

    // Moving a ticket out of its epic changes the counts of every ancestor.
    hub.conn
        .execute(
            "UPDATE tickets SET parent = NULL WHERE id = 'q44s.7.24'",
            [],
        )
        .unwrap();
    assert!(hub.text("/proj/").contains("4791/4793"));
}

#[test]
fn other_addresses_redirect_to_the_canonical_page_and_keep_the_query() {
    let hub = seeded();
    for path in [
        "/proj/tickets/proj-q44s.7.24",
        "/proj/tickets/old",
        "/proj/tickets/Tq44s.7.24/",
        "/proj/tickets/q44s.7.24",
    ] {
        let redirect = hub.get(&format!("{path}?from=bookmark"));
        assert_eq!(redirect.status, 308, "{path}");
        assert_eq!(
            header(&redirect, "location"),
            Some("/proj/tickets/Tq44s.7.24?from=bookmark"),
            "{path}"
        );
    }
}

#[test]
fn every_address_of_the_earlier_dashboards_reaches_the_page_and_keeps_the_query() {
    let hub = seeded();
    let query = [("from".to_owned(), "bookmark".to_owned())];
    let aliases = ["beryl".to_owned()];
    let ticket = "/proj/tickets/Tq44s.7.24?from=bookmark";
    // Addresses under the project: the page itself, or a redirect to it.
    for (path, location) in [
        ("/proj", "/proj/?from=bookmark"),
        ("/proj/beads/proj-q44s.7.24", ticket),
        ("/proj/beads/proj-q44s.7.24.html", ticket),
        ("/proj/beads/q44s.7.24.html", ticket),
        ("/proj/beads/old", ticket),
        ("/proj/tickets/proj-q44s.7.24", ticket),
        ("/proj/tickets/old", ticket),
        ("/proj/tickets/q44s.7.24/", ticket),
    ] {
        for method in ["GET", "HEAD"] {
            let response = hub.request(method, &format!("{path}?from=bookmark"), None);
            assert_eq!(response.status, 308, "{method} {path}");
            assert_eq!(header(&response, "location"), Some(location), "{path}");
        }
    }
    for path in ["/proj/", "/proj/tickets/Tq44s.7.24"] {
        assert_eq!(hub.get(path).status, 200, "{path}");
    }
    // Addresses under an old project name redirect, and its API paths are left to the API.
    for (path, location) in [
        ("/beryl", "/proj"),
        ("/beryl/", "/proj/"),
        ("/beryl/tickets/Tq44s.7.24", "/proj/tickets/Tq44s.7.24"),
        (
            "/beryl/beads/proj-q44s.7.24.html",
            "/proj/beads/proj-q44s.7.24.html",
        ),
    ] {
        let response = alias_redirect(path, &query, "proj", &aliases).expect(path);
        assert_eq!(response.status, 308, "{path}");
        assert_eq!(
            header(&response, "location"),
            Some(format!("{location}?from=bookmark").as_str()),
            "{path}"
        );
    }
    for path in [
        "/beryl/api/tickets/x",
        "/beryl/api",
        "/proj/",
        "/beryls/",
        "/other/",
    ] {
        assert!(
            alias_redirect(path, &query, "proj", &aliases).is_none(),
            "{path}"
        );
    }
}

#[test]
fn head_matches_get_without_a_body_and_a_missing_ticket_is_404() {
    let hub = seeded();
    let get = hub.get("/proj/tickets/Tq44s.7.24");
    let head = hub.request("HEAD", "/proj/tickets/Tq44s.7.24", None);
    assert_eq!(header(&head, "etag"), header(&get, "etag"));
    assert!(head.body.is_empty());
    let missing = hub.get("/proj/tickets/missing");
    assert_eq!(missing.status, 404);
    assert!(
        String::from_utf8(missing.body)
            .unwrap()
            .contains("No ticket has this address.")
    );
}

#[test]
fn routes_the_dashboard_does_not_own_are_left_alone() {
    let hub = seeded();
    let now = Timestamp::parse(NOW).unwrap();
    for (method, target) in [
        ("GET", "/proj/tickets/a/b"),
        ("GET", "/proj/nowhere"),
        ("POST", "/proj/"),
    ] {
        assert!(
            serve(&hub.conn, &request(method, target, None), "proj", now).is_none(),
            "{target}"
        );
    }
    assert!(static_file("GET", "/assets/nothing.css").is_none());
    assert!(static_file("POST", "/sw.js").is_none());
}

#[test]
fn the_status_fragment_lists_sessions_and_leases() {
    let hub = seeded();
    hub.conn
        .execute(
            "INSERT INTO leases VALUES ('landing', 'Merge holder', 'm-one', '2026-11-17T12:03:00.000Z', '{}', ?1)",
            [json!([{"holder": "Queued merger", "since": "2026-11-17T11:59:50.000Z"}]).to_string()],
        )
        .unwrap();
    let status = hub.get("/proj/?fragment=status");
    assert_eq!(status.status, 200);
    assert_eq!(header(&status, "x-cirrus-sequence"), Some("4993"));
    assert_eq!(header(&status, "cache-control"), Some("no-store"));
    let html = String::from_utf8(status.body).unwrap();
    for expected in [
        "session ff7437",
        "href=\"/proj/tickets/Tq44s.7.24\">Render live tickets &lt;safely&gt;</a>",
        "m-one",
        "landing lease",
        "Merge holder",
        "Queued merger",
    ] {
        assert!(html.contains(expected), "{expected}");
    }
    assert!(!html.contains("<main") && !html.contains("<script"));

    let live = hub.text("/proj/?fragment=all");
    assert!(
        live.contains("<main class=\"c-main\"")
            && live.contains("<template id=\"c-status-fragment\">")
    );
    assert!(live.contains("Queued merger") && !live.contains("<script"));
    let ticket = hub.text("/proj/tickets/Tq44s.7.24?fragment=all");
    assert!(ticket.starts_with("<title>") && ticket.contains("<main class=\"c-main\""));
}

#[test]
fn an_empty_hub_shows_empty_states() {
    let hub = Hub {
        conn: hub::open_in_memory().unwrap(),
    };
    let home = hub.text("/proj/");
    assert!(home.contains("Nothing is in progress.") && home.contains("No open tickets."));
    let status = hub.text("/proj/?fragment=status");
    assert!(
        status.contains("No active sessions reported.") && status.contains("No lease is held.")
    );
}
