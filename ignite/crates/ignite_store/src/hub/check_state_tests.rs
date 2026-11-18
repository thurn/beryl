use ignite_core::ids::{CheckName, Commit, InputHash, MachineId};
use ignite_core::protocol::hub::{CheckState, FlakyReport};
use ignite_core::time::Timestamp;

use super::{Switch, flaky_checks, flaky_ticket, state, switch};
use crate::hub::{self, checks, rows};

fn day(n: u32) -> Timestamp {
    format!("2026-11-{n:02}T12:00:00.000Z").parse().unwrap()
}

fn listed(conn: &mut rusqlite::Connection, names: &[&str]) {
    let rows: Vec<rows::Check> = names
        .iter()
        .map(|name| rows::Check {
            name: (*name).to_owned(),
            family: "rust".to_owned(),
            kind: "regression".to_owned(),
            state: "on".to_owned(),
            check_group: "group".to_owned(),
        })
        .collect();
    checks::upsert(conn, &rows).unwrap();
}

fn flaky(conn: &rusqlite::Connection, check: &str, commit: &str, command: &str, at: u32) {
    let report = FlakyReport {
        check: CheckName::new(check).unwrap(),
        hash: InputHash::new("1".repeat(64)).unwrap(),
        commit: Commit::new(commit).unwrap(),
        machine: MachineId::new("m-1").unwrap(),
        always: false,
        command: command.to_owned(),
        output: String::new(),
    };
    checks::record_flaky(conn, &report, day(at)).unwrap();
}

fn by_hand<'a>(check: &'a str, state: CheckState, ticket: Option<&'a str>) -> Switch<'a> {
    Switch {
        check,
        state,
        actor: "local_abc",
        machine: "m-1",
        note: "replaced with a unit test",
        ticket,
        at: day(12),
    }
}

#[test]
fn switching_by_hand_changes_the_state_and_records_who_did_it() {
    let mut conn = hub::open_in_memory().unwrap();
    listed(&mut conn, &["rust:a"]);
    checks::set_state(&conn, "rust:a", CheckState::Off).unwrap();

    let previous = switch(&conn, &by_hand("rust:a", CheckState::On, Some("q1"))).unwrap();
    assert_eq!(previous, Some(CheckState::Off));
    assert_eq!(state(&conn, "rust:a").unwrap(), Some(CheckState::On));
    let recorded: (String, String, String, String, Option<String>) = conn
        .query_row(
            "SELECT state, previous, actor, machine, ticket FROM check_state_changes",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        recorded,
        (
            "on".to_owned(),
            "off".to_owned(),
            "local_abc".to_owned(),
            "m-1".to_owned(),
            Some("q1".to_owned())
        )
    );
}

#[test]
fn switching_to_the_same_state_or_an_unknown_check_writes_nothing() {
    let mut conn = hub::open_in_memory().unwrap();
    listed(&mut conn, &["rust:a"]);
    assert_eq!(
        switch(&conn, &by_hand("rust:a", CheckState::On, None)).unwrap(),
        Some(CheckState::On)
    );
    assert_eq!(
        switch(&conn, &by_hand("rust:nope", CheckState::Off, None)).unwrap(),
        None
    );
    let changes: i64 = conn
        .query_row("SELECT COUNT(*) FROM check_state_changes", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(changes, 0);
}

#[test]
fn a_check_switched_back_on_needs_two_new_flaky_records_to_go_off_again() {
    let mut conn = hub::open_in_memory().unwrap();
    listed(&mut conn, &["rust:a"]);
    flaky(&conn, "rust:a", "aaaa", "", 1);
    flaky(&conn, "rust:a", "bbbb", "", 2);
    assert!(checks::flaky_repeated(&conn, "rust:a").unwrap());
    // What the hub's upkeep does when it switches the check off.
    conn.execute(
        "UPDATE flaky SET filed = 'q1' WHERE check_name = 'rust:a'",
        [],
    )
    .unwrap();
    checks::set_state(&conn, "rust:a", CheckState::Off).unwrap();

    switch(&conn, &by_hand("rust:a", CheckState::On, Some("q1"))).unwrap();
    assert!(!checks::flaky_repeated(&conn, "rust:a").unwrap());
    flaky(&conn, "rust:a", "cccc", "", 13);
    assert!(!checks::flaky_repeated(&conn, "rust:a").unwrap());
    flaky(&conn, "rust:a", "dddd", "", 14);
    assert!(checks::flaky_repeated(&conn, "rust:a").unwrap());
}

#[test]
fn the_flaky_ticket_is_the_newest_one_about_the_check_itself() {
    let mut conn = hub::open_in_memory().unwrap();
    listed(&mut conn, &["rust:a", "rust:b", "rust:c"]);
    assert_eq!(flaky_ticket(&conn, "rust:a").unwrap(), None);
    flaky(&conn, "rust:a", "aaaa", "", 1);
    flaky(&conn, "rust:a", "bbbb", "", 2);
    conn.execute(
        "UPDATE flaky SET filed = 'q1' WHERE check_name = 'rust:a'",
        [],
    )
    .unwrap();
    // A later command in which three checks flaked at once files a ticket for the machine,
    // which is not the check's ticket.
    for check in ["rust:a", "rust:b", "rust:c"] {
        flaky(&conn, check, "cccc", "run-1", 3);
    }
    conn.execute("UPDATE flaky SET filed = 'q2' WHERE command = 'run-1'", [])
        .unwrap();
    assert_eq!(
        flaky_ticket(&conn, "rust:a").unwrap().as_deref(),
        Some("q1")
    );
}

#[test]
fn the_list_holds_checks_that_are_off_flaked_or_were_switched_by_hand() {
    let mut conn = hub::open_in_memory().unwrap();
    listed(
        &mut conn,
        &[
            "rust:off",
            "rust:flaked",
            "rust:switched",
            "rust:quiet",
            "rust:machine",
        ],
    );
    checks::set_state(&conn, "rust:off", CheckState::Off).unwrap();
    flaky(&conn, "rust:flaked", "aaaa", "", 1);
    flaky(&conn, "rust:flaked", "bbbb", "", 2);
    conn.execute(
        "UPDATE flaky SET filed = 'q1' WHERE check_name = 'rust:flaked'",
        [],
    )
    .unwrap();
    // Three checks flaking in one command is the machine, so it lists none of them.
    for check in ["rust:machine", "rust:quiet", "rust:off"] {
        flaky(&conn, check, "cccc", "run-1", 3);
    }
    checks::set_state(&conn, "rust:switched", CheckState::Off).unwrap();
    switch(&conn, &by_hand("rust:switched", CheckState::On, None)).unwrap();

    let list = flaky_checks(&conn).unwrap();
    let names: Vec<&str> = list.iter().map(|c| c.check.as_str()).collect();
    assert_eq!(names, ["rust:flaked", "rust:off", "rust:switched"]);
    let flaked = &list[0];
    assert_eq!(
        (
            flaked.flaky_records,
            flaked.last_flaky,
            flaked.ticket.as_deref()
        ),
        (2, Some(day(2)), Some("q1"))
    );
    assert_eq!((list[1].state, list[1].flaky_records), (CheckState::Off, 0));
    let switched = list[2].switched.as_ref().unwrap();
    assert_eq!(
        (switched.state, switched.actor.as_str(), switched.at),
        (CheckState::On, "local_abc", day(12))
    );
}
