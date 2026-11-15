//! A frame that ends one script and starts the next allocates nothing (docs/beryl-3d.md, "The
//! frame": no allocation): scripts are compiled when their updates arrive. This binary counts
//! every heap allocation, so it holds one test.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_scene::frame::FrameBuilder;
use common::{MS, before, empty_frame, show, three_beats};

/// The system allocator, counting the allocations it makes.
struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

#[expect(unsafe_code, reason = "a global allocator is an unsafe trait")]
// SAFETY: every call is passed straight to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller upholds `alloc`'s contract, which is the system allocator's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from `alloc` above, so from the system allocator.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: as for `dealloc`, and the caller upholds `realloc`'s contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn a_frame_that_starts_a_script_allocates_nothing() {
    let (start, cards) = before();
    let (after, timings) = three_beats(&start, cards);
    let mut show = show();
    let mut frame = empty_frame();
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(after), timings);
    // The frame at 0 shows the first view and starts the three-beat script; the frame at 500 ms
    // draws it under way, and the frame at 900 ms ends it.
    for now in [0, 500 * MS, 900 * MS] {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        let mut builder = FrameBuilder::new(&mut frame);
        show.frame(now, &mut builder);
        let records = builder.finish().draws.len();
        let after = ALLOCATIONS.load(Ordering::Relaxed);
        assert!(records > 1, "the frame draws the cards");
        assert_eq!(
            after - before,
            0,
            "allocations in the frame at {} ms",
            now / MS
        );
    }
}
