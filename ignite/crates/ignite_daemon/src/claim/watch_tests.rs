use serde_json::json;

use ignite_core::ids::TreeId;
use ignite_core::protocol::client::Exit;
use ignite_store::local::{rows, trees as tree_rows};

use super::fakes::{World, id, session, ticket};
use super::watch::{GRACE_S, NUDGE, QUIET_S, RETRY_S};
use super::{Liveness, StartVerb, lock};
use crate::server::Verb;

const WORKER: &str = "local_worker";
const TREE: &str = "/trees/tree-01";

fn started() -> World {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    start(&world);
    world
}

fn start(world: &World) {
    let req = world.request("start", &["Tq.48"], "/repo", WORKER);
    let reply = StartVerb(world.claims.clone()).run(&world.ctx, &req);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
}

/// The `changed` lines one command from the tree prints.
fn command(world: &World) -> Vec<String> {
    let req = world.request("test", &["x"], TREE, WORKER);
    world.claims.changed(&world.ctx, &req)
}

fn minutes(world: &World, minutes: f64) {
    world.clock.advance(minutes * 60.0);
    world.claims.tick(&world.ctx);
}

#[test]
fn an_idle_session_gets_one_message_then_its_ticket_is_released() {
    let world = started();
    *lock(&world.dispatcher.answer) = Some(Liveness::Idle);
    world.trees.touch("tree-01", "src/wip.rs");
    minutes(&world, 29.0);
    assert!(lock(&world.dispatcher.checks).is_empty());
    minutes(&world, 1.0);
    assert_eq!(lock(&world.dispatcher.checks).len(), 1);
    minutes(&world, 1.0);
    for _ in 0..10 {
        minutes(&world, 1.0);
    }
    let messages = lock(&world.dispatcher.messages).clone();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].1, NUDGE);
    assert!(lock(&world.desk.releases).is_empty());
    minutes(&world, GRACE_S / 60.0 - 10.0);
    assert_eq!(lock(&world.dispatcher.checks).len(), 2);
    minutes(&world, 1.0);
    let releases = lock(&world.desk.releases).clone();
    assert_eq!(releases.len(), 1, "released once");
    assert!(
        releases[0].note.contains("the session is idle"),
        "{}",
        releases[0].note
    );
    assert!(releases[0].note.contains("uncommitted src/wip.rs"));
    assert_eq!(releases[0].saved.uncommitted, ["src/wip.rs"]);
    assert_eq!(lock(&world.trees.resets).len(), 1);
    assert_eq!(lock(&world.dispatcher.messages).len(), 1);
    assert!(world.claims.holder(TREE.as_ref()).is_none());
}

#[test]
fn a_running_session_is_left_alone() {
    let world = started();
    *lock(&world.dispatcher.answer) = Some(Liveness::Running);
    for _ in 0..90 {
        minutes(&world, 1.0);
    }
    assert!(lock(&world.desk.releases).is_empty());
    assert!(lock(&world.dispatcher.messages).is_empty());
    // Each running answer restarts the 30 minutes.
    assert_eq!(lock(&world.dispatcher.checks).len(), 2);
}

#[test]
fn a_failed_lookup_releases_nothing() {
    let world = started();
    *lock(&world.dispatcher.answer) = Some(Liveness::Unknown);
    for _ in 0..120 {
        minutes(&world, 1.0);
    }
    assert!(lock(&world.desk.releases).is_empty());
    assert!(lock(&world.dispatcher.messages).is_empty());
    // It is asked again every few minutes, not every tick.
    let checks = lock(&world.dispatcher.checks).len();
    let most = 2 + ((120.0 * 60.0 - QUIET_S) / RETRY_S) as usize;
    assert!(checks > 1 && checks <= most, "{checks} checks");
}

#[test]
fn a_gone_session_is_released_at_once() {
    let world = started();
    *lock(&world.dispatcher.answer) = Some(Liveness::Gone);
    minutes(&world, 30.0);
    minutes(&world, 1.0);
    assert_eq!(lock(&world.desk.releases).len(), 1);
    assert!(lock(&world.dispatcher.messages).is_empty());
}

#[test]
fn a_failed_save_keeps_the_ticket_held() {
    let world = started();
    *lock(&world.dispatcher.answer) = Some(Liveness::Gone);
    *lock(&world.trees.save_fails) = true;
    minutes(&world, 30.0);
    minutes(&world, 1.0);
    assert!(lock(&world.desk.releases).is_empty());
    assert!(world.claims.holder(TREE.as_ref()).is_some());
}

#[test]
fn a_long_command_keeps_the_ticket_alive() {
    let world = started();
    *lock(&world.dispatcher.answer) = Some(Liveness::Idle);
    lock(&world.commands.running).push(session(WORKER));
    for _ in 0..60 {
        minutes(&world, 1.0);
    }
    assert!(lock(&world.dispatcher.checks).is_empty());
    // The command ends; the server's hook records it, and the 30 minutes start then.
    lock(&world.commands.running).clear();
    command(&world);
    minutes(&world, 29.0);
    assert!(lock(&world.dispatcher.checks).is_empty());
    minutes(&world, 1.0);
    assert_eq!(lock(&world.dispatcher.checks).len(), 1);
}

