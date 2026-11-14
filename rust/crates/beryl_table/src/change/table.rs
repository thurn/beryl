use serde::{Deserialize, Serialize};

use crate::state::Phase;
use crate::{Seat, Table};

use super::Edit;

/// The previous and new value of one field outside an object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableEdit {
    SetLife {
        seat: Seat,
        edit: Edit<i32>,
    },
    SetPlayerCounters {
        seat: Seat,
        edit: Edit<Vec<(String, i32)>>,
    },
    SetActive {
        edit: Edit<Seat>,
    },
    SetTurn {
        edit: Edit<u32>,
    },
    SetPhase {
        edit: Edit<Phase>,
    },
    SetDeck {
        seat: Seat,
        edit: Edit<Vec<(String, u16)>>,
    },
    SetSideboard {
        seat: Seat,
        edit: Edit<Vec<(String, u16)>>,
    },
    SetReady {
        seat: Seat,
        edit: Edit<bool>,
    },
    SetWins {
        seat: Seat,
        edit: Edit<u32>,
    },
    SetInGame {
        edit: Edit<bool>,
    },
    SetLastWinner {
        edit: Edit<Option<Seat>>,
    },
}

impl TableEdit {
    pub(super) fn perform(&self, table: &mut Table) {
        match self {
            Self::SetLife { seat, edit } => {
                table.players[usize::from(seat.0)].life = edit.after;
            }
            Self::SetPlayerCounters { seat, edit } => {
                table.players[usize::from(seat.0)]
                    .counters
                    .clone_from(&edit.after);
            }
            Self::SetActive { edit } => {
                table.active = edit.after;
            }
            Self::SetTurn { edit } => {
                table.turn = edit.after;
            }
            Self::SetPhase { edit } => {
                table.phase = edit.after;
            }
            Self::SetDeck { seat, edit } => {
                table.game_match.seats[usize::from(seat.0)]
                    .deck
                    .clone_from(&edit.after);
            }
            Self::SetSideboard { seat, edit } => {
                table.game_match.seats[usize::from(seat.0)]
                    .sideboard
                    .clone_from(&edit.after);
            }
            Self::SetReady { seat, edit } => {
                table.game_match.seats[usize::from(seat.0)].ready = edit.after;
            }
            Self::SetWins { seat, edit } => {
                table.game_match.seats[usize::from(seat.0)].wins = edit.after;
            }
            Self::SetInGame { edit } => {
                table.game_match.in_game = edit.after;
            }
            Self::SetLastWinner { edit } => table.game_match.last_winner = edit.after,
        }
    }

    pub(super) fn inverse(&self) -> Self {
        match self {
            Self::SetLife { seat, edit } => Self::SetLife {
                seat: *seat,
                edit: edit.inverse(),
            },
            Self::SetPlayerCounters { seat, edit } => Self::SetPlayerCounters {
                seat: *seat,
                edit: edit.inverse(),
            },
            Self::SetActive { edit } => Self::SetActive {
                edit: edit.inverse(),
            },
            Self::SetTurn { edit } => Self::SetTurn {
                edit: edit.inverse(),
            },
            Self::SetPhase { edit } => Self::SetPhase {
                edit: edit.inverse(),
            },
            Self::SetDeck { seat, edit } => Self::SetDeck {
                seat: *seat,
                edit: edit.inverse(),
            },
            Self::SetSideboard { seat, edit } => Self::SetSideboard {
                seat: *seat,
                edit: edit.inverse(),
            },
            Self::SetReady { seat, edit } => Self::SetReady {
                seat: *seat,
                edit: edit.inverse(),
            },
            Self::SetWins { seat, edit } => Self::SetWins {
                seat: *seat,
                edit: edit.inverse(),
            },
            Self::SetInGame { edit } => Self::SetInGame {
                edit: edit.inverse(),
            },
            Self::SetLastWinner { edit } => Self::SetLastWinner {
                edit: edit.inverse(),
            },
        }
    }
}
