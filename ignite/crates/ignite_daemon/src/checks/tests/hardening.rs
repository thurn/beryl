//! Tip verification's view of the checks and the check runner's handling of broken listings and
//! cut-short runs: families this machine does not run, listings that fail or come back empty,
//! passes from a command that did not finish, and the gate with open regressions.

use super::*;

use ignite_core::protocol::hub::OpenRegression;
use ignite_core::protocol::tickets::TicketDetail;

use crate::tip::ListingPhase;

fn tree() -> TreeId {
    TreeId::new("tree-01").unwrap()
}

fn family(name: &str, platforms: &str, list: &str, extra: &str) -> String {
    format!(
        "[family.{name}]\nkind = \"regression\"\nadapter = \"jsonl\"\nplatforms = {platforms}\n\
         list = \"{list}\"\nrun = \"runner {{checks}}\"\ninputs = [\"src\"]\n{extra}\n"
    )
}

fn listing(lines: &[&str]) -> String {
    let mut text = String::new();
    for check in lines {
        let _ = writeln!(text, "{{\"check\": \"{check}\"}}");
    }
    text
}

#[test]
fn families_this_machine_does_not_run_are_in_the_set_as_keys_only_others_can_pass() {
    let manifest = [
        family("unit", "\"any\"", "lister", ""),
        family("other", "\"any\"", "lister", ""),
        family("win", "[\"windows\"]", "lister", ""),
        family("both", "[\"macos\", \"windows\"]", "lister", ""),
    ]
    .join("\n");
    let mut world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = listing(&["unit:a", "other:b", "win:c", "both:d"]);
    // The machine runs only the `unit` and `both` families.
    Arc::get_mut(&mut world.checks).unwrap().settings.families =
        Some(vec!["unit".to_owned(), "both".to_owned()]);

    let set = world.checks.regression_checks(&world.ctx, &tree()).unwrap();

    assert!(set.unlisted.is_empty(), "{:?}", set.unlisted);
    let keys: Vec<(&str, Option<Os>, bool)> = set
        .checks
        .iter()
        .map(|c| (c.check.as_str(), c.key.platform, c.runnable))
        .collect();
    let has = |check: &str, platform: Option<Os>, runnable: bool| {
        assert!(
            keys.contains(&(check, platform, runnable)),
            "{check} {platform:?} {runnable} not in {keys:?}"
        );
    };
    has("unit:a", None, true);
    has("other:b", None, false);
    has("win:c", Some(Os::Windows), false);
    has("both:d", Some(Os::Macos), true);
    has("both:d", Some(Os::Windows), false);
    assert_eq!(keys.len(), 5);
}

#[test]
fn tip_verification_keys_shared_inputs_outside_the_dispatch_paths() {
    let manifest = r#"
[family.simulator]
kind = "regression"
adapter = "jsonl"
platforms = ["macos"]
list = "lister"
run = "runner {checks}"
inputs = ["engine/crates/**", "client/**"]
dispatch_paths = ["client/ios/**"]
"#;
    let world = world(manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = listing(&["simulator:build"]);
    let mut trees = FakeTrees::new(&world.dir, &[]);
    trees.tracked = vec![
        "engine/crates/kernel/src/lib.rs".to_owned(),
        "client/ios/project.yml".to_owned(),
    ];
    let checks = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        trees,
    );

    world
        .git
        .set_object("client", Oid::new("c".repeat(40)).unwrap());
    world
        .git
        .set_object("engine/crates", Oid::new("a".repeat(40)).unwrap());
    let before = checks.regression_checks(&world.ctx, &tree()).unwrap();
    world
        .git
        .set_object("engine/crates", Oid::new("b".repeat(40)).unwrap());
    let after = checks.regression_checks(&world.ctx, &tree()).unwrap();

    let [before] = before.checks.as_slice() else {
        panic!("expected a Simulator check: {before:?}");
    };
    let [after] = after.checks.as_slice() else {
        panic!("expected a Simulator check: {after:?}");
    };
    assert_eq!(before.key.platform, Some(Os::Macos));
    assert_eq!(after.key.platform, Some(Os::Macos));
    assert_ne!(before.key.hash, after.key.hash);
}

