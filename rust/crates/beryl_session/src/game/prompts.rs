//! The human's open prompt in a game: its state, which the session holds (plan K26), the
//! clicks that change it, and the run of picks a click that answers it stands for.

use beryl_engine::card::Purpose;
use beryl_engine::{Choice, Decision, GameTable, Large, Layout, Relayout as _, State, view};

use super::{Answer, At, Boards, Game, Open};
use crate::prompt::{ClickReply, ClickRequest, Clicked, Held, Run};
use crate::protocol::{CandidateIndex, DecisionId, PlayerView, Status};
use crate::{SessionError, ipc};

/// Makes the state of the prompt of `view`'s open decision, if it has none yet and the
/// session holds its shape's, and writes the state as it stands into the view: a view drawn
/// again shows the prompt as the clicks since left it. `d` is the decision in the game `s`,
/// which the seat sees as `shown`; none at a halt.
pub(super) fn hold<L: Layout>(
    held: &mut Option<(DecisionId, Held)>,
    view: &mut PlayerView,
    s: &State<L>,
    shown: &State<L>,
    t: &GameTable,
    d: Option<&Decision<L>>,
) {
    let Status::Deciding { decision } = &view.status else {
        return;
    };
    let id = decision.id;
    match held {
        Some((open, held)) if *open == id => held.reword(decision),
        _ => *held = Held::open(s, shown, t, d, view).map(|held| (id, held)),
    }
    if let (Some((_, held)), Status::Deciding { decision }) = (held, &mut view.status) {
        *decision.prompt = held.view().clone();
    }
}

impl Game {
    /// Takes a click on the human's open prompt and returns its state after it, or that the
    /// click answered it: the game then plays on from the answer ([`Game::next_view`]),
    /// making the rest of the prompt's picks as their decisions arrive. A click changes
    /// nothing but the prompt until it answers, and the choice log records the picks, never
    /// the clicks.
    pub fn click(&mut self, request: ClickRequest) -> Result<ClickReply, SessionError> {
        let open = self.open().ok_or(SessionError::NotDeciding)?;
        // An answer the open decision would take is one this prompt is open for.
        open.answer(request.decision, CandidateIndex(0))?;
        let At::Human(id, at) = self.at.clone() else {
            return Err(SessionError::NotDeciding);
        };
        let Some((_, held)) = self.prompt.as_mut().filter(|(held, _)| *held == id) else {
            return Err(SessionError::PromptNotHeld);
        };
        let revision = held.view().revision;
        if request.revision != revision {
            return Err(SessionError::StalePrompt {
                revision: request.revision,
                open: revision,
            });
        }
        let clicked = held.click(request.click);
        // The prompt as the click left it: what it was, unless the click changed it.
        let prompt = Box::new(held.view().clone());
        Ok(match clicked {
            Clicked::Same | Clicked::Changed => ClickReply::Open { prompt },
            Clicked::Opens(prompt) => ClickReply::Open { prompt },
            Clicked::PassesOn => {
                self.pass_on(open.pass_on(id)?);
                ClickReply::Answered
            }
            Clicked::Answers(mut run) => {
                let choice = match &at {
                    Open::Decision(d) => self.run_pick(&mut run, d),
                    Open::Halt => Some(Choice(0)),
                };
                // A run's first pick answers the decision its prompt is open for.
                let Some(choice) = choice else {
                    return Ok(ClickReply::Open { prompt });
                };
                // A run waiting through this prompt goes on once this one's picks are made.
                let waiting = self.run.take();
                self.choose(Answer {
                    id,
                    choice,
                    continuation: false,
                });
                self.run = run.over(waiting).rest();
                ClickReply::Answered
            }
        })
    }

    /// The pick `run` makes at `d`, in the position `d` is asked in.
    fn run_pick(&mut self, run: &mut Run, d: &Decision<Large>) -> Option<Choice> {
        match &mut self.board {
            Boards::Small(b) => run.answer(view(&b.state, &self.table, &mut b.peek), &d.relayout()),
            Boards::Large(b) => run.answer(view(&b.state, &self.table, &mut b.peek), &d.relayout()),
        }
    }

    /// The answer the run of picks in progress gives the human's decision `d`, if a run is in
    /// progress and `d` is its next: the session makes it as it does its automatic answers,
    /// with no view sent. A decision the run does not answer ends it, unless the run waits
    /// through it: an order of triggers, through the targets of the one just put on the
    /// stack.
    pub(super) fn pick_in_run(&mut self, d: &Decision<Large>) -> Option<Choice> {
        let mut run = self.run.take()?;
        let choice = self.run_pick(&mut run, d);
        self.run = run.rest();
        choice
    }

    /// Checks `index` as an answer to the human's open decision `id`.
    pub fn answer(&self, id: DecisionId, index: CandidateIndex) -> Result<Answer, SessionError> {
        self.open()
            .ok_or(SessionError::NotDeciding)?
            .answer(id, index)
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

    /// Previews the human's selected waterbenders on a copy, without advancing the game.
    pub fn preview_waterbend(
        &self,
        request: &ipc::WaterbendRequest,
    ) -> Result<Option<ipc::WaterbendPreview>, SessionError> {
        self.answer(request.decision, CandidateIndex(0))?;
        let At::Human(_, Open::Decision(d)) = &self.at else {
            return Err(SessionError::NotDeciding);
        };
        if d.prompt.purpose != Purpose::Waterbend {
            return Ok(None);
        }
        Ok(match &self.board {
            Boards::Small(b) => {
                crate::preview::waterbend(&b.state, &self.table, &d.relayout(), &request.selected)
            }
            Boards::Large(b) => {
                crate::preview::waterbend(&b.state, &self.table, &d.relayout(), &request.selected)
            }
        })
    }
}
