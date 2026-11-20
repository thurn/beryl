//! A build waits while the disk has less than [`START_GB`] free, instead of starting and failing
//! with `ENOSPC` partway through.

use std::collections::VecDeque;

use super::*;
use crate::build::room::RECHECK;
use crate::build::{DiskRoom, START_GB};

/// A disk whose free space reads from a script, the last reading repeating once it runs out,
/// and whose pauses return at once.
#[derive(Default)]
struct FakeRoom {
    readings: Mutex<VecDeque<Option<u64>>>,
    reads: Mutex<usize>,
    pauses: Mutex<usize>,
}

impl FakeRoom {
    fn new(readings: &[Option<u64>]) -> Arc<Self> {
        Arc::new(Self {
            readings: Mutex::new(readings.iter().copied().collect()),
            ..Self::default()
        })
    }

    fn set(&self, readings: &[Option<u64>]) {
        *lock(&self.readings) = readings.iter().copied().collect();
    }

    fn reads(&self) -> usize {
        *lock(&self.reads)
    }

    fn pauses(&self) -> usize {
        *lock(&self.pauses)
    }
}

impl DiskRoom for FakeRoom {
    fn free_gb(&self) -> Option<u64> {
        *lock(&self.reads) += 1;
        let mut readings = lock(&self.readings);
        if readings.len() > 1 {
            readings.pop_front().flatten()
        } else {
            readings.front().copied().flatten()
        }
    }

    fn pause(&self, _: std::time::Duration) {
        *lock(&self.pauses) += 1;
        thread::yield_now();
    }
}

/// The fixture's manager, made to wait for disk space on `room`.
fn with_room(name: &str, gated: bool, room: &Arc<FakeRoom>) -> Fixture {
    let mut f = fixture(name, gated);
    let manager = Arc::try_unwrap(f.manager).ok().unwrap();
    f.manager = Arc::new(manager.with_room(Arc::clone(room) as Arc<dyn DiskRoom>));
    f
}

/// How many pauses one wait between two readings of the free space takes.
fn pauses_per_reading() -> usize {
    usize::try_from(RECHECK.as_millis() / super::flight::STOP_RECHECK.as_millis()).unwrap()
}

#[test]
fn a_build_waits_while_the_disk_is_short_and_starts_once_it_has_room() {
    let room = FakeRoom::new(&[Some(2), Some(START_GB - 1), Some(START_GB)]);
    let f = with_room("room-waits", false, &room);
    let result = f.manager.ensure(&tree(), "tests", &[]);
    assert!(matches!(result, BuildResult::Built { .. }), "{result:?}");
    // Two readings were short, each followed by a wait, and the third had room.
    assert_eq!(room.reads(), 3);
    assert_eq!(room.pauses(), 2 * pauses_per_reading());
    assert_eq!(f.launcher.builds().len(), 1);
}

#[test]
fn a_tree_that_holds_the_product_answers_without_reading_the_disk() {
    let room = FakeRoom::new(&[Some(START_GB * 2)]);
    let f = with_room("room-current", false, &room);
    assert!(matches!(
        f.manager.ensure(&tree(), "tests", &[]),
        BuildResult::Built { .. }
    ));
    room.set(&[Some(1)]);
    let reads = room.reads();
    assert_eq!(
        f.manager.ensure(&tree(), "tests", &[]),
        BuildResult::Current
    );
    assert_eq!(room.reads(), reads);
    assert_eq!(room.pauses(), 0);
}

#[test]
fn a_disk_whose_free_space_cannot_be_read_never_holds_a_build() {
    let room = FakeRoom::new(&[None]);
    let f = with_room("room-unreadable", false, &room);
    let result = f.manager.ensure(&tree(), "tests", &[]);
    assert!(matches!(result, BuildResult::Built { .. }), "{result:?}");
    assert_eq!(room.pauses(), 0);
}

#[test]
fn a_stopped_request_stops_waiting_for_disk_space_and_builds_nothing() {
    let room = FakeRoom::new(&[Some(1)]);
    let f = with_room("room-stopped", false, &room);
    let steps = crate::steps::Steps::new(Arc::new(ignite_core::time::SystemClock));
    let check = steps.begin(
        crate::steps::Kind::Verb("check".to_owned()),
        crate::steps::Work::default(),
    );
    let waiter = {
        let (manager, check) = (Arc::clone(&f.manager), Arc::clone(&check));
        thread::spawn(move || {
            let _entered = check.enter();
            manager.ensure(&tree(), "tests", &[])
        })
    };
    while room.pauses() == 0 {
        thread::yield_now();
    }
    check.request_stop().unwrap();
    let result = waiter.join().unwrap();
    let BuildResult::Failed { output, .. } = result else {
        panic!("{result:?}");
    };
    let text = fs::read_to_string(output).unwrap();
    assert!(text.contains("stopped waiting for disk space"), "{text}");
    assert!(f.launcher.builds().is_empty());
    // Nothing was remembered as failing, so the next request with room builds.
    room.set(&[Some(START_GB)]);
    let next = f.manager.ensure(&tree(), "tests", &[]);
    assert!(matches!(next, BuildResult::Built { .. }), "{next:?}");
}

/// The disk loop pauses a leased tree's builds while it deletes build files there. A request
/// made meanwhile waits and builds once the tree is resumed, and a tree with a build running
/// cannot be paused.
#[test]
fn a_paused_tree_starts_no_build_until_it_is_resumed() {
    let f = fixture("paused", true);
    assert!(f.manager.pause(&tree()));
    let request = {
        let manager = Arc::clone(&f.manager);
        thread::spawn(move || manager.ensure(&tree(), "tests", &[]))
    };
    until_waiting(&f.manager, (0, 1));
    assert!(f.launcher.builds().is_empty());
    f.manager.resume(&tree());
    f.launcher.wait_for(|s| s.active == 1);
    assert!(!f.manager.pause(&tree()));
    f.launcher.release_one();
    assert!(matches!(request.join().unwrap(), BuildResult::Built { .. }));
    assert!(f.manager.pause(&tree()));
}
