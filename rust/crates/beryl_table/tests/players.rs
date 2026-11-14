use beryl_table::action::{Adjustment, SeenObject, Selection, Target};
use beryl_table::helpers::untap_all;
use beryl_table::state::{ArrowKind, Card, Face, Object, ObjectId, Phase, Zone};
use beryl_table::{Action, Chance, Change, Refusal, Seat, Table, apply};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);

fn act(table: &mut Table, by: Seat, action: &Action) {
    apply(table, by, action, &mut Chance::fresh(42)).unwrap();
}

fn permanent(table: &mut Table, id: u32, name: &str, seat: Seat) -> SeenObject {
    let id = ObjectId(id);
    let zone = Zone::Battlefield;
    Change::Create {
        object: Object::new(id, Card::Named(name.into()), seat, zone),
        index: table.battlefield.len(),
    }
    .perform(table);
    SeenObject { id, zone }
}

fn draw_arrow(table: &mut Table, by: Seat, from: SeenObject, to: Target) {
    act(
        table,
        by,
        &Action::Arrow {
            selection: Selection {
                objects: vec![from.id],
                zone: from.zone,
            },
            to,
        },
    );
}

#[test]
fn players_can_add_and_set_life_and_named_counters() {
    let mut table = Table::new(2);
    for (adjustment, life) in [
        (Adjustment::Add(-4), 16),
        (Adjustment::Set(-2), -2),
        (Adjustment::Add(4), 2),
    ] {
        act(
            &mut table,
            HOST,
            &Action::Life {
                seat: GUEST,
                adjustment,
            },
        );
        assert_eq!(table.players[1].life, life);
    }
    for (adjustment, count) in [
        (Adjustment::Set(3), 3),
        (Adjustment::Add(2), 5),
        (Adjustment::Add(-9), 0),
    ] {
        act(
            &mut table,
            GUEST,
            &Action::PlayerCounter {
                seat: HOST,
                name: "poison".into(),
                adjustment,
            },
        );
        if count == 0 {
            assert!(table.players[0].counters.is_empty());
        } else {
            assert_eq!(table.players[0].counters, [("poison".into(), count)]);
        }
    }
    assert_eq!(table.players[0].life, 20);
    assert!(table.players[1].counters.is_empty());
}

#[test]
fn dice_and_coins_record_replayable_results() {
    let mut table = Table::new(1);
    let mut chance = Chance::fresh(5);
    for (count, sides) in [(1, 2), (5, 6), (100, 1_000_000)] {
        let action = Action::Roll { count, sides };
        let start = table.clone();
        let entry = apply(&mut table, HOST, &action, &mut chance).unwrap();
        let [Change::Random { outcomes }] = entry.changes.as_slice() else {
            panic!("roll should only record outcomes");
        };
        assert_eq!(outcomes.len(), usize::from(count));
        assert!(outcomes.iter().all(|&outcome| outcome < sides));
        assert_eq!(
            apply(
                &mut start.clone(),
                HOST,
                &action,
                &mut Chance::replaying(outcomes.clone())
            ),
            Ok(entry)
        );
    }
    let before = table.clone();
    let previous_chance = chance.clone();
    for (count, sides) in [(0, 6), (101, 6), (1, 1), (1, 1_000_001)] {
        assert_eq!(
            apply(
                &mut table,
                HOST,
                &Action::Roll { count, sides },
                &mut chance
            ),
            Err(Refusal::Invalid)
        );
        assert_eq!(table, before);
        assert_eq!(chance, previous_chance);
    }
}

