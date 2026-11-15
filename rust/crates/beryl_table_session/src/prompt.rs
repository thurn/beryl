//! Host-only prompt state. The wire view uses viewer slots, never table IDs.
use beryl_session::ipc::{TableAnswerRequest, TableLobbyRequest};
use beryl_session::protocol::{
    Answer, CardHandling, ObjectId as Slot, PileName, Side, TableAction, TablePrompt, ZoneName,
};
use beryl_table::action::Flow;
use beryl_table::assist::text::FlowChoice;
use beryl_table::state::{ObjectId, Zone};
use beryl_table::{Action, Refusal, Seat, Table};
use serde::{Deserialize, Serialize};

use crate::publication::SeatPublication;

// The link preserves the ordinary action payload and wraps prompt answers.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum Request {
    Answer(TableAnswerRequest),
    Lobby(TableLobbyRequest),
    Action(TableAction),
}

#[derive(Clone)]
pub enum PromptState {
    Sort(Sort),
    Choose {
        title: String,
        options: Vec<String>,
        selected: Option<usize>,
    },
    Opening {
        hand: Vec<ObjectId>,
        mulligans: u16,
        decision: Option<OpeningDecision>,
        bottom: Vec<ObjectId>,
    },
}

#[derive(Clone)]
pub struct Sort {
    seat: Seat,
    flow: Flow,
    title: String,
    cards: Vec<ObjectId>,
    piles: Vec<PileName>,
    placed: Vec<Vec<ObjectId>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OpeningDecision {
    Keep,
    Mulligan,
}

type ActionCompletion = Box<dyn Fn(&PromptState) -> Result<Action, Refusal> + Send + Sync>;

pub(crate) enum Completion {
    Action(ActionCompletion),
    FirstPlayer,
    OpeningHand,
}

impl Completion {
    pub(crate) fn valid(&self, table: &Table) -> bool {
        match self {
            Self::Action(_) => true,
            Self::FirstPlayer => !table.game_match.in_game,
            Self::OpeningHand => table.game_match.in_game && table.turn == 0,
        }
    }
}

pub(crate) struct OpenPrompt {
    pub state: PromptState,
    pub complete: Completion,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Progress {
    Editing,
    Complete,
    Cancel,
}

impl PromptState {
    /// Library flows start with the top cards in their original order.
    pub fn library_sort(
        table: &Table,
        seat: Seat,
        title: String,
        flow: Flow,
        count: u16,
    ) -> Result<Self, Refusal> {
        let cards: Vec<_> = table
            .zone(Zone::Library(seat))
            .ok_or(Refusal::Stale)?
            .iter()
            .take(usize::from(count))
            .copied()
            .collect();
        if cards.is_empty() {
            return Err(Refusal::INVALID);
        }
        // The pile accepting cards removed from the top is on the left on desktop.
        let (piles, placed) = match flow {
            Flow::Scry => (
                vec![PileName::Bottom, PileName::Top],
                vec![vec![], cards.clone()],
            ),
            Flow::Surveil => (
                vec![PileName::Graveyard, PileName::Top],
                vec![vec![], cards.clone()],
            ),
            Flow::LookAtTop => (vec![PileName::Top], vec![cards.clone()]),
        };
        Ok(Self::Sort(Sort {
            seat,
            flow,
            title,
            cards,
            piles,
            placed,
        }))
    }

    pub(crate) fn peek(&self) -> Option<Action> {
        let Self::Sort(sort) = self else { return None };
        Some(Action::Flow {
            seat: sort.seat,
            flow: sort.flow,
            count: u16::try_from(sort.cards.len()).expect("bounded by the requested count"),
        })
    }

    pub(crate) fn valid(&self, table: &Table, seat: Seat) -> bool {
        match self {
            Self::Sort(sort) => {
                seat == sort.seat
                    && table
                        .zone(Zone::Library(seat))
                        .is_some_and(|ids| ids.starts_with(&sort.cards))
            }
            Self::Choose { options, .. } => !options.is_empty(),
            Self::Opening { hand, .. } => table.zone(Zone::Hand(seat)) == Some(hand.as_slice()),
        }
    }

    pub(crate) fn observer_lift(&self) -> Option<CardHandling> {
        let Self::Sort(sort) = self else { return None };
        Some(CardHandling::Pile {
            side: Side::Opponent,
            pile: ZoneName::Library,
            count: u32::try_from(sort.cards.len()).expect("bounded by a u16"),
            bottom: false,
        })
    }

