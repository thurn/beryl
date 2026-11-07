//! A game between a human seat and an agent, played synchronously: the core a session runs on
//! its thread, and that tests drive directly. It plays on view by view ([`Game::next_view`]):
//! as each step of the turn begins, before each of the agent's decisions, at each of the
//! human's, and at the end, so the human sees the agent's turn as it is played. Under the
//! smart stops the session also halts the human where the engine would pass them by, and
//! answers some of their decisions for them: passing to their own spell or trigger and in
//! combat after their own attack or no blocks, and ordering their triggers.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use beryl_engine::event::Event;
use beryl_engine::layout::Queue;
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::{DECK_SLOTS, ObjectFlags, Pending, StackEntry, Turn};
use beryl_engine::{
    Ask, Candidates, CardIx, CardTable, Choice, Decision, DecisionKind, GameTable, Large, Layout,
    Limits, Observation, Opt, Outcome, Profile, Relayout, Replay, Scratch, Seat, SlotId, Small,
    StallReason, State, Step, StepStops, TurnStep, apply, next_in_step, observe, paused_events,
    view,
};
use beryl_search::{Agent, AgentDecision, Budget, GameAgent};

use crate::preview;
use crate::protocol::{
    CandidateIndex, DecisionId, Moment, PlayerView, SessionId, Side, Status, StopsSetting,
    UndoView, player_view,
};
use crate::replay::{Entry, ReplayFile};
use crate::{HandOver, Prepared, SessionError, StartGame};

/// The human's open decision, as a session records it: the one place an answer is checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenDecision {
    id: DecisionId,
    candidates: usize,
    /// The human may take back the cast it is part of ([`Game::cancel`]).
    cancellable: bool,
    /// The human may go back to the start of their turn ([`Game::undo`]).
    undoable: bool,
}

impl OpenDecision {
    fn new(id: DecisionId, open: &Open, cancellable: bool, undoable: bool) -> OpenDecision {
        OpenDecision {
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

    /// Checks that the human may go back from this decision, named `id`, to the start of their
    /// turn.
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
        })
    }
}

/// A checked answer to an open decision, which only [`OpenDecision::answer`] makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Answer {
    id: DecisionId,
    choice: Choice,
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
    /// The game is over; `conceded`, because the human conceded ([`Game::concede`]).
    Over { outcome: Outcome, conceded: bool },
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

/// Whether the engine stalled the game because a capacity of its layout ran out.
fn ran_out<L: Layout>(s: &State<L>) -> bool {
    s.outcome() == Some(Outcome::Stalled(StallReason::Capacity))
}

/// A copy of the game's state, in the layout it was in.
#[derive(Clone)]
enum Snapshot {
    Small(Box<State<Small>>),
    Large(Box<State<Large>>),
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

    /// Applies `c` to the open decision, widening the game if it runs out of capacity.
    fn apply(&mut self, t: &GameTable, c: Choice) {
        if let Boards::Small(b) = self {
            let (before, events) = (b.state, b.scratch.events.len());
            apply(&mut b.state, t, &mut b.scratch, &mut Replay, c);
            if !ran_out(&b.state) {
                return;
            }
            self.widen(&before, events);
        }
        let Boards::Large(b) = self else {
            unreachable!("a game widened")
        };
        apply(&mut b.state, t, &mut b.scratch, &mut Replay, c);
    }

    /// Plays on as [`next_in_step`] does, widening the game if it runs out of capacity.
    fn next_in_step(&mut self, t: &GameTable, seen: (u16, TurnStep)) -> Option<Step<Large>> {
        if let Boards::Small(b) = self {
            let (before, events) = (b.state, b.scratch.events.len());
            let step = next_in_step(&mut b.state, t, &mut b.scratch, &mut Replay, seen);
            if !ran_out(&b.state) {
                return step.map(|s| s.relayout());
            }
            self.widen(&before, events);
        }
        let Boards::Large(b) = self else {
            unreachable!("a game widened")
        };
        next_in_step(&mut b.state, t, &mut b.scratch, &mut Replay, seen)
    }

