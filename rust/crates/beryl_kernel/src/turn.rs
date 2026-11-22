//! The turn and the game's own bookkeeping, with the mutators that write them (architecture
//! §4.4; the crate's contract, "Mutators"): where the game is in its turns, who holds
//! priority, the budgets of the profile, and how the game ended.
//!
//! The kernel moves the turn as it is told and holds none of its rules: which step follows
//! which, what a step's turn-based actions are and when a budget stalls the game are
//! `beryl_engine`'s. Each mutator keeps what must agree: passes with priority, the per-turn
//! counts with the turn.
//!
//! Implements: none. The turn record stores progress through the turn and priority.

pub(crate) mod plan;

use beryl_card::TurnStep;

use crate::decision::Outcome;
use crate::event::Event;
use crate::fixed::Full;
use crate::format::Format;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Table};
use crate::reference::Permanents;
use crate::stack::{PendingTrigger, TriggerInfo};
use crate::state::{Budgets, State};

/// Where the game is in its turns (architecture §4.4).
///
/// Written only by [`State`]'s mutators: nothing outside the crate reaches one to write. The
/// fields are `pub(crate)`: other crates read them through the getters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Turn {
    /// 0 before the first turn.
    pub(crate) number: u16,
    pub(crate) active: Seat,
    pub(crate) starting_player: Seat,
    pub(crate) step: TurnStep,
    /// The player with priority.
    pub(crate) priority: Seat,
    /// Players who have passed in succession (see CR 117.4).
    pub(crate) passes: u8,
    /// The cleanup step is checking for state-based actions (see CR 514.3a): if none are
    /// performed, the turn ends instead of a player receiving priority.
    pub(crate) cleanup_check: bool,
    /// Creatures were declared as attackers this combat, so the declare blockers and combat
    /// damage steps happen even if none is attacking any more
    /// (see [CR 508.8](https://yawgatog.com/resources/magic-rules/#R5088)).
    pub(crate) attacked: bool,
    /// The player controlling the active player this turn, if another player does
    /// (see [CR 724.1]): "The affected player is controlled during the entire turn; the effect
    /// doesn't end until the beginning of the next turn." [`State::begin_turn`] sets it.
    ///
    /// [CR 724.1]: https://yawgatog.com/resources/magic-rules/#R7241
    pub(crate) controller: Option<Seat>,
    /// The seat the regular turn order continues from, when the last actual turn was extra
    /// or a regular turn was skipped (see [CR 500.7]): "Some effects can give
    /// a player extra turns. They do this by adding the turns directly after the specified
    /// turn." [`State::begin_turn`] sets it.
    ///
    /// [CR 500.7]: https://yawgatog.com/resources/magic-rules/#R5007
    pub(crate) after_extra: Option<Seat>,
    /// The starting player's first actual turn is still waiting after their opening turn was
    /// skipped, or is the current turn.
    pub(crate) starting_draw_later: bool,
}

/// Every field, but the controller and the turn an extra turn follows only where there is
/// one: a turn that is neither controlled nor extra hashes as it did before turns could be,
/// so the benchmark corpora pinned by their hashes still name the same positions.
impl std::hash::Hash for Turn {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let Turn {
            number,
            active,
            starting_player,
            step,
            priority,
            passes,
            cleanup_check,
            attacked,
            controller,
            after_extra,
            starting_draw_later,
        } = *self;
        (number, active, starting_player, step, priority).hash(h);
        (passes, cleanup_check, attacked).hash(h);
        if let Some(controller) = controller {
            controller.hash(h);
        }
        if let Some(regular) = after_extra {
            // Tagged, so that it hashes apart from a controller's seat.
            (u8::MAX, regular).hash(h);
        }
        if starting_draw_later {
            true.hash(h);
        }
    }
}

impl Turn {
    /// The turn's number: 0 before the first turn.
    #[inline]
    pub fn number(self) -> u16 {
        self.number
    }

    /// The active player (see [CR 102.1](https://yawgatog.com/resources/magic-rules/#R1021)):
    /// "The active player is the player whose turn it is."
    #[inline]
    pub fn active(self) -> Seat {
        self.active
    }

