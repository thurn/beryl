//! A game between a human seat and an agent, played synchronously: the core a session runs on
//! its thread, and that tests drive directly. It plays on view by view ([`Game::next_view`]):
//! as each step of the turn begins, before each of the agent's decisions, at each of the
//! human's, and at the end, so the human sees the agent's turn as it is played. Under the
//! smart stops the session also halts the human where the engine would pass them by, and
//! answers some of their decisions for them: passing to their own spell, and ordering their
//! triggers.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use beryl_engine::event::Event;
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::{Pending, StackEntry};
use beryl_engine::{
    Candidates, CardTable, Choice, Decision, DecisionKind, GameTable, Observation, Opt, Outcome,
    Profile, Replay, Scratch, Seat, State, Step, TurnStep, apply, next_in_step, observe,
    paused_events, view,
};
use beryl_search::{Agent, Budget};

use crate::preview;
use crate::protocol::{
    CandidateIndex, DecisionId, Moment, PlayerView, SessionId, Status, StopsSetting, player_view,
};
use crate::replay::{Entry, ReplayFile};
use crate::{Prepared, SessionError, StartGame};

/// The human's open decision, as a session records it: the one place an answer is checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenDecision {
    id: DecisionId,
    candidates: usize,
    /// The human may take back the cast it is part of ([`Game::cancel`]).
    cancellable: bool,
}

