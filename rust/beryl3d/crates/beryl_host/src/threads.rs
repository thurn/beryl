//! Scheduling classes, which the app sets on each of its threads rather than letting them
//! inherit one (docs/beryl-3d.md, "Threads"). A thread otherwise keeps the class of the thread
//! that made it: in the prototype, a game started from the frame thread searched at drawing's
//! class and stalled frames by 50 to 230 ms on a loaded machine.
//!
//! On Apple platforms a class is a quality-of-service class. On Windows it is a thread
//! priority within the process's priority class, which the same four levels map onto. On
//! Android it is the thread's nice value, set with `setpriority`, using the values Android's
//! own `Process.THREAD_PRIORITY_*` constants give (`Class::nice`). On Linux, setting a class
//! does nothing and no thread reports one.

/// A scheduling class, from the lowest of the four to the highest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Background work nobody waits for: the frame log's writer. The AI's search threads ran
    /// here once; they moved to `Default` so that they search in the web app's class.
    Utility,
    /// What a thread of an ordinary program gets: the session thread and the AI's search
    /// threads, as in the web app, so that the AI searches as fast as it does there.
    Default,
    /// Above the AI and below drawing: the asset workers, which decode, compress and
    /// rasterize card faces and glyphs ahead of the search.
    Initiated,
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
            Self::Initiated => "initiated",
            Self::Interactive => "interactive",
        }
    }

    /// The class `word` names, if it names one.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.word() == word)
    }

    /// The nice value Android puts a thread of this class at, from Android's
    /// `Process.THREAD_PRIORITY_*` constants: lower runs first.
    ///
    /// `Interactive` is `THREAD_PRIORITY_DISPLAY` (-4), the value Android gives an app's own
    /// render thread, rather than `THREAD_PRIORITY_URGENT_DISPLAY` (-8), which Android keeps
    /// for the system's compositor: at -4 a thread's CFS weight is already about 2.4 times
    /// that of a search thread at 0, and the scheduler preempts a search thread when the frame
    /// thread wakes either way.
    #[must_use]
    pub fn nice(self) -> i32 {
        match self {
            // THREAD_PRIORITY_BACKGROUND
            Self::Utility => 10,
            // THREAD_PRIORITY_DEFAULT
            Self::Default => 0,
            // Two steps above the default (THREAD_PRIORITY_MORE_FAVORABLE is one), between the
            // default and display.
            Self::Initiated => -2,
            // THREAD_PRIORITY_DISPLAY
            Self::Interactive => -4,
        }
    }

    const ALL: [Self; 4] = [
        Self::Utility,
        Self::Default,
        Self::Initiated,
        Self::Interactive,
    ];
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

/// Puts the calling thread in the user-initiated class, below drawing and above the AI: an
/// asset worker's (`beryl_assets`' `Preparer` and `GlyphWorker` call it as each thread starts).
pub fn asset_thread() {
    set_class(Class::Initiated);
}

/// Puts the calling thread in the utility class, below everything else Beryl runs: background
/// work nobody waits for, such as the frame log's writer.
pub fn utility_thread() {
    set_class(Class::Utility);
}

#[cfg(target_vendor = "apple")]
mod platform {
    use libc::qos_class_t::{
        QOS_CLASS_DEFAULT, QOS_CLASS_USER_INITIATED, QOS_CLASS_USER_INTERACTIVE, QOS_CLASS_UTILITY,
    };

    use super::Class;

    fn number(class: Class) -> libc::qos_class_t {
        match class {
            Class::Utility => QOS_CLASS_UTILITY,
            Class::Default => QOS_CLASS_DEFAULT,
            Class::Initiated => QOS_CLASS_USER_INITIATED,
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
        GetCurrentThread, GetThreadPriority, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
        THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_HIGHEST, THREAD_PRIORITY_NORMAL,
    };

    use super::Class;

    fn priority(class: Class) -> i32 {
        match class {
            Class::Utility => THREAD_PRIORITY_BELOW_NORMAL,
            Class::Default => THREAD_PRIORITY_NORMAL,
            Class::Initiated => THREAD_PRIORITY_ABOVE_NORMAL,
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

#[cfg(target_os = "android")]
mod platform {
    use std::ffi::CString;

    use super::Class;

    #[expect(unsafe_code, reason = "setpriority")]
    pub(super) fn set(class: Class) {
        // SAFETY: `gettid` names the calling thread, and `setpriority` changes only its nice
        // value and takes no pointers. It needs no permission for an app's own thread, since
        // Android's init raises every process's nice limit to -20, but it is checked and
        // logged in case a device refuses.
        let (tid, result) = unsafe {
            let tid = libc::gettid();
            #[expect(clippy::cast_sign_loss, reason = "a thread ID is positive")]
            let result = libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, class.nice());
            (tid, result)
        };
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        let line = if result == 0 {
            format!(
                "thread {tid} ({name}) is in class {} at nice {}",
                class.word(),
                class.nice()
            )
        } else {
            format!(
                "thread {tid} ({name}) could not be put in class {} at nice {}: {}",
                class.word(),
                class.nice(),
                std::io::Error::last_os_error()
            )
        };
        log(&line);
    }

    /// `getpriority` returns -1 both for an error and for nice -1, which no class has, so
    /// neither is mistaken for a class.
    #[expect(unsafe_code, reason = "getpriority")]
    pub(super) fn current() -> Option<Class> {
        // SAFETY: as in `set`; `getpriority` only reads the calling thread's nice value.
        let nice = unsafe {
            #[expect(clippy::cast_sign_loss, reason = "a thread ID is positive")]
            libc::getpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t)
        };
        Class::ALL.into_iter().find(|class| class.nice() == nice)
    }

    /// Writes `line` to logcat at debug level under the tag `Beryl3dThreads`, where Android
    /// sends nothing a thread writes to standard error.
    #[expect(unsafe_code, reason = "__android_log_write")]
    fn log(line: &str) {
        let Ok(text) = CString::new(line) else {
            return;
        };
        // SAFETY: both strings are NUL-terminated and outlive the call, which copies them.
        // The standard library already links liblog on Android.
        let _ = unsafe {
            ndk_sys::__android_log_write(
                ndk_sys::android_LogPriority::ANDROID_LOG_DEBUG.0.cast_signed(),
                c"Beryl3dThreads".as_ptr(),
                text.as_ptr(),
            )
        };
    }
}

#[cfg(not(any(target_vendor = "apple", windows, target_os = "android")))]
mod platform {
    use super::Class;

    pub(super) fn set(_class: Class) {}

    pub(super) fn current() -> Option<Class> {
        None
    }
}
