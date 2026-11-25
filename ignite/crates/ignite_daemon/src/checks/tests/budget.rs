//! The gate's budget: `[gate] seconds` of running checks at the `land` class's rate, with
//! builds outside it, and no cold build for a check chosen by score.

use super::*;

fn tree() -> TreeId {
    TreeId::new("tree-01").unwrap()
}

fn landing() -> RunOptions {
    RunOptions {
        verb: "land".to_owned(),
        class: Class::Land,
        rerun: false,
        ticket: Some("tk.7".parse().unwrap()),
        session: None,
        reserve: None,
    }
}

fn family(name: &str, needs: &str, always: bool) -> String {
    format!(
        "[family.{name}]\nkind = \"regression\"\nadapter = \"jsonl\"\nplatforms = \"any\"\n\
         always = {always}\nneeds = [{needs}]\nlist = \"lister\"\nrun = \"runner {{checks}}\"\n\
         inputs = [\"src\"]\n"
    )
}

const DEPS: &str =
    "[product.deps]\nbuild = \"make deps\"\ninputs = [\"src\"]\noutputs = [\"out\"]\n";

/// A record of `failures` in 20 runs after changes to `src/a.py`'s area, at `cost` billions.
fn stats(check: &str, failures: u32, cost: f64) -> CheckStats {
    let area = ignite_core::inputs::area("src/a.py", &CrateGraph::default()).to_string();
    CheckStats {
        check: name(check),
        family: name(check).family().to_owned(),
        state: CheckState::On,
        areas: vec![AreaStats {
            area,
            runs: 20,
            failures,
        }],
        median_cost: Some(cost),
        unit: "instructions".to_owned(),
    }
}

fn list(world: &World, checks: &[String]) {
    let lines: Vec<String> = checks
        .iter()
        .map(|c| format!("{{\"check\": \"{c}\"}}"))
        .collect();
    *world.launcher.listing.lock().unwrap() = lines.join("\n");
}

/// Nine checks of 350 billion each cost three budgets of 60 s at 17.5 billion a second: the
/// gate runs the three likeliest to fail, leaves six to tip verification and lands.
#[test]
fn candidates_costing_three_budgets_run_the_best_third_and_land() {
    let names: Vec<String> = (0..9).map(|i| format!("unit:c{i}")).collect();
    let mut history = FakeHistory::default();
    history.replies.insert(
        "unit".to_owned(),
        CheckStatsReply {
            startup_billions: None,
            startup_cpu_s: None,
            family: "unit".to_owned(),
            family_rate: 0.01,
            runs: None,
            checks: (0..9u32)
                .map(|i| stats(&names[i as usize], 9 - i, 350.0))
                .collect(),
        },
    );
    let mut world = world(&family("unit", "", false), &["src/a.py"], history);
    // A threshold low enough that only the budget leaves checks out.
    world.ctx.config.gate.threshold = 1e-6;
    list(&world, &names);

    let gated = world.checks.gate(&world.ctx, &tree(), &landing());

    assert!(gated.run.passed(), "{gated:?}");
    assert_eq!(gated.run.ran, 3);
    let ran: Vec<&str> = gated.run.outcomes.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(ran, ["unit:c0", "unit:c1", "unit:c2"]);
    assert_eq!(gated.left, 6);
    let budget = gated.budget.clone().unwrap();
    assert!((budget.billions() - 1050.0).abs() < 1e-9);
    assert_eq!(budget.scored.0, 3);
    assert_eq!(budget.past_budget.0, 6);
    let text = verbs::render(&verbs::gate_lines(&gated));
    assert!(
        text.contains(
            "budget   60 s at 17.5 billion/s = 1,050 billion: 3 checks (1,050 billion) within \
             it, 0 checks (0 billion) outside it, 0 checks (0 billion) cached"
        ),
        "{text}"
    );
    assert!(text.contains("build    nothing built"), "{text}");
    assert!(
        text.contains(
            "tip      6 checks left to tip verification: 6 checks (2,100 billion) past the budget"
        ),
        "{text}"
    );
}

/// A family whose checks need a product the tree has never built, and of which no listing is
/// kept, is neither listed nor built: its checks go to tip verification. The family in the
/// always set that needs the same product is listed, built and run anyway.
#[test]
fn a_product_the_tree_never_built_is_built_only_for_checks_that_must_run() {
    let manifest = [DEPS.to_owned(), family("unit", "\"deps\"", false)].join("\n");
    let world = world(&manifest, &["src/a.py"], FakeHistory::default());
    world.builds.never_built(&tree(), "deps");
    list(&world, &["unit:a".to_owned()]);

    let gated = world.checks.gate(&world.ctx, &tree(), &landing());

    assert!(gated.run.passed(), "{gated:?}");
    assert!(world.builds.requests().is_empty(), "nothing was built");
    assert!(
        world.launcher.commands("lister").is_empty(),
        "nothing was listed"
    );
    assert_eq!(gated.run.cold_families, ["unit"]);
    let text = verbs::render(&verbs::gate_lines(&gated));
    assert!(
        text.contains(
            "1 family not listed, as listing needs a product this tree has never built (unit)"
        ),
        "{text}"
    );

    let manifest = [DEPS.to_owned(), family("unit", "\"deps\"", true)].join("\n");
    let always = super::world(&manifest, &["src/a.py"], FakeHistory::default());
    always.builds.never_built(&tree(), "deps");
    list(&always, &["unit:a".to_owned()]);

    let gated = always.checks.gate(&always.ctx, &tree(), &landing());

    assert!(gated.run.passed(), "{gated:?}");
    assert_eq!(gated.run.ran, 1);
    assert!(
        always
            .builds
            .requests()
            .iter()
            .all(|(_, product, _)| product == "deps")
    );
    let text = verbs::render(&verbs::gate_lines(&gated));
    assert!(text.contains("never built here before: deps"), "{text}");
}

