//! Apple silicon mixes core types that run the same code at different speeds. `sysctl
//! hw.perflevel0` is the fastest kind (6 "Super" cores on an M5 Max; "Performance" on earlier
//! chips) and `hw.perflevel1` the slower kind ("Performance" on an M5 Max; "Efficiency"
//! before). A benchmark that moves between them measures the move.
//!
//! macOS has no way to pin a thread to a core. It places threads by quality-of-service
//! class, and keeps user-interactive threads on the fastest cores while one is free. So a
//! benchmark sets that class first, runs single-threaded (or with no more threads than
//! perflevel0 has cores), and runs on a quiet machine (`just bench` checks).

/// Sets the calling thread's quality-of-service class to user-interactive. Returns whether
/// that worked; always false off macOS, where it does nothing.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub fn prefer_fastest_cores() -> bool {
    // SAFETY: changes only the calling thread's scheduling class; takes no pointers.
    let status = unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
    };
    status == 0
}

#[cfg(not(target_os = "macos"))]
pub fn prefer_fastest_cores() -> bool {
    false
}
