//! Players, with the mutators that write them (architecture §4.4; the crate's contract,
//! "Mutators"): one for each thing the rules do to a player. No `&mut` to a player leaves the
//! crate.
//!
//! Implements: none. The player record stores values read and written by the rules.

use beryl_card::ManaColor;

use crate::event::{Event, Recipient};
use crate::fit::{Misfit, fits_number};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Number, Queue, Small, Table};
use crate::mana_memory::{REMEMBERED_MANA, RememberedMana};
use crate::object::ObjectRef;
use crate::relayout::number;
use crate::state::State;
use crate::static_ability::Inputs;

/// Where a player is in the London mulligan (see CR 103.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MulliganStatus {
    /// Yet to declare this round.
    #[default]
    Undeclared,
    /// Declared a mulligan this round, not yet taken.
    WillMulligan,
    /// Kept: the hand is the opening hand.
    Kept,
    /// Declared a free mulligan this round, not yet taken.
    WillFreeMulligan,
}

/// Unspent mana, an amount of each [`ManaColor`]
/// (see [CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect instructs
/// a player to add mana, that mana goes into a player's mana pool."
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ManaPool<L: Layout = Small>([L::Mana; 6]);

impl<L: Layout> ManaPool<L> {
    /// The amounts, indexed as [`ManaColor::ALL`].
    #[inline]
    pub fn amounts(self) -> [L::Mana; 6] {
        self.0
    }

    /// The amounts as bytes, saturating at `u8::MAX`, indexed as [`ManaColor::ALL`].
    #[inline]
    pub fn amounts_u8(self) -> [u8; 6] {
        use crate::layout::UnsignedNumber;
        self.0.map(UnsignedNumber::to_u8_saturating)
    }

    /// Whether every amount in this pool fits layout `M`.
    pub(crate) fn fits<M: Layout>(self) -> Result<(), Misfit> {
        for amount in self.0 {
            fits_number::<M::Mana>("unspent mana", amount.to_i64())?;
        }
        Ok(())
    }

    /// This pool in layout `M`.
    pub(crate) fn relayout<M: Layout>(self) -> ManaPool<M> {
        ManaPool(self.0.map(number))
    }
}

impl<L: Layout> std::ops::Index<ManaColor> for ManaPool<L> {
    type Output = L::Mana;
    #[inline]
    fn index(&self, color: ManaColor) -> &L::Mana {
        &self.0[color.index()]
    }
}

impl<L: Layout> std::ops::IndexMut<ManaColor> for ManaPool<L> {
    #[inline]
    fn index_mut(&mut self, color: ManaColor) -> &mut L::Mana {
        &mut self.0[color.index()]
    }
}

/// One player (architecture §4.4).
///
/// Written only by [`State`]'s mutators: nothing outside the crate reaches one to write. The
/// fields are `pub(crate)`: other crates read them through the getters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player<L: Layout = Small> {
    pub(crate) life: L::Life,
    /// Unspent mana.
    pub(crate) mana: ManaPool<L>,
    /// Unspent mana that remembers what it may be spent on ([`crate::mana_memory`]), oldest
    /// first: none in a game whose cards make none.
    pub(crate) remembered: L::Queue<RememberedMana<L>, REMEMBERED_MANA>,
    pub(crate) lands_played: u8,
    /// Attempted to draw from an empty library since state-based actions were last checked
    /// (see CR 704.5b).
    pub(crate) drew_from_empty: bool,
    pub(crate) mulligans: u8,
    pub(crate) mulligan: MulliganStatus,
    /// Cards still to put on the bottom of the hand just kept, one per mulligan taken.
    pub(crate) to_bottom: u8,
    /// The emblems the player has, as game indices
    /// (see [CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)): "An emblem is a
    /// marker used to represent an object that has one or more abilities, but usually no
    /// other characteristics."
    pub(crate) emblems: L::Queue<L::Card, 4>,
}

impl Copy for Player<Small> {}

/// The fields in declaration order, as a derived `Hash` feeds them, with the mana that
/// remembers fed only when there is some: a game whose cards make none hashes as it did
/// before pools could hold it, so the state hashes its tests pin stay.
impl<L: Layout> std::hash::Hash for Player<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let Player {
            life,
            mana,
            remembered,
            lands_played,
            drew_from_empty,
            mulligans,
            mulligan,
            to_bottom,
            emblems,
        } = self;
        life.hash(h);
        mana.hash(h);
        lands_played.hash(h);
        drew_from_empty.hash(h);
        mulligans.hash(h);
        mulligan.hash(h);
        to_bottom.hash(h);
        emblems.hash(h);
        if !remembered.is_empty() {
            remembered.as_slice().hash(h);
        }
    }
}

impl<L: Layout> Player<L> {
    /// The life total (see [CR 119.1](https://yawgatog.com/resources/magic-rules/#R1191)): "Each
    /// player begins the game with a starting life total of 20."
    #[inline]
    pub fn life(&self) -> L::Life {
        self.life
    }

