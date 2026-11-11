//! Players and the turn (architecture §4.4).
//!
//! Transitional: by-w8gv.5.7 writes players and the turn only through the kernel's mutators
//! and makes [`State::player_mut`](crate::State::player_mut) private.

use beryl_card::{ManaColor, TurnStep};

use crate::ids::{CardIx, Seat};
use crate::layout::{Layout, Small};

/// Where a player is in the London mulligan (CR 103.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MulliganStatus {
    /// Yet to declare this round.
    #[default]
    Undeclared,
    /// Declared a mulligan this round, not yet taken.
    WillMulligan,
    /// Kept: the hand is the opening hand.
    Kept,
}

/// Unspent mana, an amount of each [`ManaColor`]
/// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect instructs
/// a player to add mana, that mana goes into a mana pool."
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ManaPool([u8; 6]);

impl ManaPool {
    /// The amounts, indexed as [`ManaColor::ALL`].
    #[inline]
    pub fn amounts(self) -> [u8; 6] {
        self.0
    }
}

impl std::ops::Index<ManaColor> for ManaPool {
    type Output = u8;
    #[inline]
    fn index(&self, color: ManaColor) -> &u8 {
        &self.0[color.index()]
    }
}

impl std::ops::IndexMut<ManaColor> for ManaPool {
    #[inline]
    fn index_mut(&mut self, color: ManaColor) -> &mut u8 {
        &mut self.0[color.index()]
    }
}

/// One player (architecture §4.4).
///
/// Transitional: the fields are `pub`. by-w8gv.5.7 makes every write a mutator of the
/// kernel's, and by-w8gv.5.9 makes the fields private behind getters.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Player<L: Layout = Small> {
    pub life: i16,
    /// Unspent mana.
    pub mana: ManaPool,
    pub lands_played: u8,
    /// Attempted to draw from an empty library since state-based actions were last checked
    /// (CR 704.5b).
    pub drew_from_empty: bool,
    pub mulligans: u8,
    pub mulligan: MulliganStatus,
    /// Cards still to put on the bottom of the hand just kept, one per mulligan taken.
    pub to_bottom: u8,
    /// The emblems the player has, as game indices
    /// ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)): "An emblem is a
    /// marker used to represent an object that has one or more abilities, but usually no
    /// other characteristics."
    pub emblems: L::Queue<CardIx, 4>,
    /// "Until your next turn, you may cast sorcery spells as though they had flash." (Teferi,
    /// Time Raveler)
    pub sorceries_as_flash: bool,
}

impl Copy for Player<Small> {}

/// Where the game is in its turns (architecture §4.4).
///
/// Transitional: the fields are `pub`. by-w8gv.5.7 makes every write a mutator of the
/// kernel's, and by-w8gv.5.9 makes the fields private behind getters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Turn {
    /// 0 before the first turn.
    pub number: u16,
    pub active: Seat,
    pub starting_player: Seat,
    pub step: TurnStep,
    /// The player with priority.
    pub priority: Seat,
    /// Players who have passed in succession (CR 117.4).
    pub passes: u8,
    /// The cleanup step is checking for state-based actions (CR 514.3a): if none are
    /// performed, the turn ends instead of a player receiving priority.
    pub cleanup_check: bool,
    /// Creatures were declared as attackers this combat, so the declare blockers and combat
    /// damage steps happen even if none is attacking any more
    /// ([CR 508.8](https://yawgatog.com/resources/magic-rules/#R5088)).
    pub attacked: bool,
}
