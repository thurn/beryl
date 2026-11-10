//! A game between a human seat and an agent, played synchronously: the core a session runs on
//! its thread, and that tests drive directly. It plays on view by view ([`Game::next_view`]):
//! as each step of the turn begins, before each of the agent's decisions, at each of the
//! human's, and at the end, so the human sees the agent's turn as it is played. Under the
//! smart stops the session also halts the human where the engine would pass them by, and
//! answers some of their decisions for them: passing to their own spell or trigger and in
//! combat after their own attack or no blocks, and ordering their triggers.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use beryl_engine::chance::{Chance, Mismatch, Random};
use beryl_engine::event::{Event, LossCause, Recipient};
use beryl_engine::layout::Queue;
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::{ObjectFlags, ObjectRef, Pending, StackEntry, Turn};
use beryl_engine::{
    Ask, Candidates, CardTable, Choice, Decision, DecisionKind, GameTable, Large, Layout, Limits,
    NextStop, Observation, Opt, Outcome, Profile, Relayout, Replay, Scratch, Seat, Small,
    StallReason, State, Step, StepStops, TurnStep, apply, next_in_step, observe, paused_events,
    view,
};
use beryl_search::{Agent, AgentDecision, Budget, GameAgent};
use serde_json::json;

use crate::ai::{self, AgentMove, AgentMoves, SearchView};
use crate::checkpoint::Checkpoint;
use crate::protocol::{
    CandidateIndex, DecisionId, Moment, PlayerView, SessionId, Side, StackItem, Status,
    StopsSetting, player_view,
};
use crate::replay::{Entry, FORMAT_VERSION, ReplayFile};
use crate::save::{self, Chosen};
use crate::{HandOver, Prepared, SessionError, StartGame};
use crate::{journal, perturb, preview};

/// The human's open decision, as a session records it: the one place an answer is checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenDecision {
    id: DecisionId,
    candidates: usize,
    /// The human may take back the cast it is part of ([`Game::cancel`]).
    cancellable: bool,
    /// The human may take back their latest answer ([`Game::undo`]).
    undoable: bool,
    /// The human may pass on to their next stop ([`Game::pass_on`]).
    passes_on: bool,
}

impl OpenDecision {
    fn new(id: DecisionId, open: &Open, cancellable: bool, undoable: bool) -> OpenDecision {
        OpenDecision {
            passes_on: false,
            id,
            candidates: match open {
                Open::Decision(d) => d.len(),
                Open::Halt => 1,
            },
            cancellable,
            undoable,
        }
    }

    pub fn id(self) -> DecisionId {
        self.id
    }

    /// Checks that the cast this decision, named `id`, is part of can be taken back.
    pub fn cancel(self, id: DecisionId) -> Result<(), SessionError> {
        if id != self.id {
            return Err(SessionError::StaleDecision {
                decision: id,
                open: self.id,
            });
        }
        if self.cancellable {
            Ok(())
        } else {
            Err(SessionError::NotCancellable)
        }
    }

    /// Checks that the human may take back their latest answer from this decision, named `id`.
    pub fn undo(self, id: DecisionId) -> Result<(), SessionError> {
        if id != self.id {
            return Err(SessionError::StaleDecision {
                decision: id,
                open: self.id,
            });
        }
        if self.undoable {
            Ok(())
        } else {
            Err(SessionError::NothingToUndo)
        }
    }

    /// Checks that the human may pass on to their next stop from this decision, named `id`,
    /// and returns the pass that begins it: a priority's first candidate.
    pub fn pass_on(self, id: DecisionId) -> Result<Answer, SessionError> {
        if id != self.id {
            return Err(SessionError::StaleDecision {
                decision: id,
                open: self.id,
            });
        }
        if self.passes_on {
            Ok(Answer {
                id,
                choice: Choice(0),
                continuation: false,
            })
        } else {
            Err(SessionError::NoPassOn)
        }
    }

    /// Checks that `index` answers this decision, named `id`.
    pub fn answer(self, id: DecisionId, index: CandidateIndex) -> Result<Answer, SessionError> {
        if id != self.id {
            return Err(SessionError::StaleDecision {
                decision: id,
                open: self.id,
            });
        }
        if usize::from(index.0) >= self.candidates {
            return Err(SessionError::NoSuchCandidate {
                index,
                candidates: self.candidates,
            });
        }
        Ok(Answer {
            id,
            choice: Choice(index.0),
            continuation: false,
        })
    }
}

/// A checked answer to an open decision, which only [`OpenDecision::answer`] makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answer {
    id: DecisionId,
    choice: Choice,
    continuation: bool,
}

impl Answer {
    /// Joins a follow-up the client answers for the player to the preceding choice for Undo.
    #[must_use]
    pub fn as_continuation(mut self) -> Self {
        self.continuation = true;
        self
    }
}

#[derive(Clone, Copy)]
enum ChoiceOrigin {
    Explicit,
    Continuation,
    Auto,
}

/// What the human must answer.
// One lives in a game at a time; it is cloned once per decision, never in a hot loop.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
enum Open {
    /// The engine's decision, in [`Large`] whatever the game's layout: the session keeps the
    /// game's decisions, stacks and events so, and the slots keep their numbers in either.
    Decision(Decision<Large>),
    /// A halt ([`Game::halts`]): the session stops the human where the engine passes their
    /// priority for them, and they go on with its one candidate.
    Halt,
}

/// Where a game stands between views.
#[derive(Clone, Debug)]
enum At {
    /// The game plays on from here without the human: before the first view, after an
    /// answer, or as a step begins.
    Playing,
    /// The agent makes this decision next.
    Agent(Decision<Large>),
    /// The human's open decision.
    Human(DecisionId, Open),
    /// The game is over; `cause`, why the loser lost, once a view has seen its
    /// [`Event::Lose`] (or the human conceded, [`Game::concede`]), and `hurt_by`, what dealt
    /// them the damage that last lowered their life ([`Game::hurt_by`]).
    Over {
        outcome: Outcome,
        cause: Option<LossCause<Large>>,
        hurt_by: Option<ObjectRef<Large>>,
    },
    /// Stopped by the stop flag before the agent's decision.
    Stopped,
}

/// The game's state and the engine's working memory for it, in one layout.
struct Board<L: Layout> {
    state: State<L>,
    scratch: Scratch<L>,
    /// Working memory for looking ahead in a copy of the state, whose events are dropped.
    peek: Scratch<L>,
}

/// The game in the layout it is in (architecture §4.1): [`Small`], the layout search runs on,
/// until a capacity of it runs out, then [`Large`] for good. The game is widened as it stood
/// before the engine call that ran out, and that call is made again in `Large`, so no player
/// game ends stalled for a capacity of `Small` and no player sees the difference.
enum Boards {
    Small(Box<Board<Small>>),
    Large(Box<Board<Large>>),
}

/// Evaluates `$body` with `$b` bound to the board, whichever layout it is in.
macro_rules! on_board {
    ($boards:expr, $b:ident => $body:expr) => {
        match $boards {
            Boards::Small($b) => $body,
            Boards::Large($b) => $body,
        }
    };
}

/// Runs `f` with `chance` lent to the scratch's sink ([`beryl_engine::chance`]), and takes it
/// back after.
fn lend<L: Layout, R>(
    x: &mut Scratch<L>,
    chance: &mut Chance,
    f: impl FnOnce(&mut Scratch<L>) -> R,
) -> R {
    x.events.set_chance(Some(std::mem::take(chance)));
    let r = f(x);
    *chance = x.events.take_chance().expect("the chance lent");
    r
}

/// Whether the engine stalled the game because a capacity of its layout ran out.
fn ran_out<L: Layout>(s: &State<L>) -> bool {
    s.outcome() == Some(Outcome::Stalled(StallReason::Capacity))
}

/// A copy of the game's state, in the layout it was in, shared: an undo point and the cast
/// begun from it ([`BeforeCast`]) hold one copy between them.
#[derive(Clone)]
enum Snapshot {
    Small(Arc<State<Small>>),
    Large(Arc<State<Large>>),
}

impl Boards {
    fn new(state: &State<Small>) -> Boards {
        Boards::Small(Box::new(Board {
            state: *state,
            scratch: Scratch::recording(),
            peek: Scratch::new(),
        }))
    }

    /// Goes on in [`Large`] from `before`, the state before the engine call that ran out of
    /// capacity, keeping the events the scratch had recorded by then: the first `events`.
    fn widen(&mut self, before: &State<Small>, events: usize) {
        let Boards::Small(b) = self else {
            unreachable!("only a Small game is widened");
        };
        let mut scratch = Scratch::new();
        scratch.events = b.scratch.events.relayout();
        scratch.events.truncate(events);
        *self = Boards::Large(Box::new(Board {
            state: before.widen(),
            scratch,
            peek: Scratch::new(),
        }));
    }

    /// Applies `c` to the open decision with the game's `chance`, widening the game if it
    /// runs out of capacity.
    fn apply(&mut self, t: &GameTable, c: Choice, chance: &mut Chance) {
        if let Boards::Small(b) = self {
            let (before, events, lent) = (b.state, b.scratch.events.len(), chance.clone());
            lend(&mut b.scratch, chance, |x| {
                apply(&mut b.state, t, x, &mut Replay, c);
            });
            if !ran_out(&b.state) {
                return;
            }
            *chance = lent;
            self.widen(&before, events);
        }
        let Boards::Large(b) = self else {
            unreachable!("a game widened")
        };
        lend(&mut b.scratch, chance, |x| {
            apply(&mut b.state, t, x, &mut Replay, c);
        });
    }

    /// Plays on as [`next_in_step`] does with the game's `chance`, widening the game if it
    /// runs out of capacity.
    fn next_in_step(
        &mut self,
        t: &GameTable,
        seen: (u16, TurnStep),
        chance: &mut Chance,
    ) -> Option<Step<Large>> {
        if let Boards::Small(b) = self {
            let (before, events, lent) = (b.state, b.scratch.events.len(), chance.clone());
            let step = lend(&mut b.scratch, chance, |x| {
                next_in_step(&mut b.state, t, x, &mut Replay, seen)
            });
            if !ran_out(&b.state) {
                return step.map(|s| s.relayout());
            }
            *chance = lent;
            self.widen(&before, events);
        }
        let Boards::Large(b) = self else {
            unreachable!("a game widened")
        };
        lend(&mut b.scratch, chance, |x| {
            next_in_step(&mut b.state, t, x, &mut Replay, seen)
        })
    }