/// A family in the always set whose `always_when` the change touches none of is scored: a check
/// that costs more than the budget and never failed goes to tip verification. A change to a
/// path `always_when` names runs it whatever it costs.
#[test]
fn always_when_leaves_a_costly_lint_to_the_tip_for_other_changes() {
    let narrowed = family("unit", "", true).replace(
        "always = true\n",
        "always = true\nalways_when = [\"src/viewer/**\"]\n",
    );
    let history = || {
        let mut history = FakeHistory::default();
        history.replies.insert(
            "unit".to_owned(),
            CheckStatsReply {
                startup_billions: None,
                startup_cpu_s: None,
                family: "unit".to_owned(),
                family_rate: 0.0,
                runs: None,
                checks: vec![stats("unit:lint", 0, 2000.0)],
            },
        );
        history
    };

    let other = world(&narrowed, &["src/a.py"], history());
    list(&other, &["unit:lint".to_owned()]);
    let gated = other.checks.gate(&other.ctx, &tree(), &landing());
    assert!(gated.run.passed(), "{gated:?}");
    assert_eq!(gated.run.ran, 0);
    assert_eq!(gated.left, 1);

    let viewer = world(&narrowed, &["src/viewer/main.py"], history());
    list(&viewer, &["unit:lint".to_owned()]);
    let gated = viewer.checks.gate(&viewer.ctx, &tree(), &landing());
    assert!(gated.run.passed(), "{gated:?}");
    assert_eq!(gated.run.ran, 1);
    assert_eq!(gated.budget.unwrap().forced.0, 1);
}

/// `ignite check` and then `ignite land` of the same change used to spend two budgets: the
/// second gate found the first one's checks cached, at no cost, and ran the next three. The
/// budget belongs to the change, so the second gate charges those cached checks again and runs
/// nothing new.
#[test]
fn a_second_gate_of_the_same_change_spends_no_second_budget() {
    let names: Vec<String> = (0..9).map(|i| format!("unit:c{i}")).collect();
    let mut history = FakeHistory::default();
    history.replies.insert(
        "unit".to_owned(),
        CheckStatsReply {
            startup_billions: None,
            startup_cpu_s: None,
            family: "unit".to_owned(),
            family_rate: 0.01,
            runs: None,
            checks: (0..9u32)
                .map(|i| stats(&names[i as usize], 9 - i, 350.0))
                .collect(),
        },
    );
    let mut world = world(&family("unit", "", false), &["src/a.py"], history);
    world.ctx.config.gate.threshold = 1e-6;
    list(&world, &names);

    let first = world.checks.gate(&world.ctx, &tree(), &landing());
    assert!(first.run.passed(), "{first:?}");
    assert_eq!(first.run.ran, 3);

    let second = world.checks.gate(&world.ctx, &tree(), &landing());

    assert!(second.run.passed(), "{second:?}");
    assert_eq!(second.run.ran, 0, "{second:?}");
    assert_eq!(second.run.from_cache, 3);
    assert_eq!(second.left, 6);
    let text = verbs::render(&verbs::gate_lines(&second));
    assert!(
        text.contains("1,050 billion spent by an earlier gate of this change"),
        "{text}"
    );
}

/// A gate that fails still fixes what the change's next gate spends. On 2026-11-20 a change's
/// gates kept failing on a file missing from the machine's cache, and each one found the checks
/// the last had finished in the cache, at no cost, and spent a fresh budget on the next ones by
/// score. The second gate here charges and answers only what the first one chose.
#[test]
fn a_gate_after_a_failed_gate_of_the_same_change_spends_no_second_budget() {
    let names: Vec<String> = (0..9).map(|i| format!("unit:c{i}")).collect();
    let mut history = FakeHistory::default();
    history.replies.insert(
        "unit".to_owned(),
        CheckStatsReply {
            startup_billions: None,
            startup_cpu_s: None,
            family: "unit".to_owned(),
            family_rate: 0.01,
            runs: None,
            checks: (0..9u32)
                .map(|i| stats(&names[i as usize], 9 - i, 350.0))
                .collect(),
        },
    );
    let mut world = world(&family("unit", "", false), &["src/a.py"], history);
    world.ctx.config.gate.threshold = 1e-6;
    list(&world, &names);
    world
        .launcher
        .script("unit:c2", &[Some("fail"), Some("fail")]);

    let first = world.checks.gate(&world.ctx, &tree(), &landing());
    assert!(!first.run.passed(), "{first:?}");
    assert_eq!(first.left, 6);

    let second = world.checks.gate(&world.ctx, &tree(), &landing());

    // The failure is cached as well, so the second gate charges all three again.
    assert_eq!(second.run.ran + second.run.from_cache, 3, "{second:?}");
    assert_eq!(second.left, 6, "{second:?}");
    assert!((second.budget.unwrap().paid - 1050.0).abs() < 1e-6);
}
