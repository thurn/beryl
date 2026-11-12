//! What the rules keep once for each storage [`Layout`]: an ability's Rust body and an
//! intervening "if" clause, which a card's static data holds as one concrete function per
//! layout. `crate::layout` re-exports them beside the kernel's layouts.

use core::marker::PhantomData;

use crate::card::{Behavior, Custom, Flow};
use crate::effect::EffectCtx;
use crate::game::Game;
use crate::layout::{Large, Layout, PerLayout, Small};
use crate::resolution::Resolution;
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

/// An ability's Rust body, for what effect data cannot say yet (the crate's contract, "Custom
/// bodies"). A card file implements it on a unit type of its own, and its ability's `body` is
/// [`custom_body`] of that type: one definition, which each layout runs its own instance of.
pub trait CustomBody: 'static {
    /// Resolves the ability: `resolution` is of it, in `game`.
    fn resolve<L: Layout>(game: &mut Game<'_, L>, resolution: &mut Resolution<'_, L>) -> Flow;
}

/// The [`Behavior`] of the body `B`: its instance for each layout, which the vocabulary
/// carries without reading ([`Custom`]).
pub const fn custom_body<B: CustomBody>() -> Behavior {
    Behavior::Custom(Custom::new(&Instances::<B>::BODIES))
}

/// A static's worth of `B`'s instances: a `const fn` cannot name a `static` of its own for
/// each `B`, and an associated constant is one.
struct Instances<B>(PhantomData<B>);

impl<B: CustomBody> Instances<B> {
    const BODIES: Bodies = Bodies {
        small: run::<B, Small>,
        large: run::<B, Large>,
    };
}

/// `B` as the body the shim's callers run, until a [`Body`] takes the game and the resolution
/// itself.
fn run<B: CustomBody, L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    B::resolve(&mut ctx.game, &mut ctx.resolution)
}

/// The [`Behavior`](beryl_card::Behavior) of a Rust function generic over the layout: its
/// [`Bodies`], which the vocabulary carries without reading
/// ([`Custom`](beryl_card::Custom)). The bodies written before the rules library use it;
/// a new one is a [`CustomBody`].
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
