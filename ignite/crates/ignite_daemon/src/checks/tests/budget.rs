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