    /// The starting player (see [CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)):
    /// "The player chosen to take the first turn is the starting player."
    #[inline]
    pub fn starting_player(self) -> Seat {
        self.starting_player
    }

    /// Whether the draw step belongs to the starting player's first turn
    /// (see [CR 103.8a]: "the player who plays first skips the draw step").
    ///
    /// [CR 103.8a]: https://yawgatog.com/resources/magic-rules/#R1038a
    #[inline]
    pub fn is_starting_players_first_turn(self) -> bool {
        debug_assert!(self.number > 0);
        (self.number <= 1 || self.starting_draw_later) && self.active == self.starting_player
    }

    /// The step or phase the game is in (see CR 500.1).
    #[inline]
    pub fn step(self) -> TurnStep {
        self.step
    }

    /// The player with priority
    /// (see [CR 117.1](https://yawgatog.com/resources/magic-rules/#R1171)): "which player can
    /// take actions at any given time is determined by a system of priority."
    #[inline]
    pub fn priority(self) -> Seat {
        self.priority
    }

    /// How many players have passed in succession
    /// (see [CR 117.4](https://yawgatog.com/resources/magic-rules/#R1174)).
    #[inline]
    pub fn passes(self) -> u8 {
        self.passes
    }

    /// Whether the cleanup step is checking for state-based actions
    /// (see [CR 514.3a](https://yawgatog.com/resources/magic-rules/#R5143a)): if none are
    /// performed, the turn ends instead of a player receiving priority.
    #[inline]
    pub fn cleanup_check(self) -> bool {
        self.cleanup_check
    }

    /// Whether creatures were declared as attackers this combat
    /// (see [CR 508.8](https://yawgatog.com/resources/magic-rules/#R5088)).
    #[inline]
    pub fn attacked(self) -> bool {
        self.attacked
    }

    /// The player controlling the active player this turn, if another player does
    /// (see [CR 724.1]): "Some cards allow a player to control another player during that
    /// player's next turn." The active player is still the active player
    /// (see [CR 724.3]): "A player who's being controlled during their turn is still the active
    /// player."
    ///
    /// [CR 724.1]: https://yawgatog.com/resources/magic-rules/#R7241
    /// [CR 724.3]: https://yawgatog.com/resources/magic-rules/#R7243
    #[inline]
    pub fn controller(self) -> Option<Seat> {
        self.controller
    }
}

impl<L: Layout> State<L> {
    /// Names the starting player, whose turn the first will be
    /// (see [CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)): "The player chosen to
    /// take the first turn is the starting player."
    #[inline]
    pub fn set_starting_player(&mut self, starting: Seat) {
        self.turn.starting_player = starting;
        self.turn.active = starting;
    }

    /// The active player of the turn that would begin next, before a replacement can skip it.
    /// See [CR 500.7]: "The most recently created turn will be taken first."
    ///
    /// [CR 500.7]: https://yawgatog.com/resources/magic-rules/#R5007
    pub fn next_turn_player(&self) -> Seat {
        if self.turn.number == 0 && self.turn.after_extra.is_none() {
            return self.turn.starting_player;
        }
        self.effects.next_extra_turn().map_or_else(
            || self.next_player(self.turn.after_extra.unwrap_or(self.turn.active)),
            |(_, player, _)| player,
        )
    }

    /// Consumes the turn that would begin next, without starting it or taking a waiting
    /// player-controlling effect. See [CR 614.10a]: "Anything scheduled for a skipped step,
    /// phase, or turn won't happen." See [CR 724.1b]: "If a turn is skipped, any pending
    /// player-controlling effects wait until the player who would be affected actually takes
    /// a turn."
    ///
    /// [CR 614.10a]: https://yawgatog.com/resources/magic-rules/#R61410a
    /// [CR 724.1b]: https://yawgatog.com/resources/magic-rules/#R7241b
    pub fn skip_turn(&mut self) {
        let before_first = self.turn.number == 0 && self.turn.after_extra.is_none();
        if before_first || self.effects.take_extra_turn().is_none() {
            self.turn.starting_draw_later |= before_first;
            let last_regular = self.turn.after_extra.unwrap_or(self.turn.active);
            self.turn.after_extra = Some(if before_first {
                self.turn.starting_player
            } else {
                self.next_player(last_regular)
            });
        }
    }