#[test]
fn a_family_that_cannot_be_listed_leaves_the_others_and_is_named() {
    let manifest = [
        family("unit", "\"any\"", "lister", ""),
        family("shots", "\"any\"", "broken", ""),
    ]
    .join("\n");
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = listing(&["unit:a"]);

    let set = world.checks.regression_checks(&world.ctx, &tree()).unwrap();

    assert_eq!(set.checks.len(), 1);
    assert_eq!(set.unlisted.len(), 1);
    let missing = &set.unlisted[0];
    assert_eq!(missing.name, name("shots:listing"));
    assert!(missing.reportable, "this machine runs the family");
    assert!(missing.message.contains("could not be listed"));
}

#[test]
fn a_missing_tip_listing_names_only_its_failed_product() {
    let manifest = [
        family("unit", "\"any\"", "lister", "needs = [\"unit-build\"]"),
        family(
            "shots",
            "\"any\"",
            "lister",
            "needs = [\"unit-build\"]\nlist_needs = [\"shots-build\"]",
        ),
        r#"
[product.unit-build]
build = "builder"
inputs = ["src"]
outputs = ["unit-out"]

[product.shots-build]
build = "builder"
inputs = ["src"]
outputs = ["shots-out"]
"#
        .to_owned(),
    ]
    .join("\n");
    let world = world(&manifest, &[], FakeHistory::default());
    for product in ["unit-build", "shots-build"] {
        world.builds.script(
            &tree(),
            product,
            ignite_core::traits::BuildResult::Failed {
                output: world.dir.join(format!("{product}.log")),
                usage: Usage::default(),
            },
        );
    }

    let set = world.checks.regression_checks(&world.ctx, &tree()).unwrap();

    assert_eq!(set.unlisted.len(), 2);
    for missing in &set.unlisted {
        let (_, product) = missing.message.split_once(LISTING_BUILD_FAILED).unwrap();
        assert_eq!(product, format!("{}-build", missing.family));
    }
}

#[test]
fn a_foreign_family_that_cannot_be_listed_is_not_reported_and_names_its_platforms() {
    let manifest = family("win", "[\"windows\"]", "broken", "");
    let world = world(&manifest, &[], FakeHistory::default());

    let set = world.checks.regression_checks(&world.ctx, &tree()).unwrap();

    assert_eq!(set.unlisted.len(), 1);
    assert!(!set.unlisted[0].reportable);
    assert_eq!(set.unlisted[0].platforms, vec![Os::Windows]);
}

#[test]
fn a_tip_waits_for_a_foreign_build_listing_without_building_it() {
    let manifest = format!(
        "{}\n{}\n[product.binary]\nbuild = \"builder\"\ninputs = [\"src\"]\noutputs = [\"out\"]\n",
        family("unit", "\"any\"", "lister", ""),
        family(
            "desktop",
            "[\"macos\"]",
            "lister",
            "needs = [\"binary\"]\nbuilds = true"
        ),
    );
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = listing(&["unit:a", "desktop:b"]);
    let mut windows = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        FakeTrees::new(&world.dir, &[]),
    );
    Arc::get_mut(&mut windows).unwrap().settings.os = Os::Windows;

    let cold = windows.regression_checks(&world.ctx, &tree()).unwrap();
    assert_eq!(cold.checks.len(), 1);
    assert!(cold.checks[0].runnable);
    assert_eq!(cold.unlisted.len(), 1);
    assert_eq!(cold.unlisted[0].name, name("desktop:listing"));
    assert_eq!(cold.unlisted[0].platforms, vec![Os::Macos]);
    assert!(!cold.unlisted[0].reportable);
    assert!(world.builds.requests().is_empty());
    assert_eq!(world.launcher.commands("lister").len(), 1);

    world.hub.set_unreachable(true);
    let mac = world.checks.regression_checks(&world.ctx, &tree()).unwrap();
    world.hub.set_unreachable(false);
    let mac_check = mac.checks.iter().find(|c| c.family == "desktop").unwrap();
    let builds = world.builds.requests().len();
    assert!(builds > 0, "the Mac supplies the listing");
    let commands = world.launcher.commands("lister").len();

    let waiting = windows.regression_checks(&world.ctx, &tree()).unwrap();
    assert_eq!(waiting.unlisted.len(), 1, "the hub missed the listing");
    world.checks.regression_checks(&world.ctx, &tree()).unwrap();
    let warm = windows.regression_checks(&world.ctx, &tree()).unwrap();
    assert!(warm.unlisted.is_empty());
    let foreign_check = warm.checks.iter().find(|c| c.family == "desktop").unwrap();
    assert_eq!(foreign_check.key, mac_check.key);
    assert!(!foreign_check.runnable);
    let local = windows
        .regression_checks_for(&world.ctx, &tree(), ListingPhase::Build)
        .unwrap();
    assert!(local.checks.is_empty());
    assert!(local.unlisted.is_empty());
    let peers = windows
        .regression_checks_for(&world.ctx, &tree(), ListingPhase::PeerBuild)
        .unwrap();
    assert!(peers.unlisted.is_empty());
    assert_eq!(peers.checks, std::slice::from_ref(foreign_check));
    assert_eq!(world.builds.requests().len(), builds);
    assert_eq!(world.launcher.commands("lister").len(), commands);
}

