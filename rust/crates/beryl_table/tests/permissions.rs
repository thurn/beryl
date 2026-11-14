use beryl_table::action::{
    Adjustment, Destination, Portion, Position, SeenObject, Selection, Target,
};
use beryl_table::permissions::check;
use beryl_table::state::{Card, Face, Object, ObjectId, Phase, Zone};
use beryl_table::{Action, Chance, Change, Refusal, Seat, Table, apply};

const OWNER: Seat = Seat(0);
const GUEST: Seat = Seat(1);
const MISSING: Seat = Seat(2);
const HAND: Zone = Zone::Hand(OWNER);
const LIBRARY: Zone = Zone::Library(OWNER);
const GRAVEYARD: Zone = Zone::Graveyard(OWNER);

fn put(table: &mut Table, id: u32, zone: Zone) {
    let object = Object::new(ObjectId(id), Card::Named("Island".into()), OWNER, zone);
    let index = table.zone(zone).unwrap().len();
    Change::Create { object, index }.perform(table);
}

fn selection(id: u32, zone: Zone) -> Selection {
    Selection {
        objects: vec![ObjectId(id)],
        zone,
    }
}

fn move_card(id: u32, from: Zone, to: Zone) -> Action {
    Action::Move {
        selection: selection(id, from),
        to: Destination::Zone(to),
        position: Position::Top,
        face: None,
        tapped: None,
    }
}

fn act(table: &mut Table, by: Seat, action: &Action) {
    apply(table, by, action, &mut Chance::fresh(42)).unwrap();
}

fn refused(table: &mut Table, by: Seat, action: &Action, expected: Refusal) {
    let before = table.clone();
    assert_eq!(
        apply(table, by, action, &mut Chance::fresh(42)),
        Err(expected)
    );
    assert_eq!(*table, before);
}

#[test]
fn public_cards_in_all_four_zones_are_open_to_either_player() {
    for zone in [
        Zone::Battlefield,
        Zone::Stack,
        GRAVEYARD,
        Zone::Exile(OWNER),
    ] {
        let mut table = Table::new(2);
        put(&mut table, 1, zone);
        act(&mut table, GUEST, &move_card(1, zone, Zone::Hand(OWNER)));
        assert_eq!(table.zone(HAND).unwrap().len(), 1);
        let id = table.zone(HAND).unwrap()[0].0;
        refused(
            &mut table,
            GUEST,
            &move_card(id, HAND, zone),
            Refusal::Permission,
        );
    }
}

#[test]
fn returning_a_card_to_its_owner_does_not_grant_other_hidden_zone_access() {
    let mut table = Table::new(2);
    put(&mut table, 1, Zone::Battlefield);
    put(&mut table, 2, Zone::Battlefield);
    table.objects.get_mut(&ObjectId(2)).unwrap().owner = GUEST;

    let unrelated = move_card(2, Zone::Battlefield, LIBRARY);
    act(&mut table, GUEST, &move_card(1, Zone::Battlefield, LIBRARY));
    refused(&mut table, GUEST, &unrelated, Refusal::Permission);
    act(
        &mut table,
        OWNER,
        &Action::Lend {
            zone: LIBRARY,
            to: GUEST,
        },
    );
    act(&mut table, GUEST, &unrelated);
    assert_eq!(table.zone(LIBRARY).unwrap(), [ObjectId(2), ObjectId(1)]);
}

#[test]
fn life_and_player_counters_are_open_to_either_player() {
    let mut table = Table::new(2);
    for action in [
        Action::Life {
            seat: OWNER,
            adjustment: Adjustment::Add(-2),
        },
        Action::PlayerCounter {
            seat: OWNER,
            name: "poison".into(),
            adjustment: Adjustment::Add(1),
        },
    ] {
        refused(&mut table, MISSING, &action, Refusal::Invalid);
        act(&mut table, GUEST, &action);
    }
    assert_eq!(table.players[0].life, 18);
    assert_eq!(table.players[0].counters, [("poison".into(), 1)]);
}

