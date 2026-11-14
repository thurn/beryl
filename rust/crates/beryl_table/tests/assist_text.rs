use beryl_card_store::{Face, STORE};
use beryl_table::assist::text::{
    AbilityRow, Flow, FlowChoice, LoyaltyCost, abilities, ability_action, flow_line, flow_result,
    flows,
};
use beryl_table::state::{Card, Object, ObjectId, Zone};
use beryl_table::{Chance, Change, Refusal, Seat, Table, apply};

const HOST: Seat = Seat(0);

fn front(name: &str) -> Face {
    STORE.card(name).unwrap().faces.remove(0)
}

fn with_text(text: &str) -> Face {
    Face {
        oracle_text: Some(text.into()),
        ..front("Grizzly Bears")
    }
}

fn add_card(table: &mut Table, id: u32, name: &str, zone: Zone) -> ObjectId {
    let id = ObjectId(id);
    let mut object = Object::new(id, Card::Named(name.into()), HOST, zone);
    object.known_to = vec![HOST];
    Change::Create {
        object,
        index: table.zone(zone).unwrap().len(),
    }
    .perform(table);
    id
}

#[test]
fn the_documented_ability_examples() {
    let face = with_text(concat!(
        "+1: Draw a card, then discard a card.\n",
        "−X: Put X target cards from a graveyard on the bottom.\n",
        "{T}: Add {G}.\n",
        "{2}, Sacrifice this artifact: Draw a card.\n",
        "Flying (This creature can't be blocked except by…)"
    ));
    assert_eq!(
        abilities(&face),
        [
            AbilityRow::Loyalty {
                cost: LoyaltyCost::Plus(1),
                text: "Draw a card, then discard a card.".into(),
            },
            AbilityRow::Loyalty {
                cost: LoyaltyCost::MinusX,
                text: "Put X target cards from a graveyard on the bottom.".into(),
            },
            AbilityRow::Activated {
                cost: "{T}".into(),
                text: "Add {G}.".into()
            },
            AbilityRow::Activated {
                cost: "{2}, Sacrifice this artifact".into(),
                text: "Draw a card.".into(),
            },
        ]
    );
}

#[test]
fn reads_printed_abilities_and_ignores_clue_and_cycling_reminders() {
    let mut ash = front("Ashiok, Nightmare Weaver");
    for expected_minus in ['−', '-'] {
        ash.oracle_text = ash
            .oracle_text
            .map(|text| text.replace('−', &expected_minus.to_string()));
        assert!(matches!(
            abilities(&ash).as_slice(),
            [
                AbilityRow::Loyalty {
                    cost: LoyaltyCost::Plus(2),
                    ..
                },
                AbilityRow::Loyalty {
                    cost: LoyaltyCost::MinusX,
                    ..
                },
                AbilityRow::Loyalty {
                    cost: LoyaltyCost::Minus(10),
                    ..
                },
            ]
        ));
    }
    assert_eq!(
        abilities(&front("Llanowar Elves")),
        [AbilityRow::Activated {
            cost: "{T}".into(),
            text: "Add {G}.".into()
        },]
    );
    assert_eq!(
        abilities(&front("Witching Well")),
        [AbilityRow::Activated {
            cost: "{3}{U}, Sacrifice this artifact".into(),
            text: "Draw two cards.".into(),
        },]
    );
    let mut clue_maker = STORE
        .card("Daring Sleuth // Bearer of Overwhelming Truths")
        .unwrap();
    assert!(abilities(&clue_maker.faces.pop().unwrap()).is_empty());
    assert!(abilities(&front("Shark Typhoon")).is_empty());
}

#[test]
fn distinguishes_a_cost_from_colons_in_prose_and_quotes() {
    let face = with_text(concat!(
        "Create a token with “{T}: Add {G}.\n",
        "{1}, {T}: Draw a card.”\n",
        "Create a token with \"Tap this token: Draw a card.\"\n",
        "{2} (Reminder: punctuation): Draw a card.\n",
        "Threshold — Sacrifice another creature: Draw a card.\n",
        "Ultimate Magic: Deal damage to each creature.\n",
        "0: Draw a card."
    ));
    assert_eq!(
        abilities(&face),
        [
            AbilityRow::Activated {
                cost: "{2} (Reminder: punctuation)".into(),
                text: "Draw a card.".into(),
            },
            AbilityRow::Activated {
                cost: "Sacrifice another creature".into(),
                text: "Draw a card.".into(),
            },
            AbilityRow::Loyalty {
                cost: LoyaltyCost::Zero,
                text: "Draw a card.".into()
            },
        ]
    );
}