/// A round that is not catching up defers a family whose `defer_platforms` names this platform,
/// so it keys that family from the listing kept at the hub and builds nothing for it. A
/// catching-up round lists it here.
#[test]
fn a_round_that_defers_a_family_here_builds_nothing_to_list_it() {
    let manifest = format!(
        "{}\n{}\n[product.binary]\nbuild = \"builder\"\ninputs = [\"src\"]\noutputs = [\"out\"]\n",
        family("unit", "\"any\"", "lister", ""),
        family(
            "native",
            "[\"macos\", \"windows\"]",
            "lister",
            "needs = [\"binary\"]\ndefer_platforms = [\"macos\"]"
        ),
    );
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = listing(&["unit:a", "native:b"]);
    let phase = |phase| {
        world
            .checks
            .regression_checks_for(&world.ctx, &tree(), phase)
            .unwrap()
    };

    let cold = phase(ListingPhase::OtherDue);
    assert!(world.builds.requests().is_empty());
    assert_eq!(world.launcher.commands("lister").len(), 1);
    assert!(cold.checks.iter().all(|check| check.family == "unit"));
    assert!(cold.unlisted.iter().any(|missing| {
        missing.name == name("native:listing") && missing.platforms.contains(&Os::Macos)
    }));
    assert_eq!(cold.defer_platforms["native"], [Os::Macos]);

    let catching_up = phase(ListingPhase::Other);
    let listed = catching_up
        .checks
        .iter()
        .find(|check| check.family == "native")
        .unwrap();
    assert!(listed.runnable);
    let builds = world.builds.requests().len();
    assert!(builds > 0);

    let warm = phase(ListingPhase::OtherDue);
    let kept = warm
        .checks
        .iter()
        .find(|check| check.family == "native" && check.key.platform == Some(Os::Macos))
        .unwrap();
    assert_eq!(kept.key, listed.key);
    assert!(!kept.runnable);
    assert_eq!(world.builds.requests().len(), builds);
}

#[test]
fn an_ancestor_uses_kept_listings_without_building_or_listing() {
    let manifest = format!(
        "{}\n[product.binary]\nbuild = \"builder\"\ninputs = [\"src\"]\noutputs = [\"out\"]\n",
        family("unit", "\"any\"", "lister", "needs = [\"binary\"]"),
    );
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = listing(&["unit:a"]);

    let cached = || {
        world
            .checks
            .cached_regression_checks(&world.ctx, &tree())
            .unwrap()
            .listing
    };
    let cold = cached();
    assert!(cold.checks.is_empty());
    assert_eq!(cold.unlisted[0].name, name("unit:listing"));
    assert!(world.builds.requests().is_empty());
    assert!(world.launcher.commands("lister").is_empty());

    let listed = world.checks.regression_checks(&world.ctx, &tree()).unwrap();
    let builds = world.builds.requests().len();
    let commands = world.launcher.commands("lister").len();
    let warm = cached();
    assert!(warm.unlisted.is_empty());
    assert_eq!(warm.checks, listed.checks);
    assert_eq!(world.builds.requests().len(), builds);
    assert_eq!(world.launcher.commands("lister").len(), commands);
}

fn measurement_world() -> World {
    let manifest = [
        family("meter", "[\"macos\"]", "lister", "machine_relative = true"),
        family("unit", "\"any\"", "lister", ""),
    ]
    .join("\n");
    let world = world(&manifest, &[], FakeHistory::default());
    measurement_listing(&world, "calibration-1");
    world
}

fn measurement_listing(world: &World, digest: &str) {
    *world.launcher.listing.lock().unwrap() =
        format!("{{\"check\":\"meter:a\",\"digest\":\"{digest}\"}}\n{{\"check\":\"unit:a\"}}\n");
}

