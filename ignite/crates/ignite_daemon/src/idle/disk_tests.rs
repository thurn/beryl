//! The idle loop holds the trees under their disk limit, and measures them at a bounded rate.

use std::sync::atomic::Ordering;

use super::Fixture;
use crate::idle::{DISK_EVERY, FLOOR_GB, ROOM_GB};
use crate::trees::{BusyTree, FloorStuck};
use ignite_core::ids::TreeId;

#[test]
fn the_disk_is_held_at_most_once_every_thirty_minutes() {
    let f = Fixture::new();
    let idle = f.idle();

    assert!(idle.hold_disk().unwrap().is_some());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 1);

    // Rounds of the loop come every few seconds, but the trees are not measured again.
    f.clock.advance(DISK_EVERY.as_secs_f64() - 1.0);
    assert!(idle.hold_disk().unwrap().is_none());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 1);

    f.clock.advance(2.0);
    assert!(idle.hold_disk().unwrap().is_some());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 2);
}

/// A measurement walked about 290 GB for over ten minutes on jarvis, holding the idle loop in
/// one round, while 255 GB of the disk was free. With that much room the trees are not measured.
#[test]
fn the_trees_are_not_measured_while_the_disk_has_room() {
    let f = Fixture::new();
    let idle = f.idle();
    *f.host.free_gib.lock().unwrap() = Some(ROOM_GB + 100);
    assert!(idle.hold_disk().unwrap().is_none());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 0);

    *f.host.free_gib.lock().unwrap() = Some(ROOM_GB - 1);
    assert!(idle.hold_disk().unwrap().is_some());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 1);
}

#[test]
fn the_trees_are_measured_while_a_cap_can_bind_even_when_the_disk_has_room() {
    let f = Fixture::new();
    let idle = f.idle();
    *f.host.free_gib.lock().unwrap() = Some(ROOM_GB + 100);
    *f.host.cap_gb.lock().unwrap() = Some(300);

    assert!(idle.hold_disk().unwrap().is_some());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 1);
}

/// The disk filled to 4 GB free while the disk loop spent each round measuring 1.6 TB of build
/// directories, and the daemon restarted for a release before any round finished. Below the
/// floor the free trees are cleared at once, without a measurement, minutes after the last
/// measurement and not half an hour.
#[test]
fn a_disk_below_its_floor_clears_free_trees_without_measuring_them() {
    let f = Fixture::new();
    let idle = f.idle();
    *f.host.free_gib.lock().unwrap() = Some(FLOOR_GB + 10);
    assert!(idle.hold_disk().unwrap().is_some());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.freed.load(Ordering::SeqCst), 0);

    *f.host.free_gib.lock().unwrap() = Some(2);
    f.clock.advance(60.0);
    assert!(idle.hold_disk().unwrap().is_some());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.freed.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.floor.load(Ordering::SeqCst), FLOOR_GB);
}

/// A measurement of the trees can take many minutes, and on a busy machine the disk can fall
/// below its floor meanwhile. The measurement stops partway and the floor pass runs in the same
/// round, before the measurement would have finished. The next round measures again.
#[test]
fn a_disk_falling_below_its_floor_during_a_measurement_is_cleared_at_once() {
    let f = Fixture::new();
    let idle = f.idle();
    *f.host.free_gib.lock().unwrap() = Some(FLOOR_GB + 10);
    f.host.falls_during_walk.store(true, Ordering::SeqCst);

    assert!(idle.hold_disk().unwrap().is_some());

    assert_eq!(
        *f.host.events.lock().unwrap(),
        ["walk started", "walk stopped", "floor pass"]
    );
    assert_eq!(f.host.freed.load(Ordering::SeqCst), 1);

    // Back above the floor, the trees are measured at the next round, not half an hour later.
    f.host.falls_during_walk.store(false, Ordering::SeqCst);
    *f.host.free_gib.lock().unwrap() = Some(FLOOR_GB + 10);
    f.clock.advance(60.0);
    assert!(idle.hold_disk().unwrap().is_some());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 2);
}

/// On 2026-11-20 every tree was leased and running commands, and the disk stayed full while the
/// log said only what had been deleted. A floor pass that can delete nothing more reports which
/// trees hold the space and why, and the pass runs again at every round, so the first tree
/// whose command ends is cleared within a minute instead of five.
#[test]
fn a_floor_pass_with_only_busy_trees_says_why_and_runs_again_next_round() {
    let f = Fixture::new();
    let idle = f.idle();
    *f.host.free_gib.lock().unwrap() = Some(2);
    let gib = ignite_core::capacity::BYTES_PER_GIB;
    *f.host.stuck.lock().unwrap() = Some(FloorStuck {
        free: 2 * gib,
        floor: FLOOR_GB * gib,
        busy: vec![BusyTree {
            tree: TreeId::new("tree-01").unwrap(),
            why: "`test` for T.1 waits for disk space to start a build".to_owned(),
        }],
    });

    let report = idle.hold_floor().unwrap().unwrap();
    let stuck = report.stuck.expect("the pass says what holds the space");
    let said = stuck.to_string();
    assert!(
        said.contains("2.0 GiB free, under its 50 GiB floor"),
        "{said}"
    );
    assert!(
        said.contains("tree-01 (`test` for T.1 waits for disk space"),
        "{said}"
    );
    assert!(said.contains("ignite stop"), "{said}");

    f.clock.advance(60.0);
    *f.host.stuck.lock().unwrap() = None;
    let report = idle.hold_floor().unwrap().unwrap();
    assert!(report.stuck.is_none());
    assert_eq!(f.host.freed.load(Ordering::SeqCst), 2);

    // Above the floor the pass does nothing.
    *f.host.free_gib.lock().unwrap() = Some(FLOOR_GB + 1);
    assert!(idle.hold_floor().unwrap().is_none());
    assert_eq!(f.host.freed.load(Ordering::SeqCst), 2);
}

/// With no tree left holding builds, the space is outside the trees, as on 2026-11-18 when
/// files outside them grew until the disk was full.
#[test]
fn a_floor_pass_with_no_builds_left_names_the_space_outside_the_trees() {
    let gib = ignite_core::capacity::BYTES_PER_GIB;
    let stuck = FloorStuck {
        free: 3 * gib,
        floor: FLOOR_GB * gib,
        busy: vec![],
    };
    assert!(
        stuck
            .to_string()
            .contains("held outside the trees' build directories"),
        "{stuck}"
    );
}
