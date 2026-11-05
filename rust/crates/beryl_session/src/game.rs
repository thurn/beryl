//! A game between a human seat and an agent, played synchronously: the core a
//! [`crate::GameSession`] runs on its thread, and that the CLI drives directly.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    CardTable, Choice, Decision, GameTable, Observation, Outcome, Profile, Replay, Scratch, State,
    Step, apply, next, view,
};
use beryl_search::Agent;

use crate::protocol::{GameEvent, PlayerView, StopsSetting, player_view};
use crate::replay::{Entry, ReplayFile};
use crate::{SessionError, StartGame};

/// A game in progress. Between calls a decision of the human seat is open, or the game is
/// over.
pub struct Game {
    table: GameTable,
    state: State,
    scratch: Scratch,
    human: u8,
    agent: Box<dyn Agent + Send>,
    /// The human's open decision and its number.
    open: Option<(u32, Decision)>,
    seq: u32,
    stops: StopsSetting,
    replay: ReplayFile,
    /// Set from another thread to stop the agent at its next decision.
    stop: Arc<AtomicBool>,
}

impl Game {
    /// Sets up a game; [`Game::begin`] plays it to the human's first decision.
    pub fn new(cards: &'static CardTable, start: &StartGame) -> Result<Game, SessionError> {
        let agent = beryl_search::agent(&start.opponent)
            .ok_or_else(|| SessionError::UnknownAgent(start.opponent.clone()))?;
        let replay = ReplayFile {
            seed: start.seed,
            decks: start.decks.clone(),
            human: start.human,
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
            // The agent draws from its own stream, apart from the game's.
            agent: (agent.new)(SplitMix64::new(start.seed).next_u64()),
            open: None,
            seq: 0,
            stops: start.stops,
            replay,
            stop: Arc::default(),
        })
    }

    /// Replays a recorded game to where its record ends, and returns it with the human's view
    /// there. A record ends at the human's open decision or at the end of the game, so no
    /// agent plays; were it cut short anywhere else, the agent plays on.
    pub fn from_replay(
        cards: &'static CardTable,
        file: &ReplayFile,
    ) -> Result<(Game, PlayerView), SessionError> {
        let mut game = Game::new(cards, &file.start())?;
        let bad = |what: &str| SessionError::BadReplay(what.to_owned());
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
                Entry::Choice(c) if usize::from(c) < d.len() => {
                    game.apply(Choice(c));
                    open = None;
                }
                Entry::Choice(_) => return Err(bad("a choice out of range")),
            }
        }
        let view = match open {
            Some(d) => {
                game.open = Some((game.seq, d));
                let events = game.events();
                game.view(game.seq, Some(&d), None, events)
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

    /// Answers the human's open decision `seq` with its candidate `index`, and plays to their
    /// next decision or the end of the game.
    pub fn choose(&mut self, seq: u32, index: usize) -> Result<PlayerView, SessionError> {
        let Some((open, d)) = self.open else {
            return Err(SessionError::NotDeciding);
        };
        if seq != open {
            return Err(SessionError::StaleDecision { seq, open });
        }
        if index >= d.len() {
            return Err(SessionError::NoSuchCandidate {
                index,
                candidates: d.len(),
            });
        }
        self.open = None;
        self.apply(Choice(index as u8));
        Ok(self.advance())
    }

    /// Answers the human's open decision as `agent` would, for a seat an agent plays in the
    /// human's place (UI fixtures).
    pub fn choose_for_human(&mut self, agent: &mut dyn Agent) -> Result<PlayerView, SessionError> {
        let Some((seq, d)) = self.open else {
            return Err(SessionError::NotDeciding);
        };
        let c = agent.choose(self.observation(), &d);
        self.choose(seq, usize::from(c.0))
    }

    /// Changes the human's priority stops from their open decision on, and returns the view
    /// again with the new setting. The setting applies to both seats: the state holds one
    /// profile.
    pub fn set_stops(&mut self, stops: StopsSetting) -> Result<PlayerView, SessionError> {
        let Some((seq, d)) = self.open else {
            return Err(SessionError::NotDeciding);
        };
        self.stops = stops;
        self.state
            .set_profile(with_stops(self.state.profile(), stops));
        self.replay.log.push(Entry::Stops(stops));
        Ok(self.view(seq, Some(&d), None, Vec::new()))
    }

    /// A flag that, once set, stops the game before the agent's next decision: the game in
    /// progress returns a view with no decision, and no decision opens again. A session sets
    /// it when it is dropped, so an abandoned game stops thinking.
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

    /// Plays the agent's decisions until the human must decide or the game ends.
    fn advance(&mut self) -> PlayerView {
        loop {
            match next(&mut self.state, &self.table, &mut self.scratch, &mut Replay) {
                Step::Over(outcome) => {
                    let events = self.events();
                    return self.view(self.seq, None, Some(outcome), events);
                }
                Step::Decide(d) if d.player == self.human => {
                    self.seq += 1;
                    self.open = Some((self.seq, d));
                    let events = self.events();
                    return self.view(self.seq, Some(&d), None, events);
                }
                Step::Decide(_) if self.stop.load(Ordering::Relaxed) => {
                    return self.view(self.seq, None, None, Vec::new());
                }
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

    /// The human's view of the open decision's position.
    fn observation(&mut self) -> Observation<'_> {
        Observation::new(
            view(&self.state, &self.table, &mut self.scratch),
            self.human,
        )
    }

    fn events(&mut self) -> Vec<GameEvent> {
        self.scratch.events.drain().map(|e| (&e).into()).collect()
    }

    fn view(
        &mut self,
        seq: u32,
        d: Option<&Decision>,
        outcome: Option<Outcome>,
        events: Vec<GameEvent>,
    ) -> PlayerView {
        let obs = Observation::new(
            view(&self.state, &self.table, &mut self.scratch),
            self.human,
        );
        player_view(&self.table, obs, seq, d, outcome, self.stops, events)
    }
}

pub(crate) fn with_stops(profile: Profile, stops: StopsSetting) -> Profile {
    Profile {
        stops: stops.stops(),
        ..profile
    }
}
