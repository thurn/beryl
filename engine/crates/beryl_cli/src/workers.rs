//! Threads that search. The search recurses deeply on some positions, and a thread's default
//! stack (2 MiB for a spawned thread, 1 MiB for the main thread on Windows) overflowed on
//! `just gate` at two ladder rungs. Every thread that searches is started here, with a
//! stack of [`STACK`] bytes. The reservation is virtual address space, so it costs no memory
//! until the search uses it.

use std::thread::{self, Scope, ScopedJoinHandle};

/// The stack of a searching thread: 256 MiB.
pub const STACK: usize = 256 << 20;

/// Starts `f` in `scope` on a thread with a [`STACK`]-byte stack.
pub fn spawn<'scope, 'env, T: Send + 'scope>(
    scope: &'scope Scope<'scope, 'env>,
    f: impl FnOnce() -> T + Send + 'scope,
) -> ScopedJoinHandle<'scope, T> {
    thread::Builder::new()
        .stack_size(STACK)
        .spawn_scoped(scope, f)
        .expect("a thread with a large stack starts")
}

/// Runs `f` on a thread with a [`STACK`]-byte stack and returns its result, for a command
/// whose search runs on the calling thread. A panic in `f` panics here.
pub fn run<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    thread::scope(|scope| match spawn(scope, f).join() {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recurses `depth` frames, each holding a kilobyte the optimizer cannot remove.
    #[inline(never)]
    fn recurse(depth: u32) -> u32 {
        let pad = [depth as u8; 1024];
        let below = if depth == 0 { 0 } else { recurse(depth - 1) };
        below + u32::from(std::hint::black_box(pad)[0] & 1)
    }

    #[test]
    fn a_worker_survives_a_recursion_that_overflows_two_mebibytes() {
        // 8192 frames of more than 1 KiB each need over 8 MiB, far past a 2 MiB stack.
        assert_eq!(run(|| recurse(8192)), 4096);
    }

    #[test]
    fn a_scoped_worker_returns_its_value() {
        let total = thread::scope(|scope| spawn(scope, || recurse(8192)).join().unwrap());
        assert_eq!(total, 4096);
    }
}