#[test]
fn arrows_phase_and_turn_are_open_to_either_player() {
    let mut table = Table::new(2);
    put(&mut table, 1, Zone::Battlefield);
    put(&mut table, 2, HAND);
    let source = selection(1, Zone::Battlefield);
    let arrow = |to| Action::Arrow {
        selection: source.clone(),
        to,
    };
    refused(
        &mut table,
        GUEST,
        &arrow(Target::Object(SeenObject {
            id: ObjectId(2),
            zone: HAND,
        })),
        Refusal::Permission,
    );
    for action in [
        arrow(Target::Player(OWNER)),
        Action::Phase { phase: Phase::End },
        Action::NextTurn {},
    ] {
        refused(&mut table, MISSING, &action, Refusal::Invalid);
        act(&mut table, GUEST, &action);
    }
    assert_eq!(table.turn, 1);
}

#[test]
fn only_the_owner_or_borrower_may_use_hidden_zones_as_a_whole() {
    for zone in [HAND, LIBRARY, Zone::Sideboard(OWNER)] {
        let mut table = Table::new(2);
        put(&mut table, 1, zone);
        let reveal = Action::Reveal {
            zone,
            objects: None,
            random: None,
            to: vec![GUEST],
        };
        refused(&mut table, GUEST, &reveal, Refusal::Permission);
        act(&mut table, OWNER, &reveal);
        // Knowing the cards does not grant access to a whole hidden zone.
        refused(&mut table, GUEST, &reveal, Refusal::Permission);

        let lend = Action::Lend { zone, to: GUEST };
        act(&mut table, OWNER, &lend);
        act(&mut table, GUEST, &reveal);
        act(&mut table, GUEST, &move_card(1, zone, GRAVEYARD));
    }
}

