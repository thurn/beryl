//! The suspended games and separate zones of a game played within another game.
//!
//! Implements: CR 730.1a, CR 730.1b, CR 730.2, CR 730.6

use crate::fixed::Full;
use crate::format::Format;
use crate::layout::{Large, Layout, Piles, Table};
use crate::object::Object;
use crate::rng::Rng;
use crate::state::{State, Zone};

/// The layout's storage for a suspended game.
pub trait OuterGame<L: Layout>: Clone + Default + std::fmt::Debug + Eq + Send + Sync {
    const SUPPORTED: bool;
    fn get(&self) -> Option<&State<L>>;
    fn get_mut(&mut self) -> Option<&mut State<L>>;
    fn put(&mut self, state: State<L>);
    fn take(&mut self) -> Option<State<L>>;
}

impl<L: Layout> OuterGame<L> for () {
    const SUPPORTED: bool = false;

    #[inline]
    fn get(&self) -> Option<&State<L>> {
        None
    }

    fn get_mut(&mut self) -> Option<&mut State<L>> {
        None
    }

    fn put(&mut self, _: State<L>) {
        panic!("a subgame requires the Large layout");
    }

    #[inline]
    fn take(&mut self) -> Option<State<L>> {
        None
    }
}

/// The heap-owned suspended state of a Large game.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LargeOuter(Option<Box<State<Large>>>);

impl OuterGame<Large> for LargeOuter {
    const SUPPORTED: bool = true;

    fn get(&self) -> Option<&State<Large>> {
        self.0.as_deref()
    }

    fn get_mut(&mut self) -> Option<&mut State<Large>> {
        self.0.as_deref_mut()
    }

    fn put(&mut self, state: State<Large>) {
        self.0 = Some(Box::new(state));
    }

    fn take(&mut self) -> Option<State<Large>> {
        self.0.take().map(|state| *state)
    }
}

/// The turns spent in subgames that have already ended. Small has no such turns.
pub trait CompletedTurns: Clone + Default + std::fmt::Debug + Eq + Send + Sync {
    fn get(&self) -> u16;
    fn from_turns(turns: u16) -> Self;
}

impl CompletedTurns for () {
    #[inline]
    fn get(&self) -> u16 {
        0
    }

    fn from_turns(turns: u16) {
        assert_eq!(turns, 0, "subgame turns require the Large layout");
    }
}

impl CompletedTurns for u16 {
    fn get(&self) -> u16 {
        *self
    }

    fn from_turns(turns: u16) -> Self {
        turns
    }
}

impl<L: Layout> State<L> {
    /// Whether this layout can suspend a game to play another one.
    pub const SUPPORTS_SUBGAMES: bool = L::Outer::SUPPORTED;

    /// Whether the active game is being played inside a suspended game.
    #[inline]
    pub fn in_subgame(&self) -> bool {
        self.outer.get().is_some()
    }

    /// The suspended game, for checking invariants and saving the entire position.
    #[doc(hidden)]
    pub fn suspended_game(&self) -> Option<&Self> {
        self.outer.get()
    }

    /// The number of games suspended beneath the active one.
    pub fn subgame_depth(&self) -> usize {
        self.outer
            .get()
            .map_or(0, |outer| 1 + outer.subgame_depth())
    }

    /// Turns started, excluding games that ended by restarting. Subgames share the turn
    /// budget with their suspended games.
    #[inline]
    pub fn turns_for_limit(&self) -> u16 {
        self.turn
            .number()
            .saturating_add(self.subgame_turns.get())
            .saturating_add(self.outer.get().map_or(0, Self::turns_for_limit))
    }

    /// Turns started across this playthrough, including suspended, completed and restarted
    /// games. Used where time must advance across game boundaries.
    #[inline]
    pub fn elapsed_turns(&self) -> u16 {
        self.turn
            .number()
            .saturating_add(self.restarted_turns)
            .saturating_add(self.subgame_turns.get())
            .saturating_add(self.outer.get().map_or(0, Self::elapsed_turns))
    }

    /// A restart has contributed turns to this game's history. If the game is still
    /// in its opening, it may have carried cards waiting in exile.
    pub fn restarted(&self) -> bool {
        self.restarted_turns != 0
    }

