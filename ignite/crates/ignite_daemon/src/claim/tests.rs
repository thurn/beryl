use std::path::Path;

use ignite_core::ids::Commit;
use ignite_core::protocol::client::Exit;
use ignite_core::protocol::hub::DependencyNote;
use ignite_core::protocol::tickets::LandedWork;

use super::fakes::{World, id, session, ticket};
use super::{BriefVerb, CloseVerb, NEEDS_PERSON, ReleaseVerb, StartVerb, Target, lock};
use crate::server::Verb;

const WORKER: &str = "local_worker";

fn start(world: &World, args: &[&str], who: &str) -> ignite_core::protocol::client::Reply {
    StartVerb(world.claims.clone()).run(&world.ctx, &world.request("start", args, "/repo", who))
}

#[test]
fn the_brief_shows_what_a_landed_dependency_left() {
    let world = World::new();
    world.desk.add(ticket(
        "Tq.48",
        "Counter names, untap lock",
        "Edit `rust/scene/src/badges.rs` and its test.",
    ));
    world.desk.add(ticket("Tq.31", "Badge pool", ""));
    lock(&world.desk.needs).insert(
        id("Tq.48"),
        vec![DependencyNote {
            ticket: id("Tq.31"),
            title: "Badge pool".to_owned(),
            landed: Some(LandedWork {
                commits: vec![Commit::new("2f0c1d9e").unwrap()],
                files: vec!["rust/scene/src/pool.rs".to_owned()],
                note: "Badges draw from one record pool.\nMore detail.".to_owned(),
            }),
            close_reason: None,
        }],
    );
    let reply = start(&world, &["Tq.48"], WORKER);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    let lines: Vec<&str> = reply.text.lines().collect();
    assert_eq!(lines[0], "ticket   Tq.48  Counter names, untap lock");
    assert_eq!(lines[1], "tree     /trees/tree-01  at staging 8c1f2ab");
    assert!(
        reply
            .text
            .contains("title    ⚒️ [Tq.48] Counter names, untap lock")
    );
    assert!(reply.text.contains("files    rust/scene/src/badges.rs"));
    assert!(
        reply
            .text
            .contains("needs    Tq.31 landed 2f0c1d9: \"Badges draw from one record pool.\""),
        "{}",
        reply.text
    );
    assert!(reply.text.contains("touched rust/scene/src/pool.rs"));
    assert!(lines.last().unwrap().starts_with("next "));
    // The hub was asked for this session and the tree it leased.
    let claims = lock(&world.desk.claims);
    assert_eq!(claims[0].tree.as_str(), "tree-01");
    assert_eq!(claims[0].session, session(WORKER));
    // A ticket naming only a source file of unknown area is expected to compile.
    assert_eq!(lock(&world.trees.leases)[0], (id("Tq.48"), true));
}

#[test]
fn brief_prints_the_ticket_again_from_its_tree() {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    start(&world, &["Tq.48"], WORKER);
    let req = world.request("brief", &[], "/trees/tree-01/rust", WORKER);
    let reply = BriefVerb(world.claims.clone()).run(&world.ctx, &req);
    assert_eq!(reply.exit, Exit::Success);
    assert!(reply.text.starts_with("ticket   Tq.48  Counter names"));
}

#[test]
fn a_second_session_starting_the_same_ticket_loses() {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    assert_eq!(start(&world, &["Tq.48"], WORKER).exit, Exit::Success);
    let reply = start(&world, &["Tq.48"], "local_other");
    assert_eq!(reply.exit, Exit::Conflict, "{}", reply.text);
    // The same session starting it again gets its brief, not a second tree.
    assert_eq!(start(&world, &["Tq.48"], WORKER).exit, Exit::Success);
    assert_eq!(lock(&world.trees.leases).len(), 1);
}

