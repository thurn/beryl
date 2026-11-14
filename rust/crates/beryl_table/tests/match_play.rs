use beryl_table::action::Adjustment;
use beryl_table::state::{Card, Phase, Zone};
use beryl_table::{Action, Chance, Change, Entry, Refusal, Seat, Table, apply};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);

fn deck(cards: &[(&str, u16)]) -> Vec<(String, u16)> {
    cards
        .iter()
        .map(|&(name, count)| (name.into(), count))
        .collect()
}

fn act(table: &mut Table, by: Seat, action: &Action) -> Entry {
    apply(table, by, action, &mut Chance::fresh(42)).unwrap()
}

fn load(table: &mut Table, seat: Seat) {
    act(
        table,
        seat,
        &Action::LoadDeck {
            seat,
            main: deck(&[("Island", 5), ("Mystery card", 5)]),
            side: deck(&[("Plains", 2)]),
        },
    );
}

fn outcomes(entry: &Entry) -> Vec<u32> {
    entry
        .changes
        .iter()
        .flat_map(|change| match change {
            Change::Random { outcomes } => outcomes.as_slice(),
            _ => &[],
        })
        .copied()
        .collect()
}

fn undo(table: &mut Table, entry: &Entry) {
    for change in entry.changes.iter().rev() {
        change.inverse().perform(table);
    }
}

#[test]
fn starting_builds_private_piles_shuffles_and_deals_for_both_seats() {
    let mut table = Table::new(2);
    let mut chance = Chance::fresh(42);
    let action = Action::Start { first: GUEST };
    load(&mut table, HOST);
    let before = table.clone();
    assert_eq!(
        apply(&mut table, HOST, &action, &mut chance),
        Err(Refusal::INVALID)
    );
    assert_eq!(table, before);
    assert_eq!(chance, Chance::fresh(42));

    load(&mut table, GUEST);
    act(
        &mut table,
        GUEST,
        &Action::Ready {
            seat: GUEST,
            ready: true,
        },
    );
    table.game_match.starting_life = 30;
    table.game_match.starting_hand_size = 2;
    let mut replay = table.clone();
    let entry = apply(&mut table, HOST, &action, &mut chance).unwrap();
    assert_eq!(outcomes(&entry).len(), 18);
    assert_eq!(
        apply(
            &mut replay,
            HOST,
            &action,
            &mut Chance::replaying(outcomes(&entry))
        ),
        Ok(entry.clone())
    );
    assert_eq!(table, replay);
    assert!(table.game_match.in_game);
    assert_eq!(
        (table.active, table.turn, table.phase),
        (GUEST, 1, Phase::Untap)
    );

    for seat in [HOST, GUEST] {
        assert_eq!(table.players[usize::from(seat.0)].life, 30);
        for (zone, count, known) in [
            (Zone::Hand(seat), 2, &[seat][..]),
            (Zone::Library(seat), 8, &[][..]),
            (Zone::Sideboard(seat), 2, &[seat][..]),
        ] {
            let ids = table.zone(zone).unwrap();
            assert_eq!(ids.len(), count);
            assert!(
                ids.iter().all(
                    |id| table.objects[id].known_to == known && table.objects[id].owner == seat
                )
            );
        }
    }
    assert_eq!(
        table
            .objects
            .values()
            .filter(|object| object.card == Card::Named("Mystery card".into()))
            .count(),
        10
    );
    undo(&mut table, &entry);
    assert!(table.objects.is_empty());
    assert!(!table.game_match.in_game);
    assert_eq!((table.active, table.turn), (HOST, 0));
    assert!(table.game_match.seats[1].ready);
}