fn measurement_peer(world: &World, machine: &str, os: Os) -> (Context, Checks) {
    let mut ctx = world.ctx.clone();
    ctx.machine = MachineId::new(machine).unwrap();
    let mut settings = world.checks.settings.clone();
    settings.os = os;
    let cache = ResultCache::new(
        ignite_store::local::open_in_memory().unwrap(),
        world.hub.clone(),
        ctx.machine.clone(),
        ctx.clock.clone(),
    );
    let checks = Checks::new(
        settings,
        cache,
        Box::new(FakeTrees::new(&world.dir, &[])),
        Box::new(FakeHistory::default()),
    );
    (ctx, checks)
}

#[test]
fn a_measurement_pass_is_reusable_only_on_its_verifier_and_calibration() {
    let world = measurement_world();
    let names = [name("meter:a"), name("unit:a")];
    let original = run(&world, &["meter:a", "unit:a"]);
    assert!(original.passed(), "{original:?}");
    assert_eq!(run(&world, &["meter:a", "unit:a"]).from_cache, 2);
    // Publish a tip listing so the second Mac also has a listing it must bypass.
    world.checks.regression_checks(&world.ctx, &tree()).unwrap();

    let (other, peer) = measurement_peer(&world, "m-2", Os::Macos);
    world
        .launcher
        .script("meter:a", &[Some("fail"), Some("fail")]);
    let second = peer.run(&other, &tree(), &names, &options(false));
    assert_eq!(second.from_cache, 1);
    assert_eq!(outcome(&second, "unit:a"), Outcome::Pass);
    assert_eq!(outcome(&second, "meter:a"), Outcome::Fail);
    assert_ne!(original.input_hashes, second.input_hashes);
    let specs = world.launcher.runs.lock().unwrap();
    for program in ["lister", "runner"] {
        assert!(specs.iter().any(|spec| {
            spec.args[0] == program
                && spec
                    .env
                    .iter()
                    .any(|(key, value)| key == "IGNITE_MACHINE" && value == "m-2")
        }));
    }
    drop(specs);

    measurement_listing(&world, "calibration-2");
    let recalibrated = run(&world, &["meter:a", "unit:a"]);
    assert_eq!(recalibrated.from_cache, 1);
    assert!(recalibrated.passed(), "{recalibrated:?}");
    assert_ne!(original.input_hashes, recalibrated.input_hashes);
}

#[test]
fn a_foreign_tip_uses_the_published_measurement_key_without_listing_locally() {
    let world = measurement_world();
    let (windows, mut peer) = measurement_peer(&world, "m-2", Os::Windows);
    peer.settings.toolchain = "2.0".to_owned();
    let cold = peer.regression_checks(&windows, &tree()).unwrap();
    assert_eq!(cold.checks.len(), 1);
    assert_eq!(cold.unlisted.len(), 1);
    assert_eq!(cold.unlisted[0].name, name("meter:listing"));
    assert_eq!(cold.unlisted[0].platforms, vec![Os::Macos]);
    assert!(!cold.unlisted[0].reportable);

    let (secondary, second_mac) = measurement_peer(&world, "m-3", Os::Macos);
    let secondary_plan = second_mac.regression_checks(&secondary, &tree()).unwrap();
    // The second Mac stops after listing. The first Mac can verify the OS with its own key.
    let mac = world.checks.regression_checks(&world.ctx, &tree()).unwrap();
    assert!(mac.unlisted.is_empty());
    assert_eq!(mac.checks.len(), 2);
    assert_ne!(mac.checks[0].key, secondary_plan.checks[0].key);
    assert!(mac.checks[0].runnable);
    let local = run(&world, &["meter:a"]);
    assert!(local.passed(), "{local:?}");

    let lists = world.launcher.commands("lister").len();
    let warm = peer.regression_checks(&windows, &tree()).unwrap();
    assert_eq!(world.launcher.commands("lister").len(), lists);
    assert!(warm.unlisted.is_empty());
    let foreign = warm.checks.iter().find(|c| c.family == "meter").unwrap();
    assert_eq!(foreign.key, mac.checks[0].key);
    assert!(!foreign.runnable);
    let hits = world
        .hub
        .lookup(std::slice::from_ref(&foreign.key))
        .unwrap();
    assert_eq!(hits[0].as_ref().unwrap().machine, world.ctx.machine);

    let mut next = daemon(
        &world,
        ignite_store::local::open_in_memory().unwrap(),
        FakeTrees {
            head: Commit::new("def5678").unwrap(),
            ..FakeTrees::new(&world.dir, &[])
        },
    );
    Arc::get_mut(&mut next).unwrap().settings.os = Os::Windows;
    let next = next.regression_checks(&world.ctx, &tree()).unwrap();
    assert_eq!(next.checks.len(), 1);
    assert_eq!(next.unlisted[0].name, name("meter:listing"));
}

