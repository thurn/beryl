//! The macOS half of `hostcpu`: the kernel's per-state CPU tick counters and its virtual memory
//! page counts, read through the Mach host interface.

use std::io;
use std::mem::MaybeUninit;
use std::sync::Mutex;
use std::thread;

use super::{HostCpu, NATIVE_WINDOW, no_reading};
use crate::Native;

/// A CPU reader that remembers its own previous reading: the tick counters at the previous
/// call to `busy` (user, system, idle and nice). Each meter has its own, so two meters never
/// take each other's baseline.
#[derive(Debug, Default)]
pub struct HostMeter {
    previous: Mutex<Option<[u64; 4]>>,
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

    fn busy(&self) -> io::Result<f64> {
        let now = cpu_ticks()?;
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

impl HostCpu for Native {
    fn cores(&self) -> u32 {
        thread::available_parallelism().map_or(1, |cores| cores.get() as u32)
    }

    /// Reads the counters twice, [`NATIVE_WINDOW`] apart, because `Native` keeps no state.
    /// A caller that reads repeatedly uses a [`HostMeter`].
    fn busy(&self) -> io::Result<f64> {
        let before = cpu_ticks()?;
        thread::sleep(NATIVE_WINDOW);
        busy_fraction(before, cpu_ticks()?).ok_or_else(no_reading)
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        let mut info = MaybeUninit::<libc::vm_statistics64>::zeroed();
        let mut count = libc::HOST_VM_INFO64_COUNT;
        // SAFETY: `info` holds `count` integers, which is what the call may write.
        #[allow(deprecated)]
        let result = unsafe {
            libc::host_statistics64(
                libc::mach_host_self(),
                libc::HOST_VM_INFO64,
                info.as_mut_ptr().cast(),
                &raw mut count,
            )
        };
        if result != libc::KERN_SUCCESS {
            return Err(io::Error::other(format!(
                "host_statistics64 failed: {result}"
            )));
        }
        // SAFETY: the call succeeded and the record was zeroed before.
        let info = unsafe { info.assume_init() };
        // SAFETY: `sysconf` only reads its argument.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        // Free pages and inactive pages, which the kernel takes back without paging out
        // anything in use. `free_count` includes the speculative pages.
        let pages = u64::from(info.free_count) + u64::from(info.inactive_count);
        Ok(pages * page.max(0) as u64 / (1024 * 1024))
    }
}

fn cpu_ticks() -> io::Result<[u64; 4]> {
    let mut info = MaybeUninit::<libc::host_cpu_load_info>::zeroed();
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    // SAFETY: `info` holds `count` integers, which is what the call may write.
    #[allow(deprecated)]
    let result = unsafe {
        libc::host_statistics(
            libc::mach_host_self(),
            libc::HOST_CPU_LOAD_INFO,
            info.as_mut_ptr().cast(),
            &raw mut count,
        )
    };
    if result != libc::KERN_SUCCESS {
        return Err(io::Error::other(format!(
            "host_statistics failed: {result}"
        )));
    }
    // SAFETY: the call succeeded and the record was zeroed before.
    let ticks = unsafe { info.assume_init() }.cpu_ticks;
    Ok(ticks.map(u64::from))
}

/// The fraction of ticks between two readings that were not idle, or `None` if no tick passed.
/// The kernel's counters are 32 bits wide and wrap, so each difference is taken modulo 2^32.
fn busy_fraction(before: [u64; 4], now: [u64; 4]) -> Option<f64> {
    let delta = |state: libc::c_int| {
        let state = state as usize;
        (now[state].wrapping_sub(before[state])) & u64::from(u32::MAX)
    };
    let idle = delta(libc::CPU_STATE_IDLE);
    let busy =
        delta(libc::CPU_STATE_USER) + delta(libc::CPU_STATE_SYSTEM) + delta(libc::CPU_STATE_NICE);
    let total = idle + busy;
    (total != 0).then(|| busy as f64 / total as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reading is a fraction, or none: the kernel refreshes its counters on its own schedule,
    /// so two readings a moment apart may see no tick, which is the case `busy` reports as an
    /// error instead of 0.
    fn assert_fraction_or_none(reading: io::Result<f64>) {
        match reading {
            Ok(busy) => assert!((0.0..=1.0).contains(&busy), "{busy}"),
            Err(error) => assert_eq!(error.kind(), io::ErrorKind::WouldBlock),
        }
    }

    #[test]
    fn busy_is_a_fraction() {
        assert_fraction_or_none(Native.busy());
    }

    #[test]
    fn a_meter_has_no_reading_on_its_first_call_and_each_meter_has_its_own_baseline() {
        let first = HostMeter::new();
        assert_eq!(first.busy().unwrap_err().kind(), io::ErrorKind::WouldBlock);
        // Another meter's first call does not take the first one's baseline.
        let second = HostMeter::new();
        assert_eq!(second.busy().unwrap_err().kind(), io::ErrorKind::WouldBlock);
        for meter in [&first, &second] {
            assert_fraction_or_none(meter.busy());
        }
    }

    #[test]
    fn busy_fraction_counts_every_state_but_idle_and_survives_wrapping() {
        assert!((busy_fraction([0; 4], [30, 10, 50, 10]).unwrap() - 0.5).abs() < 1e-9);
        let near_wrap = u64::from(u32::MAX) - 4;
        assert!((busy_fraction([near_wrap, 0, 0, 0], [5, 0, 10, 0]).unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn no_elapsed_ticks_is_no_reading_rather_than_idle() {
        assert_eq!(busy_fraction([1; 4], [1; 4]), None);
    }

    #[test]
    fn the_machine_has_cores_and_free_memory() {
        assert!(Native.cores() >= 1);
        assert!(Native.free_memory_mb().unwrap() > 0);
    }
}
