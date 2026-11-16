//! The guided start is projected from the live log. Prompts hold only
//! uncommitted choices, such as which cards to put on the bottom.

use beryl_session::ipc::LobbyChange;
use beryl_table::action::{Destination, Position, Selection};
use beryl_table::state::{Phase, Zone};

use super::{
    Completion, OpenPrompt, Progress, PromptState, PublishedTable, Request, SeatPublication,
};
use crate::lobby;
use crate::prompt::OpeningDecision;
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
        let chooser = self.table.game_match.first.or_else(|| {
            let winner = self.table.game_match.last_winner?;
            self.table.seats().find(|&seat| seat != winner)
        });
        if let Some(chooser) = chooser {
            self.apply(
                seat,
                Request::Host(&Action::ChooseFirst { chooser }),
                |_| None,
                save,
            )?;
        } else {
            // Commit the chooser with the dice so reopening cannot roll again.
            let (chooser, mut actions) = roll_for_first(self.chance.clone())?;
            actions.push(Action::ChooseFirst { chooser });
            let roll = Action::Batch { actions };
            self.apply(seat, Request::Host(&roll), |_| None, save)?;
            for publication in &mut self.seats {
                // Start leaves the lobby route, so the new stage needs a
                // snapshot that can still play the roll announcements.
                let snapshot = crate::script::snapshot(&publication.view, publication.marks());
                if let Some(script) = &mut publication.view.script {
                    script.reset = snapshot.reset;
                    // Let each result finish before the next round or choice.
                    for timing in &mut script.timing {
                        timing.passes = false;
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn choosing_first(&self) -> bool {
        self.prompts
            .iter()
            .flatten()
            .any(|prompt| matches!(prompt.complete, Completion::FirstPlayer))
    }

    fn opening(&self) -> Opening {
        let mut opening = Opening::Inactive;
        if self.table.turn == 0 {
            for entry in self.log.live_entries() {
                opening.record(entry.by, &entry.action, self.seats.len());
            }
        }
        opening
    }

    pub(super) fn refresh_setup_prompts(&mut self) {
        let opening = self.opening();
        for (seat, current) in self.table.seats().zip(&mut self.prompts) {
            let next = opening.prompt(&self.table, seat);
            let keep_draft = match (current.as_ref(), next.as_ref()) {
                (
                    Some(OpenPrompt {
                        complete: Completion::FirstPlayer,
                        ..
                    }),
                    Some(OpenPrompt {
                        complete: Completion::FirstPlayer,
                        ..
                    }),
                )
                | (
                    Some(OpenPrompt {
                        complete: Completion::Action(_),
                        ..
                    }),
                    None,
                ) => true,
                (
                    Some(OpenPrompt {
                        state:
                            PromptState::Opening {
                                hand, mulligans, ..
                            },
                        complete: Completion::OpeningHand,
                    }),
                    Some(OpenPrompt {
                        state:
                            PromptState::Opening {
                                hand: next_hand,
                                mulligans: next_count,
                                ..
                            },
                        complete: Completion::OpeningHand,
                    }),
                ) => hand == next_hand && mulligans == next_count,
                _ => false,
            };
            if !keep_draft {
                *current = next;
            }
        }
        // Setup prompts have no lift. Attach them without replacing the roll,
        // deal, mulligan or rewind script in the current publication.
        for publication in &mut self.seats {
            let handling = publication.view.handling;
            SeatPublication::project_prompts(
                &mut publication.view,
                publication.seat,
                &publication.slots,
                &self.table,
                &self.prompts,
            );
            // An undo or rewind can restore an opening prompt while a player
            // is lifting a library pile. Snapshot the stage with the lift gone.
            if publication.view.handling != handling {
                publication.view.script = Some(crate::script::snapshot(
                    &publication.view,
                    publication.marks(),
                ));
            }
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
        self.apply_closing_prompt(chooser, &deal, save)
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
                ..
            } => {
                let count = self.table.game_match.starting_hand_size;
                let action = Action::Batch {
                    actions: vec![Action::Mulligan { seat, count }],
                };
                self.apply_closing_prompt(seat, &action, save)?;
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
                let Opening::Hands(hands) = self.opening() else {
                    return Err(Refusal::Stale);
                };
                if hands.iter().filter(|hand| hand.is_some()).count() == 1 {
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

enum Opening {
    Inactive,
    Choose(Seat),
    Hands(Vec<Option<u16>>),
}

impl Opening {
    fn record(&mut self, by: Seat, action: &Action, seats: usize) {
        match action {
            Action::ChooseFirst { chooser } => *self = Self::Choose(*chooser),
            Action::Start { opening: true, .. } => *self = Self::Hands(vec![Some(0); seats]),
            Action::Start { .. } | Action::NextGame { .. } | Action::Concede {} => {
                *self = Self::Inactive;
            }
            Action::Mulligan { seat, .. } => {
                if let Self::Hands(hands) = self
                    && let Some(count) = &mut hands[usize::from(seat.0)]
                {
                    *count = count.saturating_add(1);
                }
            }
            Action::Keep {} => {
                if let Self::Hands(hands) = self {
                    hands[usize::from(by.0)] = None;
                }
            }
            Action::Batch { actions } => {
                for action in actions {
                    self.record(by, action, seats);
                }
            }
            _ => {}
        }
    }

    fn prompt(&self, table: &beryl_table::Table, seat: Seat) -> Option<OpenPrompt> {
        match self {
            Self::Choose(chooser) if *chooser == seat && !table.game_match.in_game => {
                Some(OpenPrompt {
                    state: PromptState::Choose {
                        title: "Play or draw?".into(),
                        options: vec!["Play".into(), "Draw".into()],
                        selected: None,
                    },
                    complete: Completion::FirstPlayer,
                })
            }
            Self::Hands(hands) if table.game_match.in_game => Some(OpenPrompt {
                state: PromptState::Opening {
                    hand: table.zone(Zone::Hand(seat))?.to_vec(),
                    mulligans: hands[usize::from(seat.0)]?,
                    decision: None,
                    bottom: Vec::new(),
                },
                complete: Completion::OpeningHand,
            }),
            _ => None,
        }
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