    /// What [`Boards::next_in_step`] would return, played on a copy.
    fn ahead(&mut self, t: &GameTable, seen: (u16, TurnStep)) -> Option<Step<Large>> {
        match self {
            Boards::Small(b) => {
                let mut ahead = b.state;
                let step = next_in_step(&mut ahead, t, &mut b.peek, &mut Replay, seen);
                if !ran_out(&ahead) {
                    return step.map(|s| s.relayout());
                }
                let mut wide = b.state.widen();
                next_in_step(&mut wide, t, &mut Scratch::new(), &mut Replay, seen)
            }
            Boards::Large(b) => {
                let mut ahead = b.state.clone();
                next_in_step(&mut ahead, t, &mut b.peek, &mut Replay, seen)
            }
        }
    }

    fn snapshot(&self) -> Snapshot {
        match self {
            Boards::Small(b) => Snapshot::Small(Box::new(b.state)),
            Boards::Large(b) => Snapshot::Large(Box::new(b.state.clone())),
        }
    }

    /// Puts the game back as `snapshot` holds it, in the layout the game is in now: a game
    /// widened since keeps its layout.
    fn restore(&mut self, snapshot: Snapshot) {
        match (self, snapshot) {
            (Boards::Small(b), Snapshot::Small(s)) => b.state = *s,
            (Boards::Large(b), Snapshot::Small(s)) => b.state = s.widen(),
            (Boards::Large(b), Snapshot::Large(s)) => b.state = *s,
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

    /// Whether `player` knows the top card of their library.
    fn knows_top(&self, player: Seat) -> bool {
        on_board!(self, b => b
            .state
            .library(player)
            .last()
            .is_some_and(|&slot| b.state.knows(player, slot)))
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
struct BeforeCast {
    state: Snapshot,
    at: At,
    answered: Option<Vec<StackEntry<Large>>>,
    /// The length of the choice log then.
    log: usize,
    stops: StopsSetting,
}

/// The game as it stood at the human's first decision or halt of their latest turn, which
/// they may go back to ([`Game::undo`]) until their next turn begins. A copy of the state, so
/// going back is exact, and its random streams with it: the shuffles and other random events
/// to come fall as they did, so going back takes a play back without dealing the luck again.
/// What a player learned since is theirs to keep, and the view says so ([`UndoView::Peeked`]).
struct TurnStart {
    turn: u16,
    state: Snapshot,
    /// The open decision there.
    at: At,
    step: (u16, TurnStep),
    answered: Option<Vec<StackEntry<Large>>>,
    /// The length of the choice log when the game last stood here.
    log: usize,
    /// The agent's next decision then, if it can go back to it ([`Agent::rewind`]).
    agent: Option<AgentDecision>,
    /// For each card of both decks (`deck_slots`): its card there, and whether the human
    /// knew it.
    known: Vec<(CardIx, bool)>,
    /// The cards there the human did not know, sorted.
    hidden: Vec<CardIx>,
    /// The human has been shown a card since that they did not know there ([`Self::note`]).
    peeked: bool,
}

impl TurnStart {
    /// Notes what `shown`, a position the human is shown, tells them that they did not know
    /// at the start of the turn: a card they now know in a slot they did not know there, or a
    /// card that a shuffle has since moved, where it went, which the same shuffle would send
    /// it to again. Basic lands make the second conservative: seeing one says no more than
    /// that some unknown slot held one.
    fn note<L: Layout>(&mut self, shown: &State<L>, t: &GameTable, human: Seat) {
        if self.peeked {
            return;
        }
        self.peeked = deck_slots(t)
            .zip(&self.known)
            .any(|(slot, &(card, known))| {
                let slot = L::Slot::from_index(slot);
                if !shown.knows(human, slot) {
                    return false;
                }
                let now = shown.card(slot);
                if now == card {
                    !known
                } else {
                    self.hidden.binary_search(&now).is_ok()
                }
            });
    }
}

/// The slot index of every card of both decks, the first seat's first.
fn deck_slots(t: &GameTable) -> impl Iterator<Item = usize> + '_ {
    Seat::BOTH
        .into_iter()
        .flat_map(move |p| (0..t.deck(p).len()).map(move |i| p.index() * DECK_SLOTS + i))
}

/// A game in progress.
pub struct Game {
    table: GameTable,
    board: Boards,
    human: Seat,
    session: SessionId,
    agent: Box<dyn GameAgent + Send>,
    /// An agent answering the human's decisions in their place, if any.
    stand_in: Option<Box<dyn GameAgent + Send>>,
    /// When the stand-in hands the human's decisions back to them, if ever.
    hand_over: Option<HandOver>,
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
    /// Where the human's cast in progress began, while it can be taken back.
    before_cast: Option<BeforeCast>,
    /// Where the human's latest turn began, which they may go back to.
    turn_start: Option<TurnStart>,
    /// The coin flip's winner, who chooses who takes the first turn
    /// ([CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)), in a game begun by one.
    flip_winner: Option<Seat>,
}

impl Game {
    /// Sets up game `session`; [`Game::next_view`] plays it.
    pub fn new(
        cards: &'static CardTable,
        start: &StartGame,
        session: SessionId,
    ) -> Result<Game, SessionError> {
        let replay = ReplayFile {
            seed: start.seed,
            decks: start.decks.clone(),
            human: start.human.get(),
            opponent: start.opponent.clone(),
            stops: start.stops,
            log: Vec::new(),
        };
        let (table, state) = replay.setup(cards)?;
        // The agent draws from its own stream, apart from the game's.
        let agent = new_agent(
            &start.opponent,
            &table,
            SplitMix64::new(start.seed).next_u64(),
        )?;
        Ok(Game::assemble(
            table,
            &state,
            replay,
            start.human,
            session,
            agent,
        ))
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
        } = prepared;
        state.set_profile(with_stops(Profile::SMART, stops, human));
        let replay = ReplayFile {
            seed: 0,
            decks: [Vec::new(), Vec::new()],
            human: human.get(),
            opponent: String::new(),
            stops,
            log: Vec::new(),
        };
        let mut game = Game::assemble(table, &state, replay, human, session, opponent);
        game.stand_in = Some(stand_in);
        game.hand_over = hand_over;
        game
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
            stand_in: None,
            hand_over: None,
            at: At::Playing,
            seq: 0,
            replay,
            stop: Arc::default(),
            events: Vec::new(),
            paused: Vec::new(),
            before_cast: None,
            turn_start: None,
            flip_winner,
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
    pub fn from_replay(
        cards: &'static CardTable,
        file: &ReplayFile,
        session: SessionId,
    ) -> Result<(Game, PlayerView), SessionError> {
        let mut game = Game::new(cards, &file.start()?, session)?;
        let bad = |what: &str| SessionError::BadReplay {
            reason: what.to_owned(),
        };
        // The session's own answers are found again as it plays on.
        let mut log = file
            .log
            .iter()
            .copied()
            .filter(|e| !matches!(e, Entry::Auto(_)));
        loop {
            if matches!(game.at, At::Playing) {
                game.play_on();
                if matches!(game.at, At::Playing) {
                    // A step began, and live play went on after its view.
                    game.board.drain_events();
                    game.note_shown();
                    continue;
                }
            }
            let Some(entry) = log.next() else {
                break;
            };
            // Live play's view here took the events so far.
            game.board.drain_events();
            game.note_shown();
            match (game.at.clone(), entry) {
                (At::Agent(d), Entry::Choice(c)) if usize::from(c) < d.len() => {
                    game.apply(entry);
                    game.at = At::Playing;
                }
                (At::Human(..), Entry::Stops(stops)) => game.change_stops(stops),
                (At::Human(id, open @ Open::Decision(_)), Entry::Choice(_))
                | (At::Human(id, open @ Open::Halt), Entry::Halt) => {
                    let c = if let Entry::Choice(c) = entry { c } else { 0 };
                    let answer = OpenDecision::new(id, &open, false, false)
                        .answer(id, CandidateIndex(c))
                        .map_err(|_| bad("a choice out of range"))?;
                    game.choose(answer);
                }
                (At::Human(..), Entry::Undo) => {
                    game.undo()
                        .map_err(|_| bad("an undo with nothing to go back to"))?;
                }
                (At::Agent(_) | At::Human(..), Entry::Concede) => {
                    game.concede();
                }
                (At::Over { .. }, _) => return Err(bad("the log goes on after the game ended")),
                (_, entry) => {
                    return Err(bad(&format!("{entry:?} where the game does not ask it")));
                }
            }
        }
        if game.replay.log != file.log {
            return Err(bad("the session's own answers differ from the record's"));
        }
        let view = game.view();
        Ok((game, view))
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
                let c = on_board!(&mut self.board, b => {
                    let obs = observe(&b.state, &self.table, &mut b.scratch, d.player);
                    self.agent.choose(obs, &d.relayout())
                });
                self.apply(Entry::Choice(c.0));
                self.at = At::Playing;
                self.play_on();
            }
            At::Playing => self.play_on(),
        }
        Some(self.view())
    }

    /// The human's open decision, if any.
    pub fn open(&self) -> Option<OpenDecision> {
        match &self.at {
            At::Human(id, open) => Some(OpenDecision::new(
                *id,
                open,
                self.cancellable(),
                self.undoable(),
            )),
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
        self.board.restore(before.state);
        self.answered = before.answered;
        self.replay.log.truncate(before.log);
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

    /// Whether the human, at their open decision, may go back to where their latest turn began
    /// ([`Game::undo`]): something was decided since, not only their stops changed.
    fn undoable(&self) -> bool {
        matches!(self.at, At::Human(..))
            && self.turn_start.as_ref().is_some_and(|start| {
                self.replay.log[start.log..]
                    .iter()
                    .any(|e| !matches!(e, Entry::Stops(_)))
            })
    }

    /// Goes back to the human's first decision or halt of their latest turn, at their open
    /// decision, and returns the view of it again: the turn so far, or the opponent's turn
    /// after it, never happened. The state is a copy, random streams and all, so what comes
    /// next falls as it did unless the players play differently; the agent goes back to the
    /// same place among its own decisions where it can ([`Agent::rewind`]). This is no rule of
    /// the game: it is a practice tool, and the view says when the human saw a card since that
    /// they did not know there ([`UndoView::Peeked`]). The choice log keeps the turn taken
    /// back, then an [`Entry::Undo`], so a replay goes back the same way.
    pub fn undo(&mut self) -> Result<PlayerView, SessionError> {
        if self.open().is_none() {
            return Err(SessionError::NotDeciding);
        }
        if !self.undoable() {
            return Err(SessionError::NothingToUndo);
        }
        let start = self
            .turn_start
            .as_mut()
            .expect("an undoable game has a turn start");
        self.board.restore(start.state.clone());
        self.answered.clone_from(&start.answered);
        self.seen = start.step;
        self.before_cast = None;
        self.replay.log.push(Entry::Undo);
        start.peeked = false;
        start.log = self.replay.log.len();
        let (open, agent) = (start.at.clone(), start.agent);
        if let Some(to) = agent {
            match self.board {
                Boards::Small(_) => Agent::<Small>::rewind(&mut *self.agent, to),
                Boards::Large(_) => Agent::<Large>::rewind(&mut *self.agent, to),
            }
        }
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
        let At::Human(_, open) = open else {
            unreachable!("a turn start is at the human's decision");
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

    /// Keeps the position as the start of the human's turn, if it is the first decision or
    /// halt of theirs in a turn of theirs.
    fn note_turn_start(&mut self) {
        let turn = self.board.turn();
        let number = turn.number;
        if !matches!(self.at, At::Human(..))
            || number == 0
            || turn.active != self.human
            || self.turn_start.as_ref().is_some_and(|t| t.turn == number)
        {
            return;
        }
        let known: Vec<(CardIx, bool)> = on_board!(&self.board, b => deck_slots(&self.table)
            .map(|i| {
                let slot = SlotId::from_index(i);
                (b.state.card(slot), b.state.knows(self.human, slot))
            })
            .collect());
        let agent = match self.board {
            Boards::Small(_) => Agent::<Small>::next_decision(&*self.agent),
            Boards::Large(_) => Agent::<Large>::next_decision(&*self.agent),
        };
        let mut hidden: Vec<CardIx> = known
            .iter()
            .filter(|(_, known)| !known)
            .map(|&(card, _)| card)
            .collect();
        hidden.sort_unstable();
        self.turn_start = Some(TurnStart {
            turn: number,
            state: self.board.snapshot(),
            at: self.at.clone(),
            step: self.seen,
            answered: self.answered.clone(),
            log: self.replay.log.len(),
            agent,
            known,
            hidden,
            peeked: false,
        });
    }

    /// Notes what the human is shown in the position now ([`TurnStart::note`]), as a view
    /// would: a replay calls it where live play built one.
    fn note_shown(&mut self) {
        if let Some(start) = &mut self.turn_start {
            on_board!(&mut self.board, b => {
                start.note(view(&b.state, &self.table, &mut b.scratch), &self.table, self.human);
            });
        }
    }

    /// Checks `index` as an answer to the human's open decision `id`.
    pub fn answer(&self, id: DecisionId, index: CandidateIndex) -> Result<Answer, SessionError> {
        self.open()
            .ok_or(SessionError::NotDeciding)?
            .answer(id, index)
    }

    /// Applies an answer to the human's open decision; [`Game::next_view`] plays on from it.
    pub fn choose(&mut self, answer: Answer) {
        let At::Human(id, open) = self.at.clone() else {
            panic!("an answer is checked against the open decision");
        };
        debug_assert_eq!(id, answer.id, "an answer names the open decision");
        match open {
            Open::Decision(d) => {
                // A cast or an activation from priority can be taken back until it is paid for.
                let before = (d.kind == DecisionKind::Priority).then(|| BeforeCast {
                    state: self.board.snapshot(),
                    at: self.at.clone(),
                    answered: self.answered.clone(),
                    log: self.replay.log.len(),
                    stops: self.stops,
                });
                self.apply(Entry::Choice(answer.choice.0));
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
                let obs = observe(&b.state, &self.table, &mut b.scratch, self.human);
                stand_in.choose(obs, &without_floating(d.relayout())).0
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
            conceded: true,
        };
        Some(self.view())
    }

    /// A flag that, once set, stops the game before the agent's next decision: the game in
    /// progress returns a stopped view, and no decision opens again. A session sets it when it
    /// is dropped, so an abandoned game stops thinking.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    /// The record of the game so far.
    pub fn replay(&self) -> &ReplayFile {
        &self.replay
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
    fn play_on(&mut self) {
        debug_assert!(matches!(self.at, At::Playing));
        loop {
            let step = self.board.next_in_step(&self.table, self.seen);
            // Once a cast is complete, or another player decides, it can no longer be taken back.
            if !matches!(&step, Some(Step::Decide(d)) if d.player == self.human && self.casting()) {
                self.before_cast = None;
            }
            self.at = match step {
                None => {
                    let turn = self.board.turn();
                    self.seen = (turn.number, turn.step);
                    self.answered = None;
                    if self.halts() {
                        self.seq += 1;
                        At::Human(self.decision_id(), Open::Halt)
                    } else {
                        At::Playing
                    }
                }
                Some(Step::Over(outcome)) => At::Over {
                    outcome,
                    conceded: false,
                },
                Some(Step::Decide(d)) if d.player == self.human => {
                    if let Some(c) = self.auto_answer(&d) {
                        self.apply(Entry::Auto(c.0));
                        self.answered = Some(self.board.stack());
                        continue;
                    }
                    self.seq += 1;
                    At::Human(self.decision_id(), Open::Decision(d))
                }
                Some(Step::Decide(d)) => At::Agent(d),
            };
            self.note_turn_start();
            return;
        }
    }

    /// The session's own answer to the human's decision `d`, if it gives one: under the
    /// smart stops, it passes their priority while only their own doings sit on the stack
    /// ([`Game::own_top`]) or in combat with nothing new to answer ([`Game::quiet_combat`]),
    /// orders their simultaneous triggers ([`trigger_to_stack`]), searches their library
    /// where they may when they don't know its top card ([`Game::searches_unasked`]), and
    /// pays a waterbend cost with mana whenever mana can pay it, tapping no artifact or
    /// creature ([CR 701.67a](https://yawgatog.com/resources/magic-rules/#R70167a): "you may
    /// tap an untapped artifact or creature you control rather than pay that mana").
    /// Like halts, these are the session's alone: the engine's smart stops, which the agent's
    /// search shares, are unchanged, and exact stops ask every one. A function of the position,
    /// the stops and the human's answers, so a replay finds each again; the replay records
    /// each as an [`Entry::Auto`].
    fn auto_answer(&self, d: &Decision<Large>) -> Option<Choice> {
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
            DecisionKind::YesNo if self.searches_unasked(d) => (0..d.len() as u16)
                .map(Choice)
                .find(|&c| d.candidates.option(c) == Some(Opt::Yes)),
            DecisionKind::Waterbend => (0..d.len() as u16)
                .map(Choice)
                .find(|&c| d.candidates.option(c) == Some(Opt::Done)),
            _ => None,
        }
    }

    /// Whether `d` asks whether to search the human's library (Trinket Mage's "you may
    /// search") while they don't know its top card. Declining differs from searching and
    /// finding nothing ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b): "If
    /// a player is searching a hidden zone for cards with a stated quality ... that player
    /// isn't required to find some or all of those cards") only by the shuffle
    /// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a): "To shuffle a
    /// library ..., randomize the cards within it so that no player knows their order"), and
    /// the search itself offers taking nothing. The shuffle costs a player something worth a
    /// question only when it loses them the card they know they draw next, after a put-back
    /// or a scry to the top. Cards they know deeper down, as a London mulligan's on the
    /// bottom, are not worth a question each time they search.
    fn searches_unasked(&self, d: &Decision<Large>) -> bool {
        d.ask == Ask::MaySearch && !self.board.knows_top(self.human)
    }

    /// Whether the top of the stack is the human's own spell or ability and, since their
    /// latest answer in this step, nothing but their own entries came onto the stack or left
    /// it: what they cast, its triggers, and its resolution, but not an opponent's response
    /// or its resolution, which they get priority to see and answer. Their priority then
    /// would only let them respond to themselves, as an Arena player auto-passes.
    fn own_top(&self) -> bool {
        let answered = self.answered.as_deref().unwrap_or_default();
        let now = self.board.stack();
        let own = |e: &StackEntry<Large>| e.controller == self.human;
        let kept = answered
            .iter()
            .zip(&now)
            .take_while(|(then, now)| then == now)
            .count();
        now.last().is_some_and(own)
            && answered[kept..].iter().all(own)
            && now[kept..].iter().all(own)
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
        if self.stops.full_control || !halting_step {
            return false;
        }
        match self.board.ahead(&self.table, self.seen) {
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

    /// Records `entry`, an answer, and applies it.
    fn apply(&mut self, entry: Entry) {
        let (Entry::Choice(c) | Entry::Auto(c)) = entry else {
            unreachable!("only an answer is applied");
        };
        self.replay.log.push(entry);
        self.board.apply(&self.table, Choice(c));
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
        self.events = fresh;
        self.paused = paused;
        self.draw()
    }

    /// The human's view now, with the events of the latest view.
    fn draw(&mut self) -> PlayerView {
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
                At::Human(_, Open::Decision(d)) => Some(d.relayout()),
                _ => None,
            };
            let resolving = resolving.as_ref().map(Relayout::relayout);
            let events: Vec<_> = self.events.iter().map(Relayout::relayout).collect();
            let moment = match (&self.at, &decision) {
                (At::Playing, _) => Moment::Playing,
                (At::Agent(_), _) => Moment::Thinking,
                (At::Human(id, Open::Decision(_)), Some(d)) => {
                    Moment::Deciding(*id, d, resolving.as_ref())
                }
                (At::Human(id, _), _) => Moment::Halted(*id),
                (At::Over { outcome, conceded }, _) => Moment::Over {
                    outcome: *outcome,
                    conceded: *conceded,
                },
                (At::Stopped, _) => Moment::Stopped,
            };
            let shown = view(&b.state, &self.table, &mut b.scratch);
            if let Some(start) = &mut self.turn_start {
                start.note(shown, &self.table, self.human);
            }
            let obs = Observation::new(shown, self.human);
            let mut view = player_view(&self.table, obs, moment, self.stops, &events);
            if let Some(d) = &decision {
                preview::add_previews(&mut view, &b.state, &self.table, d);
            }
            view
        });
        view.turn.flip_winner = self.flip_winner.map(|p| {
            if p == self.human {
                Side::You
            } else {
                Side::Opponent
            }
        });
        if let Status::Deciding { decision } = &mut view.status {
            decision.cancellable = self.cancellable();
            decision.undo = self.undoable().then(|| {
                if self.turn_start.as_ref().is_some_and(|t| t.peeked) {
                    UndoView::Peeked
                } else {
                    UndoView::Clean
                }
            });
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
fn trigger_to_stack<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>) -> Choice {
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

/// `d` without the mana activations a priority offers the human on their own
/// ([`Profile::taps_mana`]), for an agent standing in for them: tapping ahead of a payment is
/// dominated by tapping during it (architecture §10), and search never sees them. The engine
/// lists them last, so every other candidate keeps its index.
fn without_floating<L: Layout>(mut d: Decision<L>) -> Decision<L> {
    if let (DecisionKind::Priority, Candidates::Options(list)) = (d.kind, &mut d.candidates) {
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
