use super::*;
use ignite_store::local::rows::Tree;

fn session(name: &str) -> SessionId {
    SessionId::new(name).unwrap()
}

fn ticket(name: &str) -> TicketId {
    TicketId::new(name).unwrap()
}

fn at(seconds: f64) -> Timestamp {
    Timestamp::EPOCH.plus_seconds(seconds)
}

/// Leases ticket tree `id` to `owner`'s `ticket`, or frees it with `None`.
fn lease(conn: &Connection, id: &str, owner: Option<(&str, &str)>) {
    trees::upsert(
        conn,
        &Tree {
            id: id.to_owned(),
            path: format!("/trees/{id}"),
            kind: "ticket".to_owned(),
            state: if owner.is_some() { "leased" } else { "free" }.to_owned(),
            ticket: owner.map(|(_, ticket)| ticket.to_owned()),
            session: owner.map(|(session, _)| session.to_owned()),
            branch: None,
            built_commit: None,
            leased_at: None,
            last_used: None,
            owner: None,
        },
    )
    .unwrap();
}

#[test]
fn a_headless_worker_cannot_rush() {
    let conn = ignite_store::local::open_in_memory().unwrap();
    assert_eq!(
        start(&conn, at(0.0), &session("cli-claude-0a1b2c3d"), None),
        Err(Refusal::Worker)
    );
    assert!(in_force(&conn, at(0.0)).is_empty());
}

#[test]
fn a_third_session_is_refused_but_a_holder_renews() {
    let conn = ignite_store::local::open_in_memory().unwrap();
    let (first, renewed) = start(&conn, at(0.0), &session("local_a"), None).unwrap();
    assert!(!renewed);
    start(&conn, at(1.0), &session("local_b"), None).unwrap();
    match start(&conn, at(2.0), &session("local_c"), None) {
        Err(Refusal::Full(holders)) => assert_eq!(holders.len(), PER_MACHINE),
        other => panic!("expected a refusal, got {other:?}"),
    }
    let (again, renewed) = start(&conn, at(60.0), &session("local_a"), None).unwrap();
    assert!(renewed);
    assert_eq!(again.started_at, first.started_at);
    assert_eq!(again.renewed_at, at(60.0));
    let order: Vec<String> = in_force(&conn, at(61.0))
        .iter()
        .map(|rush| rush.session.to_string())
        .collect();
    assert_eq!(order, ["local_a", "local_b"]);
}

#[test]
fn a_rush_lapses_unless_renewed_and_ends_on_request() {
    let conn = ignite_store::local::open_in_memory().unwrap();
    let a = session("local_a");
    let b = session("local_b");
    start(&conn, at(0.0), &a, None).unwrap();
    start(&conn, at(0.0), &b, None).unwrap();
    start(&conn, at(LAPSE_S - 1.0), &b, None).unwrap();
    let left: Vec<SessionId> = in_force(&conn, at(LAPSE_S))
        .into_iter()
        .map(|rush| rush.session)
        .collect();
    assert_eq!(left, std::slice::from_ref(&b));
    assert!(end(&conn, at(LAPSE_S), &b));
    assert!(!end(&conn, at(LAPSE_S), &b));
    assert!(in_force(&conn, at(LAPSE_S)).is_empty());
}

#[test]
fn the_ticket_is_attached_when_the_session_leases_a_tree_and_its_release_ends_the_rush() {
    let conn = ignite_store::local::open_in_memory().unwrap();
    let a = session("local_a");
    start(&conn, at(0.0), &a, None).unwrap();
    assert!(rushed(&conn, at(1.0), None, Some("local_a")));
    assert!(!rushed(&conn, at(1.0), Some(&ticket("TFIX")), None));

    lease(&conn, "tree-01", Some(("local_a", "TFIX")));
    assert_eq!(in_force(&conn, at(2.0))[0].ticket, Some(ticket("TFIX")));
    // A landing's commands name the ticket, not the session.
    assert!(rushed(&conn, at(3.0), Some(&ticket("TFIX")), None));

    // Landing, releasing and closing all give the ticket's tree back.
    lease(&conn, "tree-01", None);
    assert!(in_force(&conn, at(4.0)).is_empty());
    assert!(!rushed(&conn, at(4.0), None, Some("local_a")));
}
