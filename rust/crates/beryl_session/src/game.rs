//! A game between a human seat and an agent, played synchronously: the core a session runs on
//! its thread, and that tests drive directly.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use beryl_engine::event::Event;
use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    CardTable, Choice, Decision, GameTable, Observation, Outcome, Profile, Replay, Scratch, Seat,
    State, Step, apply, next, view,
};
use beryl_search::Agent;

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
    fn new(id: DecisionId, d: &Decision) -> OpenDecision {
        OpenDecision {
            id,
            candidates: d.len(),
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

/// A game in progress. Between calls a decision of the human seat is open, the game is over,
/// or it was stopped.
pub struct Game {
    table: GameTable,
    state: State,
    scratch: Scratch,
    human: Seat,
    session: SessionId,
    agent: Box<dyn Agent + Send>,
    /// An agent answering the human's decisions in their place, if any.
    stand_in: Option<Box<dyn Agent + Send>>,
    /// The human's open decision.
    open: Option<(DecisionId, Decision)>,
    seq: u32,
    stops: StopsSetting,
    replay: ReplayFile,
    /// Set from another thread to stop the agent at its next decision.
    stop: Arc<AtomicBool>,
    /// The engine's events the latest view reported.
    events: Vec<Event>,
}

impl Game {
    /// Sets up game `session`; [`Game::begin`] plays it to the human's first decision.
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
        Ok(Game {
            table,
            state,
            scratch: Scratch::recording(),
            human: start.human,
            session,
            // The agent draws from its own stream, apart from the game's.
            agent: new_agent(&start.opponent, SplitMix64::new(start.seed).next_u64())?,
            stand_in: None,
            open: None,
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
            SplitMix64::new(self.replay.seed ^ 0x9e37_79b9).next_u64(),
        )?);
        Ok(())
    }

    /// Replays a recorded game to where its record ends, and returns it with the human's view
    /// there. A record ends at the human's open decision or at the end of the game, so no
    /// agent plays; were it cut short anywhere else, the agent plays on.
    pub fn from_replay(
        cards: &'static CardTable,
        file: &ReplayFile,
        session: SessionId,
    ) -> Result<(Game, PlayerView), SessionError> {
        let mut game = Game::new(cards, &file.start()?, session)?;
        let bad = |what: &str| SessionError::BadReplay {
            reason: what.to_owned(),
        };
        let mut open = None;
        for &entry in &file.log {
            let d = match open {
                Some(d) => d,
                None => match next(&mut game.state, &game.table, &mut game.scratch, &mut Replay) {
                    Step::Decide(d) => {
                        if d.player == game.human {
                            // Live play emitted a view here, which took the events so far.
                            game.seq += 1;
                            game.scratch.events.drain();
                        }
                        d
                    }
                    Step::Over(_) => return Err(bad("the log goes on after the game ended")),
                },
            };
            match entry {
                Entry::Stops(stops) if d.player == game.human => {
                    game.stops = stops;
                    game.state
                        .set_profile(with_stops(game.state.profile(), stops));
                    game.replay.log.push(entry);
                    open = Some(d);
                }
                Entry::Stops(_) => return Err(bad("stops changed during the agent's decision")),
                Entry::Choice(c) => {
                    let id = game.decision_id();
                    let answer = OpenDecision::new(id, &d)
                        .answer(id, CandidateIndex(c))
                        .map_err(|_| bad("a choice out of range"))?;
                    game.apply(answer.choice);
                    open = None;
                }
            }
        }
        let view = match open {
            Some(d) => {
                game.open = Some((game.decision_id(), d));
                game.view(None)
            }
            None => game.advance(),
        };
        Ok((game, view))
    }

    /// Plays to the human's first decision, or to the end of the game.
    pub fn begin(&mut self) -> PlayerView {
        debug_assert!(self.replay.log.is_empty() && self.open.is_none());
        self.advance()
    }

    /// The human's open decision, if any.
    pub fn open(&self) -> Option<OpenDecision> {
        self.open.map(|(id, d)| OpenDecision::new(id, &d))
    }

    /// Checks `index` as an answer to the human's open decision `id`.
    pub fn answer(&self, id: DecisionId, index: CandidateIndex) -> Result<Answer, SessionError> {
        self.open()
            .ok_or(SessionError::NotDeciding)?
            .answer(id, index)
    }

    /// Applies an answer to the human's open decision, and plays to their next decision or
    /// the end of the game.
    pub fn choose(&mut self, answer: Answer) -> PlayerView {
        let open = self.open.take();
        debug_assert_eq!(
            open.map(|(id, _)| id),
            Some(answer.id),
            "an answer is checked against the open decision"
        );
        self.apply(answer.choice);
        self.advance()
    }

    /// Answers the human's open decision as the stand-in would, if there is one
    /// ([`Game::set_stand_in`]) and a decision is open.
    pub fn answer_by_stand_in(&mut self) -> Option<PlayerView> {
        let (id, d) = self.open?;
        let obs = Observation::new(
            view(&self.state, &self.table, &mut self.scratch),
            self.human,
        );
        let Choice(c) = self.stand_in.as_mut()?.choose(obs, &d);
        let answer = OpenDecision::new(id, &d)
            .answer(id, CandidateIndex(c))
            .expect("an agent chooses a candidate");
        Some(self.choose(answer))
    }

    /// Changes the human's priority stops from their open decision on, and returns the view
    /// again with the new setting. The setting applies to both seats: the state holds one
    /// profile.
    pub fn set_stops(&mut self, stops: StopsSetting) -> Result<PlayerView, SessionError> {
        if self.open.is_none() {
            return Err(SessionError::NotDeciding);
        }
        self.stops = stops;
        self.state
            .set_profile(with_stops(self.state.profile(), stops));
        self.replay.log.push(Entry::Stops(stops));
        // Nothing happened since the previous view.
        self.events.clear();
        Ok(self.draw(None))
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

    /// Plays the agent's decisions until the human must decide or the game ends.
    fn advance(&mut self) -> PlayerView {
        loop {
            match next(&mut self.state, &self.table, &mut self.scratch, &mut Replay) {
                Step::Over(outcome) => return self.view(Some(outcome)),
                Step::Decide(d) if d.player == self.human => {
                    self.seq += 1;
                    self.open = Some((self.decision_id(), d));
                    return self.view(None);
                }
                Step::Decide(_) if self.stop.load(Ordering::Relaxed) => return self.view(None),
                Step::Decide(d) => {
                    let obs = Observation::new(
                        view(&self.state, &self.table, &mut self.scratch),
                        d.player,
                    );
                    let c = self.agent.choose(obs, &d);
                    self.apply(c);
                }
            }
        }
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
    fn view(&mut self, outcome: Option<Outcome>) -> PlayerView {
        self.events.clear();
        self.events.extend(self.scratch.events.drain());
        self.draw(outcome)
    }

    /// The human's view now, with the events of the latest view: over with `outcome`, at the
    /// open decision, or stopped.
    fn draw(&mut self, outcome: Option<Outcome>) -> PlayerView {
        let moment = match (outcome, &self.open) {
            (Some(outcome), _) => Moment::Over(outcome),
            (None, Some((id, d))) => Moment::Deciding(*id, d),
            (None, None) => Moment::Stopped,
        };
        let obs = Observation::new(
            view(&self.state, &self.table, &mut self.scratch),
            self.human,
        );
        player_view(&self.table, obs, moment, self.stops, &self.events)
    }
}

fn new_agent(name: &str, seed: u64) -> Result<Box<dyn Agent + Send>, SessionError> {
    let agent = beryl_search::agent(name).ok_or_else(|| SessionError::UnknownAgent {
        name: name.to_owned(),
    })?;
    Ok((agent.new)(seed))
}

pub(crate) fn with_stops(profile: Profile, stops: StopsSetting) -> Profile {
    Profile {
        stops: stops.stops(),
        ..profile
    }
}
