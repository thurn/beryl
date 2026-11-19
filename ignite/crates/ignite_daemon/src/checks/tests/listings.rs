//! Kept listings: a family is listed once for the inputs its listing was made from, and the
//! listing outlives the daemon and serves other machines through the hub.

use super::*;

/// A local database in a file, which a restarted daemon opens again.
fn local_file(world: &World) -> PathBuf {
    world.dir.join("local.sqlite")
}

fn open_local(path: &Path) -> rusqlite::Connection {
    ignite_store::local::open(path).unwrap()
}

/// The nextest family's world, with a passing test.
fn nextest_world() -> World {
    let world = world(MANIFEST, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = NEXTEST_LISTING.to_owned();
    *world.launcher.nextest_run.lock().unwrap() =
        "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"core::core$a\"}\n".to_owned();
    world
}

fn run_on(checks: &Checks, world: &World, names: &[&str]) -> CheckRun {
    let names: Vec<CheckName> = names.iter().map(|n| name(n)).collect();
    checks.run(
        &world.ctx,
        &TreeId::new("tree-01").unwrap(),
        &names,
        &options(false),
    )
}

/// Before listings were kept, a restarted daemon's memory was empty, and the next run built
/// every Rust test program again only to list it, even with nothing it reads changed.
#[test]
fn after_a_restart_a_cached_family_builds_nothing_to_list() {
    let world = nextest_world();
    let path = local_file(&world);
    let first = daemon(&world, open_local(&path), FakeTrees::new(&world.dir, &[]));
    assert!(run_on(&first, &world, &["rs:core::a"]).passed());
    drop(first);
    let builds = world.builds.requests().len();
    let launched = world.launcher.runs.lock().unwrap().len();

    // The same machine's daemon again, with the hub down: the local copy answers.
    world.hub.set_unreachable(true);
    let restarted = daemon(&world, open_local(&path), FakeTrees::new(&world.dir, &[]));
    let again = run_on(&restarted, &world, &["rs:core::a"]);

    assert_eq!(again.from_cache, 1, "{again:?}");
    assert_eq!(world.builds.requests().len(), builds, "nothing was built");
    assert_eq!(world.launcher.runs.lock().unwrap().len(), launched);
}

#[test]
fn a_listing_one_daemon_made_serves_another_with_the_same_hub() {
    let world = jsonl_world();
    assert!(run(&world, &["unit:a"]).passed());
    let keys = world.hub.listing_keys();
    assert!(keys.iter().any(|k| k.family == "unit"), "{keys:?}");

    let other = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        FakeTrees::new(&world.dir, &[]),
    );
    let builds = world.builds.requests().len();
    let again = run_on(&other, &world, &["unit:a", "unit:b"]);

    assert!(again.passed(), "{again:?}");
    assert_eq!(
        world.launcher.commands("lister").len(),
        1,
        "not listed again"
    );
    // Only `unit:b`'s run asked for a product; nothing was asked for in order to list.
    assert_eq!(world.builds.requests().len(), builds + 1);
}

#[test]
fn an_empty_listing_is_never_kept() {
    let world = jsonl_world();
    *world.launcher.listing.lock().unwrap() = NEXTEST_LISTING.to_owned();
    let first = run(&world, &["unit:a"]);
    // A family that never listed a check may list none, but such a listing is not kept.
    assert!(!first.passed());
    assert!(
        world.hub.listing_keys().iter().all(|k| k.family != "unit"),
        "{:?}",
        world.hub.listing_keys()
    );
    *world.launcher.listing.lock().unwrap() = "{\"check\": \"unit:a\"}\n".to_owned();
    assert!(run(&world, &["unit:a"]).passed(), "it is listed again");
    assert_eq!(world.launcher.commands("lister").len(), 2);
}

#[test]
fn a_listing_with_a_digest_is_never_kept() {
    let world = jsonl_world();
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\", \"digest\": \"d1\"}\n".to_owned();
    run(&world, &["unit:a"]);
    run(&world, &["unit:a"]);
    assert!(world.hub.listing_keys().iter().all(|k| k.family != "unit"));
    assert_eq!(
        world.launcher.commands("lister").len(),
        2,
        "listed every time"
    );
}