    /// Begins the next turn and returns its active player: the starting player's first
    /// (see [CR 103.8]: "The starting player takes their first turn."), then the extra turn
    /// created last if one is waiting ([`crate::effects::extra_turn`]; see [CR 500.7]: "The most
    /// recently created turn will be taken first."), and otherwise the next player in turn
    /// order after the last turn that was not extra (see [CR 500.7]: "They do this by adding the
    /// turns directly after the specified turn."). The per-turn counts begin again with it: the
    /// decisions offered, the lands each player has played
    /// (see [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052)), and what the
    /// Trackers store noted of the turn before ([`crate::trackers`]).
    ///
    /// A player-controlling effect waiting for the new active player's turn makes its
    /// controller the turn's controller ([`Turn::controller`]), and the control of the turn
    /// before ends (see [CR 724.1]): "This effect applies to the next turn that the affected
    /// player actually takes. The affected player is controlled during the entire turn; the
    /// effect doesn't end until the beginning of the next turn." An extra turn is a turn the
    /// player actually takes, so an effect waiting for them controls it.
    ///
    /// [CR 103.8]: https://yawgatog.com/resources/magic-rules/#R1038
    /// [CR 500.7]: https://yawgatog.com/resources/magic-rules/#R5007
    /// [CR 724.1]: https://yawgatog.com/resources/magic-rules/#R7241
    #[inline]
    pub fn begin_turn(&mut self) -> Result<Seat, Full> {
        if self.turn.starting_draw_later
            && self.turn.number > 0
            && self.turn.active == self.turn.starting_player
        {
            self.turn.starting_draw_later = false;
        }
        if self.turn.number == 0 && self.turn.after_extra.is_none() {
            self.turn.active = self.turn.starting_player;
        } else {
            let regular = self.turn.after_extra.unwrap_or(self.turn.active);
            if let Some((player, trigger)) = self.effects.take_extra_turn() {
                self.turn.after_extra = Some(regular);
                self.turn.active = player;
                if let Some(of) = trigger {
                    self.delay_trigger(PendingTrigger {
                        of,
                        info: TriggerInfo::default(),
                    })?;
                }
            } else {
                self.turn.after_extra = None;
                self.turn.active = self.next_player(regular);
            }
        }
        self.turn.controller = self.effects.take_control(self.turn.active);
        // See CR 724.4: the controller sees what the controlled player sees, from the turn's
        // start; what they learn during it, they learn as that player does (`sharer`).
        if let Some(controller) = self.turn.controller {
            let seen = self.knowledge[self.turn.active.index()];
            self.knowledge[controller.index()] |= seen;
        }
        self.turn.number += 1;
        self.budgets.decisions = 0;
        for p in self.players.as_mut_slice() {
            p.lands_played = 0;
        }
        self.trackers.begin_turn();
        Ok(self.turn.active)
    }

    /// Drops delayed abilities whose stated duration has expired (see CR 603.7b).
    pub fn retain_delayed(&mut self, keep: impl Fn(&PendingTrigger<L>) -> bool) {
        self.delayed.retain(keep);
    }

    /// The player who learns what `player` learns, besides them: the player controlling them
    /// this turn, if one does (see [CR 724.4]): "If information about an object in the game would
    /// be visible to the player being controlled, it's visible to both that player and the
    /// controller of the player." Each mutator by which a player comes to know a card tells
    /// this player too, so that [`State::knows`] stays one bit test, and what the controller
    /// learned stays known after the turn.
    ///
    /// [CR 724.4]: https://yawgatog.com/resources/magic-rules/#R7244
    #[inline]
    pub(crate) fn sharer(&self, player: Seat) -> Option<Seat> {
        self.turn.controller.filter(|_| player == self.turn.active)
    }

