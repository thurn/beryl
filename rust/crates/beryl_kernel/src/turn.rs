//! The turn and the game's own bookkeeping, with the mutators that write them (architecture
//! §4.4; the crate's contract, "Mutators"): where the game is in its turns, who holds
//! priority, the budgets of the profile, and how the game ended.
//!
//! The kernel moves the turn as it is told and holds none of its rules: which step follows
//! which, what a step's turn-based actions are and when a budget stalls the game are
//! `beryl_engine`'s. Each mutator keeps what must agree: passes with priority, the per-turn
//! counts with the turn.

use beryl_card::TurnStep;

use crate::bitboard::Bitboard;
use crate::decision::Outcome;
use crate::event::Event;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::{Budgets, State};

/// Where the game is in its turns (architecture §4.4).
///
/// Written only by [`State`]'s mutators: nothing outside the crate reaches one to write. The
/// fields are `pub(crate)`: other crates read them through the getters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Turn {
    /// 0 before the first turn.
    pub(crate) number: u16,
    pub(crate) active: Seat,
    pub(crate) starting_player: Seat,
    pub(crate) step: TurnStep,
    /// The player with priority.
    pub(crate) priority: Seat,
    /// Players who have passed in succession (CR 117.4).
    pub(crate) passes: u8,
    /// The cleanup step is checking for state-based actions (CR 514.3a): if none are
    /// performed, the turn ends instead of a player receiving priority.
    pub(crate) cleanup_check: bool,
    /// Creatures were declared as attackers this combat, so the declare blockers and combat
    /// damage steps happen even if none is attacking any more
    /// ([CR 508.8](https://yawgatog.com/resources/magic-rules/#R5088)).
    pub(crate) attacked: bool,
}

impl Turn {
    /// The turn's number: 0 before the first turn.
    #[inline]
    pub fn number(self) -> u16 {
        self.number
    }

    /// The active player ([CR 102.1](https://yawgatog.com/resources/magic-rules/#R1021)):
    /// "The active player is the player whose turn it is."
    #[inline]
    pub fn active(self) -> Seat {
        self.active
    }

    /// The starting player ([CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)):
    /// "The player chosen to take the first turn is the starting player."
    #[inline]
    pub fn starting_player(self) -> Seat {
        self.starting_player
    }

    /// The step or phase the game is in (CR 500.1).
    #[inline]
    pub fn step(self) -> TurnStep {
        self.step
    }

    /// The player with priority
    /// ([CR 117.1](https://yawgatog.com/resources/magic-rules/#R1171)): "which player can
    /// take actions at any given time is determined by a system of priority."
    #[inline]
    pub fn priority(self) -> Seat {
        self.priority
    }

    /// How many players have passed in succession
    /// ([CR 117.4](https://yawgatog.com/resources/magic-rules/#R1174)).
    #[inline]
    pub fn passes(self) -> u8 {
        self.passes
    }

    /// Whether the cleanup step is checking for state-based actions
    /// ([CR 514.3a](https://yawgatog.com/resources/magic-rules/#R5143a)): if none are
    /// performed, the turn ends instead of a player receiving priority.
    #[inline]
    pub fn cleanup_check(self) -> bool {
        self.cleanup_check
    }

    /// Whether creatures were declared as attackers this combat
    /// ([CR 508.8](https://yawgatog.com/resources/magic-rules/#R5088)).
    #[inline]
    pub fn attacked(self) -> bool {
        self.attacked
    }
}

impl<L: Layout> State<L> {
    /// Names the starting player, whose turn the first will be
    /// ([CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)): "The player chosen to
    /// take the first turn is the starting player."
    #[inline]
    pub fn set_starting_player(&mut self, starting: Seat) {
        self.turn.starting_player = starting;
        self.turn.active = starting;
    }

    /// Begins the next turn and returns its active player: the starting player's first, then
    /// alternating ([CR 103.8](https://yawgatog.com/resources/magic-rules/#R1038)): "The
    /// starting player takes their first turn." The per-turn counts begin again with it: the
    /// decisions offered, and the lands each player has played
    /// ([CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052)).
    #[inline]
    pub fn begin_turn(&mut self) -> Seat {
        self.turn.active = if self.turn.number == 0 {
            self.turn.starting_player
        } else {
            self.turn.active.opponent()
        };
        self.turn.number += 1;
        self.budgets.decisions = 0;
        for p in &mut self.players {
            p.lands_played = 0;
        }
        self.turn.active
    }

