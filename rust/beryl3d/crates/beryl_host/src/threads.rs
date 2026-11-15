//! Scheduling classes, which the app sets on each of its threads rather than letting them
//! inherit one (docs/beryl-3d.md, "Threads"). A thread otherwise keeps the class of the thread
//! that made it: in the prototype, a game started from the frame thread searched at drawing's
//! class and stalled frames by 50 to 230 ms on a loaded machine.
//!
//! Windows has no class to ask for that safe code can reach, so there both calls do nothing and
//! every thread keeps the normal priority.

/// Puts the calling thread in the user-interactive class, the highest, so that the operating
/// system runs it ahead of the AI's search threads when it wakes: the frame thread.
pub fn interactive_thread() {
    #[cfg(target_os = "macos")]
    crate::macos::set_thread_class(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE);
}

/// Puts the calling thread in the utility class, below drawing: the session thread, and through
/// it every thread the session and the search start, which inherit its class.
pub fn utility_thread() {
    #[cfg(target_os = "macos")]
    crate::macos::set_thread_class(libc::qos_class_t::QOS_CLASS_UTILITY);
}