    /// Deals a fresh game from the libraries as they stand at the instruction, over the
    /// same slots. The committed game to suspend may be earlier in the resolution than
    /// this position, since resolution replays its instructions.
    ///
    /// Under [CR 730.2], "Each player takes all the cards in their main-game library, moves
    /// them to their subgame library, and shuffles them."
    /// Under [CR 730.1b], "No effects or definitions created in either the main game or the
    /// subgame have any meaning in the other, except as defined by the effect that created
    /// the subgame."
    ///
    /// A subgame of a game of the shared-deck format is one too, dealt from the shared library
    /// once and shuffled once, and its cards have no owner there. When it ends the main game
    /// resumes as it was suspended, its shared library among it, and the rules put the cards
    /// back into it ([`State::leave_subgame`]).
    ///
    /// [CR 730.2]: https://yawgatog.com/resources/magic-rules/#R7302
    /// [CR 730.1b]: https://yawgatog.com/resources/magic-rules/#R7301b
    pub fn deal_subgame(&self, format: Format) -> Result<Self, Full> {
        if !Self::SUPPORTS_SUBGAMES {
            return Err(Full);
        }
        let mut inner = Self::empty_in(
            format,
            crate::profile::Profile {
                breakpoint: None,
                ..self.profile
            },
        );
        let mut seed = self.rng.other;
        inner.rng = Rng::new(seed.next_u64());
        let shared = self.shares_zones();
        debug_assert_eq!(shared, inner.shares_zones(), "a subgame of another format");
        for player in self.zone_owners(Zone::Library).in_seat_order() {
            let p = player.index();
            for &slot in self.library(player) {
                *inner.object_mut(slot) = Object {
                    card: self.card_really_is(slot),
                    zone: Zone::Library,
                    owner: player,
                    base_controller: player,
                    ..Object::EMPTY
                };
                // The library already fit this layout in the game it came from.
                if shared {
                    inner.zones.library.push_shared(slot)?;
                    for members in inner.zones.members.as_mut_slice() {
                        members[Zone::Library.index()].insert(slot);
                    }
                } else {
                    inner.zones.library.push(p, slot)?;
                    inner.zones.members[p][Zone::Library.index()].insert(slot);
                }
            }
            inner.shuffle_library(player)?;
        }
        Ok(inner)
    }

    /// Advances the parent game's random stream past the seed used to deal a subgame.
    pub fn spend_subgame_seed(&mut self) {
        self.rng.other.skip(1);
    }

    /// Installs a newly dealt subgame and suspends the old game, which can itself be a
    /// subgame. Charge the transition to the decision budget even for a forced resolution.
    ///
    /// Under [CR 730.1a], "The main game is temporarily discontinued while the subgame is in
    /// progress. It resumes when the subgame ends."
    /// Under [CR 730.6], "A subgame can be created within a subgame."
    ///
    /// [CR 730.1a]: https://yawgatog.com/resources/magic-rules/#R7301a
    /// [CR 730.6]: https://yawgatog.com/resources/magic-rules/#R7306
    pub fn enter_subgame(&mut self, inner: Self) -> Result<(), Full> {
        if !Self::SUPPORTS_SUBGAMES {
            return Err(Full);
        }
        self.count_decision();
        let outer = std::mem::replace(self, inner);
        self.budgets.decisions = outer.budgets.decisions;
        self.outer.put(outer);
        Ok(())
    }

    /// Restores the suspended game after the subgame ends, leaving the rules to resume
    /// the effect that suspended it.
    pub fn leave_subgame(&mut self) {
        let mut outer = self.outer.take().expect("a game to resume");
        let turns = self.turn.number().saturating_add(self.subgame_turns.get());
        outer.subgame_turns =
            L::SubgameTurns::from_turns(outer.subgame_turns.get().saturating_add(turns));
        outer.restarted_turns = outer.restarted_turns.saturating_add(self.restarted_turns);
        if self.turn.number() == 0 {
            outer.budgets.decisions = self.budgets.decisions;
        }
        outer.set_profile(crate::profile::Profile {
            breakpoint: outer.profile.breakpoint,
            ..self.profile
        });
        *self = outer;
    }
}

#[cfg(test)]
mod tests;