#[test]
fn a_lapsed_reservation_code_claims_nothing() {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    lock(&world.desk.reservations).insert(id("Tq.48"), "7f3a".to_owned());
    let reply = start(&world, &["Tq.48", "--reservation", "0000"], WORKER);
    assert_eq!(reply.exit, Exit::Conflict, "{}", reply.text);
    assert!(lock(&world.desk.holders).is_empty());
    // The tree it leased for the claim is free again.
    assert_eq!(lock(&world.trees.resets).len(), 1);
    assert_eq!(lock(&world.trees.free).len(), 2);
    // A code for a ticket nobody reserved claims nothing either.
    world.desk.add(ticket("Tq.49", "Other", ""));
    let reply = start(&world, &["Tq.49", "--reservation=7f3a"], WORKER);
    assert_eq!(reply.exit, Exit::Conflict);
    // The live code claims it.
    let reply = start(&world, &["Tq.48", "--reservation", "7f3a"], WORKER);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
}

#[test]
fn a_summary_files_an_adhoc_ticket() {
    let world = World::new();
    let reply = start(&world, &["Fix the flaky timer test"], WORKER);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    let claims = lock(&world.desk.claims);
    assert_eq!(
        claims[0].target,
        Target::Adhoc("Fix the flaky timer test".to_owned())
    );
    assert!(reply.text.contains("[Tadhoc.1] Fix the flaky timer test"));
    // The tree was leased under a placeholder before the hub numbered the ticket. It takes the
    // hub's number, or its landing is recorded against a ticket the hub has never heard of: on
    // 2026-11-18 a landed change logged "Unknown ticket adhoc20261118141021392" again and again.
    let renamed = lock(&world.trees.renamed).clone();
    assert_eq!(renamed.len(), 1);
    assert_eq!(renamed[0].1.as_str(), "Tadhoc.1");
}

#[test]
fn start_needs_a_session_and_a_ticket() {
    let world = World::new();
    let mut req = world.request("start", &["Tq.48"], "/repo", WORKER);
    req.session = None;
    let reply = StartVerb(world.claims.clone()).run(&world.ctx, &req);
    assert_eq!(reply.exit, Exit::Usage);
    assert_eq!(start(&world, &[], WORKER).exit, Exit::Usage);
    assert_eq!(
        start(&world, &["Tq.48", "--bogus", "x"], WORKER).exit,
        Exit::Usage
    );
    // An unknown ticket is a usage error and leaves no tree leased.
    assert_eq!(start(&world, &["Tq.99"], WORKER).exit, Exit::Usage);
    assert_eq!(lock(&world.trees.free).len(), 2);
}

#[test]
fn a_full_machine_answers_with_a_handle() {
    let world = World::new();
    for n in 1..=3 {
        world.desk.add(ticket(&format!("Tq.{n}"), "x", ""));
    }
    start(&world, &["Tq.1"], "local_a");
    start(&world, &["Tq.2"], "local_b");
    let reply = start(&world, &["Tq.3"], "local_c");
    assert_eq!(reply.exit, Exit::Running);
    assert!(reply.handle.is_some());
    assert!(lock(&world.desk.holders).get(&id("Tq.3")).is_none());
}

fn release(world: &World, args: &[&str], who: &str) -> ignite_core::protocol::client::Reply {
    let req = world.request("release", args, "/trees/tree-01", who);
    ReleaseVerb(world.claims.clone()).run(&world.ctx, &req)
}

#[test]
fn released_uncommitted_work_comes_back_at_the_next_start() {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    start(&world, &["Tq.48"], WORKER);
    world.trees.touch("tree-01", "src/half_done.rs");
    let reply = release(
        &world,
        &["-m", "Needs the new pool first.", "--needs", "macos"],
        WORKER,
    );
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply
            .text
            .starts_with("saved    1 commit and 1 uncommitted file as ignite/Tq.48")
    );
    assert!(
        reply
            .text
            .contains("released Tq.48; tree-01 is free; do not use this path again")
    );
    assert!(reply.text.contains("title    ⛔ [Tq.48] Counter names"));
    let releases = lock(&world.desk.releases).clone();
    assert_eq!(releases[0].note, "Needs the new pool first.");
    assert_eq!(releases[0].needs.as_deref(), Some("macos"));
    assert_eq!(releases[0].saved.uncommitted, ["src/half_done.rs"]);
    // The tree is no longer the session's.
    assert_eq!(release(&world, &["-m", "again"], WORKER).exit, Exit::Usage);

    let reply = start(&world, &["Tq.48"], "local_next");
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains(
            "resumes  ignite/Tq.48: 1 commit by local_old; the last is unfinished work saved at \
             release"
        ),
        "{}",
        reply.text
    );
    let files = lock(&world.trees.files).clone();
    assert_eq!(files.values().next().unwrap(), &["src/half_done.rs"]);
}

