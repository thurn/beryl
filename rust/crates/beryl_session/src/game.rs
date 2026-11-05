//! A game between a human seat and an agent, played synchronously: the core a session runs on
//! its thread, and that tests drive directly. It plays on view by view ([`Game::next_view`]):
//! as each step of the turn begins, before each of the agent's decisions, at each of the
//! human's, and at the end, so the human sees the agent's turn as it is played.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use beryl_engine::event::Event;
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::Pending;
use beryl_engine::{
    CardTable, Choice, Decision, GameTable, Observation, Outcome, Profile, Replay, Scratch, Seat,
    State, Step, TurnStep, apply, next_in_step, observe, view,
};
use beryl_search::{Agent, Budget};

use crate::protocol::{
    CandidateIndex, DecisionId, Moment, PlayerView, SessionId, StopsSetting, player_view,
};
use crate::replay::{Entry, ReplayFile};
use crate::{SessionError, StartGame};

/// The human's open decision, as a session records it: the one place an answer is checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenDecision {
    id: DecisionId,
    candidates: usize,
}

impl OpenDecision {
    fn new(id: DecisionId, open: &Open) -> OpenDecision {
        OpenDecision {
            id,
            candidates: match open {
                Open::Decision(d) => d.len(),
                Open::Halt => 1,
            },
        }
    }

