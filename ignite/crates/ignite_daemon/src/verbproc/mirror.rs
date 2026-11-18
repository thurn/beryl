//! Keeping the daemon's view of the verb up to date: its step and waits (`host-step`), whether a
//! stop was asked (`host-poll`), and the refusal to stop (`host-refuse-stop`,
//! `host-allow-stop`).
//!
//! The verb records its steps on its own activity in this process, as any verb does (see
//! [`crate::steps`]). A thread of the verb process sends each change of step or waits to the
//! daemon as it happens, and otherwise polls the daemon about once a [`Timing::poll`], so a stop
//! asked with `ignite stop` reaches the verb within about a second. Refusing to stop goes to the
//! daemon at once, before the verb goes on: only the daemon knows whether a stop was asked.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use ignite_core::protocol::host::StepUpdate;

use super::Timing;
use super::link::Link;
use crate::steps::{Activity, StopKeeper};

/// The daemon as the keeper of the verb's stop state.
pub struct HostStop(pub Arc<Link>);

impl StopKeeper for HostStop {
    fn refuse_stop(&self, reason: &str) -> bool {
        match self.0.refuse_stop(reason) {
            Ok(reply) => reply.refused,
            // Nobody can ask for a stop while no daemon answers; the verb goes on.
            Err(error) => {
                eprintln!("ignite: cannot tell the daemon that stopping is refused: {error}");
                true
            }
        }
    }

    fn allow_stop(&self) {
        if let Err(error) = self.0.allow_stop() {
            eprintln!("ignite: cannot tell the daemon that stopping is allowed: {error}");
        }
    }
}

/// The step update that describes `activity` now.
fn update_of(link: &Link, activity: &Activity) -> StepUpdate {
    let view = activity.view();
    let work = activity.work();
    StepUpdate {
        handle: link.handle().clone(),
        step: view.step,
        waiting: activity.waits(),
        class: work.class,
        ticket: work.ticket,
    }
}

/// The thread that mirrors the verb's activity to the daemon, until it is stopped.
pub struct Mirror {
    done: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Mirror {
    pub fn start(link: Arc<Link>, activity: Arc<Activity>, timing: Timing) -> Mirror {
        let done = Arc::new(AtomicBool::new(false));
        let heard = activity.subscribe();
        let thread = {
            let done = done.clone();
            std::thread::Builder::new()
                .name("ignite-verb-mirror".to_owned())
                .spawn(move || mirror(&link, &activity, &heard, &done, timing))
                .map_err(|error| eprintln!("ignite: cannot mirror the verb's steps: {error}"))
                .ok()
        };
        Mirror { done, thread }
    }

    /// Stops the thread and waits for it.
    pub fn stop(mut self) {
        self.done.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn mirror(
    link: &Link,
    activity: &Activity,
    heard: &mpsc::Receiver<String>,
    done: &AtomicBool,
    timing: Timing,
) {
    let mut sent: Option<StepUpdate> = None;
    while !done.load(Ordering::SeqCst) {
        let update = update_of(link, activity);
        // A failure was already retried for as long as the link retries; the next round tries
        // again.
        if sent.as_ref() == Some(&update) {
            let _ = link.poll();
        } else if link.step(&update).is_ok() {
            sent = Some(update);
        }
        // Wakes at the next step or wait the verb records, or after a poll's pause. A wait
        // that ends is not heard, and is sent with the next round.
        match heard.recv_timeout(timing.poll) {
            Ok(_) => while heard.try_recv().is_ok() {},
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(timing.poll),
        }
    }
}
