//! `POST /landed` switching on the landing ticket's own switched-off regression checks that its
//! gate ran and passed.

use ignite_core::protocol::hub::CheckState;
use ignite_store::hub::{check_state, checks, rows};

use super::closing::SWITCHED_ON;
use super::tests::{at, claim, file, landed, open, post, start};

/// Workers once switched such a check on before landing its fix, and every tip round without
/// the fix then failed it again. The landing now asks the hub to switch it on.
#[test]
fn a_landing_switches_on_its_own_off_checks_and_leaves_other_off_checks_alone() {
    let mut conn = open();
    let ids = file(&mut conn, &[("fix a", &[], &[]), ("fix b", &[], &[])]);
    let (mine, other) = (ids[0].as_str(), ids[1].as_str());
    let names = ["unit:a", "unit:b", "unit:c"];
    let rows: Vec<rows::Check> = names
        .iter()
        .map(|name| rows::Check {
            name: (*name).to_owned(),
            family: "unit".to_owned(),
            kind: "regression".to_owned(),
            state: "off".to_owned(),
            check_group: "group".to_owned(),
        })
        .collect();
    checks::upsert(&mut conn, &rows).unwrap();
    // `unit:a` and `unit:c` are `mine`'s regressions, `unit:b` is `other`'s.
    for (check, ticket) in [("unit:a", mine), ("unit:b", other), ("unit:c", mine)] {
        conn.execute(
            "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
             VALUES (?1, ?2, 'b1', 'a1', 'open')",
            [check, ticket],
        )
        .unwrap();
    }
    assert_eq!(
        post(&mut conn, "/claims", &claim(mine, "local_a"), start()).0,
        200
    );
    // The gate passed `unit:a`; `unit:b` is named too, but another ticket holds it.
    let mut body = landed(mine, "local_a", "fixed a");
    body["switch_on"] = serde_json::json!(["unit:a", "unit:b"]);

    let (status, reply) = post(&mut conn, "/landed", &body, at(1.0));

    assert_eq!(status, 200, "{reply}");
    let state = |check: &str| check_state::state(&conn, check).unwrap();
    assert_eq!(state("unit:a"), Some(CheckState::On));
    assert_eq!(
        state("unit:b"),
        Some(CheckState::Off),
        "another ticket's check"
    );
    assert_eq!(
        state("unit:c"),
        Some(CheckState::Off),
        "the gate did not pass it"
    );
    let (actor, machine, note, ticket): (String, String, String, String) = conn
        .query_row(
            "SELECT actor, machine, note, ticket FROM check_state_changes
             WHERE check_name = 'unit:a' AND state = 'on'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        (
            actor.as_str(),
            machine.as_str(),
            note.as_str(),
            ticket.as_str()
        ),
        ("local_a", "m-1", SWITCHED_ON, mine)
    );
}