    pub fn id(self) -> DecisionId {
        self.id
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
// One lives in a game at a time; it is copied once per decision, never in a hot loop.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug)]
enum Open {
    /// The engine's decision.
    Decision(Decision),
    /// A halt ([`Game::halts`]): the session stops the human where the engine passes their
    /// priority for them, and they go on with its one candidate.
    Halt,
}

/// Where a game stands between views.
#[derive(Clone, Copy, Debug)]
enum At {
    /// The game plays on from here without the human: before the first view, after an
    /// answer, or as a step begins.
    Playing,
    /// The agent makes this decision next.
    Agent(Decision),
    /// The human's open decision.
    Human(DecisionId, Open),
    Over(Outcome),
    /// Stopped by the stop flag before the agent's decision.
    Stopped,
}

/// A game in progress.
pub struct Game {
    table: GameTable,
    state: State,
    scratch: Scratch,
    /// Working memory for looking ahead in a copy of the state, whose events are dropped.
    peek: Scratch,
    human: Seat,
    session: SessionId,
    agent: Box<dyn Agent + Send>,
    /// An agent answering the human's decisions in their place, if any.
    stand_in: Option<Box<dyn Agent + Send>>,
    at: At,
    /// The turn and step the latest view began, or the game's first.
    seen: (u16, TurnStep),
    seq: u32,
    stops: StopsSetting,
    replay: ReplayFile,
    /// Set from another thread to stop the agent at its next decision.
    stop: Arc<AtomicBool>,
    /// The engine's events the latest view reported.
    events: Vec<Event>,
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
        let turn = state.turn();
        Ok(Game {
            seen: (turn.number, turn.step),
            table,
            state,
            scratch: Scratch::recording(),
            peek: Scratch::new(),
            human: start.human,
            session,
            agent,
            stand_in: None,
            at: At::Playing,
            seq: 0,
            stops: start.stops,
            replay,
            stop: Arc::default(),
            events: Vec::new(),
        })
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
        let mut log = file.log.iter().copied();
        loop {
            if matches!(game.at, At::Playing) {
                game.play_on();
                if matches!(game.at, At::Playing) {
                    // A step began, and live play went on after its view.
                    game.scratch.events.drain();
                    continue;
                }
            }
            let Some(entry) = log.next() else {
                break;
            };
            // Live play's view here took the events so far.
            game.scratch.events.drain();
            match (game.at, entry) {
                (At::Agent(d), Entry::Choice(c)) if usize::from(c) < d.len() => {
                    game.apply(Choice(c));
                    game.at = At::Playing;
                }
                (At::Human(..), Entry::Stops(stops)) => game.change_stops(stops),
                (At::Human(id, open @ Open::Decision(_)), Entry::Choice(_))
                | (At::Human(id, open @ Open::Halt), Entry::Halt) => {
                    let c = if let Entry::Choice(c) = entry { c } else { 0 };
                    let answer = OpenDecision::new(id, &open)
                        .answer(id, CandidateIndex(c))
                        .map_err(|_| bad("a choice out of range"))?;
                    game.choose(answer);
                }
                (At::Over(_), _) => return Err(bad("the log goes on after the game ended")),
                (_, entry) => {
                    return Err(bad(&format!("{entry:?} where the game does not ask it")));
                }
            }
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
        match self.at {
            At::Human(..) | At::Over(_) | At::Stopped => return None,
            At::Agent(_) if self.stop.load(Ordering::Relaxed) => self.at = At::Stopped,
            At::Agent(d) => {
                let obs = observe(&self.state, &self.table, &mut self.scratch, d.player);
                let c = self.agent.choose(obs, &d);
                self.apply(c);
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
            At::Human(id, open) => Some(OpenDecision::new(*id, open)),
            _ => None,
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
        let At::Human(id, open) = self.at else {
            panic!("an answer is checked against the open decision");
        };
        debug_assert_eq!(id, answer.id, "an answer names the open decision");
        match open {
            Open::Decision(_) => self.apply(answer.choice),
            Open::Halt => self.replay.log.push(Entry::Halt),
        }
        self.at = At::Playing;
    }

    /// Answers the human's open decision as the stand-in would, if there is one
    /// ([`Game::set_stand_in`]) and a decision is open; it goes on from a halt. Returns
    /// whether it answered.
    pub fn answer_by_stand_in(&mut self) -> bool {
        let (At::Human(id, open), Some(stand_in)) = (self.at, self.stand_in.as_mut()) else {
            return false;
        };
        let c = match open {
            Open::Decision(d) => {
                let obs = observe(&self.state, &self.table, &mut self.scratch, self.human);
                stand_in.choose(obs, &d).0
            }
            Open::Halt => 0,
        };
        let answer = OpenDecision::new(id, &open)
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

    /// The game itself, for tests.
    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn table(&self) -> &GameTable {
        &self.table
    }

    /// The engine's events the latest view reported, for tests.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Plays on from [`At::Playing`] to the next place a view shows: a step beginning, where
    /// the human may be halted, a decision, or the end.
    fn play_on(&mut self) {
        debug_assert!(matches!(self.at, At::Playing));
        let step = next_in_step(
            &mut self.state,
            &self.table,
            &mut self.scratch,
            &mut Replay,
            self.seen,
        );
        self.at = match step {
            None => {
                let turn = self.state.turn();
                self.seen = (turn.number, turn.step);
                if self.halts() {
                    self.seq += 1;
                    At::Human(self.decision_id(), Open::Halt)
                } else {
                    At::Playing
                }
            }
            Some(Step::Over(outcome)) => At::Over(outcome),
            Some(Step::Decide(d)) if d.player == self.human => {
                self.seq += 1;
                At::Human(self.decision_id(), Open::Decision(d))
            }
            Some(Step::Decide(d)) => At::Agent(d),
        };
    }

    /// Whether the human is halted as the step that just began, a session-level stop the
    /// engine knows nothing of: under the smart stops, at their own precombat main phase and
    /// at the opponent's end step, unless the engine offers them a decision there before
    /// anything else is decided. The game would otherwise pass through those steps unseen
    /// whenever the human can do nothing but pass, and their turn would end, or the
    /// opponent's turn would turn into theirs, without them. Halts are a function of the
    /// position and the stops, so a replay finds them again; the agent never sees them.
    fn halts(&mut self) -> bool {
        let turn = self.state.turn();
        let own = turn.active == self.human;
        let halting_step = match turn.step {
            TurnStep::PrecombatMain => own,
            TurnStep::End => !own,
            _ => false,
        };
        if self.stops != StopsSetting::Smart || !halting_step {
            return false;
        }
        let mut ahead = self.state;
        match next_in_step(
            &mut ahead,
            &self.table,
            &mut self.peek,
            &mut Replay,
            self.seen,
        ) {
            Some(Step::Decide(d)) => d.player != self.human,
            Some(Step::Over(_)) => false,
            None => true,
        }
    }

    fn change_stops(&mut self, stops: StopsSetting) {
        self.stops = stops;
        self.state
            .set_profile(with_stops(self.state.profile(), stops));
        self.replay.log.push(Entry::Stops(stops));
    }

    fn decision_id(&self) -> DecisionId {
        DecisionId {
            session: self.session,
            seq: self.seq,
        }
    }

    fn apply(&mut self, c: Choice) {
        self.replay.log.push(Entry::Choice(c.0));
        apply(
            &mut self.state,
            &self.table,
            &mut self.scratch,
            &mut Replay,
            c,
        );
    }

    /// The human's view now, with the engine's events since the previous view.
    fn view(&mut self) -> PlayerView {
        self.events.clear();
        self.events.extend(self.scratch.events.drain());
        self.draw()
    }

    /// The human's view now, with the events of the latest view.
    fn draw(&mut self) -> PlayerView {
        // A paused resolution's entry is still on top of the committed stack; the position
        // replayed to its question has taken it off.
        let resolving = matches!(self.state.pending(), Pending::Resolving(_))
            .then(|| self.state.stack().last())
            .flatten();
        let moment = match &self.at {
            At::Playing => Moment::Playing,
            At::Agent(_) => Moment::Thinking,
            At::Human(id, Open::Decision(d)) => Moment::Deciding(*id, d, resolving),
            At::Human(id, Open::Halt) => Moment::Halted(*id),
            At::Over(outcome) => Moment::Over(*outcome),
            At::Stopped => Moment::Stopped,
        };
        let obs = Observation::new(
            view(&self.state, &self.table, &mut self.scratch),
            self.human,
        );
        player_view(&self.table, obs, moment, self.stops, &self.events)
    }
}

fn new_agent(name: &str, t: &GameTable, seed: u64) -> Result<Box<dyn Agent + Send>, SessionError> {
    let agent = beryl_search::agent(name).ok_or_else(|| SessionError::UnknownAgent {
        name: name.to_owned(),
    })?;
    Ok((agent.new)(t, seed, Budget::DEFAULT))
}

pub(crate) fn with_stops(profile: Profile, stops: StopsSetting) -> Profile {
    Profile {
        stops: stops.stops(),
        ..profile
    }
}