#[test]
fn a_measurement_check_without_a_calibration_digest_is_incomplete() {
    let world = measurement_world();
    measurement_listing(&world, "");
    let check = run(&world, &["meter:a"]);
    assert_eq!(outcome(&check, "meter:a"), Outcome::Incomplete);
    assert!(
        check.failures[0]
            .message
            .as_ref()
            .unwrap()
            .contains("no calibration digest")
    );
    assert!(world.launcher.commands("runner").is_empty());
}

#[test]
fn a_check_whose_inputs_cannot_be_resolved_is_unlisted_and_the_rest_are_kept() {
    let manifest = family("unit", "\"any\"", "lister", "");
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\"}\n{\"check\": \"unit:c\", \"inputs\": [\"missing.txt\"]}\n"
            .to_owned();

    let set = world.checks.regression_checks(&world.ctx, &tree()).unwrap();

    assert_eq!(set.checks.len(), 1);
    assert_eq!(set.unlisted.len(), 1);
    assert_eq!(set.unlisted[0].name, name("unit:c"));
    assert!(set.unlisted[0].message.contains("cannot be resolved"));
}

#[test]
fn a_check_name_ignite_refuses_makes_the_family_listing_an_incomplete_check() {
    let manifest = family("unit", "\"any\"", "lister", "");
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\"}\n{\"check\": \"unit:a land is played\"}\n".to_owned();

    // Tip verification names it among the checks it cannot run.
    let set = world.checks.regression_checks(&world.ctx, &tree()).unwrap();
    assert_eq!(set.checks.len(), 1);
    assert_eq!(set.unlisted.len(), 1);
    assert_eq!(set.unlisted[0].name, name("unit:listing"));
    assert!(
        set.unlisted[0].message.contains("unit:a land is played"),
        "{}",
        set.unlisted[0].message
    );

    // A run of the whole family, as the gate runs it, does not pass.
    let mut run = CheckRun::default();
    let open = world.checks.open(&world.ctx, &tree()).unwrap().1;
    let mut prepared =
        world
            .checks
            .prepare(&world.ctx, &open, &["unit"], &options(false), &mut run);
    for family in &mut prepared {
        family.keep(|_| true, &mut run);
    }
    assert_eq!(outcome(&run, "unit:listing"), Outcome::Incomplete);
    assert!(!run.passed());
}

