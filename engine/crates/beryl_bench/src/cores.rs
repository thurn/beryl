//! Apple silicon mixes core types that run the same code at different speeds. `sysctl
//! hw.perflevel0` is the fastest kind (6 "Super" cores on an M5 Max; "Performance" on earlier
//! chips) and `hw.perflevel1` the slower kind ("Performance" on an M5 Max; "Efficiency"
//! before). A benchmark that moves between them measures the move.
//!
//! macOS has no way to pin a thread to a core. It places threads by quality-of-service
//! class, and keeps user-interactive threads on the fastest cores while one is free. So a
//! benchmark sets that class first and runs single-threaded (or with no more threads than
//! perflevel0 has cores). On a loaded machine other work can still take the fast cores; the
//! ABBA harness counts the rounds that hit as disturbed, and its interval widens with them.
//! Windows raises the benchmark thread's priority and selects the fastest CPU sets on
//! processors with more than one performance class.

/// Requests the fastest cores for the calling thread at user-interactive priority.
/// Returns whether the system applied the request.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub fn prefer_fastest_cores() -> bool {
    // SAFETY: changes only the calling thread's scheduling class; takes no pointers.
    let status = unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
    };
    status == 0
}

#[cfg(windows)]
pub use windows::prefer_fastest_cores;

#[cfg(not(any(target_os = "macos", windows)))]
pub fn prefer_fastest_cores() -> bool {
    false
}

#[cfg(windows)]
mod windows {
    use std::ptr::null_mut;

    use windows_sys::Win32::System::SystemInformation::{
        CpuSetInformation, GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION,
        SYSTEM_CPU_SET_INFORMATION_ALLOCATED,
        SYSTEM_CPU_SET_INFORMATION_ALLOCATED_TO_TARGET_PROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThread, SetThreadPriority, SetThreadSelectedCpuSets,
        THREAD_PRIORITY_HIGHEST,
    };

    /// Raises the calling thread's priority and, on a hybrid CPU, selects its fastest CPU sets.
    /// Returns whether the system applied the request.
    #[expect(
        unsafe_code,
        reason = "sets the calling thread's Windows scheduling policy"
    )]
    pub fn prefer_fastest_cores() -> bool {
        let Some(sets) = available_cpu_sets() else {
            return false;
        };
        let fastest = fastest_cpu_sets(&sets);
        // SAFETY: GetCurrentThread returns a pseudo-handle for this thread. The set IDs came
        // from Windows for this process, and the pointer is valid for the number of IDs given.
        unsafe {
            let thread = GetCurrentThread();
            SetThreadPriority(thread, THREAD_PRIORITY_HIGHEST) != 0
                && (fastest.is_empty()
                    || SetThreadSelectedCpuSets(thread, fastest.as_ptr(), fastest.len() as u32)
                        != 0)
        }
    }

    fn fastest_cpu_sets(sets: &[(u8, u32)]) -> Vec<u32> {
        let Some(&(class, _)) = sets.iter().max_by_key(|&&(class, _)| class) else {
            return Vec::new();
        };
        // On homogeneous CPUs the priority change is sufficient. Let Windows pick any core.
        if sets.iter().all(|&(rank, _)| rank == class) {
            return Vec::new();
        }
        sets.iter()
            .filter_map(|&(rank, id)| (rank == class).then_some(id))
            .collect()
    }

    #[expect(unsafe_code, reason = "reads variable-sized Win32 CPU set records")]
    fn available_cpu_sets() -> Option<Vec<(u8, u32)>> {
        type Record = SYSTEM_CPU_SET_INFORMATION;
        let mut length = 0;
        // SAFETY: the null buffer with length zero asks Windows for the required length.
        let process = unsafe {
            let process = GetCurrentProcess();
            GetSystemCpuSetInformation(null_mut(), 0, &raw mut length, process, 0);
            process
        };
        if length == 0 {
            return None;
        }
        // Use a typed allocation so the buffer is aligned for Windows. Records can vary in
        // size, so walk the returned bytes rather than indexing it as an array of records.
        let mut buffer = vec![Record::default(); (length as usize).div_ceil(size_of::<Record>())];
        let mut written = 0;
        // SAFETY: the buffer holds at least length writable bytes. Windows writes the actual
        // number used to written and never retains the pointers.
        let ok = unsafe {
            GetSystemCpuSetInformation(buffer.as_mut_ptr(), length, &raw mut written, process, 0)
        };
        if ok == 0 || written > length {
            return None;
        }
        let bytes = buffer.as_ptr().cast::<u8>();
        let mut offset = 0;
        let mut sets = Vec::new();
        while offset < written as usize {
            let remaining = written as usize - offset;
            if remaining < 2 * size_of::<u32>() {
                return None;
            }
            // SAFETY: the bounds above cover the header's size and type fields.
            let (record, size, kind) = unsafe {
                let record = bytes.add(offset);
                let size = record.cast::<u32>().read_unaligned() as usize;
                let kind = record.add(size_of::<u32>()).cast::<i32>().read_unaligned();
                (record, size, kind)
            };
            if !(2 * size_of::<u32>()..=remaining).contains(&size) {
                return None;
            }
            if kind == CpuSetInformation {
                if size < size_of::<Record>() {
                    return None;
                }
                // SAFETY: the record fits in the buffer and its type identifies this union arm.
                let (set, flags) = unsafe {
                    let set = record.cast::<Record>().read_unaligned().Anonymous.CpuSet;
                    (set, u32::from(set.Anonymous1.AllFlags))
                };
                if flags & SYSTEM_CPU_SET_INFORMATION_ALLOCATED == 0
                    || flags & SYSTEM_CPU_SET_INFORMATION_ALLOCATED_TO_TARGET_PROCESS != 0
                {
                    // A higher efficiency class means higher performance in this API.
                    sets.push((set.EfficiencyClass, set.Id));
                }
            }
            offset += size;
        }
        Some(sets)
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn hybrid_cpus_select_all_the_fastest_sets() {
            use super::fastest_cpu_sets;

            assert_eq!(fastest_cpu_sets(&[(3, 4), (0, 5), (3, 6), (1, 7)]), [4, 6]);
            assert!(fastest_cpu_sets(&[(1, 4), (1, 5)]).is_empty());
        }

        #[test]
        #[expect(unsafe_code, reason = "verifies Windows benchmark thread settings")]
        fn windows_applies_fast_core_preference() {
            use std::ptr::null_mut;

            use windows_sys::Win32::System::Threading::{
                GetCurrentThread, GetThreadPriority, GetThreadSelectedCpuSets,
                THREAD_PRIORITY_HIGHEST,
            };

            use super::{available_cpu_sets, fastest_cpu_sets, prefer_fastest_cores};

            let mut expected = fastest_cpu_sets(&available_cpu_sets().unwrap());
            assert!(prefer_fastest_cores());
            let mut selected = vec![0; expected.len()];
            let mut written = 0;
            let ids = if selected.is_empty() {
                null_mut()
            } else {
                selected.as_mut_ptr()
            };
            // SAFETY: the pseudo-handle names this test thread and ids is writable for the
            // given number of entries, or null when there are no entries to read.
            unsafe {
                let thread = GetCurrentThread();
                assert_eq!(GetThreadPriority(thread), THREAD_PRIORITY_HIGHEST);
                assert_ne!(
                    GetThreadSelectedCpuSets(thread, ids, selected.len() as u32, &raw mut written),
                    0
                );
            }
            assert_eq!(written as usize, expected.len());
            selected.sort_unstable();
            expected.sort_unstable();
            assert_eq!(selected, expected);
        }
    }
}
