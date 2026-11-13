//! A game played through clicks on its prompts, for the hand-written expectations of prompt
//! tests: each click's reply is the prompt's state as text (`prompt/text.rs`), and
//! a click that answers plays on to the human's next decision. The same game can be played
//! by candidates, as a UI that holds a prompt's state itself answers it, to compare where the
//! two end.
//!
//! A test file takes it in with `#[path = "common/play.rs"] mod play;`.

// Each test file uses the part of this it needs.
#![allow(dead_code)]

use beryl_cards::CARDS;
use beryl_engine::layout::Queue as _;
use beryl_engine::{
    Candidates, Choice, Decision, GameTable, Layout, Observation, Opt, Replay, Scratch, Seat,
    State, Step, next,
};
use beryl_scenario::Scenario;
use beryl_search::{Agent, AgentDecision, Budget, GameAgent};
use beryl_session::prompt::{ButtonId, Click, ClickReply, ClickRequest};
use beryl_session::protocol::{
    Candidate, DecisionView, Moment, ObjectView, Side, ZoneName, player_view,
};
use beryl_session::script::Subject;
use beryl_session::{
    CandidateIndex, DecisionId, Game, PlayerView, Prepared, SessionId, StartGame, Status,
    StopsSetting,
};

pub const SESSION: SessionId = SessionId(1);

/// A game and the human's latest view of it, at their open decision or the end.
pub struct Play {
    pub game: Game,
    pub view: PlayerView,
}

/// A card by its slot.
pub const fn card(slot: u16) -> Subject {
    Subject::Object { slot }
}

/// Cards as a line of a prompt's text lists them: by slot, ascending.
pub fn cards(slots: &[u16]) -> String {
    let mut slots = slots.to_vec();
    slots.sort_unstable();
    let cards: Vec<String> = slots.iter().map(|slot| format!("#{slot}")).collect();
    cards.join(" ")
}

/// `scenario`'s position played to its first decision, for a prompt held apart from a game
/// ([`Held::opening`](beryl_session::prompt::Held::opening)): a shape no card asks yet is
/// tested from a decision built by hand in such a position.
pub fn position(scenario: &Scenario) -> (GameTable, State) {
    let (table, mut state) = beryl_scenario::position(&CARDS, scenario).unwrap();
    let mut scratch = Scratch::new();
    let Step::Decide(_) = next(&mut state, &table, &mut scratch, &mut Replay) else {
        panic!("the scenario ends before a decision");
    };
    (table, state)
}

/// The view of `state`, in which `d` is put to the first player, as their seat is sent it.
pub fn asked(table: &GameTable, state: &State, d: &Decision) -> PlayerView {
    let id = DecisionId {
        session: SESSION,
        seq: 1,
    };
    player_view(
        table,
        Observation::new(state, Seat::P0),
        Moment::Deciding(id, d, None),
        StopsSetting::SMART,
        &[],
    )
}

impl Play {
    /// `scenario`'s position with the first player the human, under the smart stops, played
    /// to their first decision. The other seat is the random agent, seeded the same each
    /// time, so two plays of one scenario that answer alike go alike.
    pub fn scenario(scenario: &Scenario) -> Play {
        Play::against(scenario, None)
    }

    /// [`Play::scenario`] against `opponent`, or the random agent.
    pub fn against(scenario: &Scenario, opponent: Option<Box<dyn GameAgent + Send>>) -> Play {
        Play::with(scenario, opponent, StopsSetting::SMART)
    }

    /// [`Play::against`] under `stops`.
    pub fn with(
        scenario: &Scenario,
        opponent: Option<Box<dyn GameAgent + Send>>,
        stops: StopsSetting,
    ) -> Play {
        let (table, state) = beryl_scenario::position(&CARDS, scenario).unwrap();
        let random = beryl_search::agent("random").unwrap();
        let game = Game::prepared(
            Prepared {
                opponent: opponent.unwrap_or_else(|| (random.new)(&table, 1, Budget::DEFAULT)),
                stand_in: (random.new)(&table, 2, Budget::DEFAULT),
                hand_over: None,
                scripted: false,
                table,
                state,
                human: Seat::P0,
                stops,
            },
            SESSION,
        );
        Play::begin(game)
    }

