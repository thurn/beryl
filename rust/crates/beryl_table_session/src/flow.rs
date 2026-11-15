//! The optional guided start. The log owns the deal, rolls, hand changes and
//! first turn. The prompts own only decisions that have not been applied yet.

use beryl_session::ipc::LobbyChange;
use beryl_table::action::{Destination, Position, Selection};
use beryl_table::state::{Phase, Zone};

use super::{
    Completion, OpenPrompt, Progress, PromptState, PublishedTable, Request, SeatPublication,
};
use crate::lobby;
use crate::prompt::OpeningDecision;
use crate::script::ProposalMarks;
use beryl_table::{Action, Chance, Entry, Refusal, Seat};

const HOST: Seat = Seat(0);
const DIE: u32 = 6;

impl PublishedTable {
    pub(crate) fn apply_lobby_checked(
        &mut self,
        seat: Seat,
        change: LobbyChange,
        save: impl FnOnce(&Entry) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        if self.choosing_first() {
            return Err(Refusal::INVALID);
        }
        if !matches!(change, LobbyChange::Start) {
            let action = lobby::action(&self.table, seat, change)?;
            self.apply_client_checked(seat, action, save)?;
            return Ok(());
        }
        if !lobby::can_start(&self.table, seat) || self.prompts.iter().any(Option::is_some) {
            return Err(if seat == HOST {
                Refusal::INVALID
            } else {
                Refusal::Permission
            });
        }
        if self.table.players.len() == 1 {
            self.deal_opening(seat, seat, save)?;
            return Ok(());
        }
        let mut chooser = self.table.game_match.first.or_else(|| {
            let winner = self.table.game_match.last_winner?;
            self.table.seats().find(|&seat| seat != winner)
        });
        if chooser.is_none() {
            // Roll both seats together so the banner shows the host die and
            // guest die side by side. Each tied round has its own notice.
            let (winner, actions) = roll_for_first(self.chance.clone())?;
            let roll = Action::Batch { actions };
            self.apply(
                seat,
                Request::Host(&roll),
                |_| ProposalMarks::default(),
                save,
            )?;
            for publication in &mut self.seats {
                // Start leaves the lobby route, so the new stage needs a
                // snapshot that can still play the roll announcements.
                let snapshot = crate::script::snapshot(&publication.view);
                if let Some(script) = &mut publication.view.script {
                    script.reset = snapshot.reset;
                    // Let each result finish before the next round or choice.
                    for timing in &mut script.timing {
                        timing.passes = false;
                    }
                }
            }
            chooser = Some(winner);
        }
        let chooser = chooser.expect("a two-seat start has a chooser");
        self.set_setup_prompt(
            chooser,
            PromptState::Choose {
                title: "Play or draw?".into(),
                options: vec!["Play".into(), "Draw".into()],
                selected: None,
            },
            Completion::FirstPlayer,
        );
        Ok(())
    }

    pub(crate) fn choosing_first(&self) -> bool {
        self.prompts
            .iter()
            .flatten()
            .any(|prompt| matches!(prompt.complete, Completion::FirstPlayer))
    }

    fn set_setup_prompt(&mut self, seat: Seat, state: PromptState, complete: Completion) {
        let index = usize::from(seat.0);
        self.prompts[index] = Some(OpenPrompt { state, complete });
        // Setup prompts have no lift. Attach the prompt to the current view
        // without losing the roll, deal or mulligan script that precedes it.
        for publication in &mut self.seats {
            SeatPublication::project_prompts(
                &mut publication.view,
                publication.seat,
                &publication.slots,
                &self.table,
                &self.prompts,
            );
        }
    }

    fn deal_opening(
        &mut self,
        chooser: Seat,
        first: Seat,
        save: impl FnOnce(&Entry) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        let deal = Action::Batch {
            actions: vec![Action::Start {
                first,
                opening: true,
            }],
        };
        self.apply_closing_prompt(chooser, &deal, save)?;
        for index in 0..self.seats.len() {
            self.open_hand(self.seats[index].seat, 0);
        }
        Ok(())
    }

    fn open_hand(&mut self, seat: Seat, mulligans: u16) {
        let hand = self.table.zone(Zone::Hand(seat)).expect("existing seat");
        self.set_setup_prompt(
            seat,
            PromptState::Opening {
                hand: hand.to_vec(),
                mulligans,
                decision: None,
                bottom: Vec::new(),
            },
            Completion::OpeningHand,
        );
    }

