use crate::action::Portion;
use crate::change::{Edit, ObjectEdit, TableEdit};
use crate::state::{Assist, Card, MatchSeat, Object, Phase, Zone};
use crate::{Action, Chance, Change, Refusal, Seat, Table};

use super::{Changes, edit, library, perform, players};

pub(super) fn set_assist(table: &Table, by: Seat, assist: Assist, on: bool) -> Vec<Change> {
    let before = &table.assists[usize::from(by.0)];
    if before.contains(&assist) == on {
        return Vec::new();
    }
    let mut after = before.clone();
    if on {
        after.push(assist);
    } else {
        after.retain(|&enabled| enabled != assist);
    }
    vec![Change::SetTable(TableEdit::SetAssists {
        seat: by,
        edit: Edit {
            before: before.clone(),
            after,
        },
    })]
}

type Deck = Vec<(String, u16)>;

pub(super) fn phase(table: &Table, phase: Phase) -> Vec<Change> {
    edit(table.phase, phase, |edit| {
        Change::SetTable(TableEdit::SetPhase { edit })
    })
    .into_iter()
    .collect()
}

fn set_turn(table: &Table, active: Seat, turn: u32, changes: &mut Vec<Change>) {
    changes.extend(edit(table.active, active, |edit| {
        Change::SetTable(TableEdit::SetActive { edit })
    }));
    changes.extend(edit(table.turn, turn, |edit| {
        Change::SetTable(TableEdit::SetTurn { edit })
    }));
    changes.push(Change::SetTable(TableEdit::ResetTurnPlays {
        edit: Edit {
            before: table.turn_plays,
            after: (turn != 0).then_some(0),
        },
    }));
    changes.extend(edit(table.phase, Phase::Untap, |edit| {
        Change::SetTable(TableEdit::SetPhase { edit })
    }));
}

pub(super) fn next_turn(table: &Table) -> Changes {
    players::player(table, table.active)?;
    let next = (usize::from(table.active.0) + 1) % table.players.len();
    let turn = table.turn.checked_add(1).ok_or(Refusal::INVALID)?;
    let mut changes = Vec::new();
    set_turn(table, Seat(next as u8), turn, &mut changes);
    changes.extend(players::remove_arrows(table, |_| true));
    changes.extend(table.objects.values().filter_map(|object| {
        edit(object.damage, 0, |edit| Change::SetObject {
            id: object.id,
            edit: ObjectEdit::Damage(edit),
        })
    }));
    Ok(changes)
}

fn lobby_seat(table: &Table, seat: Seat) -> Result<&MatchSeat, Refusal> {
    if table.game_match.in_game {
        return Err(Refusal::INVALID);
    }
    table
        .game_match
        .seats
        .get(usize::from(seat.0))
        .ok_or(Refusal::INVALID)
}

fn add_cards(deck: &mut Deck, incoming: &[(String, u16)]) -> Result<(), Refusal> {
    for (name, count) in incoming {
        if name.trim().is_empty() || *count == 0 {
            return Err(Refusal::INVALID);
        }
        if let Some((_, copies)) = deck.iter_mut().find(|(card, _)| card == name) {
            *copies = copies.checked_add(*count).ok_or(Refusal::INVALID)?;
        } else {
            deck.push((name.clone(), *count));
        }
    }
    Ok(())
}

fn take_cards(deck: &mut Deck, outgoing: &[(String, u16)]) -> Result<(), Refusal> {
    for (name, count) in outgoing {
        let (_, copies) = deck
            .iter_mut()
            .find(|(card, _)| card == name)
            .ok_or(Refusal::INVALID)?;
        *copies = copies.checked_sub(*count).ok_or(Refusal::INVALID)?;
    }
    Ok(())
}