    /// What [`Boards::next_in_step`] would return, played on a copy with a copy of `chance`,
    /// so a replay looks ahead at the cards its record has.
    fn ahead(
        &mut self,
        t: &GameTable,
        seen: (u16, TurnStep),
        chance: &Chance,
    ) -> Option<Step<Large>> {
        match self {
            Boards::Small(b) => {
                let mut ahead = b.state;
                let step = lend(&mut b.peek, &mut chance.clone(), |x| {
                    next_in_step(&mut ahead, t, x, &mut Replay, seen)
                });
                if !ran_out(&ahead) {
                    return step.map(|s| s.relayout());
                }
                let mut wide = b.state.widen();
                lend(&mut Scratch::new(), &mut chance.clone(), |x| {
                    next_in_step(&mut wide, t, x, &mut Replay, seen)
                })
            }
            Boards::Large(b) => {
                let mut ahead = b.state.clone();
                lend(&mut b.peek, &mut chance.clone(), |x| {
                    next_in_step(&mut ahead, t, x, &mut Replay, seen)
                })
            }
        }
    }

    fn snapshot(&self) -> Snapshot {
        match self {
            Boards::Small(b) => Snapshot::Small(Arc::new(b.state)),
            Boards::Large(b) => Snapshot::Large(Arc::new(b.state.clone())),
        }
    }

    /// Puts the game back as `snapshot` holds it, in the layout the game is in now: a game
    /// widened since keeps its layout.
    fn restore(&mut self, snapshot: &Snapshot) {
        match (self, snapshot) {
            (Boards::Small(b), Snapshot::Small(s)) => b.state = **s,
            (Boards::Large(b), Snapshot::Small(s)) => b.state = s.widen(),
            (Boards::Large(b), Snapshot::Large(s)) => b.state = State::clone(s),
            (Boards::Small(_), Snapshot::Large(_)) => unreachable!("a game is never narrowed"),
        }
    }

    /// The events the engine recorded since the last drain.
    fn drain_events(&mut self) -> Vec<Event<Large>> {
        on_board!(self, b => b.scratch.events.drain().map(|e| e.relayout()).collect())
    }

    /// Whether a spell is being cast or an ability activated or put on the stack, and if so
    /// whether it is a triggered ability, putting its targets.
    fn casting(&self) -> Option<bool> {
        on_board!(self, b => match b.state.pending() {
            Pending::Casting(c) => Some(c.triggered),
            _ => None,
        })
    }

    /// Whether a resolution is paused for an answer (§7.3).
    fn resolving(&self) -> bool {
        on_board!(self, b => matches!(b.state.pending(), Pending::Resolving(_)))
    }

    fn stack(&self) -> Vec<StackEntry<Large>> {
        on_board!(self, b => b.state.stack().iter().map(Relayout::relayout).collect())
    }

    fn stack_is_empty(&self) -> bool {
        on_board!(self, b => b.state.stack().is_empty())
    }

    fn turn(&self) -> &Turn {
        on_board!(self, b => b.state.turn())
    }

    /// Whether a creature is blocking.
    fn any_blocking(&self) -> bool {
        on_board!(self, b => b
            .state
            .battlefield()
            .iter()
            .any(|&slot| b.state.object(slot).flags.has(ObjectFlags::BLOCKING)))
    }

    fn outcome(&self) -> Option<Outcome> {
        on_board!(self, b => b.state.outcome())
    }

    fn profile(&self) -> Profile {
        on_board!(self, b => b.state.profile())
    }

    fn set_profile(&mut self, profile: Profile) {
        on_board!(self, b => b.state.set_profile(profile));
    }
}

/// The game as it stood at the human's priority before they began to cast a spell or activate
/// an ability, kept until its costs are paid so that they can take it back ([`Game::cancel`]).
#[derive(Clone)]
struct BeforeCast {
    state: Snapshot,
    at: At,
    answered: Option<Vec<StackEntry<Large>>>,
    /// The length of the choice log then.
    log: usize,
    stops: StopsSetting,
    /// How many undo points had been kept by then, counting those dropped since
    /// ([`Game::undo_depth`]).
    undo: usize,
}

/// The most undo points a game keeps ([`Game::undo`]), the oldest dropped first: at most
/// 20 KiB each in [`Small`], so ten megabytes at most, and more than a whole game's answers
/// under the smart stops.
pub const UNDO_POINTS: usize = 500;

/// The game as it stood at one of the human's decisions or halts before they answered it,
/// which they may go back to ([`Game::undo`]). A copy of the state, random streams and all, so
/// going back is exact: the library and every random stream are as they were, so the cards
/// drawn since come out again in the same order, and a shuffle falls the same way.
struct UndoPoint {
    state: Snapshot,
    continuation: bool,
    /// The open decision there.
    at: At,
    seen: (u16, TurnStep),
    answered: Option<Vec<StackEntry<Large>>>,
    before_cast: Option<BeforeCast>,
    /// The agent's next decision among its own then ([`Agent::next_decision`]), which it goes
    /// back to.
    agent: Option<AgentDecision>,
    hurt_by: [Option<ObjectRef<Large>>; 2],
}

/// Where a replay and its record parted ([`Game::from_replay`]): answers found away from
/// their recorded index, and places it went on where the rules left nothing to choose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Drift {
    moved: u32,
    lenient: u32,
}

/// The record's turn-start checkpoints as a replay reaches them ([`Game::from_replay`]).
struct Checks<'a> {
    /// Those not reached yet, in order: the board the record was rebuilt from, if it was, then
    /// those its log has.
    ahead: VecDeque<&'a Checkpoint>,
    /// Those reached, in order.
    reached: Vec<&'a Checkpoint>,
    /// The turns of those dropped unreached, in order.
    missed: Vec<u16>,
}

impl<'a> Checks<'a> {
    fn new(file: &'a ReplayFile) -> Checks<'a> {
        let mut logged = file
            .log
            .iter()
            .filter_map(|e| match e {
                Entry::Checkpoint(c) => Some(&**c),
                _ => None,
            })
            .peekable();
        // Resuming from a checkpoint records that same opening board in the new log.
        // It is one reached board, not a second turn that exact replay must reach again.
        if logged.peek().copied() == file.from.as_deref() {
            logged.next();
        }
        Checks {
            ahead: file.from.as_deref().into_iter().chain(logged).collect(),
            reached: Vec::new(),
            missed: Vec::new(),
        }
    }

    /// Checks `board`, the board a replay reached as a turn began, against the record's
    /// checkpoint of that turn, if it has one: where they differ, the replay has diverged.
    /// Checkpoints of earlier turns not reached are dropped; an undo past a turn's beginning
    /// begins it again, so a record may have two of a turn, which the replay reaches in turn.
    fn reached(&mut self, t: &GameTable, board: &Checkpoint) -> Result<(), SessionError> {
        while let Some(missed) = self.ahead.pop_front_if(|c| c.turn < board.turn) {
            self.missed.push(missed.turn);
        }
        let Some(recorded) = self.ahead.front().copied().filter(|c| c.turn == board.turn) else {
            return Ok(());
        };
        self.ahead.pop_front();
        self.reached.push(recorded);
        match recorded.difference(t, board) {
            None => Ok(()),
            Some(difference) => Err(SessionError::BadReplay {
                reason: format!(
                    "turn {}: the board as it began is not the record's: {difference}",
                    board.turn
                ),
            }),
        }
    }

    /// `file` rebuilt from the latest checkpoint the replay reached of turn `turn` or before,
    /// where the replay stopped for `reason` in turn `turn`: the board as that turn began, with
    /// no log yet. Fails with `reason` where there is none, or where something lasting past
    /// that turn's beginning off the board would be lost ([`Checkpoint::lingering`]).
    fn rebuild(
        &self,
        file: &ReplayFile,
        turn: u16,
        reason: String,
    ) -> Result<ReplayFile, SessionError> {
        let Some(from) = self.reached.iter().rev().find(|c| c.turn <= turn) else {
            return Err(SessionError::BadReplay { reason });
        };
        if let Some(lingering) = &from.lingering {
            return Err(SessionError::BadReplay {
                reason: format!(
                    "{reason}; no rebuild from turn {}'s beginning, which would lose {lingering}",
                    from.turn
                ),
            });
        }
        journal::warn(
            "replay",
            "rebuilt the game from a turn's beginning",
            json!({ "turn": from.turn, "reason": reason }),
        );
        Ok(ReplayFile {
            format_version: FORMAT_VERSION,
            chooser: None,
            from: Some(Box::new((*from).clone())),
            log: Vec::new(),
            ..file.clone()
        })
    }
}

/// The human's pass on to their next stop ([`Game::pass_on`]), while the session passes their
/// priority for them.
#[derive(Clone, Copy, Debug)]
struct PassingOn {
    /// The turn and step of their next stop, where it ends.
    until: (u16, TurnStep),
}

/// A game in progress.
pub struct Game {
    table: GameTable,
    board: Boards,
    human: Seat,
    session: SessionId,
    agent: Box<dyn GameAgent + Send>,
    /// The agent's latest decisions and their searches ([`crate::ai`]).
    moves: AgentMoves,
    /// An agent answering the human's decisions in their place, if any.
    stand_in: Option<Box<dyn GameAgent + Send>>,
    /// When the stand-in hands the human's decisions back to them, if ever.
    hand_over: Option<HandOver>,
    review: Option<crate::ReviewViews>,
    at: At,
    /// The turn and step the latest view began, or the game's first.
    seen: (u16, TurnStep),
    /// The stack as it was once the human's latest answer was applied, if they answered in
    /// this step: what they know of it ([`Game::own_top`]). With no answer yet in this step,
    /// they knew it empty, as every step begins
    /// ([CR 500.2](https://yawgatog.com/resources/magic-rules/#R5002): "A phase or step in
    /// which players receive priority ends when the stack is empty and all players pass in
    /// succession").
    answered: Option<Vec<StackEntry<Large>>>,
    seq: u32,
    stops: StopsSetting,
    replay: ReplayFile,
    /// Set from another thread to stop the agent at its next decision.
    stop: Arc<AtomicBool>,
    /// The engine's events the latest view reported.
    events: Vec<Event<Large>>,
    /// The events of the paused resolution the latest view was in, which it reported: the
    /// body executes them again as it goes on (architecture §7.3).
    paused: Vec<Event<Large>>,
    /// For each player, the source of the damage that last lowered their life, if damage
    /// rather than a loss of life did, among the events the views reported: what the end
    /// names when a player's life reaches 0.
    hurt_by: [Option<ObjectRef<Large>>; 2],
    /// Where the human's cast in progress began, while it can be taken back.
    before_cast: Option<BeforeCast>,
    /// The positions the human may go back to, one per answer, the latest last
    /// ([`Game::undo`]), at most [`UNDO_POINTS`].
    undo: VecDeque<UndoPoint>,
    /// How many undo points were dropped, the oldest first, to keep within [`UNDO_POINTS`].
    undo_dropped: usize,
    /// The human's pass on to their next stop, while it lasts.
    passing: Option<PassingOn>,
    /// Where the human's open decision passes on to, if it can ([`Game::pass_on`]).
    passes_on: Option<(u16, TurnStep)>,
    /// The coin flip's winner, who chooses who takes the first turn
    /// ([CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)), in a game begun by one.
    flip_winner: Option<Seat>,
    /// What the game's random events produced since it was last logged, and while a replay
    /// plays, what its record says they produced ([`beryl_engine::chance`]).
    chance: Chance,
}

impl Game {
    /// Sets up game `session`; [`Game::next_view`] plays it.
    pub fn new(
        cards: &'static CardTable,
        start: &StartGame,
        session: SessionId,
    ) -> Result<Game, SessionError> {
        let replay = ReplayFile {
            format_version: FORMAT_VERSION,
            legacy_waterbend: false,
            legacy_waterbend_mana: false,
            seed: start.seed,
            decks: start.decks.clone(),
            human: start.human.get(),
            opponent: start.opponent.clone(),
            stops: start.stops,
            chooser: None,
            from: None,
            log: Vec::new(),
            commit: None,
        };
        let (table, state) = replay.setup(cards)?;
        // The agent draws from its own stream, apart from the game's.
        let agent = new_agent(
            &start.opponent,
            &table,
            SplitMix64::new(start.seed).next_u64(),
        )?;
        let mut game = Game::assemble(table, &state, replay, start.human, session, agent);
        game.replay.chooser = game.flip_winner.map(Seat::get);
        Ok(game)
    }

