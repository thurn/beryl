//! Mana: mana abilities, the payability prefilter and the payment solver (architecture §10,
//! decision D7).
//!
//! [CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064): "When an effect instructs a
//! player to add mana, that mana goes into a player's mana pool. From there, it can be used to
//! pay costs immediately, or it can stay in the player's mana pool as unspent mana."
//!
//! The solver works over **sources**: permanents whose mana abilities cost only {T} and do
//! nothing but add mana, the lands and rocks of the cube, and those whose mana ability costs
//! {T} and sacrificing them, the Treasures. Sources that make the same mana and have no other
//! ability are interchangeable, so they form a **class**, and a plan says how many of each
//! class it taps and how much floating mana of each kind it spends. Plans that leave the same
//! mana floating and sacrifice as many sources differ only in which sources they tap: the
//! solver picks one. Plans that leave different mana floating are a decision, and so is
//! sacrificing more, when it keeps more later costs payable (§10).

mod add;
mod lookahead;
mod pay;
mod pool;
mod solver;
mod sources;
mod units;

#[cfg(doc)]
use crate::card::ManaColor;
use crate::layout::Layout;
#[cfg(doc)]
use crate::layout::Small;

pub use add::add;
pub(crate) use add::{add_instruction, add_of_kind_instruction};
pub(crate) use pay::{
    activate, manual_options, manual_pay, pay, standalone_options, waterbend_mana_options,
};
pub(crate) use pool::{Funds, payable};
#[cfg(test)]
pub(crate) use solver::Plan;
pub(crate) use solver::plans;
pub use sources::mana_contribution;
#[cfg(test)]
pub(crate) use units::Units;

/// The kinds of mana, W U B R G and C, as [`ManaColor`] indices.
const KINDS: usize = 6;
/// Classes of interchangeable sources one solve considers in [`Small`]; [`crate::Large`] holds
/// any number.
const CLASSES: usize = 16;

/// How many sources of each class a plan taps, an entry per class: in a layout that holds more
/// classes than [`Small`], an entry per slot, as every class has a member.
type Taps<L> = <L as Layout>::Table<u8, CLASSES>;
