//! Starts a subgame at a resolution boundary and answers the suspended effect with its result.
//!
//! Implements: CR 730.3, CR 730.5

use beryl_card::prompt::Purpose;
use beryl_card::{Prompt, Zone};
use beryl_kernel::chance;

use crate::actions::shuffle;
use crate::bitboard::Bitboard;
use crate::card::Stop;
use crate::decision::{Answers, Candidates, Decision, DecisionKind, Outcome, StallReason};
use crate::fixed::Full;
use crate::game::{Game, start};
use crate::ids::Seat;
use crate::interpret::{Outcome as InstructionOutcome, Run};
use crate::layout::{Layout, index};
use crate::replacement::event::Sink;
use crate::resolution::Resolution;
use crate::scratch::Scratch;
use crate::stack::resolve;
use crate::state::State;
use crate::table::GameTable;

impl<L: Layout> Resolution<'_, L> {
    /// Suspends this resolution to play a subgame, then returns its winner or a draw.
    /// Register a custom body that calls this with `Says::starts_subgame`.
    ///
    /// The subgame's actual result occupies one place in the resolution's answer record.
    /// Replaying the effect after the subgame uses that result at this same instruction.
    ///
    /// Under [CR 730.5], "The main game continues from the point at which it was discontinued:
    /// First, the spell or ability that created the subgame finishes resolving".
    ///
    /// [CR 730.5]: https://yawgatog.com/resources/magic-rules/#R7305
    pub fn play_subgame(&mut self, game: &mut Game<'_, L>) -> Result<Outcome, Stop> {
        if !State::<L>::SUPPORTS_SUBGAMES {
            return Err(Stop::Full);
        }
        debug_assert!(self.ability.starts_subgame(), "an undeclared subgame");
        if let Some(answer) = game.sink.asking.recorded() {
            game.state.spend_subgame_seed();
            // The physical cards never left these library slots in the suspended position.
            // A shared library is one library, shuffled once.
            for player in game.state.zone_owners(Zone::Library).in_seat_order() {
                shuffle::shuffle(game, player)?;
            }
            return Ok(if answer.0 == 0 {
                Outcome::Draw
            } else {
                let seat = u8::try_from(answer.0 - 1).expect("a recorded seat");
                Outcome::Win(Seat::new(seat))
            });
        }

        // The suspended resolution needs a question to hold its record. This question
        // belongs to the engine, so it has no candidates and is never offered to a player.
        let player = self.controller();
        self.question = Some(Decision {
            player,
            decider: player,
            kind: DecisionKind::YesNo,
            candidates: Candidates::Options(Default::default()),
            prompt: Prompt::new(Purpose::May),
            about: Bitboard::EMPTY,
            filter: None,
        });
        debug_assert!(
            game.sink.asking.is_open(),
            "a subgame must suspend a replayable resolution"
        );
        Err(Stop::Pause)
    }
}

/// Delivers the result to the next instruction in the suspended effect. Under
/// [CR 730.5], "the spell or ability that created the subgame finishes resolving".
///
/// [CR 730.5]: https://yawgatog.com/resources/magic-rules/#R7305
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
) -> Result<InstructionOutcome<L>, Stop> {
    let (game, resolution) = run.resolving();
    let result = resolution.play_subgame(game)?;
    let players = game.state.players();
    Ok(InstructionOutcome {
        subgame_nonwinners: players.filter(|player| result != Outcome::Win(player)),
        did: true,
        ..InstructionOutcome::NOTHING
    })
}

/// Starts the new game from the state at the instruction. The committed game is stored
/// separately so that its resolution can later replay to the instruction again.
///
/// See [CR 730.2]: "Randomly determine which player goes first. The subgame proceeds like a
/// normal game, following all other rules in rule 103, “Starting the Game.”"
/// Under [CR 730.3], "any player with fewer than seven cards in their deck will lose the subgame
/// when state-based actions are checked during the upkeep step of the first turn".
///
/// [CR 730.2]: https://yawgatog.com/resources/magic-rules/#R7302
/// [CR 730.3]: https://yawgatog.com/resources/magic-rules/#R7303
pub(crate) fn enter<L: Layout>(
    state: &mut State<L>,
    instruction: &State<L>,
    table: &GameTable,
    sink: &mut Sink<L>,
    record: L::Queue<L::Index, 32>,
    question: Decision<L>,
) -> Result<(), Full> {
    if state.budgets().decisions >= state.profile().limits.decisions_per_turn {
        state.end_game(Outcome::Stalled(StallReason::DecisionsPerTurn));
        return Ok(());
    }
    let inner = instruction.deal_subgame(table.format())?;
    resolve::keep_what_was_seen(state, instruction, chance::replaying(sink));
    state.reset_passes();
    state.pause_resolution(record, question, false);
    state.enter_subgame(inner)?;
    let starter = chance::subgame_starter(state, sink, table.format());
    start::set_starting_player(state, table, sink, starter)
}

/// Returns to the suspended resolution. Under [CR 730.5], each player "puts [the cards]
/// into their main-game library, then shuffles them", which happens when the effect reads
/// the result. An engine stall stops the whole game.
///
/// [CR 730.5]: https://yawgatog.com/resources/magic-rules/#R7305
pub(crate) fn resume<L: Layout>(
    state: &mut State<L>,
    table: &GameTable,
    scratch: &mut Scratch<L>,
    answers: &mut dyn Answers<L>,
    outcome: Outcome,
) {
    state.leave_subgame();
    let result = match outcome {
        Outcome::Stalled(_) => {
            state.end_game(outcome);
            return;
        }
        Outcome::Draw => 0,
        Outcome::Win(seat) => usize::from(seat.get()) + 1,
    };
    if state.record_answer(index::<L>(result)).is_err()
        || resolve::resolve(state, table, scratch, answers).is_err()
    {
        state.end_game(Outcome::Stalled(StallReason::Capacity));
    }
}