    /// A game dealt from `start`, played to the human's first decision.
    pub fn seeded(start: &StartGame) -> Play {
        Play::begin(Game::new(&CARDS, start, SESSION).unwrap())
    }

    fn begin(mut game: Game) -> Play {
        let view = to_human(&mut game);
        Play { game, view }
    }

    /// The human's open decision.
    pub fn decision(&self) -> &DecisionView {
        match &self.view.status {
            Status::Deciding { decision } => decision,
            status => panic!("no decision is open: {status:?}"),
        }
    }

    /// The open prompt's state as text.
    pub fn prompt(&self) -> String {
        self.decision().prompt.to_string()
    }

    /// Clicks, on the prompt's state as the last reply left it. Returns the state after as
    /// text, or none if the click answered the prompt: the game then stands at the human's
    /// next decision.
    pub fn click(&mut self, click: Click) -> Option<String> {
        let decision = self.decision();
        let request = ClickRequest {
            decision: decision.id,
            revision: decision.prompt.revision,
            click,
        };
        match self.game.click(request).unwrap() {
            ClickReply::Open { prompt } => {
                let text = prompt.to_string();
                if let Status::Deciding { decision } = &mut self.view.status {
                    decision.prompt = prompt;
                }
                Some(text)
            }
            ClickReply::Answered => {
                self.view = to_human(&mut self.game);
                None
            }
        }
    }

    /// Clicks the card in `slot`.
    pub fn on(&mut self, slot: u16) -> Option<String> {
        self.click(Click::On { on: card(slot) })
    }

    /// Clicks a button.
    pub fn press(&mut self, button: ButtonId) -> Option<String> {
        self.click(Click::Button { button })
    }

    /// Answers the open decision with the candidate `wanted` names, as a UI that holds the
    /// prompt's state itself does, and plays on to the human's next decision.
    pub fn choose(&mut self, wanted: impl Fn(&Candidate) -> bool) {
        self.answer(wanted, false);
    }

    /// [`Play::choose`], as the continuation of the answer before: what such a UI sends for
    /// the picks it plans after a cast's own.
    pub fn follow(&mut self, wanted: impl Fn(&Candidate) -> bool) {
        self.answer(wanted, true);
    }

    fn answer(&mut self, wanted: impl Fn(&Candidate) -> bool, continuation: bool) {
        let decision = self.decision();
        let index = (decision.candidates.iter())
            .position(|c| wanted(&c.option))
            .unwrap_or_else(|| panic!("no such candidate among {:?}", decision.candidates));
        let answer = (self.game)
            .answer(decision.id, CandidateIndex(index as u16))
            .unwrap();
        self.game.choose(if continuation {
            answer.as_continuation()
        } else {
            answer
        });
        self.view = to_human(&mut self.game);
    }

    /// The objects named `name` in `zone` that `side` controls, by slot, ascending.
    pub fn named(&self, name: &str, zone: ZoneName, side: Side) -> Vec<u16> {
        let mut slots: Vec<u16> = (self.view.objects.iter())
            .filter(|o| o.name.as_deref() == Some(name) && o.zone == zone && o.controller == side)
            .map(|o| o.slot)
            .collect();
        slots.sort_unstable();
        slots
    }

    /// The object in `slot`.
    pub fn object(&self, slot: u16) -> &ObjectView {
        (self.view.objects.iter())
            .find(|o| o.slot == slot)
            .expect("an object of the view")
    }
}

/// An opponent that casts whatever it can, and otherwise takes each decision's first
/// candidate.
pub struct Casts;

impl<L: Layout> Agent<L> for Casts {
    fn choose(&mut self, _: Observation<'_, L>, d: &Decision<L>, _: AgentDecision) -> Choice {
        let cast = match &d.candidates {
            Candidates::Options(options) => {
                (options.iter()).position(|o| matches!(o, Opt::Cast { .. }))
            }
            Candidates::Objects(_) => None,
        };
        Choice(cast.unwrap_or(0) as u16)
    }
}

/// Plays on to the human's next decision or the end, and returns the view there.
pub fn to_human(game: &mut Game) -> PlayerView {
    let mut last = None;
    while let Some(view) = game.next_view() {
        last = Some(view);
    }
    last.expect("the game plays on from where it is")
}