    /// The player who answers a decision that belongs to `player`: the player controlling
    /// them this turn, or `player` themselves (see [CR 724.5]): "While controlling another
    /// player, a player makes all choices and decisions the controlled player is allowed to
    /// make or is told to make by the rules or by any objects." Only the active player is
    /// ever controlled ([`Turn::controller`]), and a controller still makes their own
    /// decisions (see [CR 724.8]).
    ///
    /// [CR 724.5]: https://yawgatog.com/resources/magic-rules/#R7245
    /// [CR 724.8]: https://yawgatog.com/resources/magic-rules/#R7248
    #[inline]
    pub fn decider(&self, player: Seat) -> Seat {
        match self.turn.controller {
            Some(controller) if player == self.turn.active => controller,
            _ => player,
        }
    }

    /// Begins a step or phase, the active player to receive priority in it
    /// (see [CR 117.3a](https://yawgatog.com/resources/magic-rules/#R1173a)): "The active player
    /// receives priority at the beginning of most steps and phases, after any turn-based
    /// actions (such as drawing a card during the draw step; see rule 703) have been dealt
    /// with".
    #[inline]
    #[must_use]
    pub fn begin_step(&mut self, step: TurnStep) -> Event<L> {
        self.turn.step = step;
        self.give_priority(self.turn.active);
        Event::StepBegins {
            turn: self.turn.number,
            step,
            active: self.turn.active,
        }
    }

    /// Gives a player priority, with no player having passed since
    /// (see [CR 117.3c](https://yawgatog.com/resources/magic-rules/#R1173c)): "If a player has
    /// priority when they cast a spell, activate an ability, or take a special action, that
    /// player receives priority afterward."
    #[inline]
    pub fn give_priority(&mut self, player: Seat) {
        self.turn.priority = player;
        self.turn.passes = 0;
    }

    /// The player with priority passes it, and whether every player has now passed in
    /// succession (see [CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d)): "If a
    /// player has priority and chooses not to take any actions, that player passes. ... the
    /// next player in turn order receives priority." Once all have passed, priority stays
    /// where it is for the rules to resolve the stack's top or end the step
    /// (see [CR 117.4](https://yawgatog.com/resources/magic-rules/#R1174)).
    #[inline]
    pub fn pass_priority(&mut self, format: Format) -> bool {
        self.turn.passes += 1;
        if self.turn.passes < format.seats() {
            self.turn.priority = self.next_player(self.turn.priority);
            return false;
        }
        true
    }

    /// Forgets the passes made in succession, priority staying where it is: something
    /// happened between them (see [CR 117.4](https://yawgatog.com/resources/magic-rules/#R1174):
    /// "all players pass in succession (that is, if all players pass without taking any
    /// actions in between passing)").
    #[inline]
    pub fn reset_passes(&mut self) {
        self.turn.passes = 0;
    }

    /// The active player has declared `attackers`, each made an attacking creature already
    /// ([`State::declare_attacker`]), and the declaration is whole
    /// (see [CR 508.1](https://yawgatog.com/resources/magic-rules/#R5081)). The combat's later
    /// steps then happen even if none is attacking any more
    /// (see [CR 508.8](https://yawgatog.com/resources/magic-rules/#R5088)): "If no creatures are
    /// declared as attackers or put onto the battlefield attacking, skip the declare blockers
    /// and combat damage steps." [`State::end_combat`] forgets it.
    #[inline]
    #[must_use]
    pub fn declare_attack(&mut self, attackers: Permanents<L>) -> Event<L> {
        self.turn.attacked = true;
        Event::Attack {
            player: self.turn.active,
            attackers: attackers.slots(),
        }
    }

    /// Whether the cleanup step is checking for state-based actions and waiting triggers
    /// (see [CR 514.3a](https://yawgatog.com/resources/magic-rules/#R5143a)): "the game checks to
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
    /// have (see [CR 704.3](https://yawgatog.com/resources/magic-rules/#R7043): "If any
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

    /// Ends the game (see [CR 104.1](https://yawgatog.com/resources/magic-rules/#R1041): "A game
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
