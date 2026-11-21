//! Asking a player for a value of a catalog: a color, a creature type or a card name
//! ([`ChosenValue`]), in the catalog prompt shape ([`beryl_card::prompt::Shape::Catalog`]).
//!
//! The question offers one [`Opt::Number`] per value, numbered as [`ChosenValue::candidates`]
//! numbers them, so a recorded answer names the same value in every run that asks it. A
//! permanent asks it as it enters ([`crate::replacement`]); a resolving spell or ability asks it
//! through [`crate::interpret::choices::catalog`].
//!
//! Implements: none. Builds and decodes catalog decisions.

use beryl_card::ChosenKind;
use beryl_kernel::chosen::ChosenValue;

use crate::bitboard::Bitboard;
use crate::card::Prompt;
use crate::decision::{Decision, DecisionKind, OPTIONS, Opt};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::table::GameTable;

/// The question that asks `player` for a value of `kind`, about the objects `about`: the
/// permanent that chooses it as it enters, or none.
pub(crate) fn decision<L: Layout>(
    t: &GameTable,
    player: Seat,
    kind: ChosenKind,
    about: Bitboard<L::Slot>,
) -> Result<Decision<L>, Full> {
    let mut options: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for n in ChosenValue::<L>::candidates(t, kind) {
        options.push(Opt::Number(n))?;
    }
    let prompt = Prompt::new(kind.purpose());
    Ok(Decision::options(player, DecisionKind::ChooseObject, options, prompt).about(about))
}

/// The question that asks `player` for one of the values of `kind` that `keep` takes, by
/// number, with `prompt`, about the objects `about`.
pub(crate) fn decision_among<L: Layout>(
    t: &GameTable,
    player: Seat,
    kind: ChosenKind,
    prompt: Prompt,
    about: Bitboard<L::Slot>,
    keep: impl Fn(u8) -> bool,
) -> Result<Decision<L>, Full> {
    let mut options: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for n in ChosenValue::<L>::candidates(t, kind).filter(|&n| keep(n)) {
        options.push(Opt::Number(n))?;
    }
    Ok(Decision::options(player, DecisionKind::ChooseObject, options, prompt).about(about))
}

/// The value of `kind` that the answer `opt` to a [`decision`] of that kind names.
pub(crate) fn value<L: Layout>(kind: ChosenKind, opt: Option<Opt<L>>) -> ChosenValue<L> {
    match opt {
        Some(Opt::Number(n)) => ChosenValue::of_number(kind, n),
        _ => None,
    }
    .expect("an answer among a catalog's numbers")
}
