//! The host keeps each viewer's pending suggestions and line switches.
use beryl_session::protocol::{
    LabelView, ObjectId, PlayerView, ProposalLine, ProposalView, TableView, TargetRef,
};
use beryl_table::assist::{
    Assist, Proposal, apply_proposal, combat, enabled,
    nudges::{self, Label, ObjectOrSeat},
    upkeep,
};
use beryl_table::{Action, Refusal, Seat, Table, redact};

use crate::script::proposal_marks;
use crate::view::{Slots, Viewer};

pub(super) struct Suggestions {
    combat: Option<Proposal>,
    labels: Vec<Label>,
}

impl Suggestions {
    pub(super) fn new(table: &Table) -> Self {
        if !table.game_match.in_game {
            return Self {
                combat: None,
                labels: Vec::new(),
            };
        }
        Self {
            combat: combat::proposal(table),
            labels: nudges::labels(table),
        }
    }
}

#[derive(Clone)]
struct Pending {
    id: u32,
    revision: u32,
    proposal: Proposal,
    line_ids: Vec<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Turn {
    game: u32,
    number: u32,
    active: Seat,
}

#[derive(Clone, Default)]
pub(super) struct Assists {
    last_id: u32,
    turn: Option<Turn>,
    proposals: Vec<Pending>,
    labels: Vec<(u32, Label)>,
}

impl Assists {
    pub(super) fn refresh(&self, table: &Table, seat: Seat, suggestions: &Suggestions) -> Self {
        let turn = Turn {
            game: table.game_match.game_number,
            number: table.turn,
            active: table.active,
        };
        let mut next = Self {
            last_id: self.last_id,
            turn: Some(turn),
            ..Self::default()
        };
        let upkeep = upkeep::proposal(table, seat);
        let combat = suggestions
            .combat
            .as_ref()
            .filter(|_| enabled(table, seat, Assist::Combat));
        for proposal in upkeep.as_ref().into_iter().chain(combat) {
            let mut proposal = redact::proposal_seen_by(proposal, table, seat);
            proposal.lines.retain(|line| !line.text.is_empty());
            if proposal.lines.is_empty() {
                continue;
            }
            let old = self.proposals.iter().find(|pending| {
                self.turn == Some(turn) && pending.proposal.assist == proposal.assist
            });
            let id = old.map_or_else(|| next.issue_id(), |pending| pending.id);
            let mut line_ids = Vec::with_capacity(proposal.lines.len());
            for line in &mut proposal.lines {
                let previous =
                    old.and_then(|pending| {
                        pending.proposal.lines.iter().zip(&pending.line_ids).find(
                            |(previous, _)| {
                                previous.whose == line.whose && previous.actions == line.actions
                            },
                        )
                    });
                if let Some((previous, &id)) = previous {
                    line.on = previous.on;
                    line_ids.push(id);
                } else {
                    line_ids.push(next.issue_id());
                }
            }
            let revision = old
                .filter(|pending| pending.line_ids == line_ids)
                .map_or_else(|| next.issue_id(), |pending| pending.revision);
            next.proposals.push(Pending {
                id,
                revision,
                proposal,
                line_ids,
            });
        }
        for label in &suggestions.labels {
            if !label.show_to.contains(&seat) {
                continue;
            }
            let id = self
                .labels
                .iter()
                .find(|(_, old)| {
                    old.on == label.on && old.text == label.text && old.action == label.action
                })
                .map_or_else(|| next.issue_id(), |(id, _)| *id);
            next.labels.push((id, label.clone()));
        }
        next
    }

    fn issue_id(&mut self) -> u32 {
        self.last_id = self.last_id.checked_add(1).expect("assist IDs exhausted");
        self.last_id
    }

    pub(super) fn project(&self, view: &mut PlayerView, seat: Seat, slots: &Slots) {
        let marks = self.marks(view, slots);
        let manual = view.table.as_mut().expect("manual publication");
        manual.proposal_marks = (!marks.is_empty()).then_some(marks);
        let viewer = Viewer(seat);
        manual.proposals = self
            .proposals
            .iter()
            .map(|pending| ProposalView {
                id: pending.id,
                revision: pending.revision,
                title: match pending.proposal.assist {
                    Assist::TurnUpkeep => "Turn upkeep",
                    Assist::Combat => "Combat",
                    _ => unreachable!("only upkeep and combat offer proposals"),
                }
                .into(),
                lines: pending
                    .proposal
                    .lines
                    .iter()
                    .zip(&pending.line_ids)
                    .map(|(line, &id)| ProposalLine {
                        id,
                        text: line.text.clone(),
                        whose: viewer.side(line.whose),
                        on: line.on,
                    })
                    .collect(),
            })
            .collect();
        manual.labels = self
            .labels
            .iter()
            .filter_map(|(id, label)| {
                let on = match label.on {
                    ObjectOrSeat::Object(card) => {
                        let slot = slots.get(card)?;
                        view.objects
                            .iter()
                            .find(|object| object.slot == slot)?
                            .name
                            .as_ref()?;
                        TargetRef::Object { slot }
                    }
                    ObjectOrSeat::Seat(player) => TargetRef::Player {
                        player: viewer.side(player),
                    },
                };
                Some(LabelView {
                    id: *id,
                    on,
                    text: label.text.clone(),
                })
            })
            .collect();
    }

    fn marks(&self, view: &PlayerView, slots: &Slots) -> Vec<ObjectId> {
        proposal_marks(
            self.proposals.iter().map(|pending| &pending.proposal),
            view,
            slots,
        )
    }

    pub(super) fn toggle(&mut self, id: u32, line: u32, on: bool) -> Result<(), Refusal> {
        let index = self
            .proposals
            .iter()
            .position(|pending| pending.id == id)
            .ok_or(Refusal::Stale)?;
        let line = self.proposals[index]
            .line_ids
            .iter()
            .position(|&id| id == line)
            .ok_or(Refusal::Stale)?;
        if self.proposals[index].proposal.lines[line].on != on {
            let revision = self.issue_id();
            let pending = &mut self.proposals[index];
            pending.revision = revision;
            pending.proposal.lines[line].on = on;
        }
        Ok(())
    }

    pub(super) fn apply(&self, id: u32, revision: u32) -> Result<Action, Refusal> {
        let pending = self
            .proposals
            .iter()
            .find(|pending| pending.id == id && pending.revision == revision)
            .ok_or(Refusal::Stale)?;
        if !pending.proposal.lines.iter().any(|line| line.on) {
            return Err(Refusal::INVALID);
        }
        Ok(apply_proposal(&pending.proposal))
    }

    pub(super) fn label_action(&self, id: u32, view: &TableView) -> Result<Action, Refusal> {
        if !view.labels.iter().any(|label| label.id == id) {
            return Err(Refusal::Stale);
        }
        self.labels
            .iter()
            .find(|(label_id, _)| *label_id == id)
            .map(|(_, label)| label.action.clone())
            .ok_or(Refusal::Stale)
    }
}
