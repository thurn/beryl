//! Names used to display the state of a resolved element.

use super::{Role, StateBits};

/// How many text annotations each element can hold.
pub const ANNOTATIONS: usize = 2;

/// The vocabulary a probe uses for a scene's elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Labels {
    states: [Option<&'static str>; u32::BITS as usize],
    pub count: Option<&'static str>,
    pub annotations: [Option<&'static str>; ANNOTATIONS],
    pub roles: &'static [Role],
}

impl Labels {
    pub const DEFAULT: Self = Self {
        states: [None; u32::BITS as usize],
        count: None,
        annotations: [None; ANNOTATIONS],
        roles: &[],
    }
    .with_state(StateBits::FACE_DOWN, "face-down")
    .with_state(StateBits::MOVING, "moving")
    .with_state(StateBits::HOVERED, "hovered")
    .with_state(StateBits::HELD, "held")
    .with_state(StateBits::ELIGIBLE, "eligible")
    .with_state(StateBits::SELECTED, "selected")
    .with_state(StateBits::TARGET, "target")
    .with_state(StateBits::DISABLED, "disabled")
    .with_state(StateBits::FOCUSED, "focused")
    .with_state(StateBits::PRIMARY, "primary");

    /// Adds a display label for a free state bit.
    ///
    /// # Panics
    ///
    /// If `bit` is already labelled or has anything other than one bit set.
    #[must_use]
    pub const fn with_state(mut self, bit: u32, word: &'static str) -> Self {
        assert!(bit.is_power_of_two());
        let index = bit.trailing_zeros() as usize;
        assert!(self.states[index].is_none());
        self.states[index] = Some(word);
        self
    }

    /// The visible states in bit order.
    pub fn states(&self, state: StateBits) -> impl Iterator<Item = &'static str> {
        self.states
            .iter()
            .copied()
            .enumerate()
            .filter_map(move |(index, word)| word.filter(|_| state.has(1 << index)))
    }

    /// Whether this word names a visible state bit.
    #[must_use]
    pub fn has_state(&self, word: &str) -> bool {
        self.states.contains(&Some(word))
    }

    /// The built-in roles and the scene owner's custom roles.
    pub fn roles(&self) -> impl Iterator<Item = Role> + '_ {
        Role::ALL.into_iter().chain(self.roles.iter().copied())
    }

    #[must_use]
    pub fn role(&self, word: &str) -> Option<Role> {
        self.roles().find(|role| role.word() == word)
    }
}
