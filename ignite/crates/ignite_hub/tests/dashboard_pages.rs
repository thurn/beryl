//! The ticket views against a seeded hub database: what each page shows, revalidation, redirects
//! and the status fragment.
#![allow(clippy::format_collect)]

use rusqlite::{Connection, params};
use serde_json::{Value, json};

use ignite_core::protocol::tickets::BatchRequest;
use ignite_core::time::Timestamp;
use ignite_hub::dashboard::serve::{Site, alias_redirect, serve, serve_with, static_file};
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
        path: path.to_owned(),
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
         SELECT CASE WHEN n <= 200 THEN printf('TROOT%04d', n) ELSE printf('TQ44S.%d', n + 20) END,
                CASE WHEN n > 200 THEN 'TQ44S' END, printf('Fixture ticket %d', n),
                CASE WHEN n <= 20 THEN 'in_progress' WHEN n <= 200 THEN 'open' ELSE 'closed' END,
                'Fixture issue body', '2026-11-15T19:30:00.000Z', '2026-11-15T19:30:00.000Z', 'Sim actor', n
         FROM numbers;
         INSERT INTO tickets (id, parent, title, type, status, labels, body, created_at, updated_at, created_by, version)
         VALUES
          ('TQ44S', NULL, 'Dashboard epic', 'epic', 'open', '[]', 'Collect all dashboard work',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('TQ44S.7', 'TQ44S', 'Render milestone', 'milestone', 'open', '[]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('TQ44S.7.24', 'TQ44S.7', 'Render live tickets <safely>', 'task', 'in_progress', '[\"lane:ui\"]',
           'Read proj-q44s, q44s.7, and Terro for **context**. See `proj-path` and [proj-path](/proj/tickets/proj-path?ref=notes).',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('TQ44S.7.24.1', 'TQ44S.7.24', 'Render detail rows', 'task', 'open', '[]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('TERRO', NULL, 'Simulator failure', 'bug', 'blocked', '[\"justiciar\"]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('TBRPT', NULL, 'Filed from the app', 'bug', 'open', '[\"bug-report\"]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('TWAIT', NULL, 'Wait for deployment', 'task', 'deferred', '[]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992),
          ('TPATH', NULL, 'Validate the old path', 'task', 'open', '[]', '',
           '2026-11-16T19:30:00.000Z', '2026-11-16T19:30:00.000Z', 'Derek', 4992);
         UPDATE tickets SET assignee = 'local_ff743711', session = 'local_ff743711', machine = 'm-one',
                            claimed_at = '2026-11-16T19:31:00.000Z' WHERE id = 'TQ44S.7.24';
         UPDATE tickets SET metadata = '{\"_enact_beads\":{\"owner\":\"Old owner\",\"started_at\":\"2026-11-16T12:00:00Z\",
           \"dependencies\":[{\"type\":\"blocks\"}]},\"enact_host\":\"test-mac\"}' WHERE id = 'TWAIT';
         INSERT INTO edges VALUES ('TQ44S.7.24', 'TERRO', 'blocks'), ('TPATH', 'TQ44S.7.24', 'waits-for');
         INSERT INTO aliases VALUES ('old', 'TQ44S.7.24');
         INSERT INTO comments (ticket, n, author, at, text)
           VALUES ('TQ44S.7.24', 1, 'Derek', '2026-11-16T20:00:00.000Z', 'See proj-path.');
         INSERT INTO history VALUES ('TQ44S.7.24', 4993, 'Derek', '2026-11-16T19:30:00.000Z',
           '{\"title\":[\"Render tickets\",\"Render live tickets <safely>\"]}');
         INSERT INTO history VALUES ('TQ44S.7.24', 0, 'Derek', '2026-11-16T19:30:00.000Z',
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
    let reply = hub.get("/");
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
        "Incidents",
        "href=\"/TQ44S.7.24\"",
        "class=\"c-mono\">TQ44S</",
        "2 unestimated tickets",
        "4792 unestimated closed tickets",
        "Completion estimate unavailable",
        "<title>Tickets · Ignite</title>",
        "<meta name=\"cirrus-root\" content=\"/\">",
        "data-feed-seq=\"4993\"",
    ] {
        assert!(home.contains(expected), "{expected}");
    }
}

#[test]
fn epic_percentage_counts_unfiled_work_and_discloses_the_estimate_on_hover() {
    let mut hub = seeded();
    hub.conn
        .execute_batch(
            "DELETE FROM tickets WHERE parent = 'TQ44S' AND status = 'closed' AND id NOT IN (
                 SELECT id FROM tickets WHERE parent = 'TQ44S' AND status = 'closed' ORDER BY id LIMIT 9
             );
             UPDATE tickets SET estimate_minutes = 10 WHERE parent = 'TQ44S' AND status = 'closed';
             UPDATE tickets SET estimate_minutes = 30, unfiled_minutes = 600 WHERE id = 'TQ44S.7.24';
             UPDATE tickets SET estimate_minutes = 50 WHERE id = 'TQ44S.7.24.1';",
        )
        .unwrap();

    let home = hub.text("/");
    assert!(
        home.contains(
            "1h 30m estimated closed · 11h 20m estimated left · 10h unfiled (agent time)"
        )
    );
    assert!(home.contains(">11%</span>"));
    let milestone = hub.text("/TQ44S.7");
    assert!(
        milestone
            .contains("0m estimated closed · 11h 20m estimated left · 10h unfiled (agent time)")
    );
    assert!(milestone.contains(">0%</span>"));

    hub.batch(&json!({"op": "close", "id": "TQ44S.7.24", "reason": "Review complete"}));
    assert!(
        hub.text("/TQ44S.7")
            .contains("30m estimated closed · 10h 50m estimated left · 10h unfiled")
    );
    hub.batch(&json!({"op": "update", "id": "TQ44S.7.24", "set": {"unfiled_minutes": null}}));
    assert!(hub.text("/TQ44S.7").contains(">37%</span>"));

    hub.batch(&json!({"op": "update", "id": "TQ44S.7.24.1", "set": {"estimate_minutes": null}}));
    let milestone = hub.text("/TQ44S.7");
    assert!(milestone.contains("30m estimated closed · 1 unestimated ticket"));
    assert!(milestone.contains("Completion estimate unavailable"));
}

#[test]
fn a_ticket_page_shows_its_sections_links_and_changelog() {
    let hub = seeded();
    let page = hub.text("/TQ44S.7.24");
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
        "href=\"/TQ44S\"",
        "href=\"/TPATH\"",
        "renamed it",
        "created this",
        "class=\"c-mono\">TQ44S.7.24</",
        "href=\"/TQ44S.7\">TQ44S.7</",
        "class=\"c-row-id c-mono c-hide-narrow\">TQ44S.7.24.1</",
        "<code>TPATH</code>",
        "href=\"/TPATH?ref=notes\">TPATH</a>",
    ] {
        assert!(page.contains(expected), "{expected}");
    }
    assert!(
        page.contains("href=\"/TERRO\"")
            && page.contains("<span class=\"c-mono\">TERRO</span></a>")
    );
    assert!(!page.contains("proj-path") && !page.contains("proj-q44s"));
    assert!(
        page.contains("data-feed-tickets=\"TQ44S.7.24 TQ44S.7.24.1 TPATH TERRO TQ44S.7 TQ44S\""),
        "{page}"
    );
}

#[test]
fn subtasks_come_before_the_description() {
    let hub = seeded();
    let page = hub.text("/TQ44S");
    let subtasks = page.find("id=\"subtasks\"").expect("subtasks section");
    let description = page
        .find("id=\"description\"")
        .expect("description section");
    assert!(subtasks < description);
}

#[test]
fn subtasks_with_equal_attention_sort_by_ticket_number() {
    let hub = seeded();
    for number in [28, 3, 30] {
        hub.conn
            .execute(
                "INSERT INTO tickets (id, parent, title, status, created_at, updated_at, created_by, version)
                 VALUES (?1, 'TQ44S.7', 'Fixture subtask', 'open', ?2, ?2, 'Sim actor', 4992)",
                params![format!("TQ44S.7.{number}"), NOW],
            )
            .unwrap();
    }
    let page = hub.text("/TQ44S.7");
    let row_position = |number| {
        let row = format!("class=\"c-row-id c-mono c-hide-narrow\">TQ44S.7.{number}</");
        page.find(&row).expect("subtask row")
    };
    assert!(row_position(3) < row_position(28));
    assert!(row_position(28) < row_position(30));
}

#[test]
fn closed_subtasks_are_listed_without_a_collapsed_group() {
    let hub = seeded();
    // This epic has closed children. They used to sit behind a "<n> closed" disclosure.
    let page = hub.text("/TQ44S");
    assert!(
        page.contains("class=\"c-row-id c-mono c-hide-narrow\">TQ44S.7</"),
        "{page}"
    );
    assert!(!page.contains(" closed</summary>"), "{page}");
}

#[test]
fn imported_tickets_show_their_owner_and_start_without_the_import_key() {
    let hub = seeded();
    let page = hub.text("/TWAIT");
    assert!(page.contains("<dt>Owner</dt><dd>Old owner</dd>"));
    assert!(page.contains("<dt>Started</dt><dd><time"));
    assert!(page.contains("started work"));
    assert!(page.contains("enact_host"));
    assert!(!page.contains("_enact_beads"));
}

#[test]
fn a_ticket_released_by_two_sessions_says_it_is_held_back() {
    use ignite_core::protocol::hub::ReleaseCause;
    use ignite_store::hub::releases;
    let hub = seeded();
    let now = Timestamp::parse(NOW).unwrap();
    let gave_up = ReleaseCause::GaveUp;
    releases::record(&hub.conn, "TWAIT", "local_a", gave_up, "a", now).unwrap();
    let page = hub.text("/TWAIT");
    assert!(page.contains("<dt>Releases</dt><dd>1</dd>"), "{page}");
    assert!(!page.contains("Held back"));
    releases::record(&hub.conn, "TWAIT", "local_b", gave_up, "b", now).unwrap();
    let page = hub.text("/TWAIT");
    assert!(page.contains("<dt>Releases</dt><dd>2"), "{page}");
    assert!(page.contains("Held back"));
    assert!(page.contains("released 2 times by 2 sessions (local_a, local_b)"));
}

#[test]
fn tickets_name_the_desktop_session_that_filed_them() {
    let mut hub = seeded();
    hub.batch(&json!({"op": "create", "fields": {
        "title": "Filed by a session", "metadata": {"created_by_title": "Status <check>"}
    }}));
    let id: String = hub
        .conn
        .query_row(
            "SELECT id FROM tickets WHERE title = 'Filed by a session'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    hub.conn
        .execute(
            "UPDATE tickets SET created_by = 'local_8c29f0b2' WHERE id = ?1",
            [&id],
        )
        .unwrap();
    let page = hub.text(&format!("/{id}"));
    assert!(
        page.contains(
            "<dt>Created by</dt><dd><a href=\"claude://claude.ai/epitaxy/local_8c29f0b2\" \
             title=\"local_8c29f0b2\">Status &lt;check&gt;</a></dd>"
        ),
        "{page}"
    );
    assert!(!page.contains("created_by_title"));

    hub.conn
        .execute("UPDATE tickets SET metadata = '{}' WHERE id = ?1", [&id])
        .unwrap();
    let untitled = hub.text(&format!("/{id}"));
    assert!(untitled.contains(
        "<dd><a href=\"claude://claude.ai/epitaxy/local_8c29f0b2\" \
         title=\"local_8c29f0b2\">local_8c29f0b2</a></dd>"
    ));
    assert!(
        hub.text("/TWAIT")
            .contains("<dt>Created by</dt><dd>Derek</dd>")
    );
}

#[test]
fn sessions_that_reported_a_title_are_named_by_it_everywhere() {
    let hub = seeded();
    let now = Timestamp::parse(NOW).unwrap();
    hub::session_titles::record(&hub.conn, "local_ff743711", "Render worker", now).unwrap();
    hub::session_titles::record(&hub.conn, "Derek", "Derek's planner", now).unwrap();
    let page = hub.text("/TQ44S.7.24");
    let worker = "<span class=\"c-tip-target\" tabindex=\"0\" data-tip=\"local_ff743711\">\
                  Render worker</span>";
    assert!(page.contains(worker), "{page}");
    assert!(!page.contains("session ff7437"));
    assert!(page.contains("Derek&#39;s planner") || page.contains("Derek&#x27;s planner"));
    let status = hub.text("/?fragment=status");
    assert!(status.contains("Render worker"), "{status}");
}

#[test]
fn a_title_naming_another_ticket_is_not_shown_for_its_assignee() {
    let hub = seeded();
    let now = Timestamp::parse(NOW).unwrap();
    hub::session_titles::record(&hub.conn, "local_ff743711", "⚒️ [TW4YS] Earlier work", now)
        .unwrap();
    let page = hub.text("/TQ44S.7.24");
    assert!(!page.contains("Earlier work"), "{page}");
    assert!(page.contains("session ff7437"), "{page}");
    let status = hub.text("/?fragment=status");
    assert!(!status.contains("Earlier work"), "{status}");

    hub::session_titles::record(
        &hub.conn,
        "local_ff743711",
        "⚒️ [TQ44S.7.24] This work",
        now,
    )
    .unwrap();
    let page = hub.text("/TQ44S.7.24");
    assert!(page.contains("This work"));
    assert!(
        page.contains(
            "<a class=\"c-session-link\" href=\"claude://claude.ai/epitaxy/local_ff743711\">"
        ),
        "{page}"
    );
}

#[test]
fn pages_revalidate_only_when_their_content_changes() {
    let mut hub = seeded();
    let home = hub.get("/");
    let detail = hub.get("/TQ44S.7.24");
    let (home_tag, detail_tag) = (
        header(&home, "etag").unwrap().to_owned(),
        header(&detail, "etag").unwrap().to_owned(),
    );
    for (path, tag) in [("/", &home_tag), ("/TQ44S.7.24", &detail_tag)] {
        assert!(tag.starts_with("W/\""));
        let unchanged = hub.request("GET", path, Some(&format!("\"another\", {}", &tag[2..])));
        assert_eq!(unchanged.status, 304);
        assert!(unchanged.body.is_empty());
        assert_eq!(header(&unchanged, "etag"), Some(tag.as_str()));
    }

    hub.batch(&json!({"op": "comment", "id": "q44s.7.24", "text": "New dashboard comment"}));
    assert_eq!(hub.request("GET", "/", Some(&home_tag)).status, 304);
    let commented = hub.request("GET", "/TQ44S.7.24", Some(&detail_tag));
    assert_eq!(commented.status, 200);
    let body = String::from_utf8(commented.body.clone()).unwrap();
    assert!(body.contains("New dashboard comment"));
    assert!(body.contains("data-feed-seq=\"4994\""));

    let tag = header(&commented, "etag").unwrap().to_owned();
    hub.batch(&json!({"op": "update", "id": "erro", "set": {"title": "Renamed failure"}}));
    let changed = hub.request("GET", "/TQ44S.7.24", Some(&tag));
    assert_eq!(changed.status, 200);
    assert!(
        String::from_utf8(changed.body)
            .unwrap()
            .contains("Renamed failure")
    );

    let before = header(&hub.get("/"), "etag").unwrap().to_owned();
    hub.batch(&json!({"op": "reopen", "id": "q44s.999"}));
    let progressed = hub.request("GET", "/", Some(&before));
    assert_eq!(progressed.status, 200);
    assert!(
        String::from_utf8(progressed.body)
            .unwrap()
            .contains("3 unestimated tickets · 4791 unestimated closed tickets")
    );

    // Moving a ticket out of its epic changes the forecast of every ancestor.
    hub.conn
        .execute(
            "UPDATE tickets SET parent = NULL WHERE id = 'TQ44S.7.24'",
            [],
        )
        .unwrap();
    assert!(
        hub.text("/")
            .contains("1 unestimated ticket · 4791 unestimated closed tickets")
    );
}

#[test]
fn other_addresses_redirect_to_the_canonical_page_and_keep_the_query() {
    let hub = seeded();
    // Includes the spelling from before IDs were stored in capitals, `/Tq44s.7.24`.
    for path in [
        "/proj-q44s.7.24",
        "/old",
        "/TQ44S.7.24/",
        "/q44s.7.24",
        "/Tq44s.7.24",
    ] {
        let redirect = hub.get(&format!("{path}?from=bookmark"));
        assert_eq!(redirect.status, 308, "{path}");
        assert_eq!(
            header(&redirect, "location"),
            Some("/TQ44S.7.24?from=bookmark"),
            "{path}"
        );
    }
}

#[test]
fn every_address_of_the_earlier_dashboards_reaches_the_page_and_keeps_the_query() {
    let hub = seeded();
    let query = [("from".to_owned(), "bookmark".to_owned())];
    let aliases = ["former".to_owned()];
    let ticket = "/TQ44S.7.24?from=bookmark";
    // The dashboard's earlier ticket addresses redirect to the page at the root.
    for path in [
        "/beads/proj-q44s.7.24",
        "/beads/proj-q44s.7.24.html",
        "/beads/q44s.7.24.html",
        "/beads/old",
        "/tickets/TQ44S.7.24",
        "/tickets/proj-q44s.7.24",
        "/tickets/old",
        "/tickets/q44s.7.24/",
        "/proj-q44s.7.24",
        "/old",
        "/q44s.7.24/",
    ] {
        for method in ["GET", "HEAD"] {
            let response = hub.request(method, &format!("{path}?from=bookmark"), None);
            assert_eq!(response.status, 308, "{method} {path}");
            assert_eq!(header(&response, "location"), Some(ticket), "{path}");
        }
    }
    for path in ["/", "/TQ44S.7.24"] {
        assert_eq!(hub.get(path).status, 200, "{path}");
    }
    // Addresses under the project's name or an old one redirect to the same address at the
    // root, and their API paths are left to the API.
    for (path, location) in [
        ("/former", "/"),
        ("/former/", "/"),
        ("/proj", "/"),
        ("/proj/", "/"),
        ("/proj/machines", "/machines"),
        ("/proj/tickets/TQ44S.7.24", "/tickets/TQ44S.7.24"),
        ("/former/tickets/TQ44S.7.24", "/tickets/TQ44S.7.24"),
        (
            "/former/beads/proj-q44s.7.24.html",
            "/beads/proj-q44s.7.24.html",
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
        "/former/api/tickets/x",
        "/former/api",
        "/proj/api/tickets/x",
        "/proj/api",
        "/",
        "/formers/",
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
    let get = hub.get("/TQ44S.7.24");
    let head = hub.request("HEAD", "/TQ44S.7.24", None);
    assert_eq!(header(&head, "etag"), header(&get, "etag"));
    assert!(head.body.is_empty());
    let missing = hub.get("/missing");
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
    for (method, target) in [("GET", "/a/b"), ("GET", "/nowhere/deeper"), ("POST", "/")] {
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
    let status = hub.get("/?fragment=status");
    assert_eq!(status.status, 200);
    assert_eq!(header(&status, "x-cirrus-sequence"), Some("4993"));
    assert_eq!(header(&status, "cache-control"), Some("no-store"));
    let html = String::from_utf8(status.body).unwrap();
    for expected in [
        "session ff7437",
        "href=\"/TQ44S.7.24\">Render live tickets &lt;safely&gt;</a>",
        "m-one",
        "landing lease",
        "Merge holder",
        "Queued merger",
    ] {
        assert!(html.contains(expected), "{expected}");
    }
    assert!(!html.contains("<main") && !html.contains("<script"));

    let live = hub.text("/?fragment=all");
    assert!(
        live.contains("<main class=\"c-main\"")
            && live.contains("<template id=\"c-status-fragment\">")
    );
    assert!(live.contains("Queued merger") && !live.contains("<script"));
    let ticket = hub.text("/TQ44S.7.24?fragment=all");
    assert!(ticket.starts_with("<title>") && ticket.contains("<main class=\"c-main\""));
}

#[test]
fn an_empty_hub_shows_empty_states() {
    let hub = Hub {
        conn: hub::open_in_memory().unwrap(),
    };
    let home = hub.text("/");
    assert!(home.contains("Nothing is in progress.") && home.contains("No open tickets."));
    let status = hub.text("/?fragment=status");
    assert!(
        status.contains("No active sessions reported.") && status.contains("No lease is held.")
    );
}

#[test]
fn a_configured_wordmark_replaces_the_brand_and_the_home_page_has_no_crumb() {
    let hub = seeded();
    let header = |page: &str| {
        page[page.find("<header").unwrap()..page.find("</header>").unwrap()].to_owned()
    };
    let home = header(&hub.text("/"));
    assert!(home.contains(">Ignite</a></nav>"), "{home}");

    let now = Timestamp::parse(NOW).unwrap();
    let wordmark = "<svg aria-label=\"Mark\"></svg>";
    let page = |target: &str| {
        let reply = serve_with(
            &hub.conn,
            &request("GET", target, None),
            &Site {
                project: "proj",
                wordmark: Some(wordmark),
                editable: false,
                private: false,
            },
            now,
        )
        .expect("a dashboard route");
        header(&String::from_utf8(reply.body).unwrap())
    };
    let home = page("/");
    assert!(
        home.contains(&format!("href=\"/\">{wordmark}</a></nav>")),
        "{home}"
    );
    for target in ["/TQ44S.7.24", "/fleet", "/no-such-ticket"] {
        let other = page(target);
        assert!(other.contains(wordmark), "{target}: {other}");
        assert!(!other.contains(">Ignite<"), "{target}: {other}");
    }
}

#[test]
fn an_epic_lists_its_milestones_in_plan_order() {
    let hub = seeded();
    // M0 was filed last and is closed, M7 is the most urgent and the review gate has no
    // number; the blocking edges and the title numbers still put M0 first.
    hub.conn
        .execute_batch(
            "INSERT INTO tickets (id, parent, title, type, status, priority, created_at, updated_at, created_by, version)
             VALUES
              ('TPLAN', NULL, 'Plan', 'epic', 'open', 2, '', '2026-11-16T19:30:00.000Z', 'Derek', 1),
              ('TPLAN.1', 'TPLAN', 'M1: foundations', 'epic', 'closed', 2, '', '2026-11-16T19:30:00.000Z', 'Derek', 1),
              ('TPLAN.2', 'TPLAN', 'Final review gate', 'epic', 'open', 2, '', '2026-11-16T19:30:00.000Z', 'Derek', 1),
              ('TPLAN.3', 'TPLAN', 'M7: tools', 'epic', 'open', 0, '', '2026-11-16T19:30:00.000Z', 'Derek', 1),
              ('TPLAN.4', 'TPLAN', 'M2: the table plays', 'epic', 'open', 2, '', '2026-11-16T19:30:00.000Z', 'Derek', 1),
              ('TPLAN.14', 'TPLAN', 'M0: a prototype', 'epic', 'closed', 1, '', '2026-11-16T19:30:00.000Z', 'Derek', 1);
             INSERT INTO edges VALUES ('TPLAN.1', 'TPLAN.14', 'blocks'), ('TPLAN.4', 'TPLAN.1', 'blocks'),
               ('TPLAN.2', 'TPLAN.4', 'waits-for');",
        )
        .unwrap();
    let page = hub.text("/TPLAN");
    let at = |title: &str| {
        page.find(title)
            .unwrap_or_else(|| panic!("{title} missing"))
    };
    let order = [
        "M0: a prototype",
        "M1: foundations",
        "M2: the table plays",
        "M7: tools",
        "Final review gate",
    ];
    for pair in order.windows(2) {
        assert!(at(pair[0]) < at(pair[1]), "{} before {}", pair[0], pair[1]);
    }
}

#[test]
fn attention_names_the_first_tip_with_deferred_platform_checks() {
    use ignite_core::config::Os;
    use ignite_core::fleet::MachineState;
    use ignite_core::ids::Commit;
    use ignite_core::tipround::{DeferredPlatform, PlatformVerification, TipCoverageUpdate};
    use ignite_store::hub::{fleet_settings, tip_coverage};

    let mut dashboard = Hub {
        conn: hub::open_in_memory().unwrap(),
    };
    dashboard
        .conn
        .execute(
            "INSERT INTO machines (id, name, os, cores, memory_mb, families, version, last_seen)
         VALUES ('m-c', 'cygnus', 'windows', 4, 4096, '[]', 'abc1234', ?1)",
            [NOW],
        )
        .unwrap();
    let now = Timestamp::parse(NOW).unwrap();
    fleet_settings::set_machine_state(
        &mut dashboard.conn,
        "m-c",
        MachineState::Off,
        "operator",
        now,
    )
    .unwrap();
    let missing = PlatformVerification {
        through: Commit::new("ccc1234").unwrap(),
        deferred: Some(DeferredPlatform {
            since: Commit::new("bbb1234").unwrap(),
            last_good: Commit::new("aaa1234").unwrap(),
        }),
    };
    let defer = TipCoverageUpdate {
        platform: Os::Windows,
        expected: None,
        replacement: missing.clone(),
    };
    assert!(tip_coverage::update(&mut dashboard.conn, &defer).unwrap());
    let warning = "windows checks have not run since bbb1234";
    assert!(
        dashboard
            .text("/")
            .contains(&format!("{warning}: cygnus is off"))
    );

    fleet_settings::set_machine_state(
        &mut dashboard.conn,
        "m-c",
        MachineState::ChecksOnly,
        "operator",
        now,
    )
    .unwrap();
    let unverified = "windows checks have been unverified since bbb1234";
    assert!(dashboard.text("/").contains(unverified));
    let before = hub::feed::current(&dashboard.conn).unwrap();
    assert!(
        tip_coverage::update(
            &mut dashboard.conn,
            &TipCoverageUpdate {
                platform: Os::Windows,
                expected: Some(missing.clone()),
                replacement: PlatformVerification {
                    through: missing.through,
                    deferred: None
                },
            }
        )
        .unwrap()
    );
    assert!(!dashboard.text("/?fragment=all").contains(unverified));

    let changes = hub::feed::since(&dashboard.conn, before, 20)
        .unwrap()
        .changes;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, "attention");

    // A deferred round that finished late must leave the successful verification in place.
    assert!(!tip_coverage::update(&mut dashboard.conn, &defer).unwrap());
    assert!(!dashboard.text("/?fragment=all").contains(unverified));
}
