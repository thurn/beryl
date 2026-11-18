//! The Windows half of `hostcpu`: the system's idle, kernel and user times from
//! `GetSystemTimes`, and its available physical memory from `GlobalMemoryStatusEx`.

use std::io;
use std::sync::Mutex;
use std::thread;

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;

use super::HostCpu;
use crate::Native;

/// The times at the previous call to `busy`: idle, kernel and user, in 100-nanosecond units.
static PREVIOUS: Mutex<Option<Times>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Times {
    idle: u64,
    /// Kernel time, which on Windows includes the idle time.
    kernel: u64,
    user: u64,
}

impl HostCpu for Native {
    fn cores(&self) -> u32 {
        thread::available_parallelism().map_or(1, |cores| cores.get() as u32)
    }

    fn busy(&self) -> io::Result<f64> {
        let now = system_times()?;
        let mut previous = PREVIOUS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The first call compares against boot, when every time was zero.
        let before = previous.replace(now).unwrap_or_default();
        Ok(busy_fraction(before, now))
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        let mut status = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..MEMORYSTATUSEX::default()
        };
        // SAFETY: `status` is a valid record whose `dwLength` gives its size, as the call needs.
        if unsafe { GlobalMemoryStatusEx(&raw mut status) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // Available memory counts the standby list, which Windows takes back without paging
        // out anything in use, like the inactive pages counted on macOS.
        Ok(status.ullAvailPhys / (1024 * 1024))
    }
}

fn system_times() -> io::Result<Times> {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: the three pointers are to valid, writable records.
    if unsafe { GetSystemTimes(&raw mut idle, &raw mut kernel, &raw mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(Times {
        idle: ticks(idle),
        kernel: ticks(kernel),
        user: ticks(user),
    })
}

/// A `FILETIME`'s two halves as one count of 100-nanosecond units.
pub(crate) fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

/// The fraction of time between two readings that was not idle. Kernel time includes idle
/// time, so the total is kernel plus user.
fn busy_fraction(before: Times, now: Times) -> f64 {
    let idle = now.idle.saturating_sub(before.idle);
    let total = now.kernel.saturating_sub(before.kernel) + now.user.saturating_sub(before.user);
    if total == 0 {
        0.0
    } else {
        (total.saturating_sub(idle)) as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_is_a_fraction() {
        let busy = Native.busy().unwrap();
        assert!((0.0..=1.0).contains(&busy), "{busy}");
        let busy = Native.busy().unwrap();
        assert!((0.0..=1.0).contains(&busy), "{busy}");
    }

    #[test]
    fn busy_fraction_counts_kernel_and_user_time_but_not_idle_time() {
        let before = Times {
            idle: 100,
            kernel: 200,
            user: 300,
        };
        // 40 of the 60 new kernel units were idle; with 20 user units, 40 of 80 were busy.
        let now = Times {
            idle: 140,
            kernel: 260,
            user: 320,
        };
        assert!((busy_fraction(before, now) - 0.5).abs() < 1e-9);
        assert!(busy_fraction(now, now).abs() < 1e-9);
    }

    #[test]
    fn the_machine_has_cores_and_free_memory() {
        assert!(Native.cores() >= 1);
        assert!(Native.free_memory_mb().unwrap() > 0);
    }
}
