//! No heap allocation on the hot path (architecture §17.4, §18): a counting allocator around
//! random games, from the first `next` to the end, with the agents' choices, a determinization
//! and an invariant check at every decision; and around rollouts of the WU Control mirror
//! under the rollout policy, its questions answered inline.
//!
//! The global allocator counts every thread's allocations, so this binary runs without the
//! test harness and its threads.

// A global allocator is an `unsafe impl`; it only forwards to the system allocator.
#![allow(unsafe_code)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use beryl_engine::rng::SplitMix64;
use beryl_engine::{Observation, Outcome, Profile, Scratch, State};
use beryl_search::{RolloutPolicy, rollout};
use common::{agents, game, mirror, play};

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
    for seed in 0..4 {
        let (t, mut s) = game(seed);
        let mut x = Scratch::new();
        let mut agents = agents(seed);
        let mut rng = SplitMix64::new(seed);
        let mut decisions = 0u32;
        COUNTING.store(true, Ordering::Relaxed);
        let outcome = play(&t, &mut s, &mut x, &mut agents, |s, d| {
            decisions += 1;
            let det = Observation::new(s, d.player).determinize(&t, &mut rng);
            assert!(det.check_invariants(&t).is_ok() && s.check_invariants(&t).is_ok());
        });
        COUNTING.store(false, Ordering::Relaxed);
        let allocations = ALLOCATIONS.swap(0, Ordering::Relaxed);
        assert!(decisions > 40, "seed {seed}: a short game");
        assert_eq!(
            allocations, 0,
            "seed {seed}: {allocations} allocations in a game that ended {outcome:?}"
        );
    }
    println!("allocations: no heap allocation in 4 random games");

    let t = mirror();
    let policy = RolloutPolicy::new(&t);
    let mut x = Scratch::new();
    for seed in 0..8 {
        let mut s = State::new(&t, seed, Profile::SMART);
        let before = x.counts.decisions;
        COUNTING.store(true, Ordering::Relaxed);
        let outcome = rollout(&mut s, &t, &mut x, &policy);
        COUNTING.store(false, Ordering::Relaxed);
        let allocations = ALLOCATIONS.swap(0, Ordering::Relaxed);
        assert!(
            x.counts.decisions - before > 40,
            "seed {seed}: a short rollout"
        );
        assert!(
            matches!(outcome, Outcome::Win(_)),
            "seed {seed}: {outcome:?}"
        );
        assert_eq!(
            allocations, 0,
            "seed {seed}: {allocations} allocations in a rollout"
        );
    }
    println!("allocations: no heap allocation in 8 rollouts under the rollout policy");
}