    /// Sets up game `session` from a prepared position. Its replay file holds no decks or
    /// agent, so nothing can replay it.
    pub fn prepared(prepared: Prepared, session: SessionId) -> Game {
        let Prepared {
            table,
            mut state,
            human,
            stops,
            opponent,
            stand_in,
            hand_over,
            scripted,
        } = prepared;
        let base = if scripted {
            Profile {
                hides_pointless: [false; 2],
                ..Profile::SMART
            }
        } else {
            Profile::SMART
        };
        state.set_profile(with_stops(base, stops, human));
        let replay = ReplayFile {
            format_version: FORMAT_VERSION,
            legacy_waterbend: false,
            legacy_waterbend_mana: false,
            seed: 0,
            decks: [Vec::new(), Vec::new()],
            human: human.get(),
            opponent: String::new(),
            stops,
            chooser: None,
            from: None,
            log: Vec::new(),
            commit: None,
        };
        let mut game = Game::assemble(table, &state, replay, human, session, opponent);
        game.stand_in = Some(stand_in);
        game.hand_over = hand_over;
        game
    }

    /// After setup, let either seat answer its own decisions and publish both views.
    pub(crate) fn review(&mut self, emit: crate::ReviewViews) {
        self.review = Some(emit);
    }

    fn reviewing(&self) -> bool {
        self.review.is_some() && self.hand_over.as_ref().is_none_or(|ready| ready())
    }

    pub(crate) fn publish_review(&mut self, first: &PlayerView) {
        if self.review.is_none() {
            return;
        }
        let second = self.draw_for(self.human.opponent());
        if let Some(emit) = &self.review {
            emit(self.session, [first.clone(), second]);
        }
    }

    fn assemble(
        table: GameTable,
        state: &State,
        replay: ReplayFile,
        human: Seat,
        session: SessionId,
        agent: Box<dyn GameAgent + Send>,
    ) -> Game {
        let turn = state.turn();
        let flip_winner = match state.pending() {
            Pending::PlayOrDraw { chooser } => Some(*chooser),
            _ => None,
        };
        Game {
            seen: (turn.number, turn.step),
            answered: None,
            stops: replay.stops,
            table,
            board: Boards::new(state),
            human,
            session,
            agent,
            moves: AgentMoves::default(),
            stand_in: None,
            hand_over: None,
            review: None,
            at: At::Playing,
            seq: 0,
            replay,
            stop: Arc::default(),
            events: Vec::new(),
            paused: Vec::new(),
            hurt_by: [None; 2],
            before_cast: None,
            undo: VecDeque::new(),
            undo_dropped: 0,
            passing: None,
            passes_on: None,
            flip_winner,
            chance: Chance::default(),
        }
    }

    /// Has the agent named `name` answer the human's decisions in their place (UI fixtures):
    /// [`Game::answer_by_stand_in`] plays each.
    pub fn set_stand_in(&mut self, name: &str) -> Result<(), SessionError> {
        // The stand-in draws from its own stream, apart from the game's and the agent's.
        self.stand_in = Some(new_agent(
            name,
            &self.table,
            SplitMix64::new(self.replay.seed ^ 0x9e37_79b9).next_u64(),
        )?);
        Ok(())
    }

    /// Replays a recorded game to where its record ends, and returns it with the view live
    /// play emitted there: the human's open decision or halt, the agent about to decide, or
    /// the end. [`Game::next_view`] plays on from there.
    ///
    /// Each answer takes the candidate that does what the recorded one did
    /// ([`Chosen::find`]), wherever it now stands. Where the record and the game part, replay
    /// goes on only where the rules leave nothing to choose, journaling each time:
    /// - a decision the record lacks with one candidate (a halt among them) is answered with
    ///   it;
    /// - a priority the record lacks, before the turn and step of its next answer, is passed:
    ///   every decision is recorded, so the original game passed it unasked;
    /// - a recorded priority pass, or halt, that the game no longer asks is skipped, as passing
    ///   changes nothing where no priority is open.
    ///
    /// Anything else fails, naming the turn and step. A version 1 record, of indices alone,
    /// replays as it always did.
    ///
    /// Since version 3, chance has the outcomes the record logged after each entry, while that
    /// entry plays out and play goes on to the next: each hidden card as a player first learns
    /// it is dealt the card the record names, and each coin flip falls as it fell
    /// ([`beryl_engine::chance`]). The seed then decides only what no player has seen. An
    /// outcome the game cannot reproduce, one it makes that the record lacks, or one the record
    /// has that never happens stops the replay. An older record replays from its seed.
    ///
    /// Since version 4, the board as each turn begins is checked against the record's
    /// ([`crate::checkpoint`]): where they differ, the replay stops. Where a replay stops, the
    /// game is rebuilt from the latest board the replay reached as a turn began at or before the
    /// turn it stopped in, journaled, and resumed there: the human plays that turn again. The
    /// rebuilt game's record begins from that board ([`ReplayFile::from`]). Where no board was
    /// reached, or the board's turn had something lasting past its beginning off the board
    /// ([`crate::checkpoint::Checkpoint::lingering`]), the replay fails, saying why.
    pub fn from_replay(
        cards: &'static CardTable,
        file: &ReplayFile,
        session: SessionId,
    ) -> Result<(Game, PlayerView), SessionError> {
        let mut game = Game::begin(cards, file, session)?;
        let mut checks = Checks::new(file);
        match game.replay_log(file, &mut checks) {
            Ok(()) => {}
            Err(SessionError::BadReplay { reason }) => {
                let rebuilt = checks.rebuild(file, game.board.turn().number, reason)?;
                game = Game::begin(cards, &rebuilt, session)?;
                game.replay_log(&rebuilt, &mut Checks::new(&rebuilt))?;
            }
            Err(e) => return Err(e),
        }
        let view = game.view();
        Ok((game, view))
    }

    /// Replays `file` as [`Game::from_replay`] does, but never rebuilds the game from a turn's
    /// beginning: where the record and the game part, a turn's board included, it fails, and
    /// so does a checkpoint of the record the replay never reached. The frozen save corpus is
    /// held to this ([`crate::corpus`]).
    pub fn replay_exactly(
        cards: &'static CardTable,
        file: &ReplayFile,
        session: SessionId,
    ) -> Result<(Game, PlayerView), SessionError> {
        let mut game = Game::begin(cards, file, session)?;
        let mut checks = Checks::new(file);
        game.replay_log(file, &mut checks)?;
        if let Some(turn) = checks
            .missed
            .first()
            .or(checks.ahead.front().map(|c| &c.turn))
        {
            return Err(SessionError::BadReplay {
                reason: format!("the record's board as turn {turn} began was never reached"),
            });
        }
        let view = game.view();
        Ok((game, view))
    }

    /// Sets up the game `file` records, before its first entry: as a new game, or as the board
    /// it was rebuilt from ([`ReplayFile::from`]) stood before that turn began.
    fn begin(
        cards: &'static CardTable,
        file: &ReplayFile,
        session: SessionId,
    ) -> Result<Game, SessionError> {
        let mut game = Game::new(cards, &file.start()?, session)?;
        let Some(from) = &file.from else {
            return Ok(game);
        };
        let mut state = perturb::new_game(&game.table, file.seed, game.board.profile());
        from.arrange(&game.table, &mut state, perturb::seed(file.seed))
            .map_err(|reason| SessionError::BadReplay {
                reason: format!("the checkpoint of turn {}: {reason}", from.turn),
            })?;
        let turn = state.turn();
        game.seen = (turn.number, turn.step);
        game.board = Boards::new(&state);
        game.flip_winner = None;
        game.replay.chooser = None;
        game.replay.from = Some(from.clone());
        Ok(game)
    }

