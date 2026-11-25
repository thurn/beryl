//! The mana spent on an activated ability's X, kept on its stack entry for an effect that
//! reads it as it resolves, as Illusionary Mask's "a creature card in your hand whose mana cost
//! could be paid by some amount of, or all of, the mana you spent on {X}".
//!
//! Implements: none. Stores the amounts of each kind of mana spent on an ability's X.
//!
//! Only [`crate::Large`] holds it: [`crate::Small`]'s record is `()`, which holds only
//! "nothing spent" and refuses anything else with [`Full`], so a game records it by moving to
//! `Large` and a game whose cards never ask stores nothing in any stack entry.

use beryl_card::ManaColor;

use crate::fixed::Full;
use crate::layout::{Layout, Queue};
use crate::state::{StackEntry, State};

/// How much of each kind of mana was spent, by [`ManaColor`] index: W, U, B, R, G, C.
pub type ManaAmounts = [u8; 6];

/// A layout's record of the mana spent on an ability's X ([`Layout::Spent`]).
pub trait SpentMana: Copy + Default + std::fmt::Debug + Eq + std::hash::Hash + Send + Sync {
    /// Whether the record can hold any mana.
    const SUPPORTED: bool;

    /// The mana recorded.
    fn amounts(&self) -> ManaAmounts;

    /// The record of `amounts`, if this layout can hold it.
    fn of(amounts: ManaAmounts) -> Option<Self>;
}

/// [`crate::Small`]'s record: none.
impl SpentMana for () {
    const SUPPORTED: bool = false;

    #[inline]
    fn amounts(&self) -> ManaAmounts {
        [0; 6]
    }

    #[inline]
    fn of(amounts: ManaAmounts) -> Option<()> {
        (amounts == [0; 6]).then_some(())
    }
}

/// [`crate::Large`]'s record: the amounts themselves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Spent(ManaAmounts);

impl SpentMana for Spent {
    const SUPPORTED: bool = true;

    #[inline]
    fn amounts(&self) -> ManaAmounts {
        self.0
    }

    #[inline]
    fn of(amounts: ManaAmounts) -> Option<Spent> {
        Some(Spent(amounts))
    }
}

impl<L: Layout> StackEntry<L> {
    /// The mana spent on the ability's X as it was activated: nothing for a spell, for an
    /// ability whose effect never reads it, and for one activated with nothing spent on X.
    #[inline]
    pub fn spent_on_x(&self) -> ManaAmounts {
        self.spent.amounts()
    }
}

impl<L: Layout> State<L> {
    /// Records that `amount` mana of the kind `color` was spent on the X of the ability being
    /// activated, the top of the stack, as its cost is paid.
    ///
    /// [`Full`] in a layout that cannot hold it ([`SpentMana::SUPPORTED`]).
    pub fn record_spent_on_x(&mut self, color: ManaColor, amount: u8) -> Result<(), Full> {
        let entry = self
            .stack
            .as_mut_slice()
            .last_mut()
            .expect("the ability being activated");
        let mut amounts = entry.spent.amounts();
        amounts[color.index()] = amounts[color.index()].saturating_add(amount);
        entry.spent = L::Spent::of(amounts).ok_or(Full)?;
        Ok(())
    }
}