fn deck_changes(seat: Seat, old: &MatchSeat, main: Deck, side: Deck) -> Vec<Change> {
    let mut changes = Vec::new();
    if main != old.deck || side != old.sideboard {
        changes.extend(edit(old.deck.clone(), main, |edit| {
            Change::SetTable(TableEdit::SetDeck { seat, edit })
        }));
        changes.extend(edit(old.sideboard.clone(), side, |edit| {
            Change::SetTable(TableEdit::SetSideboard { seat, edit })
        }));
        changes.extend(edit(old.ready, false, |edit| {
            Change::SetTable(TableEdit::SetReady { seat, edit })
        }));
    }
    changes
}

pub(super) fn load_deck(
    table: &Table,
    seat: Seat,
    main: &[(String, u16)],
    side: &[(String, u16)],
) -> Changes {
    let old = lobby_seat(table, seat)?;
    let mut deck = Vec::new();
    let mut sideboard = Vec::new();
    add_cards(&mut deck, main)?;
    add_cards(&mut sideboard, side)?;
    Ok(deck_changes(seat, old, deck, sideboard))
}

pub(super) fn sideboard(
    table: &Table,
    seat: Seat,
    into_main: &[(String, u16)],
    into_side: &[(String, u16)],
) -> Changes {
    let old = lobby_seat(table, seat)?;
    let mut main = old.deck.clone();
    let mut side = old.sideboard.clone();
    // Both transfers spend copies from their original lists.
    take_cards(&mut main, into_side)?;
    take_cards(&mut side, into_main)?;
    add_cards(&mut main, into_main)?;
    add_cards(&mut side, into_side)?;
    main.retain(|(_, copies)| *copies != 0);
    side.retain(|(_, copies)| *copies != 0);
    Ok(deck_changes(seat, old, main, side))
}

pub(super) fn ready(table: &Table, seat: Seat, ready: bool) -> Changes {
    let old = lobby_seat(table, seat)?;
    Ok(edit(old.ready, ready, |edit| {
        Change::SetTable(TableEdit::SetReady { seat, edit })
    })
    .into_iter()
    .collect())
}

fn clear_zone(table: &Table, zone: Zone, changes: &mut Vec<Change>) {
    for (index, id) in table
        .zone(zone)
        .expect("existing zone")
        .iter()
        .copied()
        .enumerate()
        .rev()
    {
        if let Some(marker) = table.markers.get(&id) {
            changes.push(Change::RemoveMarker {
                id,
                marker: marker.clone(),
                index,
            });
        } else {
            changes.push(Change::Remove {
                object: table.objects[&id].clone(),
                index,
            });
        }
    }
}

fn reset_play(table: &Table) -> Vec<Change> {
    let mut changes: Vec<_> = players::remove_arrows(table, |_| true).collect();
    clear_zone(table, Zone::Stack, &mut changes);
    clear_zone(table, Zone::Battlefield, &mut changes);
    for seat in table.seats() {
        for zone in [
            Zone::Library(seat),
            Zone::Hand(seat),
            Zone::Graveyard(seat),
            Zone::Exile(seat),
            Zone::Sideboard(seat),
        ] {
            clear_zone(table, zone, &mut changes);
        }
        let player = &table.players[usize::from(seat.0)];
        changes.extend(players::set_life(
            seat,
            player.life,
            table.game_match.starting_life,
        ));
        if !player.counters.is_empty() {
            changes.push(Change::SetTable(TableEdit::SetPlayerCounters {
                seat,
                edit: Edit {
                    before: player.counters.clone(),
                    after: Vec::new(),
                },
            }));
        }
    }
    changes.extend(
        table
            .lent_zones
            .iter()
            .enumerate()
            .rev()
            .map(|(index, &(zone, to))| Change::Lend {
                zone,
                to,
                index,
                on: false,
            }),
    );
    changes.extend(
        table
            .always_reveal_top
            .iter()
            .copied()
            .enumerate()
            .rev()
            .map(|(index, seat)| Change::AlwaysRevealTop {
                seat,
                index,
                on: false,
            }),
    );
    changes.extend(
        table
            .always_look_at_top
            .iter()
            .copied()
            .enumerate()
            .rev()
            .map(|(index, seat)| Change::AlwaysLookAtTop {
                seat,
                index,
                on: false,
            }),
    );
    changes
}

