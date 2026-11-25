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
        let (run, ran) = world.checks.run_batches(
            &world.ctx,
            &tree,
            "unit",
            &batches,
            &options(true),
            &super::super::BatchCallbacks {
                claim: &claim,
                failed: &|_| {},
                stop: None,
            },
        );
        assert_eq!(ran, 2);
        assert!(run.passed(), "{:?}", run.failures);
    }
    assert_eq!(world.launcher.commands("lister").len(), 1);
}

/// After a change to one card file, `ignite test fork` listed the Rust test family for the
/// whole workspace, which built every package's test binaries (35 crates, 33 to 77 s) to run
/// one test of one package. A family whose `list` and `run` say `{packages}`, found through its
/// newest listing, is now listed and run for the packages of the matching tests and those the
/// branch changes, and that listing is not kept.
#[test]
fn a_named_test_lists_and_runs_only_its_packages() {
    let manifest = MANIFEST.replace(
        "[family.rs]\n",
        "[family.rs]\nlist = \"cargo nextest list {packages} --message-format json\"\n\
         run = \"cargo nextest run {packages} --message-format libtest-json-plus\"\n",
    );
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = NEXTEST_LISTING.to_owned();
    *world.launcher.nextest_run.lock().unwrap() =
        "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"core::core$a\"}\n".to_owned();
    let package = |name: &str| ignite_core::inputs::Package {
        name: name.to_owned(),
        dir: format!("game/{name}"),
        deps: Vec::new(),
        workspace: "game".to_owned(),
    };
    let trees = FakeTrees {
        graph: CrateGraph {
            packages: vec![package("core"), package("other"), package("idle")],
        },
        ..FakeTrees::new(&world.dir, &["game/other/src/new.rs"])
    };
    let checks = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        trees,
    );
    // The first listing, of the whole workspace, is the family's newest.
    assert!(run_on(&checks, &world, &["rs:core::a"]).passed());
    let whole = world.launcher.commands("cargo");
    assert!(whole[0].contains(&"--workspace".to_owned()), "{whole:?}");
    let kept = world.hub.listing_keys().len();

    // A change to what the family reads leaves it without a listing for the tree's inputs.
    world
        .git
        .set_object("src", Oid::new("e".repeat(40)).unwrap());
    let verb = TestVerb {
        checks,
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
    let commands = world.launcher.commands("cargo");
    let selected = ["-p", "core", "-p", "other"].map(str::to_owned);
    let (list, run) = (&commands[commands.len() - 2], &commands[commands.len() - 1]);
    assert_eq!((list[2].as_str(), &list[3..7]), ("list", &selected[..]));
    // The run builds and runs the package of the test it runs.
    assert_eq!((run[2].as_str(), &run[3..5]), ("run", &selected[..2]));
    let scoped = world
        .builds
        .requests()
        .iter()
        .any(|r| r.2 == ["core", "other"]);
    assert!(scoped, "the listing builds the packages it lists");
    assert_eq!(
        world.hub.listing_keys().len(),
        kept,
        "a scoped listing is not kept"
    );
}

