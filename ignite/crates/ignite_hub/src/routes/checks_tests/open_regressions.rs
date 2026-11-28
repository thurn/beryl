//! `/regressions/open`: every check whose regression is not fixed, with the open ticket that
//! holds it and whether the hub has switched it off.

use serde_json::json;

use ignite_core::protocol::hub::CheckState;

use super::TestHub;

#[test]
fn open_regressions_list_a_check_grouped_onto_another_checks_ticket_and_whether_it_is_off() {
    let hub = TestHub::start("open-regressions");
    {
        let conn = hub.conn.lock().unwrap();
        for (id, status) in [("tk.1", "open"), ("tk.2", "closed")] {
            conn.execute(
                "INSERT INTO tickets (id, title, status, created_at, created_by, updated_at,
                                      version)
                 VALUES (?1, 'Regression: rust:a', ?2, 'x', 't', 'x', 0)",
                [id, status],
            )
            .unwrap();
        }
        // `rust:b` was added to the open ticket of `rust:a`, so no title names it.
        for (check, ticket, state) in [
            ("rust:a", "tk.1", "open"),
            ("rust:b", "tk.1", "reopened"),
            ("rust:c", "tk.1", "fixed"),
            ("rust:d", "tk.2", "open"),
        ] {
            conn.execute(
                "INSERT INTO regressions (check_name, ticket, first_bad, last_good, state)
                 VALUES (?1, ?2, 'abcd', '0000', ?3)",
                [check, ticket, state],
            )
            .unwrap();
        }
        // Upkeep switched `rust:b` off, so no gate runs it and a landing must not close tk.1.
        ignite_store::hub::checks::set_state(&conn, "rust:b", CheckState::Off).unwrap();
    }
    let (status, reply) = hub.send("GET", "/regressions/open", None, false);
    assert_eq!(status, 200);
    assert_eq!(
        reply,
        json!({"regressions": [
            {"check": "rust:a", "ticket": "tk.1", "off": false},
            {"check": "rust:b", "ticket": "tk.1", "off": true},
        ]})
    );
}