#[test]
fn a_release_whose_push_fails_keeps_the_ticket() {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    start(&world, &["Tq.48"], WORKER);
    *lock(&world.trees.save_fails) = true;
    let reply = release(&world, &["-m", "stuck"], WORKER);
    assert_eq!(reply.exit, Exit::Unreachable, "{}", reply.text);
    assert!(lock(&world.desk.releases).is_empty());
    assert!(lock(&world.trees.resets).is_empty());
    assert_eq!(
        lock(&world.desk.holders).get(&id("Tq.48")),
        Some(&session(WORKER))
    );
    // The session still holds the tree and can release once the push goes through.
    *lock(&world.trees.save_fails) = false;
    assert_eq!(
        release(&world, &["-m", "stuck"], WORKER).exit,
        Exit::Success
    );
}

#[test]
fn release_and_close_refuse_other_sessions_and_bad_arguments() {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    start(&world, &["Tq.48"], WORKER);
    assert_eq!(
        release(&world, &["-m", "x"], "local_other").exit,
        Exit::Conflict
    );
    assert_eq!(
        release(&world, &["--needs", NEEDS_PERSON], WORKER).exit,
        Exit::Usage
    );
    let close = |args: &[&str]| {
        let req = world.request("close", args, "/trees/tree-01", WORKER);
        CloseVerb(world.claims.clone()).run(&world.ctx, &req)
    };
    assert_eq!(close(&[]).exit, Exit::Usage);
    assert!(lock(&world.desk.releases).is_empty());
    assert!(lock(&world.desk.closes).is_empty());
}

#[test]
fn close_frees_the_tree_and_drops_the_saved_branch() {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    lock(&world.trees.saved).insert(id("Tq.48"), Vec::new());
    start(&world, &["Tq.48"], WORKER);
    let req = world.request(
        "close",
        &["-m", "Already done by Tq.31."],
        "/trees/tree-01",
        WORKER,
    );
    let reply = CloseVerb(world.claims.clone()).run(&world.ctx, &req);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply
            .text
            .starts_with("closed   Tq.48: Already done by Tq.31.; tree-01 is free")
    );
    assert!(reply.text.contains("title    ✅ [Tq.48] Counter names"));
    assert_eq!(
        lock(&world.desk.closes)[0],
        (id("Tq.48"), "Already done by Tq.31.".to_owned())
    );
    assert!(lock(&world.trees.saved).is_empty());
    assert_eq!(lock(&world.trees.free).len(), 2);
}

#[test]
fn release_after_a_hand_close_frees_the_tree() {
    let world = World::new();
    world.desk.add(ticket("Tq.60", "Counter names", ""));
    start(&world, &["Tq.60"], WORKER);
    world.desk.close_by_hand("Tq.60");
    let reply = release(&world, &["-m", "Done."], WORKER);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply
            .text
            .starts_with("closed   Tq.60 was already closed; tree-01 freed"),
        "{}",
        reply.text
    );
    assert_eq!(lock(&world.trees.free).len(), 3);
    assert!(world.claims.holder(Path::new("/trees/tree-01")).is_none());
}

#[test]
fn close_after_a_hand_close_frees_the_tree() {
    let world = World::new();
    world.desk.add(ticket("Tq.61", "Counter names", ""));
    start(&world, &["Tq.61"], WORKER);
    world.desk.close_by_hand("Tq.61");
    let req = world.request("close", &["-m", "Again."], "/trees/tree-01", WORKER);
    let reply = CloseVerb(world.claims.clone()).run(&world.ctx, &req);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(reply.text.contains("was already closed; tree-01 freed"));
    assert_eq!(lock(&world.trees.free).len(), 3);
}

#[test]
fn the_tick_frees_the_tree_of_a_hand_closed_ticket() {
    let world = World::new();
    world.desk.add(ticket("Tq.63", "Counter names", ""));
    start(&world, &["Tq.63"], WORKER);
    world.claims.tick(&world.ctx);
    assert!(world.claims.holder(Path::new("/trees/tree-01")).is_some());
    world.desk.close_by_hand("Tq.63");
    world.claims.tick(&world.ctx);
    assert!(world.claims.holder(Path::new("/trees/tree-01")).is_none());
    assert_eq!(lock(&world.trees.free).len(), 3);
}

