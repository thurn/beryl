use beryl_session::protocol::{PlayerView, ZoneName};
use beryl_session::script::{Act, Fact, Script, Table as Stage, Verb, replay};
use beryl_table::action::{Adjustment, Destination, Position, SeenObject, Selection, SortBy};
use beryl_table::state::{Card, Face, Object, ObjectId, Row, Zone};
use beryl_table::testing::{Generator, Kind};
use beryl_table::{Action, Chance, Change, Log, Seat, Table};
use beryl_table_session::script::script;
use beryl_table_session::view::{Slots, view};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);
const FIELD: Zone = Zone::Battlefield;
const HAND: Zone = Zone::Hand(HOST);

const KINDS: &[Kind] = &[
    Kind::Move,
    Kind::Draw,
    Kind::Give,
    Kind::Mulligan,
    Kind::Reorder,
    Kind::Pin,
    Kind::SortHand,
    Kind::Attach,
    Kind::Unattach,
    Kind::Tap,
    Kind::Face,
    Kind::Peek,
    Kind::Counter,
    Kind::Stats,
    Kind::Damage,
    Kind::Life,
    Kind::PlayerCounter,
    Kind::Token,
    Kind::Clone,
    Kind::Marker,
    Kind::Remove,
    Kind::Flag,
    Kind::Note,
];

struct Game {
    table: Table,
    log: Log,
    chance: Chance,
    views: [(PlayerView, Slots); 2],
}

impl Game {
    fn new(table: Table) -> Self {
        let log = Log::new(&table);
        let views = [HOST, GUEST].map(|seat| {
            let mut slots = Slots::default();
            let (initial, _) = view(&table, &log, seat, &mut slots).unwrap();
            (initial, slots)
        });
        Self {
            table,
            log,
            chance: Chance::fresh(777),
            views,
        }
    }

    fn apply(&mut self, by: Seat, action: &Action) -> [Script; 2] {
        self.try_apply(by, action)
            .unwrap_or_else(|error| panic!("{action:?}: {error:?}"))
    }

    fn try_apply(
        &mut self,
        by: Seat,
        action: &Action,
    ) -> Result<[Script; 2], beryl_table::Refusal> {
        let host_before = self.table.clone();
        self.log
            .apply(&mut self.table, by, action, &mut self.chance)?;
        let entry = self.log.entries().last().unwrap();
        Ok([HOST, GUEST].map(|seat| {
            let (before, previous) = &mut self.views[usize::from(seat.0)];
            let mut slots = previous.clone();
            let (after, _) = view(&self.table, &self.log, seat, &mut slots).unwrap();
            let script = script(before, entry, &host_before, &after, seat, previous, &slots);
            assert_eq!(script.reset, None, "{action:?}");
            assert_eq!(
                replay::failure(&Stage::of(before), &script, &after),
                None,
                "{action:?}"
            );
            *before = after;
            *previous = slots;
            script
        }))
    }

    fn verbs(&mut self, by: Seat, action: &Action, verb: Verb) {
        for script in self.apply(by, action) {
            assert!(
                script
                    .beats
                    .iter()
                    .flatten()
                    .any(|step| step.act.verb() == verb),
                "{action:?}:\n{script}"
            );
        }
    }
}

fn select(zone: Zone, ids: &[u32]) -> Selection {
    Selection {
        zone,
        objects: ids.iter().copied().map(ObjectId).collect(),
    }
}

fn card(name: &str) -> Card {
    Card::Custom {
        name: name.into(),
        types: "Creature".into(),
        colors: Vec::new(),
        stats: Some((2, 2)),
    }
}

fn add(table: &mut Table, id: u32, name: &str, zone: Zone, known_to: &[Seat]) {
    let mut object = Object::new(ObjectId(id), card(name), HOST, zone);
    object.known_to = known_to.to_vec();
    Change::Create {
        object,
        index: table.zone(zone).unwrap().len(),
    }
    .perform(table);
}