#[test]
fn an_empty_nextest_listing_is_a_failed_listing_and_is_not_remembered() {
    let world = world(MANIFEST, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = "{\"rust-suites\": {}}\n".to_owned();

    let first = run(&world, &["rs:core::a"]);

    assert_eq!(first.incomplete_families.len(), 1);
    assert_eq!(first.incomplete_families[0].0, "rs");
    // The listing that follows is believed, because the empty one was not kept.
    *world.launcher.listing.lock().unwrap() = NEXTEST_LISTING.to_owned();
    *world.launcher.nextest_run.lock().unwrap() =
        "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"core::core$a\"}\n".to_owned();
    let second = run(&world, &["rs:core::a"]);
    assert!(second.passed(), "{second:?}");
}

#[test]
fn a_jsonl_family_that_listed_checks_and_then_lists_none_is_a_failed_listing() {
    let world = jsonl_world();
    let first = run(&world, &["unit:a"]);
    assert!(first.passed(), "{first:?}");
    *world.launcher.listing.lock().unwrap() = String::new();
    // New inputs, so the kept listing does not answer.
    world
        .git
        .set_object("src", Oid::new("b".repeat(40)).unwrap());

    let second = run(&world, &["unit:a"]);

    assert!(second.incomplete_families.iter().any(|(f, _)| f == "unit"));
    assert!(!second.passed());
}

#[test]
fn a_pass_on_the_last_line_of_a_cut_short_run_does_not_count() {
    let text = "{\"check\": \"a\", \"outcome\": \"pass\"}\n\
                {\"check\": \"b\", \"outcome\": \"pass\"}\n";
    let text = text
        .replace("\"a\"", "\"unit:a\"")
        .replace("\"b\"", "\"unit:b\"");

    let whole = adapters::jsonl_results(&text);
    let cut = adapters::jsonl_results_cut_short(&text);

    assert!(whole.iter().all(|r| r.outcome == Outcome::Pass));
    let outcome = |check: &str| cut.iter().find(|r| r.check.as_str() == check).unwrap();
    assert_eq!(outcome("unit:a").outcome, Outcome::Pass, "followed by b");
    assert_eq!(outcome("unit:b").outcome, Outcome::Incomplete);
    assert_eq!(outcome("unit:b").reason, None);
}

#[test]
fn a_check_whose_command_was_killed_after_its_pass_is_run_again_not_cached() {
    let world = jsonl_world();
    // `unit:b` hangs, after `unit:a` passed in the same command, then `unit:a` runs alone.
    world.launcher.script("unit:b", &[Some("hang")]);
    let first = run(&world, &["unit:a", "unit:b"]);
    assert_eq!(outcome(&first, "unit:b"), Outcome::Incomplete);
    assert_eq!(outcome(&first, "unit:a"), Outcome::Pass);
    assert_eq!(cached(&world, "unit:a"), Some(Outcome::Pass));
    assert_eq!(world.launcher.commands("runner").len(), 2);
}

#[test]
fn a_failing_check_tells_the_agent_to_rerun_with_the_flag_that_bypasses_the_cache() {
    let world = jsonl_world();
    world
        .launcher
        .script("unit:b", &[Some("fail"), Some("fail")]);
    let verb = TestVerb {
        checks: world.checks.clone(),
        express: None,
    };
    let request = |args: &[&str]| Request {
        verb: "test".to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: world.dir.join("sub"),
        session: None,
        wait_s: 1,
        pid: None,
    };

    let reply = verb.run(&world.ctx, &request(&["unit:b"]));

    assert!(
        reply.text.contains("rerun    ignite test --rerun unit:b"),
        "{}",
        reply.text
    );
    // The cached failure answers without the flag and is run again with it.
    world.launcher.script("unit:b", &[Some("pass")]);
    let cached_again = verb.run(&world.ctx, &request(&["unit:b"]));
    assert_eq!(cached_again.exit, Exit::CheckFailed);
    let rerun = verb.run(&world.ctx, &request(&["unit:b", "--rerun"]));
    assert_eq!(rerun.exit, Exit::Success, "{}", rerun.text);
}

fn regression_ticket(id: &str, check: &str) -> TicketDetail {
    serde_json::from_value(serde_json::json!({
        "id": id, "parent": null, "next_child": 1, "title": format!("Regression: {check}"),
        "body": "", "type": "bug", "status": "open", "priority": 0, "assignee": null,
        "labels": ["regression"], "defer_until": null, "close_reason": null,
        "created_at": "2027-01-01T00:00:00.000Z", "created_by": "test",
        "updated_at": "2027-01-01T00:00:00.000Z", "closed_at": null, "version": 1,
        "session": null, "edges": [], "children": [], "ready": false, "blocked_by": [],
    }))
    .unwrap()
}

/// A world whose gate chooses `unit:b`: it fails half its runs after changes to the area the
/// tree edits, and is cheap.
fn gate_world(changed: &[&str]) -> World {
    let stats = CheckStats {
        check: name("unit:b"),
        family: "unit".to_owned(),
        state: CheckState::On,
        areas: vec![AreaStats {
            area: "game_rules/tests/".to_owned(),
            runs: 100,
            failures: 50,
        }],
        median_cost: Some(1.0),
        unit: "instructions".to_owned(),
    };
    let mut history = FakeHistory::default();
    history.replies.insert(
        "unit".to_owned(),
        CheckStatsReply {
            startup_billions: None,
            startup_cpu_s: None,
            family: "unit".to_owned(),
            family_rate: 0.5,
            runs: None,
            checks: vec![stats],
        },
    );
    let world = world(&family("unit", "\"any\"", "lister", ""), changed, history);
    *world.launcher.listing.lock().unwrap() = listing(&["unit:b"]);
    world
}

fn gated(world: &World, ticket: Option<&str>) -> Vec<String> {
    let mut options = options(false);
    options.ticket = ticket.map(|t| t.parse().unwrap());
    let gate = world.checks.gate(&world.ctx, &tree(), &options);
    gate.run
        .outcomes
        .iter()
        .map(|(check, _)| check.to_string())
        .collect()
}

#[test]
fn the_gate_leaves_out_a_check_with_an_open_regression_unless_it_lands_the_fix() {
    // The change touches the family's inputs, so the family is in the gate's scope.
    let changed = ["game_rules/tests/entering.rs", "src/a.py"];
    let free = gate_world(&changed);
    assert_eq!(
        gated(&free, Some("tk.9")),
        ["unit:b"],
        "chosen while no regression is open"
    );

    let world = gate_world(&changed);
    world.hub.insert_ticket(regression_ticket("tk.1", "unit:b"));
    assert!(verbs::open_regressions(&world.ctx).contains_key(&name("unit:b")));
    assert!(
        gated(&world, Some("tk.9")).is_empty(),
        "left out: known to fail"
    );
    assert_eq!(gated(&world, Some("tk.1")), ["unit:b"], "the fix runs it");
}

#[test]
fn the_gate_leaves_out_a_check_grouped_onto_another_checks_ticket() {
    let changed = ["game_rules/tests/entering.rs", "src/a.py"];
    let world = gate_world(&changed);
    // The ticket's title names `unit:a`; the hub added `unit:b` to it without renaming it.
    world.hub.insert_ticket(regression_ticket("tk.1", "unit:a"));
    world.hub.set_open_regressions(vec![
        OpenRegression {
            check: name("unit:a"),
            ticket: TicketId::new("tk.1").unwrap(),
        },
        OpenRegression {
            check: name("unit:b"),
            ticket: TicketId::new("tk.1").unwrap(),
        },
    ]);
    let open = verbs::open_regressions(&world.ctx);
    assert_eq!(
        open.get(&name("unit:b")).map(TicketId::as_str),
        Some("tk.1")
    );
    assert!(
        gated(&world, Some("tk.9")).is_empty(),
        "left out: known to fail"
    );
    assert_eq!(gated(&world, Some("tk.1")), ["unit:b"], "the fix runs it");
}

#[test]
fn the_gate_fails_when_the_changed_files_cannot_be_read() {
    let world = gate_world(&["<error>"]);

    let gate = world.checks.gate(&world.ctx, &tree(), &options(false));

    assert!(!gate.run.passed());
    assert!(
        gate.run
            .incomplete_families
            .iter()
            .any(|(why, _)| why.contains("changed files")),
        "{:?}",
        gate.run.incomplete_families
    );
    assert!(world.launcher.commands("runner").is_empty());
}

#[test]
fn a_foreign_family_that_cannot_be_listed_is_not_listed_again_for_the_same_inputs() {
    let manifest = family("win", "[\"windows\"]", "broken", "");
    let world = world(&manifest, &[], FakeHistory::default());

    let first = world.checks.regression_checks(&world.ctx, &tree()).unwrap();
    let tries = world.launcher.commands("broken").len();
    assert!(tries > 0);
    let second = world.checks.regression_checks(&world.ctx, &tree()).unwrap();

    assert_eq!(world.launcher.commands("broken").len(), tries);
    assert_eq!(first.unlisted.len(), 1);
    assert_eq!(second.unlisted.len(), 1);
    assert_eq!(second.unlisted[0].message, first.unlisted[0].message);
}

#[test]
fn a_foreign_family_is_listed_once_and_its_checks_are_still_keyed_each_round() {
    let manifest = family("win", "[\"windows\"]", "lister", "");
    let world = world(&manifest, &[], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = listing(&["win:c"]);

    let first = world.checks.regression_checks(&world.ctx, &tree()).unwrap();
    let second = world.checks.regression_checks(&world.ctx, &tree()).unwrap();

    assert_eq!(world.launcher.commands("lister").len(), 1);
    assert_eq!(first.checks.len(), 1);
    assert_eq!(second.checks.len(), 1);
    assert_eq!(first.checks[0].key, second.checks[0].key);
}

#[test]
fn a_family_this_machine_runs_is_tried_again_after_it_failed_to_list() {
    let manifest = family("unit", "\"any\"", "broken", "");
    let world = world(&manifest, &[], FakeHistory::default());

    world.checks.regression_checks(&world.ctx, &tree()).unwrap();
    let tries = world.launcher.commands("broken").len();
    world.checks.regression_checks(&world.ctx, &tree()).unwrap();

    assert_eq!(world.launcher.commands("broken").len(), tries * 2);
}
