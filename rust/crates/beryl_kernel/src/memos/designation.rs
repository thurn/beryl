//! Class and Case designations in the memo store.
//! Implements: none. Stores designations for rules to read.

use super::{Full, Kinds, Layout, Memo, Queue, State};

impl<L: Layout> State<L> {
    /// The permanent's level (see [CR 717.2d]): without a designation, "it is treated as
    /// though its level is 1."
    ///
    /// [CR 717.2d]: https://yawgatog.com/resources/magic-rules/#R7172d
    pub fn class_level(&self, slot: L::Slot) -> u8 {
        self.memos
            .of(Kinds::CLASS_LEVEL)
            .iter()
            .find_map(|e| {
                if let Memo::ClassLevel(level) = e.memo
                    && e.slot == slot
                {
                    Some(level)
                } else {
                    None
                }
            })
            .unwrap_or(1)
    }

    /// Changes a permanent's Class level, keeping only its current designation.
    ///
    /// # Errors
    ///
    /// If a first designation has no room in the memo store.
    pub fn set_class_level(&mut self, slot: L::Slot, level: u8) -> Result<(), Full> {
        debug_assert!(level > 1);
        if self.memos.kinds.0 & Kinds::CLASS_LEVEL != 0
            && let Some(entry) = self
                .memos
                .entries
                .as_mut_slice()
                .iter_mut()
                .find(|e| e.slot == slot && matches!(e.memo, Memo::ClassLevel(_)))
        {
            entry.memo = Memo::ClassLevel(level);
            self.flags.derived_dirty = true;
            Ok(())
        } else {
            self.remember(slot, Memo::ClassLevel(level))
        }
    }

    /// Whether the object in this slot has the solved designation (see CR 720.3b).
    pub fn is_solved(&self, slot: L::Slot) -> bool {
        self.memos
            .of(Kinds::SOLVED)
            .iter()
            .any(|e| e.slot == slot && e.memo == Memo::Solved)
    }
}
