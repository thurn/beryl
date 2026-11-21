//! The Windows half of `hostcpu`: the system's idle, kernel and user times from
//! `GetSystemTimes`, and its available physical memory from `GlobalMemoryStatusEx`.

use std::io;
use std::sync::Mutex;
use std::thread;

use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, FILETIME};
use windows_sys::Win32::System::SystemInformation::{
    GetLogicalProcessorInformation, GlobalMemoryStatusEx, MEMORYSTATUSEX, RelationProcessorCore,
    SYSTEM_LOGICAL_PROCESSOR_INFORMATION,
};
use windows_sys::Win32::System::Threading::GetSystemTimes;

use super::{HostCpu, NATIVE_WINDOW, no_reading};
use crate::Native;

/// A CPU reader that remembers its own previous reading: the times at the previous call to
/// `busy`. Each meter has its own, so two meters never take each other's baseline.
#[derive(Debug, Default)]
pub struct HostMeter {
    previous: Mutex<Option<Times>>,
}

impl HostMeter {
    pub fn new() -> Self {
        Self::default()
    }
}

impl HostCpu for HostMeter {
    fn cores(&self) -> u32 {
        Native.cores()
    }

    fn physical_cores(&self) -> u32 {
        Native.physical_cores()
    }

    fn busy(&self) -> io::Result<f64> {
        let now = system_times()?;
        let mut previous = self
            .previous
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = previous.replace(now).ok_or_else(no_reading)?;
        busy_fraction(before, now).ok_or_else(no_reading)
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        Native.free_memory_mb()
    }
}

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

    fn physical_cores(&self) -> u32 {
        physical_cores().unwrap_or_else(|| self.cores())
    }

    /// Reads the times twice, [`NATIVE_WINDOW`] apart, because `Native` keeps no state.
    /// A caller that reads repeatedly uses a [`HostMeter`].
    fn busy(&self) -> io::Result<f64> {
        let before = system_times()?;
        thread::sleep(NATIVE_WINDOW);
        busy_fraction(before, system_times()?).ok_or_else(no_reading)
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

fn physical_cores() -> Option<u32> {
    let mut records = Vec::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>::new();
    let record_size = size_of::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION>();
    let mut bytes = 0;
    loop {
        let buffer = if records.is_empty() {
            std::ptr::null_mut()
        } else {
            records.as_mut_ptr()
        };
        // SAFETY: `bytes` is writable. The buffer is null for the size query, then holds at
        // least the supplied number of bytes in aligned records.
        if unsafe { GetLogicalProcessorInformation(buffer, &raw mut bytes) } != 0 {
            break;
        }
        if io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
            return None;
        }
        records.resize_with(
            (bytes as usize).div_ceil(record_size),
            SYSTEM_LOGICAL_PROCESSOR_INFORMATION::default,
        );
    }
    let cores = records[..bytes as usize / record_size]
        .iter()
        .filter(|record| record.Relationship == RelationProcessorCore)
        .count() as u32;
    (cores != 0).then_some(cores)
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

/// The fraction of time between two readings that was not idle, or `None` if no time passed.
/// Kernel time includes idle time, so the total is kernel plus user.
fn busy_fraction(before: Times, now: Times) -> Option<f64> {
    let idle = now.idle.saturating_sub(before.idle);
    let total = now.kernel.saturating_sub(before.kernel) + now.user.saturating_sub(before.user);
    (total != 0).then(|| (total.saturating_sub(idle)) as f64 / total as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_is_a_fraction() {
        // No reading is possible when the counters did not move between the two readings.
        if let Ok(busy) = Native.busy() {
            assert!((0.0..=1.0).contains(&busy), "{busy}");
        }
    }

    #[test]
    fn a_meter_has_no_reading_on_its_first_call() {
        let meter = HostMeter::new();
        assert_eq!(meter.busy().unwrap_err().kind(), io::ErrorKind::WouldBlock);
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
        assert!((busy_fraction(before, now).unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(busy_fraction(now, now), None);
    }

    #[test]
    fn the_machine_has_cores_and_free_memory() {
        assert!(Native.cores() >= 1);
        assert!((1..=Native.cores()).contains(&physical_cores().unwrap()));
        assert!(Native.free_memory_mb().unwrap() > 0);
    }
}