#[test]
fn ability_actions_create_markers_and_only_edit_loyalty_for_loyalty_abilities() {
    let mut table = Table::new(2);
    let mut chance = Chance::fresh(17);
    let walker = add_card(
        &mut table,
        101,
        "Ashiok, Nightmare Weaver",
        Zone::Battlefield,
    );
    table.objects.get_mut(&walker).unwrap().counters = vec![("loyalty".into(), 5)];
    let elf = add_card(&mut table, 102, "Llanowar Elves", Zone::Battlefield);

    let rows = abilities(&front("Ashiok, Nightmare Weaver"));
    for (row, x, loyalty) in [(&rows[0], None, 7), (&rows[1], Some(3), 4)] {
        let action = ability_action(walker, row, x);
        apply(&mut table, HOST, &action, &mut chance).unwrap();
        assert_eq!(
            table.objects[&walker].counters,
            [("loyalty".into(), loyalty)]
        );
        assert_eq!(table.markers[&table.stack[0]].source, walker);
    }

    let rows = abilities(&front("Llanowar Elves"));
    let action = ability_action(elf, &rows[0], None);
    apply(&mut table, HOST, &action, &mut chance).unwrap();
    let marker = &table.markers[&table.stack[0]];
    assert_eq!(marker.source, elf);
    assert_eq!(marker.text, "Add {G}.");
    assert!(!table.objects[&elf].tapped);
}

#[test]
fn flows_match_only_the_fixed_forms_with_explicit_counts() {
    assert_eq!(flows(&front("Serum Visions")), [Flow::Scry(2)]);
    assert_eq!(flows(&front("Consider")), [Flow::Surveil(1)]);
    assert_eq!(flows(&front("Witching Well")), [Flow::Scry(2)]);
    assert_eq!(flows(&front("Augur of Bolas")), [Flow::LookTop(3)]);
    let face = with_text(concat!(
        "Scry ten, then surveil 12. Look at the top two cards of your library.\n",
        "Scry X. Surveil twice. Look at the top card of your library.\n",
        "Look at the top five cards of target player's library.\n",
        "If you surveilled 3, draw a card. Scry 2nd.\n",
        "A token gains \"Scry 9.\" (Surveil 8.)"
    ));
    assert_eq!(
        flows(&face),
        [Flow::Scry(10), Flow::Surveil(12), Flow::LookTop(2)]
    );
}

#[test]
fn flow_results_apply_in_the_chosen_order_and_reveal_only_graveyard_cards() {
    for (flow, expected_log) in [
        (Flow::Scry(4), "Scried 4: 2 on top, 2 on the bottom"),
        (
            Flow::Surveil(4),
            "Surveilled 4: 2 on top, 2 in the graveyard",
        ),
        (Flow::LookTop(4), "Looked at 4: 4 on top"),
    ] {
        let mut table = Table::new(2);
        let mut chance = Chance::fresh(17);
        let library = Zone::Library(HOST);
        let [a, b, c, d, next] =
            [101, 102, 103, 104, 105].map(|id| add_card(&mut table, id, "Island", library));
        let (on_top, elsewhere, expected_library) = match flow {
            Flow::Scry(_) => (vec![d, b], vec![c, a], vec![d, b, next, c, a]),
            Flow::Surveil(_) => (vec![d, b], vec![c, a], vec![d, b, next]),
            Flow::LookTop(_) => (vec![d, c, a, b], Vec::new(), vec![d, c, a, b, next]),
        };
        let choice = FlowChoice {
            seat: HOST,
            on_top,
            elsewhere,
        };
        assert_eq!(flow_line(flow, &choice), expected_log);
        let result = flow_result(flow, &[a, b, c, d], &choice).unwrap();
        apply(&mut table, HOST, &result, &mut chance).unwrap();
        assert_eq!(table.zones[0].library, expected_library);
        assert!(
            table.zones[0]
                .library
                .iter()
                .all(|id| table.objects[id].known_to == [HOST])
        );
        if matches!(flow, Flow::Surveil(_)) {
            assert_eq!(table.zones[0].graveyard, [c, a]);
            assert_eq!(table.objects[&c].known_to, [HOST, Seat(1)]);
        }
    }
}

#[test]
fn flow_validation_requires_a_partition_of_the_cards_actually_seen() {
    let a = ObjectId(101);
    let b = ObjectId(102);
    let c = ObjectId(103);
    for (on_top, elsewhere) in [
        (vec![a], vec![]),
        (vec![a], vec![a]),
        (vec![a], vec![c]),
        (vec![a, b], vec![c]),
    ] {
        let choice = FlowChoice {
            seat: HOST,
            on_top,
            elsewhere,
        };
        assert_eq!(
            flow_result(Flow::Scry(3), &[a, b], &choice),
            Err(Refusal::INVALID)
        );
    }

    let choice = FlowChoice {
        seat: HOST,
        on_top: vec![a],
        elsewhere: vec![b],
    };
    assert_eq!(
        flow_result(Flow::LookTop(3), &[a, b], &choice),
        Err(Refusal::INVALID)
    );
    assert!(flow_result(Flow::Scry(3), &[a, b], &choice).is_ok());
}

#[test]
fn charge_threshold_abilities_are_read_alongside_ordinary_abilities() {
    let rows = abilities(&front("Adagia, Windswept Bastion"));
    assert!(matches!(
        rows.as_slice(),
        [
            AbilityRow::Activated { cost: mana, .. },
            AbilityRow::Activated { cost: copy, text },
        ] if mana == "{T}" && copy == "{3}{W}, {T}" && text.starts_with("Create a token")
    ));
}