    /// Begins a step or phase, the active player to receive priority in it
    /// ([CR 117.3a](https://yawgatog.com/resources/magic-rules/#R1173a)): "The active player
    /// receives priority at the beginning of most steps and phases, after any turn-based
    /// actions (such as drawing a card during the draw step; see rule 703) have been dealt
    /// with".
    #[inline]
    #[must_use]
    pub fn begin_step(&mut self, step: TurnStep) -> Event<L> {
        self.turn.step = step;
        self.give_priority(self.turn.active);
        Event::StepBegins {
            step,
            active: self.turn.active,
        }
    }

    /// Gives a player priority, with no player having passed since
    /// ([CR 117.3c](https://yawgatog.com/resources/magic-rules/#R1173c)): "If a player has
    /// priority when they cast a spell, activate an ability, or take a special action, that
    /// player receives priority afterward."
    #[inline]
    pub fn give_priority(&mut self, player: Seat) {
        self.turn.priority = player;
        self.turn.passes = 0;
    }

    /// The player with priority passes it, and whether every player has now passed in
    /// succession ([CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d)): "If a
    /// player has priority and chooses not to take any actions, that player passes. ... the
    /// next player in turn order receives priority." Once all have passed, priority stays
    /// where it is for the rules to resolve the stack's top or end the step
    /// ([CR 117.4](https://yawgatog.com/resources/magic-rules/#R1174)).
    #[inline]
    pub fn pass_priority(&mut self) -> bool {
        self.turn.passes += 1;
        if self.turn.passes < 2 {
            self.turn.priority = self.turn.priority.opponent();
            return false;
        }
        true
    }

    /// Forgets the passes made in succession, priority staying where it is: something
    /// happened between them ([CR 117.4](https://yawgatog.com/resources/magic-rules/#R1174):
    /// "all players pass in succession (that is, if all players pass without taking any
    /// actions in between passing)").
    #[inline]
    pub fn reset_passes(&mut self) {
        self.turn.passes = 0;
    }

    /// The active player has declared `attackers`, each made an attacking creature already
    /// ([`State::declare_attacker`]), and the declaration is whole
    /// ([CR 508.1](https://yawgatog.com/resources/magic-rules/#R5081)). The combat's later
    /// steps then happen even if none is attacking any more
    /// ([CR 508.8](https://yawgatog.com/resources/magic-rules/#R5088)): "If no creatures are
    /// declared as attackers or put onto the battlefield attacking, skip the declare blockers
    /// and combat damage steps." [`State::end_combat`] forgets it.
    #[inline]
    #[must_use]
    pub fn declare_attack(&mut self, attackers: Bitboard<L::Slot>) -> Event<L> {
        self.turn.attacked = true;
        Event::Attack {
            player: self.turn.active,
            attackers,
        }
    }

    /// Whether the cleanup step is checking for state-based actions and waiting triggers
    /// ([CR 514.3a](https://yawgatog.com/resources/magic-rules/#R5143a)): "the game checks to
    /// see if any state-based actions would be performed and/or any triggered abilities are
    /// waiting to be put onto the stack ... If so, ... the active player gets priority."
    #[inline]
    pub fn set_cleanup_check(&mut self, checking: bool) {
        self.turn.cleanup_check = checking;
    }

    /// The counters toward the profile's budgets (architecture §12.3).
    #[inline]
    pub fn budgets(&self) -> Budgets {
        self.budgets
    }

    /// Counts a decision offered this turn; [`State::begin_turn`] begins the count again.
    #[inline]
    pub fn count_decision(&mut self) {
        self.budgets.decisions += 1;
    }

    /// Counts a state-based check that performed something, and returns how many in a row
    /// have ([CR 704.3](https://yawgatog.com/resources/magic-rules/#R7043): "If any
    /// state-based actions are performed as a result of a check, the check is repeated").
    #[inline]
    pub fn count_state_based_check(&mut self) -> u8 {
        self.budgets.state_based_iterations += 1;
        self.budgets.state_based_iterations
    }

    /// A state-based check performed nothing: none in a row has.
    #[inline]
    pub fn reset_state_based_checks(&mut self) {
        self.budgets.state_based_iterations = 0;
    }

    /// Ends the game ([CR 104.1](https://yawgatog.com/resources/magic-rules/#R1041): "A game
    /// ends immediately when a player wins, when the game is a draw, or when the game is
    /// restarted."), or gives it up as stalled. It returns no event: the rules announce a
    /// win or a loss to each player with its cause, and a stall is not something that
    /// happened in the game. A stall takes the place of an outcome already there, since the
    /// state that ran out of a capacity is abandoned for its wider copy (the crate's
    /// contract, "Mutators").
    #[inline]
    pub fn end_game(&mut self, outcome: Outcome) {
        self.outcome = Some(outcome);
    }
}
