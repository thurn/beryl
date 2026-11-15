//! `Client::frame` allocates nothing while a recorded game plays (docs/beryl-3d.md, "The
//! frame": no allocation), with every script the session timed. This binary counts the heap
//! allocations made inside `frame`, so it holds one test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use beryl_inspect::pages::recording;
use beryl_scene::driver::{REPLY_DELAY, TICK, plays_on};
use beryl_scene::frame::Wake;
use beryl_scene::input::{Input, Insets, Size};
use beryl_scene::{Client, Config};
use beryl_session::protocol::Side;

/// The system allocator, counting the allocations this thread makes while `COUNTING` is set.
struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    if COUNTING.with(Cell::get) {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
    }
}

#[expect(unsafe_code, reason = "a global allocator is an unsafe trait")]
// SAFETY: every call is passed straight to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `alloc`'s contract, which is the system allocator's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from `alloc` above, so from the system allocator.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        // SAFETY: as for `dealloc`, and the caller upholds `realloc`'s contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Plays `verbs-zones` as the test driver does, giving each view 400 milliseconds after the
/// table rests in the one before, and counts what every frame allocates.
#[test]
fn frames_allocate_nothing_across_verbs_zones() {
    let views = recording("verbs-zones", Side::You)
        .and_then(|recording| recording.views())
        .unwrap_or_else(|why| panic!("{why}"));
    let mut client = Client::new(Config::LARGEST);
    client.handle(Input::Surface {
        size: Size {
            width: 1600.0,
            height: 900.0,
        },
        scale: 1.0,
        safe_area: Insets::default(),
    });
    let mut next = 0;
    let mut now = 0;
    let mut frames = 0;
    let mut allocated = Vec::new();
    loop {
        // Gives the next view, and every view after it that follows unasked.
        while let Some(update) = views.get(next) {
            next += 1;
            client.handle(Input::Update(Box::new(update.clone())));
            if !plays_on(update) {
                break;
            }
        }
        loop {
            COUNTING.with(|counting| counting.set(true));
            ALLOCATIONS.with(|n| n.set(0));
            client.frame(now);
            COUNTING.with(|counting| counting.set(false));
            let made = ALLOCATIONS.with(Cell::get);
            if made > 0 {
                allocated.push((now / 1_000_000, made));
            }
            frames += 1;
            now = match client.wake() {
                Wake::Never => break,
                Wake::Now => now + TICK.0,
                Wake::At(at) => at.max(now + 1),
            };
        }
        if next >= views.len() {
            break;
        }
        now += REPLY_DELAY.0;
    }
    assert!(frames > 100, "the game played: {frames} frames");
    assert_eq!(
        allocated,
        [],
        "frames that allocated, as (milliseconds, allocations)"
    );
}
