//! The macOS half of `hostcpu`: the kernel's per-state CPU tick counters and its virtual memory
//! page counts, read through the Mach host interface.

use std::io;
use std::mem::MaybeUninit;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::thread;

use super::{HostCpu, NATIVE_WINDOW, no_reading};
use crate::Native;

/// How many times `Native::busy` doubles its window when no tick passed.
const WINDOW_DOUBLINGS: u32 = 3;

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

    fn physical_cores(&self) -> u32 {
        Native.physical_cores()
    }

    fn busy(&self) -> io::Result<f64> {
        let now = cpu_ticks()?;
        let mut previous = self
            .previous
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(before) = *previous else {
            *previous = Some(now);
            return Err(no_reading());
        };
        let busy = busy_fraction(before, now).ok_or_else(no_reading)?;
        // When no tick passed, the older reading stays the baseline, so the next call measures
        // over the longer interval instead of reporting zero.
        *previous = Some(now);
        Ok(busy)
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        Native.free_memory_mb()
    }

    fn active_memory_pressure(&self) -> io::Result<bool> {
        Native.active_memory_pressure()
    }

    fn swap_used_mb(&self) -> io::Result<u64> {
        Native.swap_used_mb()
    }

    fn fseventsd_rss_mib(&self) -> io::Result<u64> {
        Native.fseventsd_rss_mib()
    }

    fn tree_cpu_seconds(&self, group: u64) -> io::Result<f64> {
        Native.tree_cpu_seconds(group)
    }

    fn process_tree_cpu_seconds(&self, pid: u32) -> io::Result<f64> {
        Native.process_tree_cpu_seconds(pid)
    }

    fn process_trees_cpu_seconds(&self, roots: &[u32]) -> io::Result<Vec<(u32, f64)>> {
        Native.process_trees_cpu_seconds(roots)
    }
}