    pub(crate) fn view(&self, slot: impl Fn(ObjectId) -> Option<Slot>) -> Option<TablePrompt> {
        let slots = |ids: &[ObjectId]| ids.iter().map(|id| slot(*id)).collect::<Option<Vec<_>>>();
        Some(match self {
            Self::Sort(sort) => TablePrompt::Sort {
                title: sort.title.clone(),
                cards: slots(&sort.cards)?,
                piles: sort.piles.clone(),
                placed: sort
                    .placed
                    .iter()
                    .map(|row| slots(row))
                    .collect::<Option<_>>()?,
            },
            Self::Choose {
                title,
                options,
                selected,
            } => TablePrompt::Choose {
                title: title.clone(),
                options: options.clone(),
                selected: *selected,
            },
            Self::Opening {
                hand,
                mulligans,
                decision,
                bottom,
            } => TablePrompt::Opening {
                hand: slots(hand)?,
                mulligans: *mulligans,
                to_bottom: (*decision == Some(OpeningDecision::Keep))
                    .then(|| bottom_limit(hand, *mulligans) as u16),
                bottom: slots(bottom)?,
            },
        })
    }

    pub(crate) fn answer(
        &mut self,
        answer: Answer,
        publication: &SeatPublication,
    ) -> Result<Progress, Refusal> {
        let object = |ids: &[ObjectId], slot| {
            ids.iter()
                .copied()
                .find(|id| publication.slot(*id) == Some(slot))
                .ok_or(Refusal::Stale)
        };
        match (self, answer) {
            (_, Answer::Cancel) => return Ok(Progress::Cancel),
            (Self::Sort(sort), Answer::Place { card, pile, index }) => {
                let id = object(&sort.cards, card)?;
                let row = sort
                    .piles
                    .iter()
                    .position(|&name| name == pile)
                    .ok_or(Refusal::INVALID)?;
                sort.unplace(id);
                if index > sort.placed[row].len() {
                    return Err(Refusal::INVALID);
                }
                sort.placed[row].insert(index, id);
            }
            (Self::Sort(sort), Answer::Unplace { card }) => {
                sort.unplace(object(&sort.cards, card)?);
            }
            (Self::Sort(sort), Answer::Done)
                if sort.placed.iter().map(Vec::len).sum::<usize>() == sort.cards.len() =>
            {
                return Ok(Progress::Complete);
            }
            (
                Self::Choose {
                    options, selected, ..
                },
                Answer::Pick { option },
            ) if option < options.len() => {
                *selected = Some(option);
            }
            (
                Self::Choose {
                    selected: Some(_), ..
                },
                Answer::Done,
            ) => return Ok(Progress::Complete),
            (
                Self::Opening {
                    mulligans,
                    decision,
                    ..
                },
                Answer::Keep,
            ) if decision.is_none() => {
                *decision = Some(OpeningDecision::Keep);
                if *mulligans == 0 {
                    return Ok(Progress::Complete);
                }
            }
            (Self::Opening { decision, .. }, Answer::Mulligan) if decision.is_none() => {
                *decision = Some(OpeningDecision::Mulligan);
                return Ok(Progress::Complete);
            }
            (
                Self::Opening {
                    hand,
                    mulligans,
                    decision: Some(OpeningDecision::Keep),
                    bottom,
                },
                Answer::Bottom { card },
            ) => {
                let id = object(hand, card)?;
                if let Some(index) = bottom.iter().position(|&selected| selected == id) {
                    bottom.remove(index);
                } else if bottom.len() < bottom_limit(hand, *mulligans) {
                    bottom.push(id);
                } else {
                    return Err(Refusal::INVALID);
                }
            }
            (
                Self::Opening {
                    hand,
                    mulligans,
                    decision: Some(OpeningDecision::Keep),
                    bottom,
                },
                Answer::Done,
            ) if bottom.is_empty() || bottom.len() == bottom_limit(hand, *mulligans) => {
                return Ok(Progress::Complete);
            }
            _ => return Err(Refusal::INVALID),
        }
        Ok(Progress::Editing)
    }
}

impl Sort {
    /// The original packet, in library order starting at the top.
    pub fn cards(&self) -> &[ObjectId] {
        &self.cards
    }

    fn unplace(&mut self, card: ObjectId) {
        for row in &mut self.placed {
            row.retain(|&id| id != card);
        }
    }

    /// The destination rows are in the order accepted by the table's flow batch.
    #[must_use]
    pub fn choice(&self) -> FlowChoice {
        let mut choice = FlowChoice {
            seat: self.seat,
            on_top: vec![],
            elsewhere: vec![],
        };
        for (pile, cards) in self.piles.iter().zip(&self.placed) {
            let destination = if *pile == PileName::Top {
                &mut choice.on_top
            } else {
                &mut choice.elsewhere
            };
            destination.clone_from(cards);
        }
        choice
    }
}

fn bottom_limit(hand: &[ObjectId], mulligans: u16) -> usize {
    hand.len().min(usize::from(mulligans))
}