    /// Replays `file`'s log on the game [`Game::begin`] set up, to where it ends, checking each
    /// turn's board against the record's checkpoint ([`Checks`]).
    fn replay_log(&mut self, file: &ReplayFile, checks: &mut Checks) -> Result<(), SessionError> {
        let chance = file.format_version >= 3;
        if chance {
            // A game rebuilt from a turn's beginning is past the coin flip.
            if let Some(chooser) = file.chooser.filter(|_| file.from.is_none()) {
                let chooser = Seat::try_new(chooser).ok_or_else(|| SessionError::BadReplay {
                    reason: format!("no seat {chooser} won the coin flip"),
                })?;
                self.set_flip_winner(chooser);
            }
            self.chance.replay([]);
        }
        // Each entry replay takes, with the outcomes of chance logged after it, and those logged
        // before the first. The session's own answers are found again as it plays on, and
        // checkpoints are checked as each turn begins.
        let mut before = Vec::new();
        let mut steps: Vec<(&Entry, Vec<Random>)> = Vec::new();
        for entry in &file.log {
            match entry.random(&self.table)? {
                Some(outcome) => match steps.last_mut() {
                    Some((_, outcomes)) => outcomes.push(outcome),
                    None => before.push(outcome),
                },
                // Damage assignments are recorded choices, even when suggested
                // automatically. Replaying them preserves earlier manual choices.
                None if matches!(entry, Entry::Auto(c)
                    if c.what.as_ref().is_some_and(|w| w.kind == save::Kind::DamageDivision)) =>
                {
                    steps.push((entry, Vec::new()));
                }
                None if matches!(entry, Entry::Auto(_) | Entry::Checkpoint(_)) => {}
                None => steps.push((entry, Vec::new())),
            }
        }
        if chance {
            self.chance.replay(before);
        }
        let mut steps = steps.into_iter().peekable();
        let mut drift = Drift::default();
        // Whether chance has the outcomes logged after the next entry.
        let mut dealt = false;
        loop {
            if matches!(self.at, At::Playing) {
                let logged = self.replay.log.len();
                self.play_on(true);
                self.chance_reproduced()?;
                if let Some(Entry::Checkpoint(reached)) = self
                    .replay
                    .log
                    .get(logged..)
                    .and_then(|new| new.iter().find(|e| matches!(e, Entry::Checkpoint(_))))
                {
                    checks.reached(&self.table, reached)?;
                }
                if matches!(self.at, At::Playing) {
                    // A step began, and live play went on after its view.
                    self.board.drain_events();
                    continue;
                }
            }
            if self.replay_implicit_waterbend(file, steps.peek().map(|(e, _)| *e)) {
                self.chance_reproduced()?;
                continue;
            }
            // Before v7, an interrupted cast could have exile picks but no mana plan yet.
            // Stop at the newly required payment instead of inventing its answer. The v7
            // migration re-records this prefix, restoring those unfinished exile picks.
            if file.format_version < 7
                && self.unpaid_cast()
                && steps.clone().all(|(entry, _)| {
                    matches!(entry, Entry::Choice(c)
                    if c.what.as_ref().is_some_and(|w| w.kind == save::Kind::ExileCost))
                })
            {
                break;
            }
            let Some((entry, outcomes)) = steps.peek_mut() else {
                break;
            };
            if !dealt {
                // The entry before played out, and play went on to here.
                self.chance_spent()?;
                if chance {
                    self.chance.replay(std::mem::take(outcomes));
                }
                dealt = true;
            }
            let entry: &Entry = entry;
            // Live play's view here took the events so far.
            self.board.drain_events();
            let took = self.replay_entry(entry, &mut drift)?;
            self.chance_reproduced()?;
            if took {
                steps.next();
                dealt = false;
            }
        }
        self.chance_spent()?;
        self.chance.stop_replaying();
        if drift != Drift::default() {
            journal::info(
                "replay",
                "the record replayed with drift",
                json!({ "moved": drift.moved, "lenient": drift.lenient }),
            );
        }
        Ok(())
    }

    /// Before v8, a required singleton Waterbend answer was made inside the engine,
    /// without a log entry. Restore it here without consuming the next recorded answer.
    fn replay_implicit_waterbend(&mut self, file: &ReplayFile, next: Option<&Entry>) -> bool {
        let At::Human(id, open @ Open::Decision(d)) = &self.at else {
            return false;
        };
        if d.kind != DecisionKind::Waterbend {
            return false;
        }
        let lone_bender = (file.legacy_waterbend || file.format_version < 8)
            && without_floating(d.clone()).len() == 1;
        // A v8 payment with only nonland mana to activate never opened the picker at all.
        let nonland_only = (file.legacy_waterbend_mana || file.format_version < 9)
            && on_board!(&self.board, b => only_nonland_mana(&b.state, &d.relayout()));
        if !lone_bender && !nonland_only {
            return false;
        }
        if matches!(next, Some(Entry::Choice(c)) if c.what.as_ref()
            .is_some_and(|w| w.kind == save::Kind::Waterbend))
        {
            return false;
        }
        let answer = OpenDecision::new(*id, open, false, false)
            .answer(*id, CandidateIndex(0))
            .expect("waterbend has one non-mana answer");
        self.choose(answer.as_continuation());
        true
    }

    /// Settles the coin flip of [CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)
    /// as a record says: `chooser` won it and chooses to play or draw.
    fn set_flip_winner(&mut self, chooser: Seat) {
        on_board!(&mut self.board, b => b.state.set_flip_winner(chooser));
        self.flip_winner = Some(chooser);
        self.replay.chooser = Some(chooser.get());
    }

    /// Whether replay is waiting for the mana choice of an unfinished cast.
    fn unpaid_cast(&self) -> bool {
        matches!(&self.at,
            At::Agent(d) | At::Human(_, Open::Decision(d))
                if matches!(d.kind, DecisionKind::PaymentPlan | DecisionKind::ManaPayment))
    }

    /// Fails where a replay's chance could not reproduce what its record says chance produced.
    fn chance_reproduced(&mut self) -> Result<(), SessionError> {
        let Some(m) = self.chance.take_mismatch() else {
            return Ok(());
        };
        let Mismatch {
            turn,
            step,
            recorded,
            happened,
        } = m;
        let what = match (recorded, happened) {
            (None, happened) => format!("{} that the record lacks", self.outcome(happened)),
            (
                Some(Random::Reveal { owner, card }),
                Random::Reveal {
                    owner: revealed, ..
                },
            ) if owner == revealed => format!(
                "the record reveals player {}'s {} where no card of theirs that no player knows \
                 can be it",
                owner.get(),
                self.table.def(card).name
            ),
            (Some(recorded), happened) => format!(
                "the record has {} where the game has {}",
                self.outcome(recorded),
                self.outcome(happened)
            ),
        };
        Err(SessionError::BadReplay {
            reason: format!("turn {turn} {step:?}: {what}"),
        })
    }

    /// Fails where a replay's record has an outcome of chance that never happened.
    fn chance_spent(&self) -> Result<(), SessionError> {
        match self.chance.next_recorded() {
            Some(outcome) => Err(self.parted(&format!(
                "the record has {} that never happened",
                self.outcome(outcome)
            ))),
            None => Ok(()),
        }
    }

    /// An outcome of chance, in words.
    fn outcome(&self, outcome: Random) -> String {
        match outcome {
            Random::Reveal { owner, card } => format!(
                "player {}'s {} revealed",
                owner.get(),
                self.table.def(card).name
            ),
            Random::Flip { player, won } => format!(
                "a coin flip player {} {}",
                player.get(),
                if won { "won" } else { "lost" }
            ),
        }
    }

    /// Plays the record's next entry, `entry`, at the game's open decision or halt. Returns
    /// whether it took the entry: false where it answered a decision the record lacks.
    fn replay_entry(&mut self, entry: &Entry, drift: &mut Drift) -> Result<bool, SessionError> {
        match (self.at.clone(), entry) {
            (At::Agent(d), Entry::Choice(chosen)) => {
                let Some(c) = self.found(&d, chosen, drift) else {
                    return self.lenient(&d, chosen, drift);
                };
                self.agent_decides(c);
            }
            (At::Human(_, Open::Decision(d)), Entry::Auto(chosen)) => {
                let Some(c) = self.found(&d, chosen, drift) else {
                    return self.lenient(&d, chosen, drift);
                };
                self.apply(&d, c, ChoiceOrigin::Auto);
                self.answered = Some(self.board.stack());
                self.at = At::Playing;
            }
            (At::Human(id, open @ Open::Decision(_)), Entry::Choice(chosen)) => {
                let Open::Decision(d) = &open else {
                    unreachable!("matched a decision")
                };
                let Some(c) = self.found(d, chosen, drift) else {
                    return self.lenient(d, chosen, drift);
                };
                let answer = OpenDecision::new(id, &open, false, false)
                    .answer(id, CandidateIndex(c.0))
                    .expect("a found candidate is in range");
                self.choose(Answer {
                    continuation: chosen.continuation,
                    ..answer
                });
            }
            (At::Human(..), Entry::Stops(stops)) => self.change_stops(*stops),
            (At::Human(_, Open::Decision(_)), Entry::PassOn) => {
                if !self.start_passing() {
                    return Err(self.parted("a pass on where there is none"));
                }
            }
            (At::Human(_, Open::Halt), Entry::Halt) => self.answer_first(),
            (At::Human(..), Entry::Undo) => {
                self.undo()
                    .map_err(|_| self.parted("an undo with nothing to take back"))?;
            }
            (At::Agent(_) | At::Human(..), Entry::Concede) => {
                self.concede();
            }
            (At::Over { .. }, _) => return Err(self.parted("the log goes on after the game ended")),
            // A halt the record lacks: the human goes on from it, their one candidate.
            (At::Human(_, Open::Halt), _) => {
                self.lenient_note("went on from a halt the record lacks", entry, drift);
                self.answer_first();
                return Ok(false);
            }
            // A recorded halt that no longer happens: going on from it changed nothing.
            (At::Agent(_) | At::Human(..), Entry::Halt) => {
                self.lenient_note("skipped a halt the game no longer makes", entry, drift);
            }
            (_, entry) => {
                return Err(self.parted(&format!("{entry:?} where the game does not ask it")));
            }
        }
        Ok(true)
    }

