//! `ignite check` stores a verdict for the tree when its gate passes with nothing incomplete,
//! and [`crate::verdicts`] finds it again by the tree's files, base and gate settings.

use ignite_core::protocol::hub::Verdict;

use super::*;
use crate::verdicts;

/// One family the gate always runs, which reads `src`.
const ALWAYS: &str = r#"
[family.unit]
kind = "regression"
adapter = "jsonl"
platforms = "any"
always = true
list = "lister"
run = "runner {checks}"
inputs = ["src"]
"#;

fn always_world() -> World {
    let world = world(ALWAYS, &["src/a.py"], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\"}\n{\"check\": \"unit:b\"}\n".to_owned();
    world.git.set_tree_id(&world.dir, "tree-1");
    world
}

fn check(world: &World) -> ignite_core::protocol::client::Reply {
    let verb = CheckVerb {
        checks: world.checks.clone(),
        express: None,
    };
    verb.run(
        &world.ctx,
        &Request {
            verb: "check".to_owned(),
            args: Vec::new(),
            cwd: world.dir.clone(),
            session: None,
            wait_s: 1,
            pid: None,
        },
    )
}

fn head() -> Commit {
    Commit::new("abc1234").unwrap()
}

/// The verdict for the tree as the world's daemon finds it now.
fn found(world: &World, ctx: &Context) -> Option<Verdict> {
    let key = verdicts::key_for(ctx, &world.dir, &head()).unwrap();
    verdicts::find(ctx, &world.checks.cache.conn(), &key)
}

#[test]
fn a_passing_check_stores_a_verdict_found_by_the_trees_files_base_and_gate() {
    let world = always_world();
    let reply = check(&world);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("\nverdict  stored for this tree\n"),
        "{}",
        reply.text
    );
    let stored = world.hub.stored_verdicts();
    assert_eq!(stored.len(), 1);
    let verdict = &stored[0];
    assert_eq!(verdict.key.tree, "tree-1");
    assert_eq!(verdict.key.base.as_str(), "ba5e123");
    assert_eq!(verdict.head, head());
    assert_eq!(verdict.machine, "m-1");
    assert_eq!(verdict.families, ["unit"]);
    assert!(verdict.always_not_run.is_empty());
    assert_eq!(found(&world, &world.ctx).as_ref(), Some(verdict));

    // An edited file gives the tree another ID.
    world.git.set_tree_id(&world.dir, "tree-2");
    assert_eq!(found(&world, &world.ctx), None);
    world.git.set_tree_id(&world.dir, "tree-1");

    // Another `[gate]` section gives another digest.
    let mut gate = world.ctx.clone();
    gate.config.gate.threshold *= 2.0;
    assert_eq!(found(&world, &gate), None);
    let mut machines = world.ctx.clone();
    machines.config.machine.clear();
    assert!(found(&world, &machines).is_some(), "only [gate] counts");

    // A moved integration branch, which the tree's `HEAD` has merged, gives another base.
    world.git.set_tip("staging", Commit::new("ba5e456").unwrap());
    assert_eq!(found(&world, &world.ctx), None);
}

#[test]
fn a_failing_gate_stores_no_verdict() {
    let world = always_world();
    world
        .launcher
        .script("unit:a", &[Some("fail"), Some("fail")]);
    let reply = check(&world);
    assert_eq!(reply.exit, Exit::CheckFailed, "{}", reply.text);
    assert!(!reply.text.contains("verdict"), "{}", reply.text);
    assert!(world.hub.stored_verdicts().is_empty());
    assert_eq!(found(&world, &world.ctx), None);
}

#[test]
fn an_incomplete_gate_stores_no_verdict() {
    let world = always_world();
    world.launcher.script("unit:b", &[Some("cannot-run")]);
    let reply = check(&world);
    assert_ne!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(world.hub.stored_verdicts().is_empty());
    assert_eq!(found(&world, &world.ctx), None);
}

#[test]
fn a_dirty_trees_verdict_is_found_after_its_edits_are_committed_unchanged() {
    let world = always_world();
    check(&world);
    // Committing the edits moves `HEAD` but leaves every file, and so the tree ID, as it was.
    let committed = Commit::new("def5678").unwrap();
    let key = verdicts::key_for(&world.ctx, &world.dir, &committed).unwrap();
    let verdict = verdicts::find(&world.ctx, &world.checks.cache.conn(), &key).unwrap();
    assert_eq!(verdict.head, head(), "the verdict keeps the HEAD it passed at");
}

#[test]
fn a_verdict_is_found_and_stored_with_the_hub_unreachable() {
    let world = always_world();
    check(&world);
    world.hub.set_unreachable(true);
    assert!(found(&world, &world.ctx).is_some(), "from the local table");

    // A check that passes while the hub is away still stores its verdict locally.
    let away = always_world();
    away.hub.set_unreachable(true);
    let reply = check(&away);
    assert!(
        reply.text.contains("\nverdict  stored for this tree\n"),
        "{}",
        reply.text
    );
    assert!(found(&away, &away.ctx).is_some());
    away.hub.set_unreachable(false);
    assert!(away.hub.stored_verdicts().is_empty());
}

#[test]
fn a_verdict_the_tree_cannot_be_keyed_for_is_reported_and_the_check_still_passes() {
    let world = always_world();
    // The integration branch is unknown, so no merge base can be found.
    let mut ctx = world.ctx.clone();
    ctx.config.repository.staging = "nowhere".to_owned();
    let verb = CheckVerb {
        checks: world.checks.clone(),
        express: None,
    };
    let reply = verb.run(
        &ctx,
        &Request {
            verb: "check".to_owned(),
            args: Vec::new(),
            cwd: world.dir.clone(),
            session: None,
            wait_s: 1,
            pid: None,
        },
    );
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.contains("\nverdict  not stored: "),
        "{}",
        reply.text
    );
    assert!(world.hub.stored_verdicts().is_empty());
}