fn create_pile(
    table: &Table,
    zone: Zone,
    owner: Seat,
    cards: &[(String, u16)],
    offset: &mut usize,
    changes: &mut Vec<Change>,
) -> Result<(), Refusal> {
    let copies = cards
        .iter()
        .flat_map(|(name, count)| std::iter::repeat_n(name, usize::from(*count)));
    for (index, name) in copies.enumerate() {
        let id = table.fresh_id(*offset).ok_or(Refusal::INVALID)?;
        *offset += 1;
        let mut object = Object::new(id, Card::Named(name.clone()), owner, zone);
        if zone == Zone::Sideboard(owner) {
            object.known_to.push(owner);
        }
        changes.push(Change::Create { object, index });
    }
    Ok(())
}

pub(super) fn start(table: &Table, first: Seat, chance: &mut Chance, action: &Action) -> Changes {
    lobby_seat(table, first)?;
    if table
        .game_match
        .seats
        .iter()
        .any(|seat| seat.deck.is_empty() && seat.sideboard.is_empty())
    {
        return Err(Refusal::INVALID);
    }

    let mut changes = reset_play(table);
    set_turn(table, first, 1, &mut changes);
    changes.push(Change::SetTable(TableEdit::SetInGame {
        edit: Edit {
            before: false,
            after: true,
        },
    }));
    let mut offset = 0;
    for seat in table.seats() {
        let loaded = &table.game_match.seats[usize::from(seat.0)];
        create_pile(
            table,
            Zone::Library(seat),
            seat,
            &loaded.deck,
            &mut offset,
            &mut changes,
        )?;
        create_pile(
            table,
            Zone::Sideboard(seat),
            seat,
            &loaded.sideboard,
            &mut offset,
            &mut changes,
        )?;
    }

    let mut staged = table.clone();
    perform(&mut staged, &changes, action);
    for seat in table.seats() {
        let shuffled = library::shuffle(&staged, seat, Portion::All, chance)?;
        perform(&mut staged, &shuffled, action);
        changes.extend(shuffled);
        let dealt = library::draw(&staged, seat, table.game_match.starting_hand_size)?;
        perform(&mut staged, &dealt, action);
        changes.extend(dealt);
    }
    Ok(changes)
}

pub(super) fn next_game(table: &Table, winner: Option<Seat>) -> Changes {
    if !table.game_match.in_game {
        return Err(Refusal::INVALID);
    }
    if let Some(winner) = winner {
        players::player(table, winner)?;
    }
    let mut changes = reset_play(table);
    set_turn(table, Seat(0), 0, &mut changes);
    changes.push(Change::SetTable(TableEdit::SetInGame {
        edit: Edit {
            before: true,
            after: false,
        },
    }));
    changes.extend(edit(table.game_match.last_winner, winner, |edit| {
        Change::SetTable(TableEdit::SetLastWinner { edit })
    }));
    for seat in table.seats() {
        let old = &table.game_match.seats[usize::from(seat.0)];
        changes.extend(edit(old.ready, false, |edit| {
            Change::SetTable(TableEdit::SetReady { seat, edit })
        }));
        if winner == Some(seat) {
            changes.push(Change::SetTable(TableEdit::SetWins {
                seat,
                edit: Edit {
                    before: old.wins,
                    after: old.wins.checked_add(1).ok_or(Refusal::INVALID)?,
                },
            }));
        }
    }
    Ok(changes)
}

pub(super) fn concede(table: &Table, by: Seat) -> Changes {
    players::player(table, by)?;
    next_game(table, table.seats().find(|&seat| seat != by))
}
