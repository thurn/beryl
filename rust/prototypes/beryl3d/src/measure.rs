//! What the statistics need from the system: a global allocator that counts the allocations made
//! on one thread while counting is switched on, peak memory and the load average.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<u64> = const { Cell::new(0) };
}

pub struct CountingAllocator;

fn note() {
    let _ = COUNTING.try_with(|counting| {
        if counting.get() {
            let _ = COUNT.try_with(|count| count.set(count.get() + 1));
        }
    });
}

// SAFETY: every method forwards to the system allocator unchanged; counting only touches
// thread-local cells, which never allocate.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: forwarded as is.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: forwarded as is.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: forwarded as is.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded as is.
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// Runs `f` and returns its result with the number of heap allocations it made on this thread.
pub fn count_allocations<R>(f: impl FnOnce() -> R) -> (R, u64) {
    COUNT.with(|c| c.set(0));
    COUNTING.with(|c| c.set(true));
    let result = f();
    COUNTING.with(|c| c.set(false));
    (result, COUNT.with(Cell::get))
}

/// The process's peak resident memory in bytes.
#[allow(unsafe_code)]
pub fn peak_memory() -> u64 {
    // SAFETY: getrusage writes into the zeroed struct it is given.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: a valid pointer to a rusage struct.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &raw mut usage) };
    // On macOS ru_maxrss is in bytes.
    usage.ru_maxrss as u64
}

/// The one-minute load average.
#[allow(unsafe_code)]
pub fn load_average() -> f64 {
    let mut loads = [0.0f64; 3];
    // SAFETY: getloadavg writes at most the 3 values it is asked for.
    let n = unsafe { libc::getloadavg(loads.as_mut_ptr(), 3) };
    if n > 0 { loads[0] } else { f64::NAN }
}

/// Asks macOS to schedule this thread in the user-interactive class, as the doc's frame thread
/// does, unless `BERYL3D_NO_QOS` is set. Returns whether it was asked.
#[allow(unsafe_code)]
pub fn interactive() -> bool {
    if std::env::var_os("BERYL3D_NO_QOS").is_some() {
        return false;
    }
    // SAFETY: pthread_set_qos_class_self_np only changes the calling thread's scheduling class.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0) == 0
    }
}

/// Puts this thread in the utility scheduling class, below the frame thread. Threads it creates
/// inherit the class. `BERYL3D_NO_QOS` leaves it unchanged, for comparison.
#[allow(unsafe_code)]
pub fn utility() {
    if std::env::var_os("BERYL3D_NO_QOS").is_some() {
        return;
    }
    // SAFETY: pthread_set_qos_class_self_np only changes the calling thread's scheduling class.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}
