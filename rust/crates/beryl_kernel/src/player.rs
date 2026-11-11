//! Players, with the mutators that write them (architecture §4.4; the crate's contract,
//! "Mutators"): one for each thing the rules do to a player. No `&mut` to a player leaves the
//! crate.

use beryl_card::ManaColor;

use crate::event::{Event, Recipient};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small, Table};
use crate::object::ObjectRef;
use crate::state::State;

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
/// a player to add mana, that mana goes into a player's mana pool."
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
/// Written only by [`State`]'s mutators: nothing outside the crate reaches one to write. The
/// fields are `pub(crate)`: other crates read them through the getters.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Player<L: Layout = Small> {
    pub(crate) life: i16,
    /// Unspent mana.
    pub(crate) mana: ManaPool,
    pub(crate) lands_played: u8,
    /// Attempted to draw from an empty library since state-based actions were last checked
    /// (CR 704.5b).
    pub(crate) drew_from_empty: bool,
    pub(crate) mulligans: u8,
    pub(crate) mulligan: MulliganStatus,
    /// Cards still to put on the bottom of the hand just kept, one per mulligan taken.
    pub(crate) to_bottom: u8,
    /// The emblems the player has, as game indices
    /// ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)): "An emblem is a
    /// marker used to represent an object that has one or more abilities, but usually no
    /// other characteristics."
    pub(crate) emblems: L::Queue<L::Card, 4>,
    /// "Until your next turn, you may cast sorcery spells as though they had flash." (Teferi,
    /// Time Raveler) Milestone 4 (by-w8gv.7.1) deletes it: written only through
    /// [`State::legacy_mut`].
    pub(crate) sorceries_as_flash: bool,
}

impl Copy for Player<Small> {}

impl<L: Layout> Player<L> {
    /// The life total ([CR 119.1](https://yawgatog.com/resources/magic-rules/#R1191)): "Each
    /// player begins the game with a starting life total of 20."
    #[inline]
    pub fn life(&self) -> i16 {
        self.life
    }

    /// The unspent mana in the player's mana pool (CR 106.4).
    #[inline]
    pub fn mana(&self) -> ManaPool {
        self.mana
    }

    /// The lands the player has played this turn
    /// ([CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052)): "A player can
    /// normally play one land during their turn".
    #[inline]
    pub fn lands_played(&self) -> u8 {
        self.lands_played
    }

    /// Whether the player attempted to draw from an empty library since state-based actions
    /// were last checked ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)):
    /// "If a player attempted to draw a card from a library with no cards in it since the
    /// last time state-based actions were checked, that player loses the game."
    #[inline]
    pub fn drew_from_empty(&self) -> bool {
        self.drew_from_empty
    }

    /// The mulligans the player has taken (CR 103.5).
    #[inline]
    pub fn mulligans(&self) -> u8 {
        self.mulligans
    }

    /// Where the player is in the mulligan process (CR 103.5).
    #[inline]
    pub fn mulligan(&self) -> MulliganStatus {
        self.mulligan
    }

    /// The cards the player has still to put on the bottom of their library from the hand
    /// they kept (CR 103.5).
    #[inline]
    pub fn to_bottom(&self) -> u8 {
        self.to_bottom
    }

    /// The emblems the player has, as game indices, oldest first
    /// ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)).
    #[inline]
    pub fn emblems(&self) -> &[L::Card] {
        self.emblems.as_slice()
    }

    /// Whether the player may cast sorcery spells as though they had flash. Milestone 4
    /// (by-w8gv.7.1) deletes it.
    #[inline]
    pub fn sorceries_as_flash(&self) -> bool {
        self.sorceries_as_flash
    }
}

impl<L: Layout> State<L> {
    /// The one way to a player to write, which never leaves the crate.
    #[inline]
    pub(crate) fn player_mut(&mut self, player: Seat) -> &mut Player<L> {
        &mut self.players[player.index()]
    }

    /// A player gains life ([CR 119.3](https://yawgatog.com/resources/magic-rules/#R1193)):
    /// "If an effect causes a player to gain life or lose life, that player's life total is
    /// adjusted accordingly."
    #[inline]
    #[must_use]
    pub fn gain_life(&mut self, player: Seat, amount: u8) -> Event<L> {
        self.player_mut(player).life += i16::from(amount);
        Event::LifeGain { player, amount }
    }

