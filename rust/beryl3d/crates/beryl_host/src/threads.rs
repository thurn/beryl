//! Scheduling classes, which the app sets on each of its threads rather than letting them
//! inherit one (docs/beryl-3d.md, "Threads"). A thread otherwise keeps the class of the thread
//! that made it: in the prototype, a game started from the frame thread searched at drawing's
//! class and stalled frames by 50 to 230 ms on a loaded machine.
//!
//! Windows has no class to ask for that safe code can reach, so there setting a class does
//! nothing, every thread keeps the normal priority, and no thread reports a class.

/// A scheduling class, from the lowest of the three to the highest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// Below drawing: the session thread and the AI's search threads.
    Utility,
    /// What a thread of an ordinary program gets: the class of most of a busy machine's work.
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
        [Self::Utility, Self::Default, Self::Interactive]
            .into_iter()
            .find(|class| class.word() == word)
    }
}

/// Puts the calling thread in `class`.
pub fn set_class(class: Class) {
    #[cfg(target_os = "macos")]
    crate::macos::set_thread_class(match class {
        Class::Utility => libc::qos_class_t::QOS_CLASS_UTILITY,
        Class::Default => libc::qos_class_t::QOS_CLASS_DEFAULT,
        Class::Interactive => libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE,
    });
    #[cfg(not(target_os = "macos"))]
    let _ = class;
}

/// The calling thread's class, or `None` where the system has no classes or the thread is in
/// one that is none of the three.
#[must_use]
pub fn current_class() -> Option<Class> {
    #[cfg(target_os = "macos")]
    {
        use libc::qos_class_t::{QOS_CLASS_DEFAULT, QOS_CLASS_USER_INTERACTIVE, QOS_CLASS_UTILITY};
        let class = crate::macos::thread_class();
        [
            (QOS_CLASS_UTILITY as u32, Class::Utility),
            (QOS_CLASS_DEFAULT as u32, Class::Default),
            (QOS_CLASS_USER_INTERACTIVE as u32, Class::Interactive),
        ]
        .into_iter()
        .find_map(|(number, named)| (number == class).then_some(named))
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// Puts the calling thread in the user-interactive class, the highest, so that the operating
/// system runs it ahead of the AI's search threads when it wakes: the frame thread.
pub fn interactive_thread() {
    set_class(Class::Interactive);
}

/// Puts the calling thread in the utility class, below drawing: the session thread, and through
/// it every thread the session and the search start, which inherit its class.
pub fn utility_thread() {
    set_class(Class::Utility);
}