    pub(super) fn finish_setup_prompt(
        &mut self,
        seat: Seat,
        state: PromptState,
        progress: Progress,
        save: impl FnOnce(&Entry) -> Result<(), Refusal>,
    ) -> Result<bool, Refusal> {
        match state {
            PromptState::Choose { selected, .. } => {
                let first = if progress == Progress::Complete && selected == Some(1) {
                    self.table
                        .seats()
                        .find(|&other| other != seat)
                        .expect("two seats")
                } else {
                    seat
                };
                self.deal_opening(seat, first, save)?;
            }
            PromptState::Opening {
                decision: Some(OpeningDecision::Mulligan),
                mulligans,
                ..
            } => {
                let count = self.table.game_match.starting_hand_size;
                let action = Action::Batch {
                    actions: vec![Action::Mulligan { seat, count }],
                };
                self.apply_closing_prompt(seat, &action, save)?;
                self.open_hand(seat, mulligans.saturating_add(1));
            }
            PromptState::Opening { bottom, .. } => {
                let mut actions = vec![Action::Keep {}];
                if progress == Progress::Complete && !bottom.is_empty() {
                    actions.push(Action::Move {
                        selection: Selection {
                            objects: bottom,
                            zone: Zone::Hand(seat),
                        },
                        to: Destination::Zone(Zone::Library(seat)),
                        position: Position::Bottom,
                        face: None,
                        tapped: None,
                    });
                }
                let other_open = self.table.seats().any(|other| {
                    other != seat
                        && self.prompts[usize::from(other.0)]
                            .as_ref()
                            .is_some_and(|prompt| {
                                matches!(prompt.complete, Completion::OpeningHand)
                            })
                });
                if !other_open {
                    actions.push(Action::Phase {
                        phase: Phase::Untap,
                    });
                }
                self.apply_closing_prompt(seat, &Action::Batch { actions }, save)?;
            }
            PromptState::Sort(_) => unreachable!("a sort does not start a game"),
        }
        Ok(true)
    }
}

fn roll_for_first(mut chance: Chance) -> Result<(Seat, Vec<Action>), Refusal> {
    let mut actions = Vec::new();
    loop {
        let host = chance.below(DIE)?;
        let guest = chance.below(DIE)?;
        actions.push(Action::Roll {
            count: 2,
            sides: DIE,
        });
        if host != guest {
            return Ok((Seat(u8::from(host < guest)), actions));
        }
    }
}

#[cfg(test)]
mod tests {
    use beryl_session::script::{Act, Notice};
    use beryl_table::{Log, Table};

    use super::*;

    #[test]
    fn a_tie_records_two_roll_rounds_before_asking_the_winner() {
        let table = Table::new(2);
        let log = Log::new(&table);
        let chance = Chance::replaying(vec![2, 2, 1, 4]);
        let mut table = PublishedTable::new(table, log, chance).unwrap();
        for seat in [Seat(0), Seat(1)] {
            table
                .apply_host(
                    seat,
                    &Action::LoadDeck {
                        seat,
                        main: vec![("Island".into(), 12)],
                        side: vec![],
                    },
                )
                .unwrap();
        }
        table
            .apply_host(
                Seat(1),
                &Action::Ready {
                    seat: Seat(1),
                    ready: true,
                },
            )
            .unwrap();
        table
            .apply_lobby_checked(HOST, LobbyChange::Start, |_| Ok(()))
            .unwrap();

        for seat in [Seat(0), Seat(1)] {
            let view = table.publication(seat).unwrap().view();
            let rolls: Vec<_> = view
                .script
                .as_ref()
                .unwrap()
                .beats
                .iter()
                .flatten()
                .filter_map(|step| match &step.act {
                    Act::Announce {
                        notice:
                            Notice::Roll {
                                sides: DIE,
                                outcomes,
                            },
                    } => Some(outcomes.as_slice()),
                    _ => None,
                })
                .collect();
            assert_eq!(rolls, [&[2, 2], &[1, 4]]);
            assert_eq!(
                view.table.as_ref().unwrap().prompt.is_some(),
                seat == Seat(1)
            );
            assert_eq!(view.turn.number, 0);
        }
        assert_eq!(table.log().entries().last().unwrap().changes.len(), 2);
    }
}
