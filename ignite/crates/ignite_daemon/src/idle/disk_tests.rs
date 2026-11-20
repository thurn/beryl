//! The idle loop holds the trees under their disk limit, and measures them at a bounded rate.

use std::sync::atomic::Ordering;

use super::Fixture;
use crate::idle::{DISK_EVERY, FLOOR_GB, LOW_EVERY, ROOM_GB};

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
    *f.host.free_gb.lock().unwrap() = Some(ROOM_GB + 100);
    assert!(idle.hold_disk().unwrap().is_none());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 0);

    *f.host.free_gb.lock().unwrap() = Some(ROOM_GB - 1);
    assert!(idle.hold_disk().unwrap().is_some());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 1);
}

/// The disk filled to 4 GB free while the disk loop spent each round measuring 1.6 TB of build
/// directories, and the daemon restarted for a release before any round finished. Below the
/// floor the free trees are cleared at once, without a measurement, and sooner again.
#[test]
fn a_disk_below_its_floor_clears_free_trees_without_measuring_them() {
    let f = Fixture::new();
    let idle = f.idle();
    *f.host.free_gb.lock().unwrap() = Some(FLOOR_GB + 10);
    assert!(idle.hold_disk().unwrap().is_some());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.freed.load(Ordering::SeqCst), 0);

    *f.host.free_gb.lock().unwrap() = Some(2);
    f.clock.advance(LOW_EVERY.as_secs_f64() + 1.0);
    assert!(idle.hold_disk().unwrap().is_some());
    assert_eq!(f.host.held.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.freed.load(Ordering::SeqCst), 1);
    assert_eq!(f.host.floor.load(Ordering::SeqCst), FLOOR_GB);
}
