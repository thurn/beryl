//! No heap allocation in the rollout loop (architecture §17.4, §18): a counting allocator
//! around the cube rollout benchmark's rollouts, every start played to the end under the
//! rollout policy, mid-game positions with their stacks, payments and resolutions.
//!
//! The global allocator counts every thread's allocations, so this binary runs without the
//! test harness and its threads.

// A global allocator is an `unsafe impl`; it only forwards to the system allocator.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use beryl_bench::rollout::{self, Workload};

struct Counting;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn main() {
    let w = Workload::new();
    let (mut x, mut s) = w.memory();
    COUNTING.store(true, Ordering::Relaxed);
    rollout::roll(&w.table, &w.starts, &w.policy, &mut s, &mut x);
    COUNTING.store(false, Ordering::Relaxed);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(
        allocations, 0,
        "{allocations} allocations in {} rollouts",
        w.summary.rollouts
    );
    assert!(w.summary.rollouts > 0 && w.summary.decisions > 1000);
    println!(
        "allocations: no heap allocation in {} rollouts of the cube rollout benchmark",
        w.summary.rollouts
    );
}