    /// The candidate of `d` that does what `chosen` did, if any; one found elsewhere than its
    /// recorded index counts as drift.
    fn found(&mut self, d: &Decision<Large>, chosen: &Chosen, drift: &mut Drift) -> Option<Choice> {
        let c = on_board!(&mut self.board, b => chosen.find(
            &self.table,
            view(&b.state, &self.table, &mut b.peek),
            &d.relayout(),
        ))?;
        if c.0 != chosen.index {
            drift.moved += 1;
        }
        Some(c)
    }

    /// Goes on where no candidate of `d` does what `chosen` did, if the rules leave nothing to
    /// choose ([`Game::from_replay`]). Returns whether it took the entry.
    fn lenient(
        &mut self,
        d: &Decision<Large>,
        chosen: &Chosen,
        drift: &mut Drift,
    ) -> Result<bool, SessionError> {
        let entry = Entry::Choice(chosen.clone());
        if d.len() == 1 {
            self.lenient_note("answered a decision the record lacks", &entry, drift);
            self.answer_first();
            return Ok(false);
        }
        let now = on_board!(&self.board, b => save::when(&b.state));
        let when = chosen.when();
        // Every decision is recorded: one before the record's next, which the original game
        // never asked, is a priority it passed unasked.
        if d.kind == DecisionKind::Priority && when.is_some_and(|w| w > now) {
            self.lenient_note("passed a priority the record lacks", &entry, drift);
            debug_assert_eq!(d.candidates.option(Choice(0)), Some(Opt::Pass));
            self.answer_first();
            return Ok(false);
        }
        if chosen.is_priority_pass()
            && (d.kind != DecisionKind::Priority || when.is_some_and(|w| w < now))
        {
            self.lenient_note("skipped a pass where no priority is open", &entry, drift);
            return Ok(true);
        }
        Err(self.parted(&format!(
            "no candidate of player {}'s {:?} decision does {}",
            d.player,
            d.kind,
            serde_json::to_string(chosen).expect("an answer serializes"),
        )))
    }

    /// Answers the open decision, the agent's or the human's, or the human's halt, with its
    /// first candidate.
    fn answer_first(&mut self) {
        match self.at.clone() {
            At::Human(id, open) => {
                let answer = OpenDecision::new(id, &open, false, false)
                    .answer(id, CandidateIndex(0))
                    .expect("a decision has a first candidate");
                self.choose(answer);
            }
            At::Agent(_) => self.agent_decides(Choice(0)),
            _ => unreachable!("answered at a decision"),
        }
    }

    /// Journals a place where replay went on though the record and the game parted.
    fn lenient_note(&self, what: &str, entry: &Entry, drift: &mut Drift) {
        drift.lenient += 1;
        let turn = self.board.turn();
        journal::info(
            "replay",
            what,
            json!({ "turn": turn.number, "step": format!("{:?}", turn.step), "entry": entry }),
        );
    }

    /// Why a replay stops where the record and the game parted: `what`, at this turn and step.
    fn parted(&self, what: &str) -> SessionError {
        let turn = self.board.turn();
        SessionError::BadReplay {
            reason: format!("turn {} {:?}: {what}", turn.number, turn.step),
        }
    }

    /// The next view as the game plays on to the human's next decision or the end: as a step
    /// of the turn begins, before each of the agent's decisions, at the human's decision, and
    /// at the end. None while the human's decision is open, until they answer it, and once
    /// the end was shown. With the stop flag set, the agent's next decision is not made: the
    /// game shows that it stopped.
    pub fn next_view(&mut self) -> Option<PlayerView> {
        match &self.at {
            At::Human(..) | At::Over { .. } | At::Stopped => return None,
            At::Agent(_) if self.stop.load(Ordering::Relaxed) => self.at = At::Stopped,
            At::Agent(d) => {
                let started = Instant::now();
                let kind = d.kind.into();
                let (c, candidates) = on_board!(&mut self.board, b => {
                    let obs = observe(&b.state, &self.table, &mut b.scratch, d.player);
                    let d = d.relayout();
                    let labels = crate::protocol::candidate_labels(&self.table, obs, &d);
                    (self.agent.choose(obs, &d), labels)
                });
                self.note_agent_move(kind, candidates, c, started);
                self.agent_decides(c);
                self.play_on(false);
            }
            At::Playing => self.play_on(false),
        }
        Some(self.view())
    }

    /// The human's open decision, if any.
    pub fn open(&self) -> Option<OpenDecision> {
        match &self.at {
            At::Human(id, open) => Some(OpenDecision {
                passes_on: self.passes_on.is_some(),
                ..OpenDecision::new(*id, open, self.cancellable(), self.undoable())
            }),
            _ => None,
        }
    }

    /// Whether the human's open decision is part of a spell they are casting or an ability
    /// they are activating from their priority, whose costs are not yet paid: they may take
    /// it back ([`Game::cancel`]).
    fn cancellable(&self) -> bool {
        self.before_cast.is_some()
            && matches!(self.at, At::Human(_, Open::Decision(_)))
            && self.casting()
    }

    /// Whether a spell is being cast or an ability activated, short of being paid for: not a
    /// triggered ability's targets being chosen, which the engine asks the same way once a
    /// cast is complete ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d):
    /// "The remainder of the process for putting a triggered ability on the stack is
    /// identical to the process for casting a spell").
    fn casting(&self) -> bool {
        self.board.casting() == Some(false)
    }

    /// Takes back the spell the human is casting or the ability they are activating, before
    /// its costs are paid, and returns the view of their priority again, as it was before they
    /// began. The choice log forgets the cast, so a replay never sees it. Like a client's
    /// cancel button, this follows the rule for a cast that cannot be completed
    /// ([CR 601.2](https://yawgatog.com/resources/magic-rules/#R6012)): "the casting of the
    /// spell is illegal; the game returns to the moment before the casting of that spell was
    /// proposed".
    pub fn cancel(&mut self) -> Result<PlayerView, SessionError> {
        if self.open().is_none() {
            return Err(SessionError::NotDeciding);
        }
        if !self.cancellable() {
            return Err(SessionError::NotCancellable);
        }
        let before = self
            .before_cast
            .take()
            .expect("a cancellable cast began somewhere");
        self.board.restore(&before.state);
        self.answered = before.answered;
        self.replay.log.truncate(before.log);
        self.undo
            .truncate(before.undo.saturating_sub(self.undo_dropped));
        // Stops changed during the cast stay changed, recorded again at the priority.
        if self.stops != before.stops {
            self.change_stops(self.stops);
        }
        // The priority asked again is a new decision: an answer sent to the cast's is stale.
        let At::Human(_, open) = before.at else {
            unreachable!("a cast begins at the human's decision");
        };
        self.seq += 1;
        self.at = At::Human(self.decision_id(), open);
        self.board.drain_events();
        // The view says nothing happened: the cast never was.
        self.events.clear();
        Ok(self.draw())
    }

    /// Whether the human, at their open decision, may take back an answer ([`Game::undo`]).
    fn undoable(&self) -> bool {
        matches!(self.at, At::Human(..)) && !self.undo.is_empty()
    }

    /// Takes back the human's latest answer, at their open decision, and returns the view of
    /// the decision or halt it answered, asked again. A follow-up sent by the client as part
    /// of that same choice (such as a cast's payment plan) goes back with it.
    /// Each call takes back one more, as far back as the [`UNDO_POINTS`] kept, across the
    /// agent's decisions and turns: the agent goes back with the game
    /// ([`Agent::rewind`]) and decides again as play goes on, from the seed it had, so the
    /// same position gets the same search. This is no rule of the game but a practice tool
    /// the human asked for, and it shows them what they would not know: what the agent did,
    /// and the cards drawn since, which come out again in the same order. The choice log keeps
    /// the answers and the agent's choices taken back, then an [`Entry::Undo`], so a replay
    /// goes back the same way.
    pub fn undo(&mut self) -> Result<PlayerView, SessionError> {
        if self.open().is_none() {
            return Err(SessionError::NotDeciding);
        }
        if !self.undoable() {
            return Err(SessionError::NothingToUndo);
        }
        let mut point = self.undo.pop_back().expect("an undoable game has a point");
        while point.continuation {
            let Some(previous) = self.undo.pop_back() else {
                break;
            };
            point = previous;
        }
        self.board.restore(&point.state);
        self.answered = point.answered;
        self.seen = point.seen;
        self.before_cast = point.before_cast;
        self.hurt_by = point.hurt_by;
        self.passing = None;
        if let Some(to) = point.agent
            && self.agent_next() != Some(to)
        {
            match &self.board {
                Boards::Small(_) => Agent::<Small>::rewind(&mut *self.agent, to),
                Boards::Large(_) => Agent::<Large>::rewind(&mut *self.agent, to),
            }
        }
        self.replay.log.push(Entry::Undo);
        // Stops changed since stay changed. The log has them already, and a replay goes back
        // with the same ones.
        self.board
            .set_profile(with_stops(self.board.profile(), self.stops, self.human));
        // What the view there reported of a paused resolution, as it was reported then.
        self.paused = on_board!(&mut self.board, b => {
            paused_events(&b.state, &self.table, &mut b.scratch)
                .iter()
                .map(Relayout::relayout)
                .collect()
        });
        let At::Human(_, open) = point.at else {
            unreachable!("an undo point is at the human's decision");
        };
        // The decision asked again is a new one: an answer sent to an earlier one is stale.
        self.seq += 1;
        self.at = At::Human(self.decision_id(), open);
        self.board.drain_events();
        self.events.clear();
        let mut view = self.draw();
        view.undone = true;
        Ok(view)
    }

