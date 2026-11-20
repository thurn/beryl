//! The landing gate stops at the first fault it knows of: an invalid manifest entry refuses the
//! landing before any family that needs a product is listed, and a confirmed failure starts no
//! further command in any family.

use std::sync::atomic::AtomicBool;

use ignite_core::protocol::hub::CachedResult;

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

/// A jsonl family in the always set, so the gate runs every check it lists.
fn family(name: &str, needs: &str) -> String {
    format!(
        "[family.{name}]\nkind = \"regression\"\nadapter = \"jsonl\"\nplatforms = \"any\"\n\
         always = true\nneeds = [{needs}]\nlist = \"lister\"\nrun = \"runner {{checks}}\"\n\
         inputs = [\"src\"]\n"
    )
}

const DEPS: &str =
    "[product.deps]\nbuild = \"make deps\"\ninputs = [\"src\"]\noutputs = [\"out\"]\n";

/// A landing was refused for three invalid manifest entries only after it built every product
/// and ran 5,478 checks, though the entries were known to be invalid once listed. The families
/// that list without building now go first, while the products the other families need to be
/// listed build, and an invalid entry among them refuses the landing before any other family
/// is listed or any check runs.
#[test]
fn an_invalid_entry_refuses_the_landing_before_anything_runs() {
    let manifest = [
        DEPS.to_owned(),
        family("unit", "\"deps\""),
        family("zlint", ""),
    ]
    .join("\n");
    let world = world(&manifest, &["src/a.py"], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = "{\"check\": \"unit:a\"}\n\
         {\"check\": \"zlint:ok\"}\n\
         {\"check\": \"zlint:bad\", \"inputs\": [\"docs/*.json\"]}\n"
        .to_owned();

    let gated = world.checks.gate(&world.ctx, &tree(), &landing());

    assert!(!gated.run.passed());
    // The product `unit` needs to be listed was built while `zlint` was listed.
    assert_eq!(world.builds.requests().len(), 1, "only deps");
    assert!(world.launcher.commands("runner").is_empty(), "nothing ran");
    assert_eq!(world.launcher.commands("lister").len(), 1, "only zlint");
    assert_eq!(gated.run.unlisted, ["unit"]);
    let bad = &gated.run.failures[0];
    assert_eq!(bad.check, name("zlint:bad"));
    assert_eq!(bad.reason, Some(IncompleteReason::InvalidEntry));
    let text = verbs::render(&verbs::gate_lines(&gated));
    assert!(text.contains("INCOMPLETE zlint:bad"), "{text}");
    assert!(
        text.contains("stopped  at the first fault: 1 family not listed"),
        "{text}"
    );
}

/// The history of a family in which each check costs `cost` billion instructions.
fn history(family: &str, checks: &[&str], cost: f64) -> CheckStatsReply {
    CheckStatsReply {
        startup_billions: None,
        startup_cpu_s: None,
        family: family.to_owned(),
        family_rate: 0.1,
        checks: checks
            .iter()
            .map(|check| CheckStats {
                check: name(check),
                family: family.to_owned(),
                state: CheckState::On,
                areas: Vec::new(),
                median_cost: Some(cost),
                unit: "instructions".to_owned(),
            })
            .collect(),
    }
}

/// The gate ran its families in name order and finished every one after a check had failed.
/// It now starts every family at once, and once a failure is confirmed by its rerun starts no
/// further command. Whether the long family's one command started before the failure was
/// confirmed depends on how the threads interleave, so either answer is accepted; the
/// deterministic tests of the stop flag are below.
#[test]
fn a_confirmed_failure_in_a_cheap_family_refuses_the_landing() {
    let manifest = [family("along", ""), family("cheap", "")].join("\n");
    let mut replies = FakeHistory::default();
    let long = ["along:a", "along:b", "along:c"];
    replies
        .replies
        .insert("along".to_owned(), history("along", &long, 100.0));
    replies
        .replies
        .insert("cheap".to_owned(), history("cheap", &["cheap:x"], 1.0));
    let world = world(&manifest, &["src/a.py"], replies);
    *world.launcher.listing.lock().unwrap() = "{\"check\": \"along:a\"}\n\
         {\"check\": \"along:b\"}\n{\"check\": \"along:c\"}\n{\"check\": \"cheap:x\"}\n"
        .to_owned();
    world
        .launcher
        .script("cheap:x", &[Some("fail"), Some("fail")]);

    let gated = world.checks.gate(&world.ctx, &tree(), &landing());

    let runs = world.launcher.commands("runner");
    let cheap = runs.iter().filter(|r| r[1] == "cheap:x").count();
    assert_eq!(cheap, 2, "{runs:?}");
    assert!(!gated.run.passed());
    assert!(
        gated
            .run
            .outcomes
            .contains(&(name("cheap:x"), Outcome::Fail))
    );
    let along = gated
        .run
        .outcomes
        .iter()
        .filter(|(c, o)| c.family() == "along" && *o == Outcome::Pass)
        .count();
    // The long family either ran its one command whole or never started it.
    assert_eq!(along + gated.run.skipped as usize, 3, "{gated:?}");
}

/// The checks of `family` in the tree as `world` lists them, and the opened tree.
fn listed(world: &World, family: &str) -> (Checks, OpenTree, Vec<(Keyed, Option<CachedResult>)>) {
    let (checks, open) = world.checks.open(&world.ctx, &tree()).unwrap();
    let mut run = CheckRun::default();
    let prepared = checks.prepare(&world.ctx, &open, &[family], &landing(), &mut run);
    let items = prepared
        .into_iter()
        .flat_map(|p| p.keyed)
        .map(|k| (k, None))
        .collect();
    (checks, open, items)
}

/// The gate runs its families at once, sharing one stop flag: a family that has not yet
/// started when another family's failure is confirmed starts no command and counts its checks
/// as skipped.
#[test]
fn a_raised_stop_flag_starts_no_command() {
    let world = world(&family("unit", ""), &["src/a.py"], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\"}\n{\"check\": \"unit:b\"}\n".to_owned();
    let (checks, open, items) = listed(&world, "unit");
    let stop = AtomicBool::new(true);
    let mut run = CheckRun::default();

    checks.execute(
        &world.ctx,
        &open,
        "unit",
        items,
        &landing(),
        &mut run,
        Some(&stop),
    );

    assert!(world.launcher.commands("runner").is_empty());
    assert_eq!(run.skipped, 2);
    assert!(!run.passed());
}

/// A failure confirmed by its rerun raises the flag the gate's other families read.
#[test]
fn a_confirmed_failure_raises_the_stop_flag() {
    let world = world(&family("unit", ""), &["src/a.py"], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() = "{\"check\": \"unit:a\"}\n".to_owned();
    world
        .launcher
        .script("unit:a", &[Some("fail"), Some("fail")]);
    let (checks, open, items) = listed(&world, "unit");
    let stop = AtomicBool::new(false);
    let mut run = CheckRun::default();

    checks.execute(
        &world.ctx,
        &open,
        "unit",
        items,
        &landing(),
        &mut run,
        Some(&stop),
    );

    assert!(stop.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(run.outcomes, [(name("unit:a"), Outcome::Fail)]);
}

/// Within one family, a check left unreported was tried again after another check's failure
/// had been confirmed, though the landing was already refused.
#[test]
fn a_confirmed_failure_stops_the_retries_of_an_unreported_check() {
    let world = world(&family("unit", ""), &["src/a.py"], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\"}\n{\"check\": \"unit:b\"}\n".to_owned();
    world
        .launcher
        .script("unit:a", &[Some("fail"), Some("fail")]);
    world.launcher.script("unit:b", &[None, None, None]);

    let gated = world.checks.gate(&world.ctx, &tree(), &landing());

    assert_eq!(world.launcher.commands("runner").len(), 2);
    assert_eq!(gated.run.outcomes, [(name("unit:a"), Outcome::Fail)]);
    assert_eq!(gated.run.skipped, 1);
    assert!(!gated.run.passed());
}

/// `ignite test` is not the gate: it gives every check it was asked for a verdict.
#[test]
fn outside_the_gate_an_unreported_check_is_still_tried_three_times() {
    let world = world(&family("unit", ""), &["src/a.py"], FakeHistory::default());
    *world.launcher.listing.lock().unwrap() =
        "{\"check\": \"unit:a\"}\n{\"check\": \"unit:b\"}\n".to_owned();
    world
        .launcher
        .script("unit:a", &[Some("fail"), Some("fail")]);
    world.launcher.script("unit:b", &[None, None, None]);

    let run = world.checks.run(
        &world.ctx,
        &tree(),
        &[name("unit:a"), name("unit:b")],
        &options(false),
    );

    assert_eq!(world.launcher.commands("runner").len(), 3);
    assert_eq!(run.skipped, 0);
    assert_eq!(run.outcomes.len(), 2);
}
