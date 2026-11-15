//! Projects the lobby for one authenticated seat and translates its commands.

use std::hash::{BuildHasher, RandomState};

use beryl_card_store::STORE;
pub use beryl_session::ipc::LobbyView;
use beryl_session::ipc::{LobbyChange, LobbySeat, LobbyUpdate};
use beryl_table::deck::{Deck, unknown};
use beryl_table::state::MatchSeat;
use beryl_table::{Action, Refusal, Seat, Table};

const HOST: Seat = Seat(0);

fn loaded(seat: &MatchSeat) -> bool {
    !seat.deck.is_empty() || !seat.sideboard.is_empty()
}

fn can_start(table: &Table, viewer: Seat) -> bool {
    viewer == HOST
        && !table.game_match.in_game
        && table
            .game_match
            .seats
            .iter()
            .enumerate()
            .all(|(index, seat)| loaded(seat) && (index == usize::from(HOST.0) || seat.ready))
}

pub(super) fn project(
    table: &Table,
    you: Seat,
    names: &[Option<String>],
    invite: Option<&str>,
) -> LobbyUpdate {
    LobbyUpdate {
        view: (!table.game_match.in_game).then(|| view(table, you, names, invite)),
    }
}

fn view(table: &Table, you: Seat, names: &[Option<String>], invite: Option<&str>) -> LobbyView {
    let count = |cards: &[(String, u16)]| {
        cards.iter().fold(0_u32, |sum, (_, copies)| {
            sum.saturating_add(u32::from(*copies))
        })
    };
    let seats = table
        .seats()
        .map(|seat| {
            let index = usize::from(seat.0);
            let current = &table.game_match.seats[index];
            LobbySeat {
                seat,
                name: names.get(index).cloned().flatten(),
                loaded: loaded(current),
                main_count: count(&current.deck),
                side_count: count(&current.sideboard),
                ready: current.ready,
                wins: current.wins,
            }
        })
        .collect();
    let index = usize::from(you.0);
    let own = &table.game_match.seats[index];
    let deck = Deck {
        name: String::new(),
        main: own.deck.clone(),
        side: own.sideboard.clone(),
        skipped: Vec::new(),
    };
    LobbyView {
        you,
        seats,
        unknown: unknown(&deck, &STORE),
        main: deck.main,
        side: deck.side,
        assists: table.assists[index].clone(),
        settings: table.game_match.settings(),
        game_number: table.game_match.game_number,
        invite: invite.filter(|_| you == HOST).map(str::to_owned),
        can_start: can_start(table, you),
    }
}

pub(super) fn action(
    table: &Table,
    seat: Seat,
    change: LobbyChange,
) -> Result<beryl_session::protocol::TableAction, Refusal> {
    if table.game_match.in_game {
        return Err(Refusal::INVALID);
    }
    Ok(match change {
        LobbyChange::LoadDeck { main, side } => Action::LoadDeck { seat, main, side },
        LobbyChange::Sideboard {
            into_main,
            into_side,
        } => Action::Sideboard {
            seat,
            into_main,
            into_side,
        },
        LobbyChange::Ready { ready } => Action::Ready { seat, ready },
        LobbyChange::SetAssist { assist, on } => Action::SetAssist { assist, on },
        LobbyChange::Settings { settings } if seat == HOST => Action::GameSettings { settings },
        LobbyChange::Start if can_start(table, seat) => {
            let first = table.game_match.first.unwrap_or_else(|| {
                // A solo table always has the first turn. A hosted table has two seats.
                if table.players.len() == 1 {
                    HOST
                } else {
                    Seat(u8::from(
                        !RandomState::new().hash_one(0_u8).is_multiple_of(2),
                    ))
                }
            });
            Action::Start { first }
        }
        LobbyChange::Start if seat == HOST => return Err(Refusal::INVALID),
        LobbyChange::Start | LobbyChange::Settings { .. } => return Err(Refusal::Permission),
        LobbyChange::View | LobbyChange::Solo => return Err(Refusal::INVALID),
    })
}