    /// A player loses life (CR 119.3), as one who pays life does
    /// ([CR 119.4](https://yawgatog.com/resources/magic-rules/#R1194)).
    #[inline]
    #[must_use]
    pub fn lose_life(&mut self, player: Seat, amount: u8) -> Event<L> {
        self.player_mut(player).life -= i16::from(amount);
        Event::LifeLoss { player, amount }
    }

    /// Deals damage to a player
    /// ([CR 120.3a](https://yawgatog.com/resources/magic-rules/#R1203a)): "Damage dealt to a
    /// player by a source without infect causes that player to lose that much life."
    /// [`State::mark_damage`] is the same for a permanent.
    #[inline]
    #[must_use]
    pub fn damage_player(
        &mut self,
        source: ObjectRef<L>,
        player: Seat,
        amount: u8,
        combat: bool,
    ) -> Event<L> {
        self.player_mut(player).life -= i16::from(amount);
        Event::Damage {
            source,
            to: Recipient::Player(player),
            amount,
            combat,
        }
    }

    /// Adds mana to a player's mana pool, from `source` if an object produced it
    /// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
    /// instructs a player to add mana, that mana goes into a player's mana pool."
    #[inline]
    #[must_use]
    pub fn add_mana(
        &mut self,
        player: Seat,
        source: Option<ObjectRef<L>>,
        color: ManaColor,
        amount: u8,
    ) -> Event<L> {
        let pool = &mut self.player_mut(player).mana[color];
        *pool = pool.saturating_add(amount);
        Event::Mana {
            player,
            source,
            color,
            amount,
        }
    }

    /// Spends mana a player's pool holds
    /// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "From there, it can
    /// be used to pay costs immediately, or it can stay in the player's mana pool as unspent
    /// mana."
    #[inline]
    pub fn spend_mana(&mut self, player: Seat, color: ManaColor, amount: u8) {
        let pool = &mut self.player_mut(player).mana[color];
        debug_assert!(*pool >= amount, "spending mana the pool lacks");
        *pool -= amount;
    }

    /// Empties every player's mana pool
    /// ([CR 500.5](https://yawgatog.com/resources/magic-rules/#R5005)): "As a step or phase
    /// ends ... any unspent mana left in a player's mana pool empties."
    #[inline]
    pub fn empty_mana_pools(&mut self) {
        for p in self.players.as_mut_slice() {
            p.mana = ManaPool::default();
        }
    }

    /// Counts a land a player played this turn
    /// ([CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052)): "A player can
    /// normally play one land during their turn". [`State::begin_turn`] begins the count
    /// again.
    #[inline]
    pub fn count_land_played(&mut self, player: Seat) {
        self.player_mut(player).lands_played += 1;
    }

    /// A player declares that they will take a mulligan
    /// ([CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)): "First, the starting
    /// player declares whether they will take a mulligan."
    #[inline]
    pub fn declare_mulligan(&mut self, player: Seat) {
        self.player_mut(player).mulligan = MulliganStatus::WillMulligan;
    }

    /// A player keeps their hand, with `to_bottom` of its cards still to put on the bottom of
    /// their library ([CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)): "Once a
    /// player chooses not to take a mulligan, the remaining cards become that player's
    /// opening hand".
    #[inline]
    pub fn keep_hand(&mut self, player: Seat, to_bottom: u8) {
        let p = self.player_mut(player);
        p.mulligan = MulliganStatus::Kept;
        p.to_bottom = to_bottom;
    }

    /// A player has put one of the cards they owe on the bottom of their library (CR 103.5).
    #[inline]
    pub fn count_bottomed(&mut self, player: Seat) {
        self.player_mut(player).to_bottom -= 1;
    }

    /// A player has taken the mulligan they declared, and is to declare again
    /// ([CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)): "This process is then
    /// repeated until no player takes a mulligan."
    #[inline]
    pub fn count_mulligan(&mut self, player: Seat) {
        let p = self.player_mut(player);
        p.mulligans += 1;
        p.mulligan = MulliganStatus::Undeclared;
    }

    /// Gives a player an emblem
    /// ([CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142)): "'[Player] gets an
    /// emblem with [ability].' This means that [player] puts an emblem with [ability] into
    /// the command zone. The emblem is both owned and controlled by that player."
    #[inline]
    pub fn add_emblem(&mut self, player: Seat, card: L::Card) -> Result<(), Full> {
        self.player_mut(player).emblems.push(card)
    }
}