    /// The agent's next decision among its own, in the layout the game is in.
    fn agent_next(&self) -> Option<AgentDecision> {
        match &self.board {
            Boards::Small(_) => Agent::<Small>::next_decision(&*self.agent),
            Boards::Large(_) => Agent::<Large>::next_decision(&*self.agent),
        }
    }

    /// Keeps the position at the human's open decision, before their answer is applied, as one
    /// they may go back to, dropping the oldest beyond [`UNDO_POINTS`].
    fn keep_undo_point(&mut self, continuation: bool) {
        self.undo.push_back(UndoPoint {
            state: self.board.snapshot(),
            continuation,
            at: self.at.clone(),
            seen: self.seen,
            answered: self.answered.clone(),
            before_cast: self.before_cast.clone(),
            agent: self.agent_next(),
            hurt_by: self.hurt_by,
        });
        if self.undo.len() > UNDO_POINTS {
            self.undo.pop_front();
            self.undo_dropped += 1;
        }
    }

    /// How many undo points stand before the human's open decision, counting those dropped
    /// beyond [`UNDO_POINTS`]: the decision's place among their answers, the same whenever an
    /// undo comes back to it ([`crate::protocol::DecisionView::undo_depth`]).
    fn undo_depth(&self) -> usize {
        self.undo_dropped + self.undo.len()
    }

    /// Keeps the agent's decision just made, with its search, and journals it.
    fn note_agent_move(
        &mut self,
        kind: crate::protocol::Kind,
        candidates: Vec<String>,
        c: Choice,
        started: Instant,
    ) {
        let search = match &self.board {
            Boards::Small(_) => Agent::<Small>::last_search(&*self.agent),
            Boards::Large(_) => Agent::<Large>::last_search(&*self.agent),
        }
        .map(SearchView::of);
        let turn = self.board.turn();
        let m = AgentMove {
            log: self.replay.log.len(),
            turn: turn.number,
            step: turn.step.into(),
            kind,
            candidates,
            chosen: c.0,
            ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            search,
        };
        journal::debug(
            "agent",
            m.candidates
                .get(usize::from(c.0))
                .cloned()
                .unwrap_or_default(),
            json!({
                "session": self.session.0,
                "log": m.log,
                "kind": m.kind,
                "candidates": m.candidates.len(),
                "ms": m.ms,
                "rollouts": m.search.as_ref().map(|s| s.rollouts),
            }),
        );
        ai::keep(&self.moves, m);
    }

    /// The agent's latest decisions, shared: the session reads them for a report.
    pub fn agent_moves(&self) -> AgentMoves {
        Arc::clone(&self.moves)
    }

    fn agent_decides(&mut self, c: Choice) {
        let At::Agent(d) = self.at.clone() else {
            unreachable!("the agent decides at its decision");
        };
        self.apply(&d, c, ChoiceOrigin::Explicit);
        self.at = At::Playing;
    }

    /// Passes the human's priority, at their open decision, with `answer` (checked by
    /// [`OpenDecision::pass_on`]), and goes on passing it for them until their next stop in a
    /// later step: where passing would stop them again in this one, for the stack resolving or
    /// combat damage, this goes on past it, through whatever the opponent does: the human
    /// asked to pass until then, as Arena's end-turn button does. They stop sooner only at a
    /// decision other than a priority. The choice log records an [`Entry::PassOn`] before the pass, so a replay
    /// passes on the same way. [`Game::next_view`] plays on from it.
    pub fn pass_on(&mut self, answer: Answer) {
        let started = self.start_passing();
        debug_assert!(started, "a pass on is checked against the open decision");
        self.choose(answer);
    }

    /// Begins passing on to the human's next stop from their open decision, if it can pass on,
    /// and records it. Returns whether it began.
    fn start_passing(&mut self) -> bool {
        let Some(until) = self.pass_on_target() else {
            return false;
        };
        self.passing = Some(PassingOn { until });
        self.replay.log.push(Entry::PassOn);
        true
    }

    /// The turn and step of the human's next stop in a later step, if their open decision is a
    /// priority from which passing would stop them again in this one
    /// ([`crate::protocol::DecisionView::pass_on`]).
    fn pass_on_target(&mut self) -> Option<(u16, TurnStep)> {
        if self.reviewing()
            || !matches!(&self.at, At::Human(_, Open::Decision(d)) if d.kind == DecisionKind::Priority)
        {
            return None;
        }
        let next = on_board!(&mut self.board, b => {
            let obs = Observation::new(view(&b.state, &self.table, &mut b.scratch), self.human);
            crate::protocol::pass_on(obs, self.stops)
        })?;
        let turn = self.board.turn().number;
        Some(match next {
            NextStop::Step(step) => (turn, step),
            NextStop::NextTurn => (turn + 1, TurnStep::Untap),
            NextStop::Resolve | NextStop::CombatDamage => {
                unreachable!("a later step's stop is a step or the next turn")
            }
        })
    }

    /// Whether the session passes the human's priority for them as they pass on to their next
    /// stop ([`Game::pass_on`]): short of it, whatever the opponent puts on the stack or
    /// attacks with meanwhile.
    fn passes_for(&self, passing: PassingOn) -> bool {
        let turn = self.board.turn();
        (turn.number, turn.step) < passing.until
    }

    /// Checks `index` as an answer to the human's open decision `id`.
    pub fn answer(&self, id: DecisionId, index: CandidateIndex) -> Result<Answer, SessionError> {
        self.open()
            .ok_or(SessionError::NotDeciding)?
            .answer(id, index)
    }

    /// Previews the human's selected waterbenders on a copy, without advancing the game.
    pub fn preview_waterbend(
        &self,
        request: &crate::ipc::WaterbendRequest,
    ) -> Result<Option<crate::ipc::WaterbendPreview>, SessionError> {
        self.answer(request.decision, CandidateIndex(0))?;
        let At::Human(_, Open::Decision(d)) = &self.at else {
            return Err(SessionError::NotDeciding);
        };
        if d.kind != DecisionKind::Waterbend {
            return Ok(None);
        }
        Ok(
            on_board!(&self.board, b => crate::preview::waterbend(&b.state, &self.table, &d.relayout(), &request.selected)),
        )
    }

    /// Applies an answer to the human's open decision; [`Game::next_view`] plays on from it.
    pub fn choose(&mut self, answer: Answer) {
        let At::Human(id, open) = self.at.clone() else {
            panic!("an answer is checked against the open decision");
        };
        debug_assert_eq!(id, answer.id, "an answer names the open decision");
        let continuation = answer.continuation && self.casting();
        self.keep_undo_point(continuation);
        match open {
            Open::Decision(d) => {
                // A cast or an activation from priority can be taken back until it is paid for.
                let before = (d.kind == DecisionKind::Priority).then(|| BeforeCast {
                    state: self.undo.back().expect("a point just kept").state.clone(),
                    at: self.at.clone(),
                    answered: self.answered.clone(),
                    log: self.replay.log.len(),
                    stops: self.stops,
                    undo: self.undo_depth() - 1,
                });
                let origin = if continuation {
                    ChoiceOrigin::Continuation
                } else {
                    ChoiceOrigin::Explicit
                };
                self.apply(&d, answer.choice, origin);
                if let Some(before) = before {
                    self.before_cast = self.casting().then_some(before);
                }
            }
            Open::Halt => self.replay.log.push(Entry::Halt),
        }
        self.answered = Some(self.board.stack());
        self.at = At::Playing;
    }

    /// Answers the human's open decision as the stand-in would, if there is one
    /// ([`Game::set_stand_in`]) and a decision is open; it goes on from a halt. Returns
    /// whether it answered.
    pub fn answer_by_stand_in(&mut self) -> bool {
        if self.hand_over.as_ref().is_some_and(|h| h()) {
            self.stand_in = None;
            self.hand_over = None;
        }
        let (At::Human(id, open), Some(stand_in)) = (self.at.clone(), self.stand_in.as_mut())
        else {
            return false;
        };
        let c = match &open {
            Open::Decision(d) => on_board!(&mut self.board, b => {
                let d = without_floating(d.relayout());
                // These confirmations are for the human. A stand-in has only the one
                // ordinary payment choice and need not advance its random stream.
                if d.kind == DecisionKind::Waterbend && d.len() == 1 {
                    0
                } else {
                    let obs = observe(&b.state, &self.table, &mut b.scratch, self.human);
                    stand_in.choose(obs, &d).0
                }
            }),
            Open::Halt => 0,
        };
        let answer = OpenDecision::new(id, &open, false, false)
            .answer(id, CandidateIndex(c))
            .expect("an agent chooses a candidate");
        self.choose(answer);
        true
    }

    /// Changes the human's priority stops from their open decision on, and returns the view
    /// again with the new setting. The setting applies to both seats: the state holds one
    /// profile.
    pub fn set_stops(&mut self, stops: StopsSetting) -> Result<PlayerView, SessionError> {
        if self.open().is_none() {
            return Err(SessionError::NotDeciding);
        }
        self.change_stops(stops);
        // Nothing happened since the previous view.
        self.events.clear();
        Ok(self.draw())
    }

    /// The human concedes, and the view of the game's end is returned. A concession is taken
    /// where the game waits on a decision, the human's or the agent's (before the agent
    /// thinks), which a replay finds again; elsewhere, and once the game is over, there is
    /// none. A cast in progress is taken back first ([`Game::cancel`]), so the end shows the
    /// table as it stood.
    pub fn concede(&mut self) -> Option<PlayerView> {
        if !matches!(self.at, At::Agent(_) | At::Human(..)) {
            return None;
        }
        if self.cancellable() {
            self.cancel().expect("a cancellable cast can be taken back");
        }
        self.before_cast = None;
        on_board!(&mut self.board, b => beryl_engine::concede(&mut b.state, &mut b.scratch, self.human));
        self.replay.log.push(Entry::Concede);
        let outcome = self.board.outcome().expect("a concession ends the game");
        self.at = At::Over {
            outcome,
            cause: Some(LossCause::Concede),
            hurt_by: None,
        };
        Some(self.view())
    }