    /// The unspent mana in the player's mana pool (see CR 106.4).
    #[inline]
    pub fn mana(&self) -> ManaPool<L> {
        self.mana
    }

    /// The unspent mana that remembers what it may be spent on, oldest first (see CR 106.6).
    #[inline]
    pub fn remembered_mana(&self) -> &[RememberedMana<L>] {
        self.remembered.as_slice()
    }

    /// The lands the player has played this turn
    /// (see [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052)): "A player can
    /// normally play one land during their turn".
    #[inline]
    pub fn lands_played(&self) -> u8 {
        self.lands_played
    }

    /// Whether the player attempted to draw from an empty library since state-based actions
    /// were last checked (see [CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)):
    /// "If a player attempted to draw a card from a library with no cards in it since the
    /// last time state-based actions were checked, that player loses the game."
    #[inline]
    pub fn drew_from_empty(&self) -> bool {
        self.drew_from_empty
    }

    /// The mulligans the player has taken (see CR 103.5).
    #[inline]
    pub fn mulligans(&self) -> u8 {
        self.mulligans
    }

    /// Where the player is in the mulligan process (see CR 103.5).
    #[inline]
    pub fn mulligan(&self) -> MulliganStatus {
        self.mulligan
    }

    /// The cards the player has still to put on the bottom of their library from the hand
    /// they kept (see CR 103.5).
    #[inline]
    pub fn to_bottom(&self) -> u8 {
        self.to_bottom
    }

    /// The emblems the player has, as game indices, oldest first
    /// (see [CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)).
    #[inline]
    pub fn emblems(&self) -> &[L::Card] {
        self.emblems.as_slice()
    }
}

impl<L: Layout> State<L> {
    /// The one way to a player to write, which never leaves the crate.
    #[inline]
    pub(crate) fn player_mut(&mut self, player: Seat) -> &mut Player<L> {
        &mut self.players[player.index()]
    }

    /// A player gains life (see [CR 119.3](https://yawgatog.com/resources/magic-rules/#R1193)):
    /// "If an effect causes a player to gain life or lose life, that player's life total is
    /// adjusted accordingly." [`Full`] if the life total would overflow [`Layout::Life`].
    #[inline]
    pub fn gain_life(&mut self, player: Seat, amount: u8) -> Result<Event<L>, Full> {
        let life = &mut self.player_mut(player).life;
        *life = life.checked_add(L::Life::from(amount)).ok_or(Full)?;
        self.life_changed();
        Ok(Event::LifeGain { player, amount })
    }

    /// Marks the layer results stale while a static ability on the battlefield compares a
    /// life total ([`Inputs::LIFE`]).
    #[inline]
    pub(crate) fn life_changed(&mut self) {
        if self.layer_inputs.contains(Inputs::LIFE) {
            self.flags.derived_dirty = true;
        }
    }

    /// Whether `player` has the life to pay `amount`
    /// (see [CR 119.4](https://yawgatog.com/resources/magic-rules/#R1194)): "If a cost or effect
    /// allows a player to pay an amount of life greater than 0, the player may do so only if
    /// their life total is greater than or equal to the amount of the payment." And
    /// (see [CR 119.4b](https://yawgatog.com/resources/magic-rules/#R1194b)): "Players can always
    /// pay 0 life, no matter what their (or their team's) life total is, and even if an effect
    /// says players can't pay life." This is only half of whether life can be paid: an effect can
    /// also stop a player's life total from changing, which the kernel can't see. Rules code asks
    /// `beryl_rules::game::life::can_pay`, which asks both.
    #[inline]
    #[must_use]
    pub fn has_life_to_pay(&self, player: Seat, amount: u8) -> bool {
        amount == 0 || self.player(player).life() >= L::Life::from(amount)
    }

    /// A player loses life (see CR 119.3), as one who pays life does
    /// (see [CR 119.4](https://yawgatog.com/resources/magic-rules/#R1194)). [`Full`] if the life
    /// total would underflow [`Layout::Life`].
    #[inline]
    pub fn lose_life(&mut self, player: Seat, amount: i16) -> Result<Event<L>, Full> {
        debug_assert!(amount >= 0);
        let life = &mut self.player_mut(player).life;
        *life = life.checked_sub(L::Life::from(amount)).ok_or(Full)?;
        self.life_changed();
        Ok(Event::LifeLoss { player, amount })
    }