#[test]
fn revealing_a_hand_card_lets_the_other_player_discard_it() {
    let mut table = Table::new(2);
    put(&mut table, 12, HAND);
    put(&mut table, 15, HAND);
    let discard =
        serde_json::from_str(r#"{"move":{"objects":[12],"in":{"hand":0},"to":{"graveyard":0}}}"#)
            .unwrap();
    refused(&mut table, GUEST, &discard, Refusal::Permission);

    act(
        &mut table,
        OWNER,
        &Action::Reveal {
            zone: HAND,
            objects: Some(vec![ObjectId(12)]),
            random: None,
            to: vec![GUEST],
        },
    );
    refused(
        &mut table,
        GUEST,
        &move_card(15, HAND, GRAVEYARD),
        Refusal::Permission,
    );
    act(&mut table, GUEST, &discard);
    assert_eq!(table.zone(GRAVEYARD).unwrap(), [ObjectId(12)]);
    assert_eq!(table.zone(HAND).unwrap(), [ObjectId(15)]);
}

#[test]
fn knowing_a_library_card_does_not_grant_access_and_shuffling_ends_a_loan() {
    let mut table = Table::new(2);
    put(&mut table, 1, LIBRARY);
    let look = Action::Look {
        seat: OWNER,
        portion: Portion::All,
    };
    let lend = Action::Lend {
        zone: LIBRARY,
        to: GUEST,
    };
    refused(&mut table, GUEST, &look, Refusal::Permission);
    // A third seat cannot lend a library it does not own.
    let mut three_seats = Table::new(3);
    refused(&mut three_seats, MISSING, &lend, Refusal::Permission);
    act(
        &mut table,
        OWNER,
        &Action::Reveal {
            zone: LIBRARY,
            objects: Some(vec![ObjectId(1)]),
            random: None,
            to: vec![GUEST],
        },
    );
    refused(
        &mut table,
        GUEST,
        &move_card(1, LIBRARY, GRAVEYARD),
        Refusal::Permission,
    );

    act(&mut table, OWNER, &lend);
    act(&mut table, GUEST, &look);
    act(
        &mut table,
        GUEST,
        &Action::Shuffle {
            seat: OWNER,
            portion: Portion::All,
        },
    );
    refused(&mut table, GUEST, &look, Refusal::Permission);
}

#[test]
fn a_face_down_card_can_be_peeked_at_by_its_controller() {
    let mut table = Table::new(2);
    put(&mut table, 1, Zone::Battlefield);
    let card = table.objects.get_mut(&ObjectId(1)).unwrap();
    card.controller = GUEST;
    card.face = Face::Down;

    let peek = Action::Peek {
        selection: selection(1, Zone::Battlefield),
    };
    refused(&mut table, OWNER, &peek, Refusal::Permission);
    act(&mut table, GUEST, &peek);
    assert_eq!(table.objects[&ObjectId(1)].known_to, [GUEST]);
}

#[test]
fn decks_readiness_and_concession_belong_to_their_seat() {
    let mut table = Table::new(2);
    for action in [
        Action::LoadDeck {
            seat: OWNER,
            main: vec![("Island".into(), 7)],
            side: vec![],
        },
        Action::Sideboard {
            seat: OWNER,
            into_main: vec![],
            into_side: vec![],
        },
        Action::Ready {
            seat: OWNER,
            ready: true,
        },
    ] {
        refused(&mut table, GUEST, &action, Refusal::Permission);
        act(&mut table, OWNER, &action);
    }

    let concede = Action::Concede {};
    refused(&mut table, MISSING, &concede, Refusal::Invalid);
    act(
        &mut table,
        GUEST,
        &Action::LoadDeck {
            seat: GUEST,
            main: vec![("Plains".into(), 7)],
            side: vec![],
        },
    );
    act(
        &mut table,
        GUEST,
        &Action::Ready {
            seat: GUEST,
            ready: true,
        },
    );
    act(&mut table, OWNER, &Action::Start { first: OWNER });
    act(&mut table, GUEST, &concede);
    assert_eq!(table.game_match.last_winner, Some(OWNER));
}

#[test]
fn either_seat_may_request_a_rewind_or_undo() {
    let table = Table::new(2);
    // Action execution needs a log; the table checker only validates the actor.
    for action in [Action::Rewind { to: 1 }, Action::Undo {}] {
        assert_eq!(check(&table, OWNER, &action), Ok(()));
        assert_eq!(check(&table, GUEST, &action), Ok(()));
        assert_eq!(check(&table, MISSING, &action), Err(Refusal::Invalid));
    }
}

#[test]
fn all_batch_members_must_pass_the_check_before_any_change() {
    let mut table = Table::new(2);
    put(&mut table, 1, HAND);
    let life = Action::Life {
        seat: OWNER,
        adjustment: Adjustment::Add(-2),
    };
    let batch = Action::Batch {
        actions: vec![life.clone(), Action::NextTurn {}],
    };
    assert_eq!(check(&table, GUEST, &batch), Ok(()));

    let batch = Action::Batch {
        actions: vec![life, move_card(1, HAND, GRAVEYARD)],
    };
    refused(&mut table, GUEST, &batch, Refusal::Permission);
}

#[test]
fn staleness_precedes_permissions_for_primary_and_secondary_targets() {
    let mut table = Table::new(2);
    put(&mut table, 1, HAND);
    put(&mut table, 2, Zone::Battlefield);
    act(
        &mut table,
        OWNER,
        &move_card(2, Zone::Battlefield, GRAVEYARD),
    );

    let stale = move_card(2, Zone::Battlefield, Zone::Stack);
    refused(&mut table, GUEST, &stale, Refusal::Stale);
    let arrow = Action::Arrow {
        selection: selection(1, HAND),
        to: Target::Object(SeenObject {
            id: ObjectId(2),
            zone: Zone::Battlefield,
        }),
    };
    refused(&mut table, GUEST, &arrow, Refusal::Stale);
}

#[test]
fn nonexistent_references_are_invalid_before_permissions() {
    let mut table = Table::new(2);
    put(&mut table, 1, HAND);
    for action in [
        move_card(2, HAND, GRAVEYARD),
        move_card(1, HAND, Zone::Graveyard(MISSING)),
        Action::Reveal {
            zone: HAND,
            objects: Some(vec![ObjectId(1)]),
            random: None,
            to: vec![MISSING],
        },
        Action::Arrow {
            selection: selection(1, HAND),
            to: Target::Player(MISSING),
        },
    ] {
        refused(&mut table, GUEST, &action, Refusal::Invalid);
    }
    // None of the refusals consumed an entry number.
    let entry = apply(
        &mut table,
        OWNER,
        &move_card(1, HAND, GRAVEYARD),
        &mut Chance::fresh(42),
    )
    .unwrap();
    assert_eq!(entry.n, 1);
}