/// After a one-line change to the card Fork, `ignite test fork` matched one Rust test and also
/// the recordings of Fork in two interface families of the game's 3D client, whose listing needs
/// that client built. It listed those families for 24 to 55 minutes before it ran the Rust
/// test. A family whose listing needs a build is now left out when every name it matches also
/// matches a family that is cheap to list, and the reply names it; a name with its prefix, or a
/// name only it matches, still lists it.
#[test]
fn a_name_a_cheap_family_matches_skips_families_whose_listing_needs_a_build() {
    let manifest = MANIFEST.replace(
        "[family.rs]\n",
        "[family.rs]\nlist = \"cargo nextest list {packages} --message-format json\"\n\
         run = \"cargo nextest run {packages} --message-format libtest-json-plus\"\n",
    );
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        format!("{{\"check\": \"unit:core::a/you\"}}\n{NEXTEST_LISTING}\n");
    *world.launcher.nextest_run.lock().unwrap() =
        "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"core::core$a\"}\n".to_owned();
    let trees = FakeTrees {
        graph: CrateGraph {
            packages: vec![ignite_core::inputs::Package {
                name: "core".to_owned(),
                dir: "game/core".to_owned(),
                deps: Vec::new(),
                workspace: "game".to_owned(),
            }],
        },
        ..FakeTrees::new(&world.dir, &[])
    };
    let checks = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        trees,
    );
    assert!(run_on(&checks, &world, &["unit:core::a/you", "rs:core::a"]).passed());
    // A change every family reads, as a card file is, leaves neither with a kept listing.
    world
        .git
        .set_object("src", Oid::new("e".repeat(40)).unwrap());
    let verb = TestVerb {
        checks,
        express: None,
    };
    let test = |words: &[&str]| {
        let request = Request {
            verb: "test".to_owned(),
            args: words.iter().map(|w| (*w).to_owned()).collect(),
            cwd: world.dir.join("sub"),
            session: None,
            wait_s: 1,
            pid: None,
        };
        let reply = verb.run(&world.ctx, &request);
        assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
        reply.text
    };
    let listers = world.launcher.commands("lister").len();
    let cargo = world.launcher.commands("cargo").len();

    let reply = test(&["core::a"]);
    assert_eq!(
        world.launcher.commands("lister").len(),
        listers,
        "the family whose listing needs a build is not listed"
    );
    assert!(
        world.launcher.commands("cargo").len() > cargo,
        "the Rust family is listed and run"
    );
    assert!(
        reply.contains(
            "skipped  unit also hold matching checks, but listing them needs a build; \
             `ignite test unit:core::a` runs them"
        ),
        "{reply}"
    );

    // A name no cheap family matches lists it.
    let reply = test(&["/you"]);
    assert_eq!(world.launcher.commands("lister").len(), listers + 1);
    assert!(!reply.contains("skipped"), "{reply}");

    // So does its prefix, after another change leaves it without a kept listing again.
    world
        .git
        .set_object("src", Oid::new("f".repeat(40)).unwrap());
    let reply = test(&["unit:core::a", "core::a"]);
    assert_eq!(world.launcher.commands("lister").len(), listers + 2);
    assert!(!reply.contains("skipped"), "{reply}");
}

