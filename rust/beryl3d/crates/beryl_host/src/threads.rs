//! Scheduling classes, which the app sets on each of its threads rather than letting them
//! inherit one (docs/beryl-3d.md, "Threads"). A thread otherwise keeps the class of the thread
//! that made it: in the prototype, a game started from the frame thread searched at drawing's
//! class and stalled frames by 50 to 230 ms on a loaded machine.
//!
//! On Apple platforms a class is a quality-of-service class. On Windows it is a thread
//! priority within the process's priority class, which the same three levels map onto. On
//! Android and Linux, setting a class does nothing and no thread reports one.

/// A scheduling class, from the lowest of the three to the highest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Below drawing and below the asset workers: the session thread and the AI's search
    /// threads.
    Utility,
    /// What a thread of an ordinary program gets: the asset workers, which decode, compress
    /// and rasterize ahead of the AI but behind drawing.
    Default,
    /// The highest: the frame thread, which the system runs ahead of the others when it wakes.
    Interactive,
}

impl Class {
    /// The word the command line uses for it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Utility => "utility",
            Self::Default => "default",
            Self::Interactive => "interactive",
        }
    }

    /// The class `word` names, if it names one.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.word() == word)
    }

    const ALL: [Self; 3] = [Self::Utility, Self::Default, Self::Interactive];
}

/// Puts the calling thread in `class`.
pub fn set_class(class: Class) {
    platform::set(class);
}

/// The calling thread's class, or `None` where the system has no classes or the thread is in
/// one that is none of the three.
#[must_use]
pub fn current_class() -> Option<Class> {
    platform::current()
}

/// Puts the calling thread in the user-interactive class, the highest, so that the operating
/// system runs it ahead of the AI's search threads when it wakes: the frame thread.
pub fn interactive_thread() {
    set_class(Class::Interactive);
}

/// Puts the calling thread in the default class, below drawing and above the AI: an asset
/// worker's (`beryl_assets`' `Preparer` and `GlyphWorker` call it as each thread starts).
pub fn asset_thread() {
    set_class(Class::Default);
}

/// Puts the calling thread in the utility class, below drawing: the session thread, and through
/// it every thread the session and the search start, which inherit its class.
pub fn utility_thread() {
    set_class(Class::Utility);
}

#[cfg(target_vendor = "apple")]
mod platform {
    use libc::qos_class_t::{QOS_CLASS_DEFAULT, QOS_CLASS_USER_INTERACTIVE, QOS_CLASS_UTILITY};

    use super::Class;

    fn number(class: Class) -> libc::qos_class_t {
        match class {
            Class::Utility => QOS_CLASS_UTILITY,
            Class::Default => QOS_CLASS_DEFAULT,
            Class::Interactive => QOS_CLASS_USER_INTERACTIVE,
        }
    }

    #[expect(unsafe_code, reason = "pthread_set_qos_class_self_np")]
    pub(super) fn set(class: Class) {
        // SAFETY: `pthread_set_qos_class_self_np` changes only the calling thread's class and
        // takes no pointers. It fails only for an invalid class or priority, which these are
        // not.
        let _ = unsafe { libc::pthread_set_qos_class_self_np(number(class), 0) };
    }

    /// Reads the class as a number, not as `qos_class_t`, so that a class newer than libc's
    /// list is not an invalid value of the enum.
    #[expect(unsafe_code, reason = "pthread_get_qos_class_np")]
    pub(super) fn current() -> Option<Class> {
        let mut class = 0u32;
        let mut priority = 0;
        // SAFETY: `pthread_self` names the calling thread, which is alive. Both pointers are to
        // locals that outlive the call, and `qos_class_t` is `repr(u32)`, so the class fits.
        let _ = unsafe {
            libc::pthread_get_qos_class_np(
                libc::pthread_self(),
                (&raw mut class).cast::<libc::qos_class_t>(),
                &raw mut priority,
            )
        };
        Class::ALL
            .into_iter()
            .find(|named| number(*named) as u32 == class)
    }
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, GetThreadPriority, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        THREAD_PRIORITY_HIGHEST, THREAD_PRIORITY_NORMAL,
    };

    use super::Class;

    fn priority(class: Class) -> i32 {
        match class {
            Class::Utility => THREAD_PRIORITY_BELOW_NORMAL,
            Class::Default => THREAD_PRIORITY_NORMAL,
            Class::Interactive => THREAD_PRIORITY_HIGHEST,
        }
    }

    #[expect(unsafe_code, reason = "SetThreadPriority")]
    pub(super) fn set(class: Class) {
        // SAFETY: `GetCurrentThread` returns a pseudo-handle for the calling thread that needs
        // no closing, and `SetThreadPriority` takes no pointers. It fails only for an invalid
        // handle or priority, which these are not.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), priority(class)) };
    }

    #[expect(unsafe_code, reason = "GetThreadPriority")]
    pub(super) fn current() -> Option<Class> {
        // SAFETY: as in `set`; `GetThreadPriority` only reads the calling thread's priority.
        let current = unsafe { GetThreadPriority(GetCurrentThread()) };
        Class::ALL
            .into_iter()
            .find(|class| priority(*class) == current)
    }
}

#[cfg(not(any(target_vendor = "apple", windows)))]
mod platform {
    use super::Class;

    pub(super) fn set(_class: Class) {}

    pub(super) fn current() -> Option<Class> {
        None
    }
}
