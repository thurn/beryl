//! `ignite close` on a regression ticket that still holds a check the hub has switched off.

use ignite_core::ids::TicketId;
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::OpenRegression;

use super::fakes::{World, id, ticket};
use super::{CloseVerb, StartVerb, lock};
use crate::server::Verb;

const WORKER: &str = "local_worker";

/// A world whose worker holds `Tq.48`, a regression ticket holding `rust:a`, which the hub
/// has switched off.
fn holding_an_off_check() -> World {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Regression: rust:a", ""));
    world.hub.set_open_regressions(vec![OpenRegression {
        check: "rust:a".parse().unwrap(),
        ticket: TicketId::new("Tq.48").unwrap(),
        off: true,
    }]);
    let req = world.request("start", &["Tq.48"], "/repo", WORKER);
    StartVerb(world.claims.clone()).run(&world.ctx, &req);
    world
}

fn close(world: &World, args: &[&str]) -> Reply {
    let req = world.request("close", args, "/trees/tree-01", WORKER);
    CloseVerb(world.claims.clone()).run(&world.ctx, &req)
}

#[test]
fn a_regression_ticket_holding_a_switched_off_check_does_not_close() {
    let world = holding_an_off_check();

    let reply = close(&world, &["-m", "Fixed the overlap."]);

    assert_eq!(reply.exit, Exit::CheckFailed, "{}", reply.text);
    assert!(reply.text.contains("rust:a"), "{}", reply.text);
    assert!(
        reply.text.contains("ignite ticket regression move"),
        "{}",
        reply.text
    );
    assert!(lock(&world.desk.closes).is_empty());
}

#[test]
fn the_override_closes_a_regression_ticket_holding_a_switched_off_check() {
    let world = holding_an_off_check();

    let reply = close(&world, &["-m", "Moved by hand.", "--allow-off-checks"]);

    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(
        lock(&world.desk.closes)[0],
        (id("Tq.48"), "Moved by hand.".to_owned())
    );
}
