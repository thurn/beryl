//! An ability's Rust body, for what effect data cannot say (the crate's contract, "Custom
//! bodies"): the escape hatch of plan K1. A card's static data holds one concrete function
//! for each storage [`Layout`], which [`custom_body`] makes of the one generic definition.

use core::marker::PhantomData;

use crate::card::{Behavior, Custom, Flow, Says};
use crate::game::Game;
use crate::layout::{Large, Layout, PerLayout, Small};
use crate::resolution::Resolution;

/// An ability's Rust body. A card file implements it on a unit type of its own, and its
/// ability's `body` is [`custom_body`] of that type: one definition, which each layout runs
/// its own instance of.
pub trait CustomBody: 'static {
    /// What the body says of itself, since nothing can read code: whether it may ask a
    /// player a question, and how it takes each of its targets from its controller. Effect
    /// data says both itself.
    const SAYS: Says = Says::NOTHING;

    /// Resolves the ability: `resolution` is of it, in `game`.
    fn resolve<L: Layout>(game: &mut Game<'_, L>, resolution: &mut Resolution<'_, L>) -> Flow;
}

/// The [`Behavior`] of the body `B`: its instance for each layout, which the vocabulary
/// carries without reading ([`Custom`]), and what it says of itself.
pub const fn custom_body<B: CustomBody>() -> Behavior {
    Behavior::Custom(Custom::new(&Instances::<B>::BODIES, &B::SAYS))
}

/// The body of an ability, run on a state of layout `L`.
type Body<L> = for<'a, 'b, 'c> fn(&'b mut Game<'a, L>, &'c mut Resolution<'a, L>) -> Flow;

/// A body as each layout runs it: one generic function, instantiated once per layout, since
/// a card's static data can hold only concrete function pointers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bodies {
    small: Body<Small>,
    large: Body<Large>,
}

/// A static's worth of `B`'s instances: a `const fn` cannot name a `static` of its own for
/// each `B`, and an associated constant is one.
struct Instances<B>(PhantomData<B>);

impl<B: CustomBody> Instances<B> {
    const BODIES: Bodies = Bodies {
        small: B::resolve::<Small>,
        large: B::resolve::<Large>,
    };
}

impl PerLayout for Bodies {
    type Of<L: Layout> = Body<L>;
}

impl Bodies {
    /// The instance of the body for states of layout `L`. Card definitions are static data,
    /// so each layout reads its own instance from them.
    #[inline]
    pub(crate) fn of<L: Layout>(&self) -> Body<L> {
        L::of::<Bodies>(self.small, self.large)
    }
}
