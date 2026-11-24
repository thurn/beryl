//! A session's panics, told to whoever waits on its views. A session plays on a thread of its
//! own and emits its views from there. When that thread panics (a debug build's script checks
//! are assertions), its views just stop: [`Sessions`](beryl_session::Sessions) still
//! holds the callback, so a caller waiting on the next view waits for its timeout, or forever,
//! and learns nothing of what broke. Two ways tell it the panic's message and place instead:
//!
//! - a [`Guard`] the session's thread owns (one of its agents) tells of the panic it unwinds
//!   through, however early;
//! - an emit callback that [`watch`]es its thread tells of a panic after the session's first
//!   view, for a session that owns nothing of the caller's (a seeded game's agents are named).

use std::cell::RefCell;
use std::panic::PanicHookInfo;
use std::sync::{Mutex, MutexGuard, Once, PoisonError};
use std::thread::ThreadId;

type Report = Box<dyn Fn(String)>;

thread_local! {
    /// Who is told of a panic on this thread.
    static WATCHER: RefCell<Option<Report>> = const { RefCell::new(None) };
    /// The latest panic on this thread, for a [`Guard`] dropped as it unwinds.
    static LAST: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// A panic nothing was told of, numbered in the order the hook saw them.
struct Untold {
    number: u64,
    thread: ThreadId,
    panic: String,
}

/// The panics nothing watched, once the hook is installed, and the next one's number. A
/// [`Guard`] takes its thread's out as it tells of it.
static UNWATCHED: Mutex<(u64, Vec<Untold>)> = Mutex::new((0, Vec::new()));

fn untold() -> MutexGuard<'static, (u64, Vec<Untold>)> {
    UNWATCHED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Tells `report()` of a panic on the calling thread, unless something already watches it: call
/// it from a session's emit callback, which runs on the session's own thread. A watched panic is
/// not printed: the watcher says what it means.
pub fn watch<F: Fn(String) + 'static>(report: impl FnOnce() -> F) {
    install();
    WATCHER.with(|w| {
        let mut w = w.borrow_mut();
        if w.is_none() {
            *w = Some(Box::new(report()));
        }
    });
}

/// Installs, once, the hook that tells a thread's watcher of its panic, and leaves a thread no
/// one watches to the hook that was there.
fn install() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = LAST.try_with(|l| l.try_borrow_mut().map(|mut l| *l = Some(describe(info))));
            let told = WATCHER
                .try_with(|w| {
                    let w = w.try_borrow().ok()?;
                    let report = w.as_ref()?;
                    report(describe(info));
                    Some(())
                })
                .ok()
                .flatten();
            if told.is_none() {
                let mut untold = untold();
                let number = untold.0;
                untold.0 += 1;
                untold.1.push(Untold {
                    number,
                    thread: std::thread::current().id(),
                    panic: describe(info),
                });
                drop(untold);
                previous(info);
            }
        }));
    });
}

/// Tells `report` of the panic its thread is unwinding through when it is dropped, if one is:
/// moved into something a session's thread owns, such as its agents, it is dropped with them as
/// that thread unwinds.
pub struct Guard {
    report: Box<dyn Fn(String) + Send>,
}

impl Guard {
    pub fn new(report: impl Fn(String) + Send + 'static) -> Guard {
        install();
        Guard {
            report: Box::new(report),
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        let Some(panic) = LAST.try_with(|l| l.borrow().clone()).ok().flatten() else {
            return;
        };
        // Told, it is no longer a panic nothing watched.
        let thread = std::thread::current().id();
        untold().1.retain(|u| u.thread != thread);
        (self.report)(panic);
    }
}

/// The panics, since the first [`watch`] or [`Guard`], that nothing was told of: a session's that
/// panicked before its first view and owned no guard, whose waiter only times out.
pub fn unwatched() -> Vec<String> {
    untold().1.iter().map(|u| u.panic.clone()).collect()
}

/// The number the next panic nothing watched will have, for [`unwatched_since`].
pub fn mark() -> u64 {
    untold().0
}

/// The first panic nothing was told of from `mark` on, if one is, with its number: a guard
/// takes its own out as its thread unwinds, so one still here a while later is a thread's that
/// owned none.
pub fn unwatched_since(mark: u64) -> Option<(u64, String)> {
    let untold = untold();
    let first = untold.1.iter().find(|u| u.number >= mark)?;
    Some((first.number, first.panic.clone()))
}

/// Takes the panic numbered `number` out of those nothing was told of: its waiter has taken it
/// for its own.
pub fn claim(number: u64) {
    untold().1.retain(|u| u.number != number);
}

/// A panic's message and where it was raised.
fn describe(info: &PanicHookInfo) -> String {
    let message = info.payload_as_str().unwrap_or("a panic with no message");
    match info.location() {
        Some(at) => format!("panicked at {}:{}: {message}", at.file(), at.line()),
        None => format!("panicked: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A thread named as a session's, which other panic hooks in the process leave alone.
    fn session_thread(f: impl FnOnce() + Send + 'static) -> std::thread::JoinHandle<()> {
        std::thread::Builder::new()
            .name(beryl_session::SESSION_THREAD.to_owned())
            .spawn(f)
            .unwrap()
    }

    /// A watched thread's panic reaches its watcher, and another thread's does not.
    #[test]
    fn a_watched_thread_tells_its_panic() {
        let (told, heard) = std::sync::mpsc::channel();
        let watched = session_thread(move || {
            watch(move || move |m| told.send(m).unwrap());
            panic!("the script broke");
        });
        assert!(watched.join().is_err());
        let message = heard.recv().unwrap();
        assert!(message.contains("the script broke"), "{message}");
        assert!(message.contains("panics.rs"), "{message}");
        assert!(heard.try_recv().is_err());
    }

    /// A guard dropped as its thread unwinds tells of the panic, though nothing watched the
    /// thread; dropped otherwise, it tells nothing.
    #[test]
    fn a_guard_tells_the_panic_it_unwinds_through() {
        let (told, heard) = std::sync::mpsc::channel();
        let quiet = told.clone();
        drop(Guard::new(move |m| quiet.send(m).unwrap()));
        let guard = Guard::new(move |m| told.send(m).unwrap());
        let guarded = session_thread(move || {
            let _guard = guard;
            panic!("before the first view");
        });
        assert!(guarded.join().is_err());
        let message = heard.recv().unwrap();
        assert!(message.contains("before the first view"), "{message}");
        assert!(heard.try_recv().is_err());
        assert!(
            !unwatched()
                .iter()
                .any(|p| p.contains("before the first view"))
        );
    }
}
