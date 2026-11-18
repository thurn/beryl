//! The idle loop holds the trees under their disk limit, and measures them at a bounded rate.

use std::sync::atomic::Ordering;

use super::Fixture;
use crate::idle::DISK_EVERY;

#[test]
fn the_disk_is_held_at_most_once_every_ten_minutes() {
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
