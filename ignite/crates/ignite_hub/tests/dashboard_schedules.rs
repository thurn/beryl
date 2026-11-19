//! Meta-tickets on the dashboard: the home page's "Scheduled" section, the schedule on a
//! meta-ticket's page and the link from a copy back to its meta-ticket.

use rusqlite::Connection;
use serde_json::{Value, json};

use ignite_core::protocol::tickets::BatchRequest;
use ignite_core::time::Timestamp;
use ignite_hub::dashboard::serve::serve;
use ignite_hub::routes::Request;
use ignite_store::hub::{self, batch};

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
    let reply = serve(conn, &request, "proj", Timestamp::parse(NOW).unwrap()).unwrap();
    assert_eq!(reply.status, 200, "{path}");
    String::from_utf8(reply.body).unwrap()
}

fn apply(conn: &mut Connection, op: &Value) -> String {
    let request: BatchRequest = serde_json::from_value(json!({"ops": [op]})).unwrap();
    match batch::apply(
        conn,
        &request,
        "Derek",
        "m-test",
        Timestamp::parse(NOW).unwrap(),
    ) {
        Ok(batch::BatchOutcome::Applied(reply)) => reply.results[0].id.to_string(),
        other => panic!("{op}: {other:?}"),
    }
}

#[test]
fn a_meta_ticket_shows_its_schedule_and_its_copies_link_back() {
    let mut conn = hub::open_in_memory().unwrap();
    let meta = apply(
        &mut conn,
        &json!({"op": "create", "fields": {"title": "Sweep stale tickets", "type": "meta",
                                           "every_hours": 6}}),
    );
    assert!(meta.starts_with('M'), "{meta}");

    let home = page(&conn, "/");
    assert!(home.contains("id=\"scheduled\""), "{home}");
    assert!(home.contains(&format!("href=\"/{meta}\"")));

    let detail = page(&conn, &format!("/{meta}"));
    assert!(detail.contains("6 hours"));
    assert!(detail.contains("Next copy"));
    // No copy yet, so the first is due at the meta-ticket's creation.
    assert!(
        detail.contains("datetime=\"2026-11-17T12:00:00"),
        "{detail}"
    );

    let copy = apply(
        &mut conn,
        &json!({"op": "create", "parent": meta, "fields": {"title": "Sweep stale tickets"}}),
    );
    let detail = page(&conn, &format!("/{meta}"));
    assert!(detail.contains("When the open copy closes"));
    let copy_page = page(&conn, &format!("/{copy}"));
    assert!(copy_page.contains("Schedule"), "{copy_page}");

    apply(
        &mut conn,
        &json!({"op": "update", "id": meta, "set": {"status": "deferred"}}),
    );
    assert!(page(&conn, &format!("/{meta}")).contains("Paused"));
}