impl OpenDecision {
    fn new(id: DecisionId, open: &Open, cancellable: bool) -> OpenDecision {
        OpenDecision {
            id,
            candidates: match open {
                Open::Decision(d) => d.len(),
                Open::Halt => 1,
            },
            cancellable,
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

/// The game as it stood at the human's priority before they began to cast a spell or activate
/// an ability, kept until its costs are paid so that they can take it back ([`Game::cancel`]).
#[derive(Clone, Debug)]
struct BeforeCast {
    state: State,
    at: At,
    answered: Option<Vec<StackEntry>>,
    /// The length of the choice log then.
    log: usize,
    stops: StopsSetting,
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
    /// The stack as it was once the human's latest answer was applied, if they answered in
    /// this step: what they know of it ([`Game::own_top`]).
    answered: Option<Vec<StackEntry>>,
    seq: u32,
    stops: StopsSetting,
    replay: ReplayFile,
    /// Set from another thread to stop the agent at its next decision.
    stop: Arc<AtomicBool>,
    /// The engine's events the latest view reported.
    events: Vec<Event>,
    /// The events of the paused resolution the latest view was in, which it reported: the
    /// body executes them again as it goes on (architecture §7.3).
    paused: Vec<Event>,
    /// Where the human's cast in progress began, while it can be taken back.
    before_cast: Option<BeforeCast>,
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
        game
    }

    fn assemble(
        table: GameTable,
        state: &State,
        replay: ReplayFile,
        human: Seat,
        session: SessionId,
        agent: Box<dyn Agent + Send>,
    ) -> Game {
        let turn = state.turn();
        Game {
            seen: (turn.number, turn.step),
            answered: None,
            stops: replay.stops,
            table,
            state: *state,
            scratch: Scratch::recording(),
            peek: Scratch::new(),
            human,
            session,
            agent,
            stand_in: None,
            at: At::Playing,
            seq: 0,
            replay,
            stop: Arc::default(),
            events: Vec::new(),
            paused: Vec::new(),
            before_cast: None,
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
                    game.apply(entry);
                    game.at = At::Playing;
                }
                (At::Human(..), Entry::Stops(stops)) => game.change_stops(stops),
                (At::Human(id, open @ Open::Decision(_)), Entry::Choice(_))
                | (At::Human(id, open @ Open::Halt), Entry::Halt) => {
                    let c = if let Entry::Choice(c) = entry { c } else { 0 };
                    let answer = OpenDecision::new(id, &open, false)
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
        match self.at {
            At::Human(..) | At::Over(_) | At::Stopped => return None,
            At::Agent(_) if self.stop.load(Ordering::Relaxed) => self.at = At::Stopped,
            At::Agent(d) => {
                let obs = observe(&self.state, &self.table, &mut self.scratch, d.player);
                let c = self.agent.choose(obs, &d);
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
            At::Human(id, open) => Some(OpenDecision::new(*id, open, self.cancellable())),
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
        matches!(self.state.pending(), Pending::Casting(c) if !c.triggered)
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
        self.state = before.state;
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
        self.scratch.events.drain();
        // The view says nothing happened: the cast never was.
        self.events.clear();
        Ok(self.draw())
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
            Open::Decision(d) => {
                // A cast or an activation from priority can be taken back until it is paid for.
                let before = (d.kind == DecisionKind::Priority).then(|| BeforeCast {
                    state: self.state,
                    at: self.at,
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
        self.answered = Some(self.state.stack().to_vec());
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
        let answer = OpenDecision::new(id, &open, false)
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
        loop {
            let step = next_in_step(
                &mut self.state,
                &self.table,
                &mut self.scratch,
                &mut Replay,
                self.seen,
            );
            // Once a cast is complete, or another player decides, it can no longer be taken back.
            if !matches!(step, Some(Step::Decide(d)) if d.player == self.human && self.casting()) {
                self.before_cast = None;
            }
            self.at = match step {
                None => {
                    let turn = self.state.turn();
                    self.seen = (turn.number, turn.step);
                    self.answered = None;
                    if self.halts() {
                        self.seq += 1;
                        At::Human(self.decision_id(), Open::Halt)
                    } else {
                        At::Playing
                    }
                }
                Some(Step::Over(outcome)) => At::Over(outcome),
                Some(Step::Decide(d)) if d.player == self.human => {
                    if let Some(c) = self.auto_answer(&d) {
                        self.apply(Entry::Auto(c.0));
                        self.answered = Some(self.state.stack().to_vec());
                        continue;
                    }
                    self.seq += 1;
                    At::Human(self.decision_id(), Open::Decision(d))
                }
                Some(Step::Decide(d)) => At::Agent(d),
            };
            return;
        }
    }

    /// The session's own answer to the human's decision `d`, if it gives one: under the
    /// smart stops, it passes their priority while only their own doings sit on the stack
    /// ([`Game::own_top`]), orders their simultaneous triggers ([`trigger_to_stack`]), and
    /// pays a waterbend cost with mana whenever mana can pay it, tapping no artifact or
    /// creature ([CR 701.67a](https://yawgatog.com/resources/magic-rules/#R70167a): "you may
    /// tap an untapped artifact or creature you control rather than pay that mana").
    /// Like halts, these are the session's alone: the engine's smart stops, which the agent's
    /// search shares, are unchanged, and exact stops ask every one. A function of the position,
    /// the stops and the human's answers, so a replay finds each again; the replay records
    /// each as an [`Entry::Auto`].
    fn auto_answer(&self, d: &Decision) -> Option<Choice> {
        if self.stops != StopsSetting::Smart {
            return None;
        }
        match d.kind {
            DecisionKind::Priority if self.own_top() => {
                debug_assert_eq!(d.candidates.option(Choice(0)), Some(Opt::Pass));
                Some(Choice(0))
            }
            DecisionKind::TriggerOrder => Some(trigger_to_stack(&self.state, &self.table, d)),
            DecisionKind::Waterbend => (0..d.len() as u8)
                .map(Choice)
                .find(|&c| d.candidates.option(c) == Some(Opt::Done)),
            _ => None,
        }
    }

    /// Whether the top of the stack is the human's own spell or ability and, since their
    /// latest answer in this step, nothing but their own entries came onto the stack or left
    /// it: what they cast, its triggers, and its resolution, but not an opponent's response
    /// or its resolution, which they get priority to see and answer. Their priority then
    /// would only let them respond to themselves, as an Arena player auto-passes.
    fn own_top(&self) -> bool {
        let Some(answered) = &self.answered else {
            return false;
        };
        let now = self.state.stack();
        let own = |e: &StackEntry| e.controller == self.human;
        let kept = answered
            .iter()
            .zip(now)
            .take_while(|(then, now)| then == now)
            .count();
        now.last().is_some_and(own)
            && answered[kept..].iter().all(own)
            && now[kept..].iter().all(own)
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
            .set_profile(with_stops(self.state.profile(), stops, self.human));
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
        apply(
            &mut self.state,
            &self.table,
            &mut self.scratch,
            &mut Replay,
            Choice(c),
        );
    }

    /// The human's view now, with the engine's events since the previous view. A view in a
    /// paused resolution shows its body done up to its question (§7.3), so it reports the
    /// events of that, once: the card a loot draws comes into the hand before the discard is
    /// asked, not after. The body executes them again whenever it runs on, after an answer,
    /// and those repeats go unreported.
    fn view(&mut self) -> PlayerView {
        let mut fresh: Vec<Event> = self.scratch.events.drain().collect();
        let seen = self.paused.len();
        if seen > 0
            && let Some(at) = fresh.windows(seen).position(|w| w == self.paused)
        {
            fresh.drain(at..at + seen);
        }
        let paused = paused_events(&self.state, &self.table, &mut self.scratch);
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
        let mut view = player_view(&self.table, obs, moment, self.stops, &self.events);
        if let At::Human(_, Open::Decision(d)) = &self.at {
            preview::add_previews(&mut view, &self.state, &self.table, d);
        }
        if let Status::Deciding { decision } = &mut view.status {
            decision.cancellable = self.cancellable();
        }
        view
    }
}

fn new_agent(name: &str, t: &GameTable, seed: u64) -> Result<Box<dyn Agent + Send>, SessionError> {
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
fn trigger_to_stack(s: &State, t: &GameTable, d: &Decision) -> Choice {
    let resolves_late = |opt: Opt| {
        let Opt::Trigger(i) = opt else {
            unreachable!("a trigger-order decision offers only triggers");
        };
        let tr = &s.triggers()[usize::from(i)];
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
    Choice(u8::try_from(last).expect("at most 64 candidates"))
}

/// `profile` under the human's `stops`. Full control asks the human every priority they get,
/// as Arena does, even where the engine would pass for them; their opponent's passes stay
/// silent.
pub fn with_stops(profile: Profile, stops: StopsSetting, human: Seat) -> Profile {
    Profile {
        stops: stops.stops(),
        full_control: (stops == StopsSetting::Exact).then_some(human),
        ..profile
    }
}