#[test]
fn arrows_keep_their_kind_and_each_seat_clears_its_own() {
    let mut table = Table::new(2);
    let attacker = permanent(&mut table, 1, "Grizzly Bears", HOST);
    let blocker = permanent(&mut table, 2, "Llanowar Elves", GUEST);
    let walker = permanent(&mut table, 3, "Jace, the Mind Sculptor", GUEST);
    act(
        &mut table,
        HOST,
        &Action::Phase {
            phase: Phase::BeginningOfCombat,
        },
    );
    draw_arrow(&mut table, HOST, attacker, Target::Player(GUEST));
    draw_arrow(&mut table, HOST, attacker, Target::Object(walker));
    act(
        &mut table,
        GUEST,
        &Action::Phase {
            phase: Phase::DeclareBlockers,
        },
    );
    draw_arrow(&mut table, GUEST, blocker, Target::Object(attacker));
    draw_arrow(&mut table, GUEST, blocker, Target::Player(GUEST));
    act(
        &mut table,
        HOST,
        &Action::Phase {
            phase: Phase::EndOfCombat,
        },
    );
    draw_arrow(&mut table, HOST, attacker, Target::Player(GUEST));
    assert_eq!(
        table
            .arrows
            .iter()
            .map(|arrow| arrow.kind)
            .collect::<Vec<_>>(),
        [
            ArrowKind::Attack,
            ArrowKind::Attack,
            ArrowKind::Block,
            ArrowKind::FriendlyTarget,
            ArrowKind::HostileTarget,
        ]
    );
    act(
        &mut table,
        HOST,
        &Action::ClearArrows {
            from: Some(attacker),
            to: Some(Target::Object(walker)),
        },
    );
    assert_eq!(table.arrows.len(), 4);
    assert_eq!(table.arrows[1].kind, ArrowKind::Block);
    act(
        &mut table,
        HOST,
        &Action::ClearArrows {
            from: None,
            to: None,
        },
    );
    assert_eq!(table.arrows.len(), 2);
    assert!(table.arrows.iter().all(|arrow| arrow.by == GUEST));
}

#[test]
fn an_arrow_does_not_reveal_the_printed_type_of_a_face_down_card() {
    let mut table = Table::new(2);
    let attacker = permanent(&mut table, 1, "Grizzly Bears", HOST);
    table.objects.get_mut(&attacker.id).unwrap().face = Face::Down;
    act(
        &mut table,
        HOST,
        &Action::Phase {
            phase: Phase::DeclareAttackers,
        },
    );
    draw_arrow(&mut table, HOST, attacker, Target::Player(GUEST));
    assert_eq!(table.arrows[0].kind, ArrowKind::HostileTarget);
}

#[test]
fn passing_the_turn_clears_arrows_and_damage_for_every_seat() {
    for (seat_count, next) in [(1, HOST), (2, GUEST)] {
        let mut table = Table::new(seat_count);
        let source = permanent(&mut table, 1, "Grizzly Bears", HOST);
        draw_arrow(&mut table, HOST, source, Target::Player(HOST));
        table.phase = Phase::End;
        table.turn = 1;
        table.objects.get_mut(&source.id).unwrap().damage = 3;
        act(&mut table, HOST, &Action::NextTurn {});
        assert_eq!(
            (table.active, table.turn, table.phase),
            (next, 2, Phase::Untap)
        );
        assert_eq!(table.objects[&source.id].damage, 0);
        assert!(table.arrows.is_empty());
    }
}

#[test]
fn untap_all_affects_only_unflagged_cards_controlled_by_the_seat() {
    let mut table = Table::new(2);
    let own = permanent(&mut table, 1, "Island", HOST);
    let flagged = permanent(&mut table, 2, "Forest", HOST);
    let other = permanent(&mut table, 3, "Mountain", HOST);
    for object in table.objects.values_mut() {
        object.tapped = true;
    }
    table.objects.get_mut(&flagged.id).unwrap().doesnt_untap = true;
    table.objects.get_mut(&other.id).unwrap().controller = GUEST;
    let action = untap_all(&table, HOST);
    act(&mut table, HOST, &action);
    assert!(!table.objects[&own.id].tapped);
    assert!(table.objects[&flagged.id].tapped);
    assert!(table.objects[&other.id].tapped);
}
