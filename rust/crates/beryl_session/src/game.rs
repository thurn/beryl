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

use beryl_engine::card::Purpose;
use beryl_engine::chance::Chance;
use beryl_engine::event::{Event, LossCause};
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::{ObjectRef, Pending, StackEntry};
use beryl_engine::{
    CardTable, Choice, Decision, DecisionKind, GameTable, Large, Limits, Outcome, Profile,
    Relayout, Seat, Small, State, StepStops, TurnStep, observe, two_player::TwoPlayerSeat as _,
    view,
};
use beryl_search::{Agent, AgentDecision, GameAgent};
use serde_json::json;

use crate::ai::{self, AgentMove, AgentMoves, SearchView};
use crate::journal;
use crate::protocol::{CandidateIndex, DecisionId, PlayerView, SessionId, StopsSetting};
use crate::replay::{Entry, FORMAT_VERSION, ReplayFile};
use crate::save::Chosen;
use crate::{HandOver, Prepared, SessionError, StartGame};

mod boards;
mod passing;
mod playing;
mod prompts;
mod replaying;
mod undo;
mod views;

use boards::{Boards, Snapshot, on_board};
use passing::PassingOn;
pub use playing::trigger_to_stack;
use playing::without_floating;
pub use undo::UNDO_POINTS;
use undo::{BeforeCast, UndoPoint};

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
    /// What each seat's screen last showed, which its next script starts from.
    screens: [crate::script::Screen; 2],
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
    /// The state of the human's open prompt, where the session holds its shape's, with the
    /// decision it is for: made as that decision's view is drawn and changed by clicks.
    prompt: Option<(DecisionId, crate::prompt::Held)>,
    /// The picks still to make of the prompt the human's latest click answered.
    run: Option<crate::prompt::Run>,
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
        let agent = ai::new_agent(
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
            seen: (turn.number(), turn.step()),
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
            screens: Default::default(),
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
            prompt: None,
            run: None,
        }
    }

    /// Has the agent named `name` answer the human's decisions in their place (UI fixtures):
    /// [`Game::answer_by_stand_in`] plays each.
    pub fn set_stand_in(&mut self, name: &str) -> Result<(), SessionError> {
        // The stand-in draws from its own stream, apart from the game's and the agent's.
        self.stand_in = Some(ai::new_agent(
            name,
            &self.table,
            SplitMix64::new(self.replay.seed ^ 0x9e37_79b9).next_u64(),
        )?);
        Ok(())
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

    /// The agent's next decision among its own, in the layout the game is in.
    fn agent_next(&self) -> Option<AgentDecision> {
        match &self.board {
            Boards::Small(_) => Agent::<Small>::next_decision(&*self.agent),
            Boards::Large(_) => Agent::<Large>::next_decision(&*self.agent),
        }
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
            turn: turn.number(),
            step: turn.step().into(),
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

    /// Applies an answer to the human's open decision; [`Game::next_view`] plays on from it.
    /// A client's follow-up joins the answer before it only while a cast is in progress.
    pub fn choose(&mut self, answer: Answer) {
        self.answer_as(answer, answer.continuation && self.casting());
    }

    /// Applies `answer` to the human's open decision, as a `continuation` of their answer
    /// before it or as one of its own: a recorded answer, or a later pick of a prompt's run.
    fn answer_as(&mut self, answer: Answer, continuation: bool) {
        let At::Human(id, open) = self.at.clone() else {
            panic!("an answer is checked against the open decision");
        };
        debug_assert_eq!(id, answer.id, "an answer names the open decision");
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
                if d.prompt.purpose == Purpose::Waterbend && d.len() == 1 {
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
            // That view is never sent: the end starts each screen over.
            self.screens = Default::default();
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
