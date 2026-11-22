//! An ability's effect data or Rust body and the information the engine reads about it.

use std::any::Any;
use std::fmt;
use std::panic::RefUnwindSafe;

use crate::effect::Effect;
use crate::target::Takes;

/// What an ability does as it resolves: effect data, or a Rust function where the
/// vocabulary cannot say it yet.
#[derive(Clone, Copy, Debug)]
pub enum Behavior {
    /// Typed data, which the engine's interpreter runs and which tools read: what the engine,
    /// AI and UI need to know about the ability comes from here, not from a declaration
    /// beside it.
    Data(&'static Effect),
    /// The escape hatch: a Rust function only the engine can run and nothing can read.
    Custom(Custom),
}

/// An ability's Rust body: a value this crate carries without knowing its type, the engine's
/// function pointers, which take engine types this crate cannot name, and what the body's
/// registration says of it, since nothing can read code. The crate that made one reads the
/// value back with [`Custom::get`].
#[derive(Clone, Copy)]
pub struct Custom {
    body: &'static (dyn Any + Send + Sync + RefUnwindSafe),
    says: &'static Says,
}

/// What a Rust body's registration says of it, which effect data says of itself.
#[derive(Clone, Copy, Debug)]
pub struct Says {
    /// Whether it may ask a player a question mid-resolution ([`Ability::asks`](super::Ability::asks)).
    pub asks: bool,
    /// Whether the body can insert, skip or cut short a step or phase.
    pub changes_turn_structure: bool,
    /// Whether the body can suspend the game to play a subgame.
    pub starts_subgame: bool,
    /// How it takes what is chosen for each instance of the word "target" from its
    /// controller, in the order of the ability's targets, with none for one it leaves alone
    /// and for any past the list's end ([`Ability::takes_target`](super::Ability::takes_target)).
    pub takes: &'static [Option<Takes>],
}

impl Says {
    /// A body that asks nothing and takes no target.
    pub const NOTHING: Says = Says {
        asks: false,
        changes_turn_structure: false,
        starts_subgame: false,
        takes: &[],
    };
}

impl Custom {
    pub const fn new<T: Any + Send + Sync + RefUnwindSafe>(
        body: &'static T,
        says: &'static Says,
    ) -> Custom {
        Custom { body, says }
    }

    /// The value, if it is a `T`.
    pub fn get<T: Any>(self) -> Option<&'static T> {
        let body: &'static dyn Any = self.body;
        body.downcast_ref()
    }

    /// What its registration says of it.
    pub const fn says(self) -> &'static Says {
        self.says
    }
}

impl fmt::Debug for Custom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Custom(..)")
    }
}
