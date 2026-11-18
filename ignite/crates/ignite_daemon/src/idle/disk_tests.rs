//! The idle loop holds the trees under their disk limit, and measures them at a bounded rate.

use std::sync::atomic::Ordering;

use super::Fixture;
use crate::idle::{DISK_EVERY, ROOM_GB};

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
