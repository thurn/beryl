//! Props: a prompt shape no card asks yet, stood on a board so that the UI has it to draw
//! (plan K26). The session holds the state of an entry chosen from a catalog and of an amount
//! divided among recipients, and tests both from a decision built by hand
//! (`beryl_session`'s `tests/divisions.rs`); a prop builds the same decision in a board's
//! opening position and records it as the seat's first view would ask it: the prompt as
//! [`Held`] opens it, and each click with the state it leaves. No game takes the answer. The
//! click that answers is followed by the view the board really stopped at, which asks what the
//! game asks there.
//!
//! The seat across the table is shown the same decision as one its opponent is making: the
//! first view of its recording, in which that player is asked something, says they are asked
//! this instead.

use beryl_cards::CARDS;
use beryl_engine::card::{Prompt, Purpose};
use beryl_engine::{
    Bitboard, Decision, DecisionKind, GameTable, Layout, Observation, Opt, Replay, Scratch, Seat,
    Small, State, Step, next,
};
use beryl_scenario::Scenario;
use beryl_session::prompt::{ClickReply, ClickRequest, Clicked, Held};
use beryl_session::protocol::{Moment, player_view};
use beryl_session::script::Script;
use beryl_session::{DecisionId, GameUpdate, Status, StopsSetting};

/// The player a prop's decision is put to: the first, whose turn a prop's board opens in.
const ASKED: Seat = Seat::P0;

/// Which decision is stood on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Prop {
    /// A color, of the five.
    Color,
    /// Five damage divided among the permanents on the battlefield.
    Divide,
}

/// What a division prop gives out.
const DAMAGE: u8 = 5;

impl Prop {
    pub(super) fn parse(text: &str) -> Result<Prop, String> {
        match text {
            "color" => Ok(Prop::Color),
            "divide" => Ok(Prop::Divide),
            _ => Err(format!("no prop is named {text:?}")),
        }
    }

    /// The decision, in the position `state`.
    fn decision(self, state: &State) -> Result<Decision, String> {
        let numbers = |from: u8, to: u8, prompt: Prompt| {
            let mut offered = <Small as Layout>::Queue::<Opt, 64>::default();
            for n in from..=to {
                offered
                    .push(Opt::Number(n))
                    .map_err(|_| "too many numbers".to_owned())?;
            }
            Ok::<_, String>(Decision::options(
                ASKED,
                DecisionKind::ChooseObject,
                offered,
                prompt,
            ))
        };
        match self {
            // A number for each color, in the rules' order.
            Prop::Color => numbers(0, 4, Prompt::new(Purpose::Color)),
            // The decision is about the recipients still to be given a share and offers the
            // shares the first may take: each of the others is left at least one.
            Prop::Divide => {
                let among = (state.battlefield().iter())
                    .fold(Bitboard::EMPTY, |all, &slot| all | Bitboard::single(slot));
                let others = u8::try_from(state.battlefield().iter().count())
                    .ok()
                    .and_then(|recipients| recipients.checked_sub(1))
                    .filter(|&others| others < DAMAGE)
                    .ok_or("a division prop's board has one to five permanents")?;
                let prompt = Prompt::new(Purpose::DivideDamage)
                    .count(DAMAGE)
                    .most(DAMAGE);
                Ok(numbers(1, DAMAGE - others, prompt)?.about(among))
            }
        }
    }
}

/// A prop, the board it stands on and the stops its views are drawn under.
#[derive(Clone, Copy)]
pub(super) struct Standing {
    pub(super) prop: Prop,
    pub(super) board: &'static Scenario,
    pub(super) stops: StopsSetting,
}

/// A prop stood on a board: the position it is asked in, and its prompt as the clicks made so
/// far have left it.
pub(super) struct Stood {
    table: GameTable,
    state: State,
    decision: Decision,
    held: Held,
    /// The decision's name, which a click must give.
    id: DecisionId,
    answered: bool,
}

