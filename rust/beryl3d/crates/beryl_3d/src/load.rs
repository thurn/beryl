//! `--load <threads>[:<class>]` (docs/beryl-3d.md, "Driving the running app"): busy threads in
//! a scheduling class of their own, so that a run can reproduce a loaded machine on purpose
//! instead of waiting for one. On this machine the usual load of 20 to 50 comes from other
//! sessions' builds and tests, in the default class.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::thread::JoinHandle;

use beryl_host::Class;

/// The busy threads, which stop when this is dropped.
pub struct Load {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    /// The class each thread found itself in once it had set its own, or `None` where the
    /// system reports none.
    pub classes: Vec<Option<Class>>,
}

/// Reads `<threads>[:<class>]`: a number of threads and `default`, `utility` or
/// `interactive`, `default` when no class is given.
///
/// # Errors
///
/// When the number or the class is not one.
pub fn parse(text: &str) -> Result<(usize, Class), String> {
    let (count, class) = text.split_once(':').unwrap_or((text, "default"));
    let count = count
        .parse()
        .map_err(|_| format!("--load {text}: '{count}' is not a number of threads"))?;
    let class = Class::from_word(class).ok_or_else(|| {
        format!("--load {text}: a class is default, utility or interactive, not '{class}'")
    })?;
    Ok((count, class))
}

/// Starts `count` threads in `class`, each spinning until the load is dropped, and waits until
/// each has set its class.
///
/// # Panics
///
/// When the system cannot start a thread.
#[must_use]
pub fn start(count: usize, class: Class) -> Load {
    let stop = Arc::new(AtomicBool::new(false));
    let (sender, classes) = channel();
    let threads = (0..count)
        .map(|number| {
            let stop = Arc::clone(&stop);
            let sender = sender.clone();
            std::thread::Builder::new()
                .name(format!("load {number}"))
                .spawn(move || {
                    beryl_host::set_class(class);
                    // The receiver waits for every thread's report before `start` returns.
                    let _ = sender.send(beryl_host::current_class());
                    while !stop.load(Ordering::Relaxed) {
                        let mut sum = 0u64;
                        for step in 0..100_000u64 {
                            sum = std::hint::black_box(sum.wrapping_mul(31).wrapping_add(step));
                        }
                        std::hint::black_box(sum);
                    }
                })
                .expect("a load thread starts")
        })
        .collect();
    let classes = classes.iter().take(count).collect();
    Load {
        stop,
        threads,
        classes,
    }
}

impl Drop for Load {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