#[test]
fn sideboarding_swaps_counts_and_rejects_an_overdraw_atomically() {
    let mut table = Table::new(1);
    load(&mut table, HOST);
    act(
        &mut table,
        HOST,
        &Action::Ready {
            seat: HOST,
            ready: true,
        },
    );
    let before = table.clone();
    let mut chance = Chance::fresh(42);
    let invalid = Action::Sideboard {
        seat: HOST,
        into_main: deck(&[("Plains", 1), ("Plains", 2)]),
        into_side: deck(&[("Island", 1)]),
    };
    assert_eq!(
        apply(&mut table, HOST, &invalid, &mut chance),
        Err(Refusal::INVALID)
    );
    assert_eq!(table, before);
    let swapped = act(
        &mut table,
        HOST,
        &Action::Sideboard {
            seat: HOST,
            into_main: deck(&[("Plains", 2)]),
            into_side: deck(&[("Island", 1)]),
        },
    );
    let seat = &table.game_match.seats[0];
    assert_eq!(
        seat.deck,
        deck(&[("Island", 4), ("Mystery card", 5), ("Plains", 2)])
    );
    assert_eq!(seat.sideboard, deck(&[("Island", 1)]));
    assert!(!seat.ready);
    undo(&mut table, &swapped);
    assert_eq!(table.game_match, before.game_match);
    act(
        &mut table,
        HOST,
        &Action::LoadDeck {
            seat: HOST,
            main: vec![],
            side: vec![],
        },
    );
    assert!(!table.game_match.seats[0].ready);
    assert!(table.game_match.seats[0].deck.is_empty());
}

#[test]
fn ending_a_game_updates_the_score_resets_play_and_keeps_decks() {
    let mut table = Table::new(2);
    for seat in [HOST, GUEST] {
        load(&mut table, seat);
    }
    act(
        &mut table,
        GUEST,
        &Action::Ready {
            seat: GUEST,
            ready: true,
        },
    );
    act(&mut table, HOST, &Action::Start { first: GUEST });
    act(
        &mut table,
        HOST,
        &Action::Life {
            seat: GUEST,
            adjustment: Adjustment::Set(4),
        },
    );
    let before = table.clone();
    let live = [
        Action::Sideboard {
            seat: HOST,
            into_main: deck(&[("Plains", 1)]),
            into_side: vec![],
        },
        Action::Start { first: HOST },
        Action::NextGame {
            winner: Some(Seat(2)),
        },
    ];
    for invalid in live {
        assert_eq!(
            apply(&mut table, HOST, &invalid, &mut Chance::fresh(42)),
            Err(Refusal::INVALID)
        );
        assert_eq!(table, before);
    }
    let ended = act(
        &mut table,
        HOST,
        &Action::NextGame {
            winner: Some(GUEST),
        },
    );
    assert!(!table.game_match.in_game);
    assert!(table.objects.is_empty());
    assert!(table.game_match.seats.iter().all(|seat| !seat.ready));
    assert_eq!(
        table.game_match.seats[0].deck,
        before.game_match.seats[0].deck
    );
    assert_eq!(
        table.game_match.seats[1].sideboard,
        before.game_match.seats[1].sideboard
    );
    assert_eq!(table.game_match.seats[1].wins, 1);
    assert_eq!(table.game_match.last_winner, Some(GUEST));
    assert_eq!(table.players[1].life, 20);
    let mut restored = table.clone();
    undo(&mut restored, &ended);
    assert_eq!(restored.objects, before.objects);
    assert_eq!(restored.players, before.players);
    assert_eq!(restored.zones, before.zones);
    assert_eq!(restored.game_match, before.game_match);
    assert_eq!((restored.active, restored.turn), (GUEST, 1));

    act(
        &mut table,
        GUEST,
        &Action::Ready {
            seat: GUEST,
            ready: true,
        },
    );
    act(&mut table, HOST, &Action::Start { first: HOST });
    assert!(
        table
            .objects
            .keys()
            .all(|id| !before.objects.contains_key(id))
    );
    act(&mut table, GUEST, &Action::Concede {});
    assert_eq!(table.game_match.last_winner, Some(HOST));
    assert_eq!(
        table
            .game_match
            .seats
            .iter()
            .map(|seat| seat.wins)
            .collect::<Vec<_>>(),
        [1, 1]
    );
    act(&mut table, HOST, &Action::Start { first: HOST });
    act(&mut table, HOST, &Action::NextGame { winner: None });
    assert_eq!(table.game_match.last_winner, None);
    assert!(table.game_match.seats.iter().all(|seat| seat.wins == 1));
}

#[test]
fn a_solo_table_can_start_and_concede_without_awarding_a_win() {
    let mut table = Table::new(1);
    load(&mut table, HOST);
    act(&mut table, HOST, &Action::Start { first: HOST });
    assert_eq!(table.zone(Zone::Hand(HOST)).unwrap().len(), 7);
    act(&mut table, HOST, &Action::Concede {});
    assert!(!table.game_match.in_game);
    assert_eq!(table.game_match.last_winner, None);
    assert_eq!(table.game_match.seats[0].wins, 0);
}
