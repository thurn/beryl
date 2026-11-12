//! What the rules keep once for each storage [`Layout`]: an ability's Rust body and an
//! intervening "if" clause, which a card's static data holds as one concrete function per
//! layout. `crate::layout` re-exports them beside the kernel's layouts.

use crate::card::Flow;
use crate::effect::EffectCtx;
use crate::layout::{Large, Layout, PerLayout, Small};
use crate::state::{PendingTrigger, State};
use crate::table::GameTable;

/// The body of an ability, run on a state of layout `L`.
pub type Body<L = Small> = for<'a, 'b> fn(&'b mut EffectCtx<'a, L>) -> Flow;

/// A triggered ability's intervening "if" clause, read on a state of layout `L`.
pub type Condition<L = Small> = fn(&State<L>, &GameTable, &PendingTrigger<L>) -> bool;

/// An ability's Rust body as each layout runs it: one generic function, instantiated once per
/// layout, since a card's static data can hold only concrete function pointers. Card files
/// write [`bodies!`](crate::bodies) around the generic function, which makes the ability's
/// [`Behavior::Custom`](beryl_card::Behavior::Custom) of them.
#[derive(Clone, Copy, Debug)]
pub struct Bodies {
    pub small: Body<Small>,
    pub large: Body<Large>,
}

/// An intervening "if" clause written in Rust, as each layout reads it, as [`Bodies`]; card
/// files write [`conditions!`](crate::conditions), which makes the ability's
/// [`Condition::Custom`](beryl_card::Condition::Custom) of them.
#[derive(Clone, Copy, Debug)]
pub struct Conditions {
    pub small: Condition<Small>,
    pub large: Condition<Large>,
}

/// The [`Behavior`](beryl_card::Behavior) of a Rust function generic over the layout: its
/// [`Bodies`], which the vocabulary carries without reading
/// ([`Custom`](beryl_card::Custom)).
#[macro_export]
macro_rules! bodies {
    ($body:expr) => {
        $crate::card::Behavior::Custom($crate::card::Custom::new(&$crate::layout::Bodies {
            small: $body,
            large: $body,
        }))
    };
}

/// The [`Condition`](beryl_card::Condition) of a Rust function generic over the layout: its
/// [`Conditions`], as [`bodies!`](crate::bodies). A card definition holds a condition by
/// reference, so this is one, for a `static` or `const` to hold.
#[macro_export]
macro_rules! conditions {
    ($condition:expr) => {
        &$crate::card::Condition::Custom($crate::card::Custom::new(&$crate::layout::Conditions {
            small: $condition,
            large: $condition,
        }))
    };
}

impl PerLayout for Bodies {
    type Of<L: Layout> = Body<L>;
}

impl Bodies {
    /// The instance of the body for states of layout `L`. Card definitions are static data,
    /// so each layout reads its own instance from them.
    #[inline]
    pub fn of<L: Layout>(&self) -> Body<L> {
        L::of::<Bodies>(self.small, self.large)
    }
}

impl PerLayout for Conditions {
    type Of<L: Layout> = Condition<L>;
}

impl Conditions {
    /// The instance of the clause for states of layout `L`, as [`Bodies::of`].
    #[inline]
    pub fn of<L: Layout>(&self) -> Condition<L> {
        L::of::<Conditions>(self.small, self.large)
    }
}