/// A test the branch had just written, `ignite test a_turn_past_small_does_not_narrow`, matched
/// no listing, so every family without a kept listing was listed: 1241 s, most of it waiting
/// for the 3D client and the scenario dump to build, to run two tests the Rust family listed.
/// A family whose listing needs a build now waits until the cheap families are listed, and is
/// listed only when a word still matches nothing.
#[test]
fn a_new_test_a_cheap_family_lists_skips_families_whose_listing_needs_a_build() {
    let manifest = MANIFEST.replace(
        "[family.rs]\n",
        "[family.rs]\nlist = \"cargo nextest list {packages} --message-format json\"\n\
         run = \"cargo nextest run {packages} --message-format libtest-json-plus\"\n",
    );
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        format!("{{\"check\": \"unit:core::a/you\"}}\n{NEXTEST_LISTING}\n");
    *world.launcher.nextest_run.lock().unwrap() =
        "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"core::core$a\"}\n\
         {\"type\":\"test\",\"event\":\"ok\",\"name\":\"core::core$fresh\"}\n"
            .to_owned();
    let trees = FakeTrees {
        graph: CrateGraph {
            packages: vec![ignite_core::inputs::Package {
                name: "core".to_owned(),
                dir: "game/core".to_owned(),
                deps: Vec::new(),
                workspace: "game".to_owned(),
            }],
        },
        ..FakeTrees::new(&world.dir, &[])
    };
    let checks = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        trees,
    );
    assert!(run_on(&checks, &world, &["unit:core::a/you", "rs:core::a"]).passed());
    let verb = TestVerb {
        checks,
        express: None,
    };
    let test = |words: &[&str]| {
        let request = Request {
            verb: "test".to_owned(),
            args: words.iter().map(|w| (*w).to_owned()).collect(),
            cwd: world.dir.join("sub"),
            session: None,
            wait_s: 1,
            pid: None,
        };
        let reply = verb.run(&world.ctx, &request);
        assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
        reply
    };
    // The branch adds a Rust test, and its change leaves neither family with a kept listing.
    let fresh = r#""fresh": {"ignored": false, "filter-match": {"status": "matches"}}"#;
    let with_fresh =
        NEXTEST_LISTING.replace(r#""testcases": {"#, &format!(r#""testcases": {{{fresh}, "#));
    *world.launcher.listing.lock().unwrap() =
        format!("{{\"check\": \"unit:core::a/you\"}}\n{with_fresh}\n");
    world
        .git
        .set_object("src", Oid::new("e".repeat(40)).unwrap());
    let listers = world.launcher.commands("lister").len();

    let reply = test(&["fresh"]);
    assert_eq!(
        world.launcher.commands("lister").len(),
        listers,
        "the family whose listing needs a build is not listed"
    );
    let ran = reply.json.as_ref().unwrap()["outcomes"].clone();
    assert_eq!(ran[0]["check"], "rs:core::fresh", "{}", reply.text);
    assert!(
        reply.text.contains(
            "skipped  unit also hold matching checks, but listing them needs a build; \
             `ignite test unit:fresh` runs them"
        ),
        "{}",
        reply.text
    );

    // A new check that only the family whose listing needs a build holds still lists it.
    *world.launcher.listing.lock().unwrap() = format!(
        "{{\"check\": \"unit:core::a/you\"}}\n{{\"check\": \"unit:brand_new\"}}\n{with_fresh}\n"
    );
    world
        .git
        .set_object("src", Oid::new("f".repeat(40)).unwrap());
    let reply = test(&["brand_new"]);
    assert_eq!(world.launcher.commands("lister").len(), listers + 1);
    assert!(!reply.text.contains("skipped"), "{}", reply.text);
}

/// A listing made on Windows held a test that only exists there (`cfg(windows)`), and a Mac that
/// reused it from the hub asked cargo-nextest for the test and got "no tests to run", so the
/// check stayed incomplete. A `nextest` listing is therefore made once per operating system,
/// even when the family's checks pass on any.
#[test]
fn a_nextest_listing_is_not_shared_between_operating_systems() {
    let world = nextest_world();
    let mut settings = world.checks.settings.clone();
    settings
        .manifest
        .families
        .get_mut("rs")
        .unwrap()
        .family
        .platforms = ignite_core::manifest::Platforms::Any(ignite_core::manifest::AnyPlatform::Any);
    let on = |os: Os| {
        let mut settings = settings.clone();
        settings.os = os;
        let cache = ResultCache::new(
            ignite_store::local::open_in_memory().unwrap(),
            world.hub.clone(),
            world.ctx.machine.clone(),
            world.ctx.clock.clone(),
        );
        Arc::new(Checks::new(
            settings,
            cache,
            Box::new(FakeTrees::new(&world.dir, &[])),
            Box::new(FakeHistory::default()),
        ))
    };
    let mac = on(Os::Macos);
    assert!(run_on(&mac, &world, &["rs:core::a"]).passed());
    let before = world.hub.listing_keys().len();
    let windows = on(Os::Windows);
    assert!(run_on(&windows, &world, &["rs:core::a"]).passed());
    assert!(
        world.hub.listing_keys().len() > before,
        "Windows listed for itself"
    );
}