impl HostCpu for Native {
    fn fseventsd_rss_mib(&self) -> io::Result<u64> {
        // Task-info calls fail for this root-owned process from a login service. macOS
        // ps can report its RSS in KiB without elevated privileges.
        let output = Command::new("/bin/ps")
            .args(["-U", "root", "-o", "rss=,comm="])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "/bin/ps exited with {}",
                output.status
            )));
        }
        fseventsd_rss_mib(&String::from_utf8_lossy(&output.stdout))
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn cores(&self) -> u32 {
        thread::available_parallelism().map_or(1, |cores| cores.get() as u32)
    }

    fn physical_cores(&self) -> u32 {
        let mut cores = 0u32;
        let mut size = size_of::<u32>();
        // SAFETY: the name is NUL-terminated and `cores` holds the supplied number of bytes.
        let result = unsafe {
            libc::sysctlbyname(
                c"hw.physicalcpu".as_ptr(),
                (&raw mut cores).cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if result == 0 && size == size_of::<u32>() && cores > 0 {
            cores
        } else {
            self.cores()
        }
    }

    /// Reads the counters twice, [`NATIVE_WINDOW`] apart, because `Native` keeps no state.
    /// The kernel refreshes its counters on its own schedule, so the two readings may see no
    /// tick; the window is then doubled and tried again, up to [`WINDOW_DOUBLINGS`] times,
    /// measuring from the first reading. A caller that reads repeatedly uses a [`HostMeter`].
    fn busy(&self) -> io::Result<f64> {
        let before = cpu_ticks()?;
        let mut window = NATIVE_WINDOW;
        for _ in 0..=WINDOW_DOUBLINGS {
            thread::sleep(window);
            if let Some(busy) = busy_fraction(before, cpu_ticks()?) {
                return Ok(busy);
            }
            window *= 2;
        }
        Err(no_reading())
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
        // Free, inactive and purgeable pages: what the kernel can hand out without taking
        // memory from a running process. `free_count` includes the speculative pages. macOS
        // keeps free pages to a few hundred megabytes and leaves the rest of reclaimable
        // memory inactive, so leaving inactive pages out reported about 1 GB free on a 64 GB
        // machine with 76% free (`memory_pressure`), and the scheduler stopped granting any
        // command that asked for memory.
        let pages = u64::from(info.free_count)
            + u64::from(info.inactive_count)
            + u64::from(info.purgeable_count);
        Ok(pages * page.max(0) as u64 / (1024 * 1024))
    }

    fn active_memory_pressure(&self) -> io::Result<bool> {
        let mut level = 0u32;
        let mut size = size_of::<u32>();
        // SAFETY: the name is NUL-terminated and level holds the supplied number of bytes.
        let result = unsafe {
            libc::sysctlbyname(
                c"kern.memorystatus_vm_pressure_level".as_ptr(),
                (&raw mut level).cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        if size != size_of::<u32>() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid memory pressure size",
            ));
        }
        // The sysctl returns XNU's NOTE_MEMORYSTATUS_PRESSURE_{NORMAL,WARN,CRITICAL} flags.
        match level {
            1 => Ok(false),
            2 | 4 => Ok(true),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown memory pressure level",
            )),
        }
    }

    /// The `vm.swapusage` sysctl, which `sysctl vm.swapusage` prints.
    fn swap_used_mb(&self) -> io::Result<u64> {
        let mut usage = MaybeUninit::<libc::xsw_usage>::zeroed();
        let mut size = size_of::<libc::xsw_usage>();
        // SAFETY: the name is a NUL-terminated string, and `usage` holds `size` bytes, which is
        // what the call may write.
        let result = unsafe {
            libc::sysctlbyname(
                c"vm.swapusage".as_ptr(),
                usage.as_mut_ptr().cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call succeeded and the record was zeroed before.
        let usage = unsafe { usage.assume_init() };
        Ok(usage.xsu_used / (1024 * 1024))
    }

    fn tree_cpu_seconds(&self, group: u64) -> io::Result<f64> {
        self.tree_cpu_seconds_of(group)
    }

    fn process_tree_cpu_seconds(&self, pid: u32) -> io::Result<f64> {
        Ok(Self::process_tree_cpu_seconds_of(pid))
    }

    fn process_trees_cpu_seconds(&self, roots: &[u32]) -> io::Result<Vec<(u32, f64)>> {
        Ok(Self::process_trees_cpu_seconds_of(roots))
    }
}

fn fseventsd_rss_mib(processes: &str) -> Option<u64> {
    processes
        .lines()
        .filter_map(|line| {
            let (rss, executable) = line.trim().split_once(char::is_whitespace)?;
            if Path::new(executable.trim_start()).file_name()? == "fseventsd" {
                let kib = rss.parse::<u64>().ok()?;
                Some(kib.div_ceil(1024))
            } else {
                None
            }
        })
        .max()
}

#[cfg(test)]
#[test]
fn reads_fseventsd_rss_in_mib_from_the_macos_process_list() {
    let processes = "  99999999 /usr/local/bin/fseventsd-helper\n\
                       7340033 /System/Library/Frameworks/FSEvents.framework/Support/fseventsd\n\
                       invalid /System/Library/Frameworks/FSEvents.framework/Support/fseventsd\n";
    assert_eq!(fseventsd_rss_mib(processes), Some(7169));
    assert_eq!(
        fseventsd_rss_mib("123 /usr/local/bin/fseventsd-helper"),
        None
    );
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
        assert!((1..=Native.cores()).contains(&Native.physical_cores()));
        assert!(Native.free_memory_mb().unwrap() > 0);
        // Swap may be zero, but it can be read.
        Native.swap_used_mb().unwrap();
        Native.active_memory_pressure().unwrap();
    }

    /// Free memory counts the inactive pages that `vm_stat` reports. Leaving them out once made
    /// a machine with most of its memory reclaimable look full, and the scheduler stopped.
    #[test]
    fn free_memory_counts_inactive_pages() {
        let out = std::process::Command::new("vm_stat").output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let page: u64 = text
            .split("page size of ")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap();
        let pages = |label: &str| -> u64 {
            text.lines()
                .find(|line| line.starts_with(label))
                .and_then(|line| line.rsplit(':').next())
                .and_then(|n| n.trim().trim_end_matches('.').parse().ok())
                .unwrap()
        };
        let reclaimable_mb = (pages("Pages free") + pages("Pages inactive")) * page / (1024 * 1024);
        // The two readings are moments apart, so allow for pages moving between lists.
        assert!(
            Native.free_memory_mb().unwrap() * 2 >= reclaimable_mb,
            "{} MB reported, {reclaimable_mb} MB free or inactive",
            Native.free_memory_mb().unwrap()
        );
    }
}
