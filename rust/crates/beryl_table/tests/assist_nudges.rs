use beryl_table::assist::Assist;
use beryl_table::assist::nudges::{ObjectOrSeat, labels};
use beryl_table::state::{Card, Face, Object, ObjectId, Zone};
use beryl_table::{Action, Chance, Change, Seat, Table, apply};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);

fn add(table: &mut Table, id: u32, card: Card, owner: Seat, edit: impl FnOnce(&mut Object)) {
    let mut object = Object::new(ObjectId(id), card, owner, Zone::Battlefield);
    object.known_to = vec![HOST, GUEST];
    edit(&mut object);
    Change::Create {
        object,
        index: table.battlefield.len(),
    }
    .perform(table);
}

fn named(name: &str) -> Card {
    Card::Named(name.into())
}

fn enable(table: &mut Table, seat: Seat) {
    apply(
        table,
        seat,
        &Action::SetAssist {
            assist: Assist::StateNudges,
            on: true,
        },
        &mut Chance::fresh(1),
    )
    .unwrap();
}

fn ids(table: &Table) -> Vec<u32> {
    labels(table)
        .iter()
        .filter_map(|label| match label.on {
            ObjectOrSeat::Object(id) => Some(id.0),
            ObjectOrSeat::Seat(_) => None,
        })
        .collect()
}

#[test]
fn shown_toughness_and_damage_determine_creature_nudges() {
    let mut table = Table::new(2);
    add(&mut table, 1, named("Grizzly Bears"), HOST, |card| {
        card.damage = 3;
        card.counters.push(("+1/+1".into(), 1));
        card.controller = GUEST;
    });
    add(&mut table, 2, named("Grizzly Bears"), HOST, |card| {
        card.stats = Some((1, 1));
        card.counters.push(("-1/-1".into(), 1));
    });
    add(&mut table, 3, named("Grizzly Bears"), HOST, |card| {
        card.damage = 1;
    });
    add(&mut table, 4, named("Grizzly Bears"), HOST, |card| {
        card.face = Face::Down;
        card.known_to = vec![HOST];
        card.damage = 3;
    });
    assert!(labels(&table).is_empty());
    enable(&mut table, GUEST);
    assert_eq!(ids(&table), [1, 2]);
    let label = labels(&table).remove(0);
    assert_eq!(label.show_to, [GUEST]);
    apply(&mut table, GUEST, &label.action, &mut Chance::fresh(1)).unwrap();
    assert_eq!(table.zone(Zone::Graveyard(HOST)).unwrap(), [ObjectId(1)]);
    assert_eq!(ids(&table), [2]);
}

#[test]
fn only_planeswalkers_without_loyalty_are_suggested() {
    let mut table = Table::new(2);
    for (id, name, has_loyalty) in [
        (1, "Jace Beleren", false),
        (2, "Liliana Vess", true),
        (3, "Island", false),
    ] {
        add(&mut table, id, named(name), HOST, |card| {
            if has_loyalty {
                card.counters.push(("loyalty".into(), 1));
            }
        });
    }
    enable(&mut table, HOST);
    assert_eq!(ids(&table), [1]);
    let label = labels(&table).remove(0);
    apply(&mut table, HOST, &label.action, &mut Chance::fresh(1)).unwrap();
    assert!(labels(&table).is_empty());
}

#[test]
fn each_legend_under_the_same_controller_is_a_choice() {
    let mut table = Table::new(2);
    for (id, owner) in [(1, HOST), (2, GUEST), (3, GUEST)] {
        add(&mut table, id, named("Jace Beleren"), owner, |card| {
            card.counters.push(("loyalty".into(), 1));
            if id == 2 {
                card.controller = HOST;
            }
        });
    }
    for id in [4, 5] {
        add(&mut table, id, named("Island"), HOST, |_| {});
    }
    enable(&mut table, HOST);
    enable(&mut table, GUEST);
    assert_eq!(ids(&table), [1, 2]);
    let label = labels(&table).remove(1);
    assert_eq!(label.show_to, [HOST, GUEST]);
    apply(&mut table, HOST, &label.action, &mut Chance::fresh(1)).unwrap();
    assert_eq!(table.zone(Zone::Graveyard(GUEST)).unwrap(), [ObjectId(2)]);
    assert!(labels(&table).is_empty());
}

#[test]
fn zero_life_concession_is_only_offered_to_the_seat_that_would_concede() {
    let mut table = Table::new(2);
    table.game_match.in_game = true;
    table.players[0].life = 0;
    table.players[1].life = -1;
    enable(&mut table, HOST);
    let offered = labels(&table);
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0].on, ObjectOrSeat::Seat(HOST));
    assert_eq!(offered[0].show_to, [HOST]);
    enable(&mut table, GUEST);
    let offered = labels(&table);
    assert_eq!(offered.len(), 2);
    assert_eq!(offered[1].on, ObjectOrSeat::Seat(GUEST));
    assert_eq!(offered[1].show_to, [GUEST]);
    apply(&mut table, GUEST, &offered[1].action, &mut Chance::fresh(1)).unwrap();
    assert_eq!(table.game_match.last_winner, Some(HOST));
    assert!(labels(&table).is_empty());
}

#[test]
fn lethal_damage_nudge_respects_indestructible_but_zero_toughness_still_applies() {
    let mut table = Table::new(2);
    add(&mut table, 1, named("Darksteel Myr"), HOST, |card| {
        card.damage = 3;
    });
    add(&mut table, 2, named("Darksteel Myr"), GUEST, |card| {
        card.stats = Some((0, 0));
    });
    enable(&mut table, HOST);
    assert_eq!(ids(&table), [2]);
}
