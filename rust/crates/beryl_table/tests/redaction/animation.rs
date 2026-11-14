use super::*;
use beryl_table::Entry;

fn animation_batch_table() -> Table {
    let mut table = Table::new(2);
    let moved = ObjectId(40);
    let revealed = ObjectId(42);
    let looked = ObjectId(43);
    add(
        &mut table,
        moved,
        "Public Return",
        Zone::Battlefield,
        &[HOST, GUEST],
    );
    table.objects.get_mut(&moved).unwrap().owner = GUEST;
    add(&mut table, ObjectId(41), "Drawn Name", LIBRARY, &[]);
    add(&mut table, looked, "Looked Name", LIBRARY, &[]);
    add(
        &mut table,
        revealed,
        "Revealed Name",
        LIBRARY,
        &[HOST, GUEST],
    );
    table
}

fn mixed_animation_batch(table: &mut Table) -> Entry {
    let moved = table.battlefield[0];
    let revealed = table.zone(LIBRARY).unwrap()[2];
    let guest_hand = Zone::Hand(GUEST);
    let look = Action::Look {
        seat: HOST,
        portion: Portion::Top(1),
    };
    let reveal = Action::Reveal {
        zone: LIBRARY,
        objects: Some(vec![revealed]),
        random: None,
        to: vec![HOST, GUEST],
    };
    apply(
        table,
        HOST,
        &Action::Batch {
            actions: vec![
                Action::Move {
                    selection: Selection {
                        objects: vec![moved],
                        zone: Zone::Battlefield,
                    },
                    to: Destination::Zone(guest_hand),
                    position: Position::Top,
                    face: None,
                    tapped: None,
                },
                Action::Batch {
                    actions: vec![
                        Action::Shuffle {
                            seat: HOST,
                            portion: Portion::Top(2),
                        },
                        Action::Draw {
                            seat: HOST,
                            count: 1,
                        },
                        look.clone(),
                    ],
                },
                look,
                reveal,
            ],
        },
        &mut Chance::replaying(vec![0]),
    )
    .unwrap()
}

#[test]
fn animation_projects_nested_batch_members_using_their_own_visibility() {
    let mut table = animation_batch_table();
    let before = table.clone();
    let revealed = before.zone(LIBRARY).unwrap()[2];
    let entry = mixed_animation_batch(&mut table);
    let guest_hand = Zone::Hand(GUEST);

    let card = |id, name: &str| ShownCard {
        id,
        card: Card::Named(name.into()),
        face: Face::Front,
    };
    let returned = table.zone(guest_hand).unwrap()[0];
    let drawn = table.zone(HAND).unwrap()[0];
    let looked = table.zone(LIBRARY).unwrap()[0];
    let mut shuffled = table.zone(LIBRARY).unwrap().to_vec();
    shuffled.insert(0, ObjectId(45));
    for viewer in [HOST, GUEST] {
        let animation = animation_seen_by(&entry, &before, viewer);
        let json = serde_json::to_string(&animation).unwrap();
        let members: Vec<_> = animation
            .members
            .into_iter()
            .map(|member| member.events)
            .collect();
        let look = AnimationEvent::Look {
            library: LIBRARY,
            count: 1,
            cards: if viewer == HOST {
                vec![card(looked, "Looked Name")]
            } else {
                vec![]
            },
        };
        let handling = AnimationEvent::Change(look_starts());
        let first_look = if viewer == HOST {
            vec![
                AnimationEvent::Learn(card(looked, "Looked Name")),
                handling,
                look.clone(),
            ]
        } else {
            vec![handling, look.clone()]
        };
        assert_eq!(
            members,
            vec![
                vec![AnimationEvent::HandMove {
                    owner_transfer: (viewer == GUEST).then_some((before.battlefield[0], returned)),
                    from: Zone::Battlefield,
                    to: guest_hand,
                    hand_card: (viewer == GUEST).then(|| card(returned, "Public Return")),
                }],
                vec![AnimationEvent::Shuffle {
                    library: LIBRARY,
                    order: shuffled.clone(),
                }],
                vec![AnimationEvent::HandMove {
                    owner_transfer: (viewer == HOST).then_some((ObjectId(45), drawn)),
                    from: LIBRARY,
                    to: HAND,
                    hand_card: (viewer == HOST).then(|| card(drawn, "Drawn Name")),
                }],
                first_look,
                vec![look],
                vec![AnimationEvent::Reveal {
                    zone: LIBRARY,
                    cards: vec![card(revealed, "Revealed Name")],
                }],
            ]
        );
        for forbidden in ["replace_id", "outcomes"] {
            assert!(!json.contains(forbidden), "{json}");
        }
        if viewer == GUEST {
            assert!(!json.contains("Drawn Name"));
            assert!(!json.contains("Looked Name"));
        }
    }
}