    /// A flag that, once set, stops the game before the agent's next decision: the game in
    /// progress returns a stopped view, and no decision opens again. A session sets it when it
    /// is dropped, so an abandoned game stops thinking.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    /// The session this game is.
    pub fn session(&self) -> SessionId {
        self.session
    }

    /// The record of the game so far.
    pub fn replay(&self) -> &ReplayFile {
        &self.replay
    }

    /// Records that the build at `commit` writes the game's replay from here on.
    pub(crate) fn set_commit(&mut self, commit: String) {
        self.replay.commit = Some(commit);
    }

    /// The game itself, for tests, while it is in [`Small`].
    ///
    /// # Panics
    ///
    /// If the game has been widened ([`Game::large`]).
    pub fn state(&self) -> &State {
        match &self.board {
            Boards::Small(b) => &b.state,
            Boards::Large(_) => panic!("the game has been widened into Large"),
        }
    }

    /// The game itself, for tests, once it has been widened into [`Large`].
    pub fn large(&self) -> Option<&State<Large>> {
        match &self.board {
            Boards::Small(_) => None,
            Boards::Large(b) => Some(&b.state),
        }
    }

    pub fn table(&self) -> &GameTable {
        &self.table
    }

    /// The engine's events the latest view reported, for tests.
    pub fn events(&self) -> &[Event<Large>] {
        &self.events
    }

    /// Plays on from [`At::Playing`] to the next place a view shows: a step beginning, where
    /// the human may be halted, a decision, or the end.
    fn play_on(&mut self, replaying: bool) {
        debug_assert!(matches!(self.at, At::Playing));
        loop {
            let step = self
                .board
                .next_in_step(&self.table, self.seen, &mut self.chance);
            self.log_chance();
            // Once a cast is complete, or another player decides, it can no longer be taken back.
            if !matches!(&step, Some(Step::Decide(d)) if d.player == self.human && self.casting()) {
                self.before_cast = None;
            }
            self.at = match step {
                None => {
                    let turn = self.board.turn();
                    let began = turn.number != self.seen.0;
                    self.seen = (turn.number, turn.step);
                    self.answered = None;
                    if began {
                        let checkpoint =
                            on_board!(&self.board, b => Checkpoint::of(&self.table, &b.state));
                        self.replay
                            .log
                            .push(Entry::Checkpoint(Box::new(checkpoint)));
                    }
                    if self.halts() {
                        self.seq += 1;
                        At::Human(self.decision_id(), Open::Halt)
                    } else {
                        At::Playing
                    }
                }
                Some(Step::Over(outcome)) => At::Over {
                    outcome,
                    cause: None,
                    hurt_by: None,
                },
                Some(Step::Decide(d)) if d.player == self.human || self.reviewing() => {
                    if !self.reviewing()
                        && let Some(c) = self.auto_answer(&d, replaying)
                    {
                        self.apply(&d, c, ChoiceOrigin::Auto);
                        self.answered = Some(self.board.stack());
                        continue;
                    }
                    self.seq += 1;
                    At::Human(self.decision_id(), Open::Decision(d))
                }
                Some(Step::Decide(d)) => At::Agent(d),
            };
            if matches!(self.at, At::Human(..)) {
                self.passing = None;
            }
            return;
        }
    }

    /// The session's own answer to the human's decision `d`, if it gives one: under the
    /// smart stops, it passes their priority while only their own doings sit on the stack
    /// ([`Game::own_top`]) or in combat with nothing new to answer ([`Game::quiet_combat`]),
    /// orders their simultaneous triggers ([`trigger_to_stack`]), searches their library
    /// whenever they may ([`Game::searches_unasked`]), and takes unambiguous lethal combat
    /// assignments ([`beryl_engine::preferred_damage`]). Waterbend remains a human payment
    /// choice even when mana alone could pay it.
    /// Like halts, these are the session's alone: the engine's smart stops, which the agent's
    /// search shares, are unchanged, and exact stops ask every one. A function of the position,
    /// the stops and the human's answers, so a replay finds each again; the replay records
    /// each as an [`Entry::Auto`]. Replays play recorded damage assignments directly,
    /// including any manual choices made before automatic assignment was added.
    fn auto_answer(&self, d: &Decision<Large>, replaying: bool) -> Option<Choice> {
        if d.kind == DecisionKind::Priority && self.passing.is_some_and(|p| self.passes_for(p)) {
            debug_assert_eq!(d.candidates.option(Choice(0)), Some(Opt::Pass));
            return Some(Choice(0));
        }
        if self.stops.full_control {
            return None;
        }
        match d.kind {
            DecisionKind::Priority if self.own_top() || self.quiet_combat() => {
                debug_assert_eq!(d.candidates.option(Choice(0)), Some(Opt::Pass));
                Some(Choice(0))
            }
            DecisionKind::TriggerOrder => Some(on_board!(&self.board, b => {
                trigger_to_stack(&b.state, &self.table, &d.relayout())
            })),
            DecisionKind::DamageDivision if !replaying => on_board!(&self.board, b => {
                beryl_engine::preferred_damage(&b.state, &d.relayout())
                    .and_then(|(choice, unambiguous)| unambiguous.then_some(choice))
            }),
            DecisionKind::YesNo if Self::searches_unasked(d) => (0..d.len() as u16)
                .map(Choice)
                .find(|&c| d.candidates.option(c) == Some(Opt::Yes)),
            _ => None,
        }
    }

    /// Whether `d` asks whether to search the human's library (Trinket Mage's "you may
    /// search"), which the smart stops answer by searching, as Arena does: the search itself
    /// offers taking nothing. Declining differs from searching and finding nothing
    /// ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b): "If a player is
    /// searching a hidden zone for cards with a stated quality ... that player isn't required
    /// to find some or all of those cards") only by the shuffle
    /// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a): "To shuffle a
    /// library ..., randomize the cards within it so that no player knows their order"). That
    /// shuffle loses a player a card they put back on top, a cost not worth a question before
    /// every search: a player who must keep it plays under exact stops, which ask.
    fn searches_unasked(d: &Decision<Large>) -> bool {
        d.ask == Ask::MaySearch
    }

    /// Whether the top of the stack is the human's own spell or ability and, since their
    /// latest answer in this step, nothing of the opponent's came onto the stack: what they
    /// cast, its triggers, and its resolution, or the resolution of an opponent's response
    /// they already passed to, but not a new response, which they get priority to see and
    /// answer. Their priority then would only let them respond to themselves, as an Arena
    /// player auto-passes.
    fn own_top(&self) -> bool {
        let answered = self.answered.as_deref().unwrap_or_default();
        let now = self.board.stack();
        let own = |e: &StackEntry<Large>| e.controller == self.human;
        let kept = answered
            .iter()
            .zip(&now)
            .take_while(|(then, now)| then == now)
            .count();
        now.last().is_some_and(own) && now[kept..].iter().all(own)
    }

    /// Whether the human's priority, with the stack empty, comes in a combat step where they
    /// have nothing new to answer: after declaring their own attackers, which they chose
    /// knowing all a response could use, or after a declaration of no blockers, which leaves
    /// no block for a trick to change. Players get priority in each of those steps once its
    /// declaration is made
    /// ([CR 117.3a](https://yawgatog.com/resources/magic-rules/#R1173a): "The active player
    /// receives priority at the beginning of most steps and phases, after any turn-based
    /// actions ... have been dealt with"), and the engine's smart stops, which the agent's
    /// search shares, give it to both players whenever creatures attack. Like Arena, the
    /// session stops the human after the opponent's attack, and after blocks.
    fn quiet_combat(&self) -> bool {
        if !self.board.stack_is_empty() {
            return false;
        }
        let turn = self.board.turn();
        let attacker = turn.active == self.human;
        match turn.step {
            TurnStep::DeclareAttackers => attacker,
            TurnStep::DeclareBlockers => !self.any_blocking(),
            _ => false,
        }
    }

    fn any_blocking(&self) -> bool {
        self.board.any_blocking()
    }

    /// Whether the human is halted as the step that just began, a session-level stop the
    /// engine knows nothing of: under the smart stops, at their own precombat main phase while
    /// it is one of their stops ([`StopsSetting::yours`]), unless the engine offers them a decision there before anything else is decided. The
    /// game would otherwise pass through it unseen whenever the human can do nothing but
    /// pass, and their turn would end without them. The opponent's turn plays on to theirs
    /// unhalted, as on Arena: the engine stops the human at its end step only when they can
    /// act there. Halts are a function of the position and the stops, so a replay finds them
    /// again; the agent never sees them.
    fn halts(&mut self) -> bool {
        let turn = self.board.turn();
        let halting_step = turn.step == TurnStep::PrecombatMain
            && turn.active == self.human
            && self.stops.yours.has(TurnStep::PrecombatMain);
        if self.review.is_some() || self.stops.full_control || !halting_step {
            return false;
        }
        match self.board.ahead(&self.table, self.seen, &self.chance) {
            Some(Step::Decide(d)) => d.player != self.human,
            Some(Step::Over(_)) => false,
            None => true,
        }
    }

    fn change_stops(&mut self, stops: StopsSetting) {
        self.stops = stops;
        self.board
            .set_profile(with_stops(self.board.profile(), stops, self.human));
        self.replay.log.push(Entry::Stops(stops));
    }

    fn decision_id(&self) -> DecisionId {
        DecisionId {
            session: self.session,
            seq: self.seq,
        }
    }

    /// Records candidate `c` of `d` by what it does in the
    /// position it is asked in (in a paused resolution, its body done up to the question: a
    /// card it drew is in the hand), and applies it.
    fn apply(&mut self, d: &Decision<Large>, c: Choice, origin: ChoiceOrigin) {
        let mut chosen = on_board!(&mut self.board, b => Chosen::new(
            &self.table,
            view(&b.state, &self.table, &mut b.peek),
            &d.relayout(),
            c,
        ));
        chosen.continuation = matches!(origin, ChoiceOrigin::Continuation);
        self.replay
            .log
            .push(if matches!(origin, ChoiceOrigin::Auto) {
                Entry::Auto(chosen)
            } else {
                Entry::Choice(chosen)
            });
        self.board.apply(&self.table, c, &mut self.chance);
        self.log_chance();
    }

