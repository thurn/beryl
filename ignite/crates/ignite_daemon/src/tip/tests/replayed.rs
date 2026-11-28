//! A landing pushed replayed over landings its gate never saw is reverted at once when the
//! combination fails a check (`[revert] replayed`, see `tip::replayed`).

use ignite_core::traits::RevertOutcome;

use super::*;

/// A world where `tk.39` (`abab000`, changing `over`) landed while the gate of `tk.40` ran on
/// `aaaa000`, and `tk.40` (`bbbb000`, changing `src/a/lib.rs`) was then pushed replayed onto
/// it, with the gate's base recorded as `gated_base`. The check fails at the tip, `bbbb000`,
/// and passes at `abab000`.
fn replayed(over: &str, gated_base: Option<&str>) -> (World, Context, Commit, TipCheck) {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    world.work.add_landing("tk.39", &["abab000"], &[over]);
    world
        .work
        .add_landing("tk.40", &["bbbb000"], &["src/a/lib.rs"]);
    world.work.landings.lock().unwrap()[1].gated_base = gated_base.map(commit);
    let failing = check("a", 'a');
    world.work.set_checks(&tip, vec![failing.clone()]);
    world.work.set_outcome(&tip, &failing.check, Outcome::Fail);
    world
        .work
        .set_outcome(&commit("abab000"), &failing.check, Outcome::Pass);
    world.git.set_subject(&tip, "fix: phone rows");
    world.git.script_revert(RevertOutcome::Clean {
        commit: commit("cccc000"),
    });
    let mut ctx = world.ctx("m-1");
    ctx.config.revert.replayed = true;
    (world, ctx, tip, failing)
}

fn staging_pushes(world: &World) -> Vec<Commit> {
    world
        .git
        .pushes()
        .into_iter()
        .filter(|(_, _, branch)| branch == "staging")
        .map(|(_, at, _)| at)
        .collect()
}

fn verify(world: &World, ctx: &Context, tip: &Commit) {
    world
        .verifier()
        .pausing_with(Arc::new(|_| {}))
        .verify(ctx, tip)
        .unwrap();
}

/// On 2026-11-28 a landing of phone fixes passed its gate on one base and was pushed over a
/// landing that changed the effects its checks read. The combination failed seven phone
/// checks that had no recorded product catch, so only a regression was filed, and `master`
/// stood still for over four hours.
#[test]
fn a_landing_replayed_over_a_change_to_the_checks_inputs_is_reverted_at_once() {
    let (world, ctx, tip, failing) = replayed("src/a/effects.rs", Some("aaaa000"));

    verify(&world, &ctx, &tip);

    assert_eq!(staging_pushes(&world), [commit("cccc000")]);
    assert!(world.hub.regressions().is_empty());
    let reports = world.hub.reverted_landings();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].ticket, Some(ticket("tk.40")));
    assert_eq!(reports[0].checks, [failing.check]);
    // The reopened ticket says why, and names the landing it was replayed over.
    assert!(reports[0].output.contains("gate passed on aaaa000"));
    assert!(reports[0].output.contains("tk.39 (abab000)"));
}

#[test]
fn a_landing_whose_gate_ran_on_its_parent_is_reported() {
    let (world, ctx, tip, _) = replayed("src/a/effects.rs", None);

    verify(&world, &ctx, &tip);

    assert!(staging_pushes(&world).is_empty());
    assert_eq!(world.hub.regressions().len(), 1);
    assert_eq!(world.hub.regressions()[0].caused_by, Some(ticket("tk.40")));
}

/// The landings it was replayed over changed nothing the check reads, so the landing's own
/// change is what fails, as its gate could have seen.
#[test]
fn a_landing_replayed_over_unrelated_changes_is_reported() {
    let (world, ctx, tip, _) = replayed("docs/notes.md", Some("aaaa000"));

    verify(&world, &ctx, &tip);

    assert!(staging_pushes(&world).is_empty());
    assert_eq!(world.hub.regressions().len(), 1);
}

#[test]
fn without_the_setting_a_replayed_landing_is_reported() {
    let (world, mut ctx, tip, _) = replayed("src/a/effects.rs", Some("aaaa000"));
    ctx.config.revert.replayed = false;

    verify(&world, &ctx, &tip);

    assert!(staging_pushes(&world).is_empty());
    assert_eq!(world.hub.regressions().len(), 1);
}
