//! The macOS half of `hostcpu`: the kernel's per-state CPU tick counters and its virtual memory
//! page counts, read through the Mach host interface.

use std::io;
use std::mem::MaybeUninit;
use std::sync::Mutex;
use std::thread;

use super::HostCpu;
use crate::Native;

/// The tick counters at the previous call to `busy`: user, system, idle and nice.
static PREVIOUS: Mutex<Option<[u64; 4]>> = Mutex::new(None);

impl HostCpu for Native {
    fn cores(&self) -> u32 {
        thread::available_parallelism().map_or(1, |cores| cores.get() as u32)
    }

    fn busy(&self) -> io::Result<f64> {
        let now = cpu_ticks()?;
        let mut previous = PREVIOUS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The first call compares against boot, when every counter was zero.
        let before = previous.replace(now).unwrap_or_default();
        Ok(busy_fraction(before, now))
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

/// The fraction of ticks between two readings that were not idle. The kernel's counters are
/// 32 bits wide and wrap, so each difference is taken modulo 2^32.
fn busy_fraction(before: [u64; 4], now: [u64; 4]) -> f64 {
    let delta = |state: libc::c_int| {
        let state = state as usize;
        (now[state].wrapping_sub(before[state])) & u64::from(u32::MAX)
    };
    let idle = delta(libc::CPU_STATE_IDLE);
    let busy =
        delta(libc::CPU_STATE_USER) + delta(libc::CPU_STATE_SYSTEM) + delta(libc::CPU_STATE_NICE);
    let total = idle + busy;
    if total == 0 {
        0.0
    } else {
        busy as f64 / total as f64
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
    fn busy_fraction_counts_every_state_but_idle_and_survives_wrapping() {
        assert!((busy_fraction([0; 4], [30, 10, 50, 10]) - 0.5).abs() < 1e-9);
        let near_wrap = u64::from(u32::MAX) - 4;
        assert!((busy_fraction([near_wrap, 0, 0, 0], [5, 0, 10, 0]) - 0.5).abs() < 1e-9);
        assert!(busy_fraction([1; 4], [1; 4]).abs() < 1e-9);
    }

    #[test]
    fn the_machine_has_cores_and_free_memory() {
        assert!(Native.cores() >= 1);
        assert!(Native.free_memory_mb().unwrap() > 0);
    }
}