#[test]
fn a_review_ticket_names_its_guide() {
    let world = World::new();
    let mut review = ticket("Tq.50", "Review landing", "");
    review.labels = vec!["review".to_owned()];
    world.desk.add(review);
    let reply = start(&world, &["Tq.50"], WORKER);
    assert!(
        reply.text.contains("guide    ignite guide review"),
        "{}",
        reply.text
    );
}

#[test]
fn a_regression_ticket_is_titled_with_the_fire_engine() {
    let world = World::new();
    let mut regression = ticket("Tq.51", "Regression: copied face aura", "");
    regression.labels = vec!["regression".to_owned()];
    world.desk.add(regression);
    let reply = start(&world, &["Tq.51"], WORKER);
    assert!(
        reply
            .text
            .contains("title    🚒 [Tq.51] Regression: copied face aura"),
        "{}",
        reply.text
    );
}

#[test]
fn a_justiciar_ticket_names_its_guide_and_is_titled_with_fire() {
    let world = World::new();
    let mut emergency = ticket("Tq.52", "master has not moved for 4 hours", "");
    emergency.labels = vec!["justiciar".to_owned(), "hub".to_owned()];
    world.desk.add(emergency);
    let reply = start(&world, &["Tq.52"], WORKER);
    assert!(
        reply.text.contains("guide    ignite guide justiciar"),
        "{}",
        reply.text
    );
    assert!(
        reply
            .text
            .contains("title    🔥 [Tq.52] master has not moved"),
        "{}",
        reply.text
    );
}

#[test]
fn a_tree_whose_reset_failed_after_a_refused_claim_is_reset_by_a_later_tick() {
    let world = World::new();
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    lock(&world.desk.reservations).insert(id("Tq.48"), "7f3a".to_owned());
    *lock(&world.trees.reset_fails) = true;
    let reply = start(&world, &["Tq.48", "--reservation", "0000"], WORKER);
    assert_eq!(reply.exit, Exit::Conflict, "{}", reply.text);
    assert!(reply.text.contains("reset"), "{}", reply.text);
    assert_eq!(lock(&world.trees.free).len(), 1);

    // The reset still fails on the tick, and is tried again.
    world.claims.tick(&world.ctx);
    assert_eq!(lock(&world.trees.free).len(), 1);
    *lock(&world.trees.reset_fails) = false;
    world.claims.tick(&world.ctx);
    assert_eq!(lock(&world.trees.free).len(), 2);
    // Once it worked, it is not reset again.
    world.claims.tick(&world.ctx);
    assert_eq!(lock(&world.trees.resets).len(), 1);
}

#[test]
fn a_ticket_whose_tree_was_freed_by_a_landing_is_no_longer_held() {
    use ignite_store::local::{rows, trees as tree_rows};
    let path = std::env::temp_dir().join(format!("ignite-claim-landed-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let world = World::with_db(ignite_store::local::open(&path).unwrap());
    world.desk.add(ticket("Tq.48", "Counter names", ""));
    assert_eq!(start(&world, &["Tq.48"], WORKER).exit, Exit::Success);
    let conn = ignite_store::local::open(&path).unwrap();
    tree_rows::upsert(
        &conn,
        &rows::Tree {
            id: "tree-01".to_owned(),
            path: "/trees/tree-01".to_owned(),
            kind: "ticket".to_owned(),
            ticket: Some("Tq.48".to_owned()),
            session: Some(WORKER.to_owned()),
            branch: Some("ignite/Tq.48".to_owned()),
            built_commit: None,
            leased_at: None,
            last_used: None,
            owner: None,
            state: tree_rows::State::Leased.as_str().to_owned(),
        },
    )
    .unwrap();
    assert!(world.claims.holder("/trees/tree-01".as_ref()).is_some());

    // The landing frees the tree through the build trees, not through the claims.
    tree_rows::release(&conn, "tree-01", None, "t").unwrap();

    assert!(world.claims.holder("/trees/tree-01".as_ref()).is_none());
    drop(conn);
    let _ = std::fs::remove_file(&path);
}