    /// Deals damage to a player
    /// (see [CR 120.3a](https://yawgatog.com/resources/magic-rules/#R1203a)): "Damage dealt to a
    /// player by a source without infect causes that player to lose that much life."
    /// [`State::mark_damage`] is the same for a permanent. With `loses_life` false the damage is
    /// dealt and the life total stays, as for a player whose life total can't change: the damage
    /// still has its other results, such as lifelink's gain. [`Full`] if the life total would
    /// underflow [`Layout::Life`].
    #[inline]
    pub fn damage_player(
        &mut self,
        source: ObjectRef<L>,
        player: Seat,
        amount: u8,
        combat: bool,
        loses_life: bool,
    ) -> Result<Event<L>, Full> {
        if loses_life {
            let life = &mut self.player_mut(player).life;
            *life = life.checked_sub(L::Life::from(amount)).ok_or(Full)?;
            self.life_changed();
        }
        Ok(Event::Damage {
            source,
            to: Recipient::Player(player),
            amount,
            combat,
        })
    }

    /// Adds mana to a player's mana pool, from `source` if an object produced it
    /// (see [CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
    /// instructs a player to add mana, that mana goes into a player's mana pool." [`Full`] if
    /// the pool's amount for `color` would overflow [`Layout::Mana`].
    #[inline]
    pub fn add_mana(
        &mut self,
        player: Seat,
        source: Option<ObjectRef<L>>,
        color: ManaColor,
        amount: u8,
    ) -> Result<Event<L>, Full> {
        let pool = &mut self.player_mut(player).mana[color];
        *pool = pool.checked_add(L::Mana::from(amount)).ok_or(Full)?;
        Ok(Event::Mana {
            player,
            source,
            color,
            amount,
            restricted: false,
        })
    }

    /// Spends mana a player's pool holds
    /// (see [CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "From there, it can
    /// be used to pay costs immediately, or it can stay in the player's mana pool as unspent
    /// mana."
    #[inline]
    pub fn spend_mana(&mut self, player: Seat, color: ManaColor, amount: u8) {
        let pool = &mut self.player_mut(player).mana[color];
        let take = L::Mana::from(amount);
        debug_assert!(*pool >= take, "spending mana the pool lacks");
        *pool -= take;
    }

    /// Empties every player's mana pool
    /// (see [CR 500.5](https://yawgatog.com/resources/magic-rules/#R5005)): "As a step or phase
    /// ends ... any unspent mana left in a player's mana pool empties."
    #[inline]
    pub fn empty_mana_pools(&mut self) {
        for p in self.players.as_mut_slice() {
            p.mana = ManaPool::default();
            p.remembered.clear();
        }
    }

    /// Counts a land a player played this turn
    /// (see [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052)): "A player can
    /// normally play one land during their turn". [`State::begin_turn`] begins the count
    /// again.
    #[inline]
    pub fn count_land_played(&mut self, player: Seat) {
        self.player_mut(player).lands_played += 1;
    }

    /// A player declares that they will take a mulligan
    /// (see [CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)): "First, the starting
    /// player declares whether they will take a mulligan."
    #[inline]
    pub fn declare_mulligan(&mut self, player: Seat) {
        self.player_mut(player).mulligan = MulliganStatus::WillMulligan;
    }

    /// A player declares that they will take a free mulligan this round, which redraws without
    /// counting against their opening hand size (`Format::SHARED_DECK`).
    #[inline]
    pub fn declare_free_mulligan(&mut self, player: Seat) {
        self.player_mut(player).mulligan = MulliganStatus::WillFreeMulligan;
    }

    /// A player keeps their hand, with `to_bottom` of its cards still to put on the bottom of
    /// their library (see [CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)): "Once a
    /// player chooses not to take a mulligan, the remaining cards become that player's
    /// opening hand".
    #[inline]
    pub fn keep_hand(&mut self, player: Seat, to_bottom: u8) {
        let p = self.player_mut(player);
        p.mulligan = MulliganStatus::Kept;
        p.to_bottom = to_bottom;
    }

    /// A player has put one of the cards they owe on the bottom of their library (see CR 103.5).
    #[inline]
    pub fn count_bottomed(&mut self, player: Seat) {
        self.player_mut(player).to_bottom -= 1;
    }

    /// A player has taken the mulligan they declared, and is to declare again
    /// (see [CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)): "This process is then
    /// repeated until no player takes a mulligan."
    #[inline]
    pub fn count_mulligan(&mut self, player: Seat) {
        let p = self.player_mut(player);
        p.mulligans += 1;
        p.mulligan = MulliganStatus::Undeclared;
    }

    /// A player has taken the free mulligan they declared, and is to declare again without
    /// incrementing their mulligan count (`Format::SHARED_DECK`).
    #[inline]
    pub fn finish_free_mulligan(&mut self, player: Seat) {
        self.player_mut(player).mulligan = MulliganStatus::Undeclared;
    }

    /// Gives a player an emblem
    /// (see [CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142)): "'[Player] gets an
    /// emblem with [ability].' This means that [player] puts an emblem with [ability] into
    /// the command zone. The emblem is both owned and controlled by that player."
    #[inline]
    pub fn add_emblem(&mut self, player: Seat, card: L::Card) -> Result<(), Full> {
        self.player_mut(player).emblems.push(card)
    }
}
