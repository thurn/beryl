//! `--load <threads>[:<class>]` (docs/beryl-3d.md, "Driving the running app"): busy threads in
//! a scheduling class of their own, so that a run can reproduce a loaded machine on purpose
//! instead of waiting for one. On this machine the usual load of 20 to 50 comes from other
//! sessions' builds and tests, in the default class. `averages` reads the load a run's frame
//! log records.

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

/// Reads `<threads>[:<class>]`: a number of threads and `utility`, `default`, `initiated` or
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
        format!(
            "--load {text}: a class is utility, default, initiated or interactive, not '{class}'"
        )
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

/// The machine's load averages, where the system reports them: `sysctl` on macOS, the proc
/// file on Linux. Read once, before the host starts.
#[cfg(not(windows))]
#[must_use]
pub fn averages() -> Option<[f64; 3]> {
    let text = if cfg!(target_os = "macos") {
        let output = std::process::Command::new("sysctl")
            .args(["-n", "vm.loadavg"])
            .output()
            .ok()?;
        String::from_utf8(output.stdout).ok()?
    } else {
        std::fs::read_to_string("/proc/loadavg").ok()?
    };
    let numbers: Vec<f64> = text
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .take(3)
        .collect();
    numbers.try_into().ok()
}

/// Samples the number of busy logical processors over 100 ms. Windows supplies no 1-, 5-
/// or 15-minute load averages, so it uses the same sample for all three frame-log values.
/// On machines with multiple processor groups, Windows reports the calling thread's group.
#[cfg(windows)]
#[must_use]
pub fn averages() -> Option<[f64; 3]> {
    let (idle_before, total_before) = windows::cpu_times()?;
    std::thread::sleep(std::time::Duration::from_millis(100));
    let (idle_after, total_after) = windows::cpu_times()?;
    let total = total_after.checked_sub(total_before)?;
    if total == 0 {
        return None;
    }
    let idle = idle_after.checked_sub(idle_before)?;
    let processors = windows::processor_count()?;
    let busy = 1.0 - idle as f64 / total as f64;
    Some([busy.clamp(0.0, 1.0) * f64::from(processors); 3])
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::GROUP_AFFINITY;
    use windows_sys::Win32::System::Threading::{
        GetActiveProcessorCount, GetCurrentThread, GetSystemTimes, GetThreadGroupAffinity,
    };

    #[expect(unsafe_code, reason = "samples cumulative Windows CPU times")]
    pub(super) fn cpu_times() -> Option<(u64, u64)> {
        let (mut idle, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: all three FILETIME output pointers are valid for the call.
        let ok = unsafe { GetSystemTimes(&raw mut idle, &raw mut kernel, &raw mut user) };
        let ticks =
            |time: FILETIME| u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime);
        // Kernel time already includes idle time.
        (ok != 0).then(|| (ticks(idle), ticks(kernel) + ticks(user)))
    }

    #[expect(
        unsafe_code,
        reason = "counts the active processors that GetSystemTimes samples"
    )]
    pub(super) fn processor_count() -> Option<u32> {
        let mut affinity = GROUP_AFFINITY::default();
        // SAFETY: the output pointer is valid, and the pseudo-handle names the calling thread.
        let ok = unsafe { GetThreadGroupAffinity(GetCurrentThread(), &raw mut affinity) };
        if ok == 0 {
            return None;
        }
        // SAFETY: Group came from Windows and GetActiveProcessorCount takes no pointers.
        let count = unsafe { GetActiveProcessorCount(affinity.Group) };
        (count != 0).then_some(count)
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn windows_load_reports_busy_logical_processors() {
        let [now, five, fifteen] = super::averages().expect("Windows CPU times");
        let processors = super::windows::processor_count().expect("processor count");
        assert!((0.0..=f64::from(processors)).contains(&now));
        assert_eq!(now.to_bits(), five.to_bits());
        assert_eq!(now.to_bits(), fifteen.to_bits());
    }
}