#[test]
fn tip_verification_uses_the_nextest_checks_listed_by_each_operating_system() {
    let manifest = MANIFEST.replace(
        "platforms = [\"macos\"]",
        "platforms = [\"macos\", \"windows\"]",
    );
    let world = world(&manifest, &[], FakeHistory::default());
    let tree = TreeId::new("tree-01").unwrap();
    let mut windows = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        FakeTrees::new(&world.dir, &[]),
    );
    Arc::get_mut(&mut windows).unwrap().settings.os = Os::Windows;
    *world.launcher.listing.lock().unwrap() =
        format!("{{\"check\": \"unit:a\"}}\n{NEXTEST_LISTING}\n");
    let first = world.checks.regression_checks(&world.ctx, &tree).unwrap();
    assert!(first.unlisted.iter().any(|missing| {
        missing.name == name("rs:listing") && missing.platforms == [Os::Windows]
    }));

    let windows_listing = NEXTEST_LISTING.replace("\"a\"", "\"windows_only\"");
    *world.launcher.listing.lock().unwrap() = windows_listing;
    *world.launcher.nextest_run.lock().unwrap() =
        "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"core::core$windows_only\"}\n".to_owned();
    assert!(run_on(&windows, &world, &["rs:core::windows_only"]).passed());

    let listed = world.checks.regression_checks(&world.ctx, &tree).unwrap();
    assert!(listed.unlisted.is_empty());
    let keys: Vec<_> = listed
        .checks
        .iter()
        .filter(|c| c.family == "rs")
        .map(|c| (c.check.as_str(), c.key.platform, c.runnable))
        .collect();
    assert_eq!(
        keys,
        [
            ("rs:core::a", Some(Os::Macos), true),
            ("rs:core::windows_only", Some(Os::Windows), false),
        ]
    );
    let windows_key = listed
        .checks
        .iter()
        .find(|c| c.check == name("rs:core::windows_only"))
        .unwrap();
    let results = world
        .hub
        .lookup(std::slice::from_ref(&windows_key.key))
        .unwrap();
    assert_eq!(results[0].as_ref().unwrap().outcome, Outcome::Pass);
}

#[test]
fn a_batch_reports_only_the_exact_keys_of_its_failed_checks() {
    let world = jsonl_world();
    world
        .launcher
        .script("unit:a", &[Some("fail"), Some("fail")]);
    world.launcher.script("unit:b", &[None, None, None]);
    let tree = TreeId::new("tree-01").unwrap();
    let batches = [(0, vec![name("unit:a")]), (1, vec![name("unit:b")])];
    let keys = Mutex::new(Vec::new());
    let callbacks = BatchCallbacks {
        claim: &|batch| {
            if batch == 1 {
                assert_eq!(keys.lock().unwrap().len(), 1);
            }
            true
        },
        failed: &|reported| keys.lock().unwrap().extend_from_slice(reported),
        stop: None,
    };

    let (run, ran) = world.checks.run_batches(
        &world.ctx,
        &tree,
        "unit",
        &batches,
        &options(false),
        &callbacks,
    );

    assert_eq!(ran, 2);
    assert_eq!(outcome(&run, "unit:a"), Outcome::Fail);
    assert_eq!(outcome(&run, "unit:b"), Outcome::Incomplete);
    let keys = keys.into_inner().unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].check, name("unit:a"));
    let cached = world.hub.lookup(&keys).unwrap();
    assert_eq!(cached[0].as_ref().unwrap().outcome, Outcome::Fail);
}

#[test]
fn run_batches_skips_remaining_batches_once_stop_is_raised() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let world = jsonl_world();
    let tree = TreeId::new("tree-01").unwrap();
    let batches = [(0, vec![name("unit:a")]), (1, vec![name("unit:b")])];
    let stop = AtomicBool::new(false);
    let callbacks = BatchCallbacks {
        claim: &|batch| {
            if batch == 0 {
                stop.store(true, Ordering::SeqCst);
            }
            true
        },
        failed: &|_| {},
        stop: Some(&stop),
    };
    let (run, ran) = world.checks.run_batches(
        &world.ctx,
        &tree,
        "unit",
        &batches,
        &options(false),
        &callbacks,
    );
    assert_eq!(ran, 1);
    assert!(run.outcomes.is_empty());
    assert_eq!(run.skipped, 1);
    assert!(world.launcher.commands("runner").is_empty());
}