/// A listing whose checks carry digests is never kept, but `ignite test <word>` still reads
/// which checks it names, so a word it does not name lists the family no more. Before, such a
/// family had no newest listing and was listed for every word, building what its `list` needs.
#[test]
fn a_word_an_unkept_listing_does_not_name_lists_its_family_no_more() {
    let world = jsonl_world();
    *world.launcher.listing.lock().unwrap() =
        format!("{{\"check\": \"unit:a\", \"digest\": \"d1\"}}\n{NEXTEST_LISTING}\n");
    *world.launcher.nextest_run.lock().unwrap() =
        "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"core::core$a\"}\n".to_owned();
    assert!(run(&world, &["unit:a", "rs:core::a"]).passed());
    assert!(world.hub.listing_keys().iter().all(|k| k.family != "unit"));
    let listers = world.launcher.commands("lister").len();
    let verb = TestVerb {
        checks: world.checks.clone(),
        express: None,
    };
    let request = Request {
        verb: "test".to_owned(),
        args: vec!["core::a".to_owned()],
        cwd: world.dir.join("sub"),
        session: None,
        wait_s: 1,
        pid: None,
    };
    let reply = verb.run(&world.ctx, &request);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(
        world.launcher.commands("lister").len(),
        listers,
        "the family whose listing holds no such check is not listed"
    );
    // A word the listing names lists the family again, since its listing is not kept.
    let request = Request {
        args: vec!["it:a".to_owned()],
        ..request
    };
    let reply = verb.run(&world.ctx, &request);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert_eq!(world.launcher.commands("lister").len(), listers + 1);
    assert!(world.hub.listing_keys().iter().all(|k| k.family != "unit"));
}

/// The oracle family's checks carry a digest of each scenario's exported input, so its listing
/// was never kept and every landing listed it, after building what listing needs. A family
/// that declares the files its digests are computed from has its listing kept under a hash
/// that covers them, and listed again when one of them changes.
#[test]
fn a_listing_with_digests_from_declared_files_is_kept_until_they_change() {
    let manifest = MANIFEST.replace(
        "list = \"lister\"\n",
        "list = \"lister\"\ndigest_inputs = [\"data/core.json\"]\n",
    );
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        format!("{{\"check\": \"unit:a\", \"digest\": \"d1\"}}\n{NEXTEST_LISTING}\n");
    world
        .git
        .set_object("data/core.json", Oid::new("a".repeat(40)).unwrap());
    assert!(run(&world, &["unit:a"]).passed());
    assert!(run(&world, &["unit:a"]).passed());
    assert!(world.hub.listing_keys().iter().any(|k| k.family == "unit"));
    assert_eq!(world.launcher.commands("lister").len(), 1, "kept");

    world
        .git
        .set_object("data/core.json", Oid::new("b".repeat(40)).unwrap());
    run(&world, &["unit:a"]);
    assert_eq!(
        world.launcher.commands("lister").len(),
        2,
        "a file its digests are computed from changed"
    );
}

/// A check's own inputs are not in the listing hash, so a kept listing also records what they
/// named; a change to them, or a new file beside them, means the listing may be out of date.
#[test]
fn a_kept_listing_whose_checks_own_inputs_changed_is_made_again() {
    let world = jsonl_world();
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\", \"inputs\": [\"data/core.json\"]}\n".to_owned();
    world
        .git
        .set_object("data/core.json", Oid::new("c".repeat(40)).unwrap());
    world
        .git
        .set_object("data", Oid::new("d".repeat(40)).unwrap());
    run(&world, &["unit:a"]);
    run(&world, &["unit:a"]);
    assert_eq!(
        world.launcher.commands("lister").len(),
        1,
        "kept while they hold"
    );

    // A new file in `data/` changes the directory, as a new test module would.
    world
        .git
        .set_object("data", Oid::new("e".repeat(40)).unwrap());
    run(&world, &["unit:a"]);
    assert_eq!(world.launcher.commands("lister").len(), 2, "listed again");
    run(&world, &["unit:a"]);
    assert_eq!(world.launcher.commands("lister").len(), 2, "and kept again");
}

/// A tip round lists a family once and runs its batches from that listing; with the listing
/// kept, a later round on the same inputs does not list it at all.
#[test]
fn tip_batches_run_from_a_kept_listing() {
    let world = jsonl_world();
    let tree = TreeId::new("tree-01").unwrap();
    let batches = vec![(0, vec![name("unit:a")]), (1, vec![name("unit:b")])];
    let claim = |_: u32| true;
    for _ in 0..2 {
        let (run, ran) =
            world
                .checks
                .run_batches(&world.ctx, &tree, "unit", &batches, &options(true), &claim);
        assert_eq!(ran, 2);
        assert!(run.passed(), "{:?}", run.failures);
    }
    assert_eq!(world.launcher.commands("lister").len(), 1);
}
