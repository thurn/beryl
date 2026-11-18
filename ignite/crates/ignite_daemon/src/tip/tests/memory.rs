//! The verifier's memory across a restart: a verifier built over the database an earlier one
//! saved to goes on from what the earlier one reported and held off.

use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

/// A database file of its own, which a verifier built later can open again.
fn database() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ignite-tip-memory-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("daemon.db")
}

/// A verifier that keeps its memory in the database at `path`, as the daemon builds one after
/// a restart.
fn remembering(world: &World, path: &std::path::Path) -> TipVerifier {
    world
        .verifier()
        .with_memory(ignite_store::local::open(path).unwrap())
}

fn clean_up(path: &std::path::Path) {
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_failure_reported_before_a_restart_is_not_reported_again() {
    let path = database();
    let world = World::new();
    let ctx = world.ctx("m-1");
    world.work.add_landing("tk.21", &["bbbb000"], &["src/a"]);
    let broken = commit("bbbb000");
    world.land(&broken);
    world.work.set_checks(&broken, vec![check("a", 'a')]);
    world
        .work
        .set_outcome(&broken, &name("unit:a"), Outcome::Fail);
    remembering(&world, &path).verify(&ctx, &broken).unwrap();
    assert_eq!(world.hub.regressions().len(), 1);

    // A later landing leaves the check's inputs alone, so its failure still stands. The
    // verifier after the restart is not the one that reported it.
    let unrelated = commit("cccc000");
    world.land(&unrelated);
    world.work.set_checks(&unrelated, vec![check("a", 'a')]);
    world
        .work
        .set_outcome(&unrelated, &name("unit:a"), Outcome::Fail);
    remembering(&world, &path).verify(&ctx, &unrelated).unwrap();

    assert_eq!(world.hub.regressions().len(), 1);
    clean_up(&path);
}

#[test]
fn a_check_held_off_before_a_restart_stays_held_off() {
    let path = database();
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    world.work.set_checks(&tip, vec![check("a", 'a')]);
    let ctx = world.ctx("m-1");
    world
        .work
        .set_outcome(&tip, &name("unit:a"), Outcome::Incomplete);
    remembering(&world, &path).verify(&ctx, &tip).unwrap();
    assert_eq!(world.work.runs().len(), 1);
    assert_eq!(world.hub.incomplete_reports().len(), 1);

    // Half an hour later, within the hour's hold-off, the restarted verifier neither runs the
    // check nor reports it again.
    world.clock.advance(1_800.0);
    remembering(&world, &path).verify(&ctx, &tip).unwrap();
    assert_eq!(world.work.runs().len(), 1);
    assert_eq!(world.hub.incomplete_reports().len(), 1);
    clean_up(&path);
}

#[test]
fn the_wait_for_another_platform_keeps_its_start_across_a_restart() {
    let path = database();
    let world = World::new();
    let ctx = world.ctx("m-1");
    let mut elsewhere = check("w", 'e');
    elsewhere.runnable = false;
    elsewhere.key.platform = Some(Os::Windows);
    let tip = commit("bbbb000");
    world.land(&tip);
    world.work.set_checks(&tip, vec![elsewhere]);
    remembering(&world, &path).verify(&ctx, &tip).unwrap();

    // The wait began before the restart, so an hour after that start it is reported at once.
    world.clock.advance(MACHINE_WAIT_S);
    remembering(&world, &path).verify(&ctx, &tip).unwrap();
    assert_eq!(world.hub.tips_waiting().len(), 1);
    clean_up(&path);
}

#[test]
fn a_row_from_an_older_build_loads_with_the_missing_fields_empty() {
    let path = database();
    let conn = ignite_store::local::open(&path).unwrap();
    ignite_store::local::tip_state::put(&conn, r#"{"settled":"bbbb000"}"#, "t").unwrap();
    drop(conn);
    let world = World::new();

    let verifier = remembering(&world, &path);

    assert_eq!(verifier.settled, Some(commit("bbbb000")));
    assert!(verifier.reported.is_empty());
    assert!(verifier.held_off.is_empty());
    clean_up(&path);
}

#[test]
fn a_row_that_cannot_be_read_starts_the_verifier_empty() {
    let path = database();
    let conn = ignite_store::local::open(&path).unwrap();
    ignite_store::local::tip_state::put(&conn, "not json", "t").unwrap();
    drop(conn);
    let world = World::new();

    let verifier = remembering(&world, &path);

    assert_eq!(verifier.settled, None);
    clean_up(&path);
}