    /// Logs what the game's random events produced since the last log, after the entry that
    /// led to them.
    fn log_chance(&mut self) {
        for outcome in self.chance.take() {
            self.replay.log.push(Entry::of(&self.table, outcome));
        }
    }

    /// The human's view now, with the engine's events since the previous view. A view in a
    /// paused resolution shows its body done up to its question (§7.3), so it reports the
    /// events of that, once: the card a loot draws comes into the hand before the discard is
    /// asked, not after. The body executes them again whenever it runs on, after an answer,
    /// and those repeats go unreported.
    fn view(&mut self) -> PlayerView {
        let mut fresh = self.board.drain_events();
        let seen = self.paused.len();
        if seen > 0
            && let Some(at) = fresh.windows(seen).position(|w| w == self.paused)
        {
            fresh.drain(at..at + seen);
        }
        let paused: Vec<Event<Large>> = on_board!(&mut self.board, b => {
            paused_events(&b.state, &self.table, &mut b.scratch)
                .iter()
                .map(Relayout::relayout)
                .collect()
        });
        fresh.extend_from_slice(
            paused
                .strip_prefix(self.paused.as_slice())
                .unwrap_or(&paused),
        );
        for e in &fresh {
            match *e {
                Event::Damage {
                    source,
                    to: Recipient::Player(player),
                    ..
                } => self.hurt_by[player.index()] = Some(source),
                Event::LifeLoss { player, .. } => self.hurt_by[player.index()] = None,
                _ => {}
            }
        }
        // The loser's cause, from the event that ended the game.
        if let At::Over {
            outcome: Outcome::Win(winner),
            cause: cause @ None,
            hurt_by,
        } = &mut self.at
        {
            let loser = winner.opponent();
            *cause = fresh.iter().find_map(|e| match *e {
                Event::Lose { player, cause } if player == loser => Some(cause),
                _ => None,
            });
            *hurt_by = self.hurt_by[loser.index()];
        }
        self.events = fresh;
        self.paused = paused;
        self.draw()
    }

    /// The human's view now, with the events of the latest view.
    fn draw(&mut self) -> PlayerView {
        let view = self.draw_for(self.human);
        self.passes_on = self.pass_on_target();
        view
    }

    fn draw_for(&mut self, viewer: Seat) -> PlayerView {
        // A paused resolution's entry is still on top of the committed stack; the position
        // replayed to its question has taken it off.
        let resolving = self
            .board
            .resolving()
            .then(|| self.board.stack().pop())
            .flatten();
        let mut view = on_board!(&mut self.board, b => {
            // The session's decision, stack entry and events, in the game's layout.
            let decision = match &self.at {
                At::Agent(d) | At::Human(_, Open::Decision(d)) => Some(d.relayout()),
                _ => None,
            };
            let resolving = resolving.as_ref().map(Relayout::relayout);
            let events: Vec<_> = self.events.iter().map(Relayout::relayout).collect();
            let moment = match (&self.at, &decision) {
                (At::Playing, _) => Moment::Playing,
                (At::Human(id, Open::Decision(_)), Some(d)) if d.player == viewer => {
                    Moment::Deciding(*id, d, resolving.as_ref())
                }
                (At::Human(id, Open::Halt), _) if viewer == self.human => Moment::Halted(*id),
                (At::Agent(_) | At::Human(..), _) => Moment::Thinking {
                    resolving: resolving.as_ref(),
                    decision: decision.as_ref(),
                },
                (
                    At::Over {
                        outcome,
                        cause,
                        hurt_by,
                    },
                    _,
                ) => Moment::Over {
                    outcome: *outcome,
                    cause: cause.as_ref().map(Relayout::relayout),
                    hurt_by: hurt_by.as_ref().map(Relayout::relayout),
                },
                (At::Stopped, _) => Moment::Stopped,
            };
            let shown = view(&b.state, &self.table, &mut b.scratch);
            let obs = Observation::new(shown, viewer);
            let mut view = player_view(&self.table, obs, moment, self.stops, &events);
            // Let the opponent watch the Waterbend payment before the activation
            // lifts from its source. The controller's payment view still needs the
            // pending stack item to identify what they are paying for.
            if matches!(shown.pending(), Pending::Casting(_))
                && matches!(
                    view.stack.last(),
                    Some(StackItem::Ability {
                        controller: Side::Opponent,
                        waterbend: Some(true),
                        ..
                    })
                )
            {
                view.stack.pop();
            }
            if let Some(d) = &decision
                && d.player == viewer
            {
                preview::add_previews(&mut view, &b.state, &self.table, d);
            }
            view
        });
        view.turn.flip_winner = self.flip_winner.map(|p| {
            if p == viewer {
                Side::You
            } else {
                Side::Opponent
            }
        });
        if let Status::Deciding { decision } = &mut view.status {
            if self.reviewing() {
                decision.pass_on = None;
            }
            decision.cancellable = self.cancellable();
            decision.undo = if self.undoable() {
                u16::try_from(self.undo.len()).unwrap_or(u16::MAX)
            } else {
                0
            };
            decision.undo_depth = u32::try_from(self.undo_depth()).unwrap_or(u32::MAX);
        }
        view
    }
}

fn new_agent(
    name: &str,
    t: &GameTable,
    seed: u64,
) -> Result<Box<dyn GameAgent + Send>, SessionError> {
    let agent = beryl_search::agent(name).ok_or_else(|| SessionError::UnknownAgent {
        name: name.to_owned(),
    })?;
    Ok((agent.new)(t, seed, Budget::DEFAULT))
}

/// Which of the human's waiting triggers to put on the stack next, under the smart stops. Each
/// pick goes on the stack above the ones before, so the first picked resolves last
/// ([CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b): "each player, in APNAP
/// order, puts each triggered ability they control with a trigger condition that has been
/// met on the stack in any order they choose").
///
/// The heuristic, by what the engine knows of each ability, resolves:
///
/// 1. first, the triggers with neither targets nor a question as they resolve: plain effects
///    (cards, tokens, counters, life, a sacrifice), whose results the others may act on;
/// 2. then the ones with targets, which were chosen as they went on the stack (CR 603.3d);
/// 3. last, the ones that ask their controller something as they resolve, so the answer is
///    made seeing everything the others did;
///
/// and, within each group, in the order they triggered.
pub fn trigger_to_stack<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>) -> Choice {
    let resolves_late = |opt: Opt<L>| {
        let Opt::Trigger(i) = opt else {
            unreachable!("a trigger-order decision offers only triggers");
        };
        let tr = &s.triggers()[Into::<usize>::into(i)];
        let ability = &t.def(tr.card).face(tr.face).abilities[usize::from(tr.ability)];
        let group = if ability.may_pause {
            2
        } else {
            u8::from(!ability.targets.is_empty())
        };
        (group, i)
    };
    let Candidates::Options(triggers) = &d.candidates else {
        unreachable!("a trigger-order decision offers options");
    };
    let (last, _) = triggers
        .iter()
        .enumerate()
        .max_by_key(|&(_, &opt)| resolves_late(opt))
        .expect("a decision has a candidate");
    Choice(u16::try_from(last).expect("a choice names its candidate"))
}

/// A payment that v8 completed without a prompt: it needs no benders and none of its
/// optional mana activations were offered at the time.
fn only_nonland_mana<L: Layout>(s: &State<L>, d: &Decision<L>) -> bool {
    d.candidates.option(Choice(0)) == Some(Opt::Done)
        && (1..d.len() as u16).all(|i| {
            matches!(d.candidates.option(Choice(i)), Some(Opt::Mana { slot, .. })
                if !s.derived(slot).types.contains(beryl_engine::card::Types::LAND))
        })
}

/// `d` without the optional mana activations offered at human priority or during waterbend
/// ([`Profile::taps_mana`]), for an agent standing in for them: tapping ahead of a payment is
/// dominated by tapping during it (architecture §10), and search never sees them. The engine
/// lists them last, so every other candidate keeps its index.
fn without_floating<L: Layout>(mut d: Decision<L>) -> Decision<L> {
    if let (DecisionKind::Priority | DecisionKind::Waterbend, Candidates::Options(list)) =
        (d.kind, &mut d.candidates)
    {
        let kept = list
            .iter()
            .take_while(|o| !matches!(o, Opt::Mana { .. }))
            .count();
        debug_assert!(
            list.as_slice()[kept..]
                .iter()
                .all(|o| matches!(o, Opt::Mana { .. })),
            "mana activations come last"
        );
        list.retain(|o| !matches!(o, Opt::Mana { .. }));
    }
    d
}

/// `profile` under the human's `stops`. Full control asks the human every priority they get,
/// as Arena does, even where the engine would pass for them, and offers every activation;
/// their opponent's passes stay silent. Under the smart stops, the human stops in the steps
/// they chose, and the agent in the smart stops' own, as its search plays. The agent is not
/// offered an activation that would do nothing ([`Profile::hides_pointless`]); the human is,
/// but it makes no stop of its own ([`Profile::quiet_pointless`]). Whatever the stops, the
/// human picks each target of their own spells and abilities, even a lone one, so a click never
/// aims a spell for them ([`Profile::asks_targets`]), and may tap their mana sources at
/// priority, floating the mana, as an Arena player clicks a land ([`Profile::taps_mana`]). The
/// game's budgets are a player's ([`Limits::PLAYER`]): a human repeating a loop by hand never
/// runs into a rollout's.
pub fn with_stops(profile: Profile, stops: StopsSetting, human: Seat) -> Profile {
    let mut step_stops = [StepStops::SMART; 2];
    step_stops[human.index()] = stops.step_stops();
    // The human is offered every activation; under the smart stops, those that would do
    // nothing make no stop of their own (Profile::quiet_pointless).
    let mut hides_pointless = profile.hides_pointless;
    hides_pointless[human.index()] = false;
    Profile {
        stops: stops.stops(),
        step_stops,
        full_control: stops.full_control.then_some(human),
        asks_targets: Some(human),
        taps_mana: Some(human),
        hides_pointless,
        quiet_pointless: (!stops.full_control).then_some(human),
        limits: Limits::PLAYER,
        ..profile
    }
}
