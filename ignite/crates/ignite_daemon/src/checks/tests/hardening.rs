//! Tip verification's view of the checks and the check runner's handling of broken listings and
//! cut-short runs: families this machine does not run, listings that fail or come back empty,
//! passes from a command that did not finish, and the gate with open regressions.

use super::*;

use ignite_core::protocol::hub::OpenRegression;
use ignite_core::protocol::tickets::TicketDetail;

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
fn a_foreign_family_that_cannot_be_listed_is_not_reported_and_names_its_platforms() {
    let manifest = family("win", "[\"windows\"]", "broken", "");
    let world = world(&manifest, &[], FakeHistory::default());

    let set = world.checks.regression_checks(&world.ctx, &tree()).unwrap();

    assert_eq!(set.unlisted.len(), 1);
    assert!(!set.unlisted[0].reportable);
    assert_eq!(set.unlisted[0].platforms, vec![Os::Windows]);
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
    let open = world.checks.open(&tree()).unwrap();
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
