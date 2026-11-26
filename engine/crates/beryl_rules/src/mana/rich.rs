//! The mana solver's path for what its classes of sources cannot say: mana abilities with a
//! price ([`crate::table::is_priced`]) and products of several terms or of a counted amount
//! ([`crate::card::Ability::product`]). It is entered only in a game whose cards have one
//! ([`Capability::PricedMana`], [`Capability::ManaChanges`]), and only where the plain path
//! found no payment ([`super::Funds::pays_for`]) or after it listed its plans
//! ([`super::plans_accepting`]), so a game without these cards runs none of it.
//!
//! A payment here is a sequence: mana abilities activated in order, each paying its own price
//! from the mana in the pool as it then stands, the floating mana and what earlier activations
//! made, and then the cost paid from the pool (see [CR 605.3a]): "A player may activate an
//! activated mana ability whenever they have priority, whenever they are casting a spell or
//! activating an ability that requires a mana payment, or whenever a rule or effect asks for a
//! mana payment". No card the engine pays has such an ability yet: the card table refuses
//! each (`beryl_kernel`'s `payable`) until the payment of its kind is built here, so both
//! entry points answer as the plain path does.
//!
//! Implements: none. Plans the mana abilities that `mana::pay` lets a player activate (CR 605.3).
//!
//! [CR 605.3a]: https://yawgatog.com/resources/magic-rules/#R6053a

use crate::bitboard::Bitboard;
use crate::card::ManaCost;
use crate::card::ObjectAbility;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Small};
use crate::replacement::event::Sink;
use crate::state::State;
#[cfg(doc)]
use crate::table::Capability;
use crate::table::GameTable;

use super::pool::Funds;
use super::remembered::Paying;
use super::solver::Plan;
use super::sources::Sources;

/// Whether `player`, whose funds the plain path found are `funds`, can pay `cost` with none of
/// `exclude` for what `paying` is for by activating priced mana abilities or abilities whose
/// product the classes cannot say, where the plain path found they cannot
/// ([`Funds::pays_for`]). Such sources only add to what pays, as restricted mana does.
///
/// No priced source is paid yet, so nothing more pays: false.
#[cold]
#[inline(never)]
pub(super) fn pays<L: Layout>(
    funds: &Funds<L>,
    s: &State<L>,
    t: &GameTable,
    cost: ManaCost,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
) -> bool {
    let _ = (funds, s, t, cost, exclude, paying);
    false
}

/// Adds to `out` the plans of a payment of `cost` by `player` with none of `exclude`, for what
/// `paying` is for, that activate the sources the classes of `sources` cannot say
/// (`Sources::extras`), each as [`super::plans_accepting`] offers a plan: kept only where
/// `accepted` takes it, and one for each outcome. Under `greedy`, the first such plan alone,
/// and only where `out` holds none.
///
/// No priced source is paid yet, so no plan is added.
#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
#[allow(
    clippy::unnecessary_wraps,
    reason = "a plan this adds may find `out` full, once it adds any"
)]
pub(super) fn plans<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    (player, cost, exclude, paying): (Seat, ManaCost, Bitboard<L::Slot>, Paying<L>),
    greedy: bool,
    accepted: &mut impl FnMut(&Plan<L>) -> bool,
    sources: &Sources<L>,
    out: &mut L::Queue<Plan<L>, 64>,
) -> Result<(), Full> {
    let _ = (
        s, t, sink, player, cost, exclude, greedy, paying, accepted, out,
    );
    debug_assert!(sources.extras.is_empty(), "no priced source is found yet");
    Ok(())
}

/// A source the classes cannot say: a mana ability of the permanent in `slot` that has a price
/// ([`crate::table::is_priced`]) or makes more than one term ([`crate::card::Ability::product`]).
#[derive(Clone, Copy, Debug)]
#[expect(
    dead_code,
    reason = "the rich path reads these once it plans a kind of priced source"
)]
pub(crate) struct Extra<L: Layout = Small> {
    pub(super) slot: L::Slot,
    pub(super) ability: ObjectAbility,
}