/// The opening position of `board`, at its first decision, and `prop`'s decision in it.
fn position(prop: Prop, board: &'static Scenario) -> Result<(GameTable, State, Decision), String> {
    let (table, mut state) = beryl_scenario::position(&CARDS, board).map_err(|e| e.to_string())?;
    let mut scratch = Scratch::new();
    let Step::Decide(_) = next(&mut state, &table, &mut scratch, &mut Replay) else {
        return Err("the board ends before a decision".to_owned());
    };
    let decision = prop.decision(&state)?;
    Ok((table, state, decision))
}

/// Stands a prop on its board in place of what `stop` asks the first player, the view its
/// recording stopped at with nothing yet played: the view to record in that one's place, and
/// the prop to click.
pub(super) fn stand(
    Standing { prop, board, stops }: Standing,
    stop: &GameUpdate,
) -> Result<(GameUpdate, Stood), String> {
    let Status::Deciding { decision: asked } = &stop.view.status else {
        return Err("a prop stands where its board's seat is first asked".to_owned());
    };
    let id = asked.id;
    let (table, state, decision) = position(prop, board)?;
    let mut view = player_view(
        &table,
        Observation::new(&state, ASKED),
        Moment::Deciding(id, &decision, None),
        stops,
        &[],
    );
    let held = Held::opening(&view, &state, &table, Some(&decision))
        .ok_or("the session holds no state for the prop's prompt")?;
    if let Status::Deciding { decision } = &mut view.status {
        *decision.prompt = held.view().clone();
    }
    let mut update = stop.clone();
    update.view.status = view.status;
    let stood = Stood {
        table,
        state,
        decision,
        held,
        id,
        answered: false,
    };
    Ok((update, stood))
}

/// `first`, the first view of a recording from `seat`, across the table from `prop`, saying
/// that the first player is asked its decision; or as it is, with no prop.
pub(super) fn watched(
    prop: Option<Standing>,
    seat: Seat,
    first: &GameUpdate,
) -> Result<GameUpdate, String> {
    let Some(Standing { prop, board, stops }) = prop else {
        return Ok(first.clone());
    };
    if !matches!(first.view.status, Status::Thinking) {
        return Err(
            "a prop is watched from a first view in which the other player decides".to_owned(),
        );
    }
    let (table, state, decision) = position(prop, board)?;
    let view = player_view(
        &table,
        Observation::new(&state, seat),
        Moment::Thinking {
            resolving: None,
            decision: Some(&decision),
        },
        stops,
        &[],
    );
    let mut update = first.clone();
    update.view.status = view.status;
    Ok(update)
}

impl Stood {
    /// The prompt's reply to a click, as the session would give it.
    pub(super) fn click(&mut self, request: ClickRequest) -> Result<ClickReply, String> {
        if self.answered {
            return Err("the prop is answered".to_owned());
        }
        if request.decision != self.id || request.revision != self.held.view().revision {
            return Err("a click on another state of the prop".to_owned());
        }
        let (state, table) = (&self.state, &self.table);
        let clicked = (self.held).click(state, table, Some(&self.decision), request.click);
        Ok(match clicked {
            Clicked::Same | Clicked::Changed => ClickReply::Open {
                prompt: Box::new(self.held.view().clone()),
            },
            Clicked::Answers(_) => {
                self.answered = true;
                ClickReply::Answered
            }
            Clicked::PassesOn => return Err("a prop passes nothing on".to_owned()),
        })
    }
}

/// The view the answer to a prop leads to: `stop`, what its board really asks there, as a
/// decision of its own with nothing to play first.
pub(super) fn after(stop: &GameUpdate) -> GameUpdate {
    let mut update = stop.clone();
    if let Status::Deciding { decision } = &mut update.view.status {
        decision.id.seq += 1;
    }
    update.view.script = Some(Script {
        reset: None,
        beats: Vec::new(),
    });
    update
}