fn movement(selection: Selection, to: Zone, position: Position) -> Action {
    Action::Move {
        selection,
        to: Destination::Zone(to),
        position,
        face: None,
        tapped: None,
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one table covers each card action in turn"
)]
fn card_changes_use_the_expected_verbs_for_both_seats() {
    let mut table = Table::new(2);
    add(&mut table, 1, "Bear One", FIELD, &[HOST, GUEST]);
    add(&mut table, 2, "Bear Two", FIELD, &[HOST, GUEST]);
    add(&mut table, 3, "Second", HAND, &[HOST]);
    add(&mut table, 4, "First", HAND, &[HOST]);
    for id in 5..9 {
        add(
            &mut table,
            id,
            &format!("Library {id}"),
            Zone::Library(HOST),
            &[],
        );
    }
    let mut game = Game::new(table);
    let creature = || select(FIELD, &[1]);
    for (action, verb) in [
        (
            Action::Reorder {
                selection: creature(),
                position: Position::Bottom,
            },
            Verb::Fly,
        ),
        (
            Action::Pin {
                selection: creature(),
                row: Row::Others,
            },
            Verb::Fly,
        ),
        (
            Action::SortHand {
                seat: HOST,
                by: SortBy::Name,
            },
            Verb::Fly,
        ),
        (
            Action::Attach {
                selection: creature(),
                host: SeenObject {
                    id: ObjectId(2),
                    zone: FIELD,
                },
            },
            Verb::Fly,
        ),
        (
            Action::Unattach {
                selection: creature(),
            },
            Verb::Fly,
        ),
        (
            Action::Tap {
                selection: creature(),
                tapped: true,
            },
            Verb::Tap,
        ),
        (
            Action::Face {
                selection: creature(),
                face: Face::Down,
            },
            Verb::Flip,
        ),
        (
            Action::Counter {
                selection: creature(),
                name: "+1/+1".into(),
                adjustment: Adjustment::Set(3),
            },
            Verb::Count,
        ),
        (
            Action::Counter {
                selection: creature(),
                name: "custom".into(),
                adjustment: Adjustment::Set(4),
            },
            Verb::Count,
        ),
        (
            Action::Stats {
                selection: creature(),
                stats: Some((3, 5)),
            },
            Verb::Count,
        ),
        (
            Action::Damage {
                selection: creature(),
                damage: 2,
            },
            Verb::Count,
        ),
        (
            Action::Life {
                seat: GUEST,
                adjustment: Adjustment::Add(-3),
            },
            Verb::Count,
        ),
        (
            Action::PlayerCounter {
                seat: HOST,
                name: "U".into(),
                adjustment: Adjustment::Set(1),
            },
            Verb::Count,
        ),
        (
            Action::PlayerCounter {
                seat: GUEST,
                name: "energy".into(),
                adjustment: Adjustment::Set(1),
            },
            Verb::Count,
        ),
        (
            Action::Flag {
                selection: creature(),
                doesnt_untap: true,
            },
            Verb::Set,
        ),
        (
            Action::Note {
                selection: creature(),
                note: Some("remember".into()),
            },
            Verb::Set,
        ),
        (
            Action::Clone {
                selection: creature(),
            },
            Verb::Appear,
        ),
        (
            Action::Token {
                card: card("New Token"),
                count: 2,
                attached_to: None,
                note: None,
            },
            Verb::Appear,
        ),
        (
            Action::Marker {
                selection: select(FIELD, &[1, 2]),
                text: "An ability".into(),
            },
            Verb::Emerge,
        ),
        (
            movement(select(HAND, &[3, 4]), Zone::Stack, Position::Bottom),
            Verb::Fly,
        ),
        (
            Action::Draw {
                seat: HOST,
                count: 2,
            },
            Verb::Fly,
        ),
        (
            Action::Give {
                selection: creature(),
                to: GUEST,
            },
            Verb::Fly,
        ),
        (
            Action::Mulligan {
                seat: HOST,
                count: 2,
            },
            Verb::Fly,
        ),
    ] {
        game.verbs(HOST, &action, verb);
    }
    let marker = game.table.markers.keys().next().unwrap().0;
    game.verbs(
        HOST,
        &Action::Remove {
            selection: select(Zone::Stack, &[marker]),
        },
        Verb::Vanish,
    );
}

