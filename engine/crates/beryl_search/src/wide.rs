//! Search on a [`Small`] game going on in [`Large`] where the game outgrows `Small`, as a
//! session's game does (`beryl_session`'s `Boards`).
//!
//! The engine stalls a game, `Outcome::Stalled(StallReason::Capacity)`, when one of its
//! layout's capacities runs out, and some things only `Large` holds at all: a subgame, and the
//! mana spent on an activated ability's X ([`beryl_engine::Layout::Spent`]). A search that
//! scored such a stall as a draw would misjudge every action whose result `Small` cannot hold,
//! as activating Illusionary Mask for one mana or more. So an iteration or a rollout that
//! stalls this way is played again from its start in `Large`, with the same choices and the
//! same random streams. Widening is lossless ([`State::widen`]), so the game reaches the
//! position where `Small` ran out and goes on past it.
//!
//! Nothing here runs unless a game stalls, and a rollout in the WU Control mirror never does,
//! so the search's ordinary path pays only for a check of the outcome after each iteration.

use beryl_engine::layout::PerLayout;
use beryl_engine::{Large, Layout, Outcome, Scratch, Small, StallReason, State};

/// The function that widens a state of a layout into [`Large`]: none for `Large` itself.
struct Widener;

impl PerLayout for Widener {
    type Of<L: Layout> = Option<fn(&State<L>) -> State<Large>>;
}

/// Whether a layout can widen into [`Large`]: `true` for [`Small`], `false` for [`Large`].
struct CanWiden;

impl PerLayout for CanWiden {
    type Of<L: Layout> = bool;
}

/// Whether a state in layout `L` can widen into [`Large`] (`true` for [`Small`], `false` for
/// [`Large`]).
#[inline]
pub(crate) fn can_widen<L: Layout>() -> bool {
    L::of::<CanWiden>(true, false)
}

/// Whether the engine stalled `s` because a capacity of its layout ran out.
#[inline]
pub(crate) fn ran_out<L: Layout>(s: &State<L>) -> bool {
    s.outcome() == Some(Outcome::Stalled(StallReason::Capacity))
}

/// `s` in [`Large`], if it is a [`Small`] state. A `Large` state has nowhere wider to go, so a
/// capacity that runs out there leaves its game stalled.
pub(crate) fn widened<L: Layout>(s: &State<L>) -> Option<State<Large>> {
    let widen = L::of::<Widener>(Some(State::<Small>::widen), None)?;
    Some(widen(s))
}

/// Charges the engine work done in `wide`, a `Large` replay's scratch, to `x`, the scratch
/// whose counts a search's budget reads.
pub(crate) fn charge<L: Layout>(x: &mut Scratch<L>, wide: &Scratch<Large>) {
    x.counts.decisions += wide.counts.decisions;
    x.counts.silent += wide.counts.silent;
    x.counts.state_copies += wide.counts.state_copies;
}
