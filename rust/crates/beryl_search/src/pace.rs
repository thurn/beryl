//! How many threads a long search runs while the app is animating.
//!
//! In the desktop app, the stage dropped about one frame in ten while the opponent's long
//! search ran (2 to 3 seconds on every core) and about one in a hundred otherwise. The search
//! runs at the default quality of service, below the webview's, yet a thread at the
//! user-interactive class that wakes every frame woke more than 8 ms late about 4 to 9% of the
//! time while 18 search threads ran, and as rarely as with no search (under 1%) while 6 ran.
//! A separate process made no difference. Fewer threads at all times would slow every search
//! by half or more on a loaded Mac, so the app says when its stage animates
//! ([`set_animating`]), and only then do threads beyond the first third of the cores wait.
//!
//! A tree's growth depends on nothing but itself, so which thread grows it, and when, never
//! changes the move.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

/// Whether the app's stage is animating.
static ANIMATING: AtomicBool = AtomicBool::new(false);

/// How long a waiting thread sleeps before it looks again: a few frames.
const NAP: Duration = Duration::from_millis(5);

/// Tells long searches that the app's stage started or stopped animating.
pub fn set_animating(animating: bool) {
    ANIMATING.store(animating, Ordering::Relaxed);
}

/// The threads that may run while the stage animates: a third of the cores.
fn animating_threads() -> usize {
    thread::available_parallelism().map_or(1, |n| (n.get() / 3).max(1))
}

/// Whether worker `k` must wait.
fn held(k: usize) -> bool {
    k > 0 && ANIMATING.load(Ordering::Relaxed) && k >= animating_threads()
}

/// Runs `grow` over every item on `threads` threads, each taking the next item not yet
/// taken. `grow(item, i, held)` grows item `i` until it is done, and returns `true`, or
/// until `held()`, and returns `false`; the thread then puts the item back for a thread that
/// may run and waits until it may run again. Worker 0 is never held, so every item gets done.
pub(crate) fn share<T: Send>(
    items: &mut [T],
    threads: usize,
    grow: impl Fn(&mut T, usize, &dyn Fn() -> bool) -> bool + Sync,
) {
    let queue: Vec<Mutex<&mut T>> = items.iter_mut().map(Mutex::new).collect();
    let taken = AtomicUsize::new(0);
    let returned: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    let done = AtomicUsize::new(0);
    let (queue, taken, returned, done, grow) = (&queue, &taken, &returned, &done, &grow);
    thread::scope(|scope| {
        for k in 0..threads {
            scope.spawn(move || {
                while done.load(Ordering::Acquire) < queue.len() {
                    if held(k) {
                        thread::sleep(NAP);
                        continue;
                    }
                    let next = returned.lock().expect("returned items").pop().or_else(|| {
                        let i = taken.fetch_add(1, Ordering::Relaxed);
                        (i < queue.len()).then_some(i)
                    });
                    // Every item is taken: others grow the last ones, or hold one to return.
                    let Some(i) = next else {
                        thread::sleep(NAP);
                        continue;
                    };
                    let mut item = queue[i].lock().expect("an item");
                    if grow(&mut item, i, &|| held(k)) {
                        done.fetch_add(1, Ordering::Release);
                    } else {
                        returned.lock().expect("returned items").push(i);
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Holding threads while animating changes nothing an item gets: each is grown to its
    /// end exactly once, and by steps, however the threads were held between them.
    #[test]
    fn every_item_is_grown_to_its_end_while_threads_are_held() {
        let mut items = vec![0u32; 40];
        let flip = AtomicUsize::new(0);
        share(&mut items, 8, |item, _, held| {
            while *item < 500 {
                // Animates now and then, as the stage does.
                let n = flip.fetch_add(1, Ordering::Relaxed);
                set_animating(n % 300 < 150);
                if held() {
                    return false;
                }
                *item += 1;
            }
            true
        });
        set_animating(false);
        assert!(items.iter().all(|&n| n == 500));
    }
}