#[test]
fn peeking_turns_the_card_without_disclosing_its_face_to_the_other_seat() {
    let mut table = Table::new(2);
    add(&mut table, 1, "Secret Face", FIELD, &[]);
    table.objects.get_mut(&ObjectId(1)).unwrap().face = Face::Down;
    let mut game = Game::new(table);
    let [owner, guest] = game.apply(
        HOST,
        &Action::Peek {
            selection: select(FIELD, &[1]),
        },
    );
    for script in [&owner, &guest] {
        assert!(matches!(script.beats[0][0].act, Act::Flip { .. }));
    }
    assert!(
        serde_json::to_string(&owner)
            .unwrap()
            .contains("Secret Face")
    );
    assert!(
        !serde_json::to_string(&guest)
            .unwrap()
            .contains("Secret Face")
    );
}

#[test]
fn a_draw_moves_anonymous_backs_as_one_group() {
    let mut table = Table::new(2);
    add(
        &mut table,
        1,
        "Top Card",
        Zone::Library(HOST),
        &[HOST, GUEST],
    );
    add(&mut table, 2, "Second Card", Zone::Library(HOST), &[]);
    let mut game = Game::new(table);
    let old = game.views[1].1.get(ObjectId(1)).unwrap();
    let [owner, guest] = game.apply(
        HOST,
        &Action::Draw {
            seat: HOST,
            count: 2,
        },
    );
    assert_eq!(owner.beats.len(), 1);
    assert_eq!(guest.beats.len(), 1);
    assert_eq!(
        guest.beats[0]
            .iter()
            .filter(|step| step.act.verb() == Verb::Fly)
            .count(),
        2
    );
    assert!(matches!(
        guest.beats[0][0].act,
        Act::Set {
            fact: Fact::Forget { .. }
        }
    ));
    for step in guest.beats[0].iter().skip(1) {
        let Act::Fly {
            origin: Some(origin),
            object,
            ..
        } = &step.act
        else {
            panic!("{step}")
        };
        assert_eq!(origin.zone, ZoneName::Library);
        assert!(object.name.is_none());
        assert_ne!(object.slot, old);
    }
    assert!(!serde_json::to_string(&guest).unwrap().contains("Top Card"));
}

#[test]
fn one_thousand_random_card_actions_replay_for_both_seats() {
    let mut table = Table::new(2);
    for i in 0..16 {
        add(
            &mut table,
            i,
            &format!("<library-{i}>"),
            Zone::Library(HOST),
            &[],
        );
    }
    let mut game = Game::new(table);
    let mut generator = Generator::new(227);
    let mut tested = 0;
    for _ in 0..2_000 {
        let (_, by, action) = generator.next(&game.table, KINDS);
        let Ok(scripts) = game.try_apply(by, &action) else {
            continue;
        };
        tested += 1;
        for seat in game.table.seats() {
            let json = serde_json::to_string(&scripts[usize::from(seat.0)]).unwrap();
            for object in game.table.objects.values() {
                let Card::Custom { name, .. } = &object.card else {
                    continue;
                };
                // Clones can share a name with a hidden card. Check the
                // record at every hidden card's current slot even then.
                if object.known_to.contains(&seat) {
                    continue;
                }
                let (view, slots) = &game.views[usize::from(seat.0)];
                assert!(
                    scripts[usize::from(seat.0)]
                        .beats
                        .iter()
                        .flatten()
                        .all(|step| {
                            step.act.object().is_none_or(|record| {
                                Some(record.slot) != slots.get(object.id) || record.name.is_none()
                            })
                        }),
                    "{action:?} named hidden slot {:?}",
                    slots.get(object.id)
                );
                if view
                    .objects
                    .iter()
                    .all(|record| record.name.as_ref() != Some(name))
                {
                    assert!(
                        !json.contains(name),
                        "{action:?} leaked {name} to {seat:?}: {json}"
                    );
                }
            }
        }
        if tested == 1_000 {
            break;
        }
    }
    assert_eq!(tested, 1_000);
}