#[test]
fn a_command_after_the_message_starts_the_quiet_time_again() {
    let world = started();
    *lock(&world.dispatcher.answer) = Some(Liveness::Idle);
    minutes(&world, 30.0);
    minutes(&world, 1.0);
    assert_eq!(lock(&world.dispatcher.messages).len(), 1);
    command(&world);
    for _ in 0..29 {
        minutes(&world, 1.0);
    }
    assert!(lock(&world.desk.releases).is_empty());
    assert_eq!(lock(&world.dispatcher.checks).len(), 1);
}

#[test]
fn another_authors_comment_shows_once_and_the_sessions_own_never() {
    let world = started();
    world.desk.comment(
        "Tq.48",
        "derek",
        "Do not wait for multivac.\nLand when the gate passes.",
    );
    world
        .desk
        .comment("Tq.48", WORKER, "Noting my own progress.");
    world.hub.push_change("ticket", Some(id("Tq.48")));
    let lines = command(&world);
    assert_eq!(
        lines,
        [
            "changed  comment by derek: Do not wait for multivac.",
            "         Land when the gate passes."
        ]
    );
    assert!(command(&world).is_empty());
    // An edit to the acceptance is named, and the next brief is read again.
    world.desk.edit(
        "Tq.48",
        "derek",
        json!({"acceptance": ["a", "b"], "updated_at": [1, 2]}),
    );
    world
        .desk
        .edit("Tq.48", WORKER, json!({"title": ["a", "b"]}));
    world.hub.push_change("ticket", Some(id("Tq.48")));
    let lines = command(&world);
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].starts_with("changed  edit by derek: acceptance"),
        "{lines:?}"
    );
}

#[test]
fn start_shows_nothing_as_changed_and_the_hub_is_read_only_after_the_feed() {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    world
        .desk
        .comment("Tq.48", "derek", "Read the pool code first.");
    start(&world);
    let reads = *lock(&world.desk.detail_reads);
    assert!(command(&world).is_empty());
    assert!(command(&world).is_empty());
    assert_eq!(
        *lock(&world.desk.detail_reads),
        reads,
        "no change in the feed, no read"
    );
    // A change to another ticket does not read this one.
    world.hub.push_change("ticket", Some(id("Tq.77")));
    assert!(command(&world).is_empty());
    assert_eq!(*lock(&world.desk.detail_reads), reads);
}

#[test]
fn more_than_five_changes_are_counted() {
    let world = started();
    for n in 0..7 {
        world.desk.comment("Tq.48", "derek", &format!("note {n}"));
    }
    world.hub.push_change("ticket", Some(id("Tq.48")));
    let lines = command(&world);
    assert_eq!(lines.len(), 6);
    assert_eq!(lines[5], "changed  and 2 more: ignite ticket show Tq.48");
}

#[test]
fn a_restarted_daemon_does_not_repeat_a_change() {
    let path = std::env::temp_dir().join(format!("ignite-claim-restart-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let world = World::with_db(ignite_store::local::open(&path).unwrap());
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    start(&world);
    // The trees module records the lease in the same database.
    let conn = ignite_store::local::open(&path).unwrap();
    tree_rows::upsert(
        &conn,
        &rows::Tree {
            id: "tree-01".to_owned(),
            path: TREE.to_owned(),
            kind: "ticket".to_owned(),
            ticket: Some("Tq.48".to_owned()),
            session: Some(WORKER.to_owned()),
            branch: Some("ignite/Tq.48".to_owned()),
            built_commit: None,
            leased_at: None,
            last_used: None,
            state: tree_rows::State::Leased.as_str().to_owned(),
        },
    )
    .unwrap();
    world.desk.comment("Tq.48", "derek", "First correction.");
    world.hub.push_change("ticket", Some(id("Tq.48")));
    assert_eq!(command(&world).len(), 1);

    let restarted = World::with_db(ignite_store::local::open(&path).unwrap());
    // The same tickets on the hub; a fresh daemon in front of them.
    *lock(&restarted.desk.details) = lock(&world.desk.details).clone();
    let holder = restarted.claims.holder(TREE.as_ref()).unwrap();
    assert_eq!(holder.0, TreeId::new("tree-01").unwrap());
    assert!(
        command(&restarted).is_empty(),
        "the comment was shown before the restart"
    );
    restarted
        .desk
        .comment("Tq.48", "derek", "Second correction.");
    restarted.hub.push_change("ticket", Some(id("Tq.48")));
    assert_eq!(
        command(&restarted),
        ["changed  comment by derek: Second correction."]
    );
    drop(conn);
    let _ = std::fs::remove_file(&path);
}
