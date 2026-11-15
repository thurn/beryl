use beryl_session::protocol::{CardHandling, Side, TablePhase, ZoneName};
use beryl_session::script::{Act, Fact, Notice, Script, Span, Verb};
use beryl_table::action::{Destination, Portion, Position, SeenObject, Target};
use beryl_table::state::{ObjectId, Phase, Zone};
use beryl_table::{Action, Seat, Table};

mod script_common;
use script_common::{FIELD, GUEST, Game, HOST, add, select};

fn game() -> Game {
    let mut table = Table::new(2);
    for seat in [HOST, GUEST] {
        let offset = u32::from(seat.0) * 10;
        for (index, name) in ["Top", "Middle", "Bottom"].into_iter().enumerate() {
            add(
                &mut table,
                offset + u32::try_from(index).unwrap(),
                &format!("{name} {}", seat.0),
                Zone::Library(seat),
                &[],
            );
        }
    }
    add(&mut table, 21, "First permanent", FIELD, &[HOST, GUEST]);
    add(&mut table, 22, "Second permanent", FIELD, &[HOST, GUEST]);
    Game::new(table)
}

fn acts(script: &Script) -> impl Iterator<Item = &Act> {
    script.beats.iter().flatten().map(|step| &step.act)
}

fn notices(script: &Script) -> Vec<&Notice> {
    acts(script)
        .filter_map(|act| match act {
            Act::Announce { notice } => Some(notice),
            _ => None,
        })
        .collect()
}

fn move_permanent() -> Action {
    Action::Move {
        selection: select(FIELD, &[22]),
        to: Destination::Zone(Zone::Graveyard(HOST)),
        position: Position::Top,
        face: None,
        tapped: None,
    }
}

#[test]
fn a_bottom_look_carries_only_its_count_to_the_other_seat() {
    for actor in [HOST, GUEST] {
        let mut game = game();
        let scripts = game.apply(
            actor,
            &Action::Look {
                seat: actor,
                portion: Portion::Bottom(1),
            },
        );
        for (index, script) in scripts.iter().enumerate() {
            let own = index == usize::from(actor.0);
            let handling = CardHandling::Pile {
                side: if own { Side::You } else { Side::Opponent },
                pile: ZoneName::Library,
                count: 1,
                bottom: own,
            };
            assert!(acts(script).any(|act| matches!(act,
                Act::Lift { span: Span::Begin, handling: held } if *held == handling
            )));
            assert_eq!(game.views()[index].view().handling, Some(handling));
            let shown = serde_json::to_string(script).unwrap();
            assert_eq!(shown.contains(&format!("Bottom {}", actor.0)), own);
            assert!(!shown.contains(&format!("Top {}", actor.0)));
        }
        // The other seat cannot close the actor's viewer.
        let other = Seat(1 - actor.0);
        for script in game.apply(other, &Action::CloseLook {}) {
            assert!(!acts(&script).any(|act| act.verb() == Verb::Lift));
        }
        for script in game.apply(actor, &Action::CloseLook {}) {
            assert!(matches!(
                acts(&script).next(),
                Some(Act::Lift {
                    span: Span::End,
                    ..
                })
            ));
        }
        assert!(
            game.views()
                .iter()
                .all(|publication| publication.view().handling.is_none())
        );
        assert!(
            game.published.table().objects[&ObjectId(u32::from(actor.0) * 10 + 2)]
                .known_to
                .contains(&actor)
        );
    }
}

#[test]
fn a_reveal_showcases_only_for_the_recipient_even_if_repeated() {
    let mut game = game();
    let reveal = Action::Reveal {
        zone: Zone::Library(HOST),
        objects: Some(vec![ObjectId(0)]),
        random: None,
        to: vec![GUEST],
    };
    for _ in 0..2 {
        let [host, guest] = game.apply(HOST, &reveal);
        assert!(!acts(&host).any(|act| act.verb() == Verb::Showcase));
        assert!(!serde_json::to_string(&host).unwrap().contains("Top 0"));
        assert!(acts(&guest).any(|act| matches!(act,
            Act::Showcase { object } if object.name.as_deref() == Some("Top 0")
        )));
    }
}

#[test]
fn lending_lifts_without_revealing_and_moving_a_card_ends_the_lift() {
    let mut game = game();
    let library = Zone::Library(HOST);
    let [host, guest] = game.apply(
        HOST,
        &Action::Lend {
            zone: library,
            to: GUEST,
        },
    );
    for script in [host, guest] {
        assert!(acts(&script).any(|act| matches!(
            act,
            Act::Lift {
                span: Span::Begin,
                handling: CardHandling::Pile { count: 3, .. }
            }
        )));
        assert!(!serde_json::to_string(&script).unwrap().contains("Top 0"));
    }

    for script in game.apply(
        HOST,
        &Action::Draw {
            seat: HOST,
            count: 1,
        },
    ) {
        assert!(matches!(
            script.beats[0][0].act,
            Act::Lift {
                span: Span::End,
                ..
            }
        ));
        assert!(
            script.beats[1]
                .iter()
                .any(|step| step.act.verb() == Verb::Fly)
        );
    }
    assert!(
        game.published
            .table()
            .lent_zones
            .contains(&(library, GUEST))
    );
}

#[test]
fn arrows_end_when_cleared_or_before_either_endpoint_moves() {
    let mut game = game();
    let arrow = Action::Arrow {
        selection: select(FIELD, &[21]),
        to: Target::Object(SeenObject {
            id: ObjectId(22),
            zone: FIELD,
        }),
    };
    game.verbs(HOST, &arrow, Verb::Point);
    // Identical arrows are drawn once, even if both players drew one.
    for script in game.apply(GUEST, &arrow) {
        assert!(!acts(&script).any(|act| act.verb() == Verb::Point));
    }
    let clear = Action::ClearArrows {
        from: None,
        to: None,
    };
    for script in game.apply(HOST, &clear) {
        assert!(!acts(&script).any(|act| act.verb() == Verb::Point));
    }
    for script in game.apply(GUEST, &clear) {
        assert!(acts(&script).any(|act| matches!(
            act,
            Act::Point {
                span: Span::End,
                ..
            }
        )));
    }
    game.verbs(HOST, &arrow, Verb::Point);
    for script in game.apply(HOST, &move_permanent()) {
        assert!(matches!(
            script.beats[0][0].act,
            Act::Point {
                span: Span::End,
                ..
            }
        ));
        assert!(
            script.beats[1]
                .iter()
                .any(|step| step.act.verb() == Verb::Fly)
        );
    }
}

#[test]
fn turn_ends_lifts_and_arrows_before_the_banner() {
    let mut game = game();
    game.apply(
        HOST,
        &Action::Look {
            seat: HOST,
            portion: Portion::All,
        },
    );
    game.apply(
        HOST,
        &Action::Arrow {
            selection: select(FIELD, &[21]),
            to: Target::Player(GUEST),
        },
    );
    for script in game.apply(HOST, &Action::NextTurn {}) {
        assert!(script.beats[0].iter().any(|step| matches!(
            step.act,
            Act::Lift {
                span: Span::End,
                ..
            }
        )));
        assert!(script.beats[0].iter().any(|step| matches!(
            step.act,
            Act::Point {
                span: Span::End,
                ..
            }
        )));
        assert_eq!(
            notices(&script),
            [&Notice::TableTurn {
                name: "Seat 2".into()
            }]
        );
    }
}

#[test]
fn both_seats_get_roll_phase_and_shuffle_notices() {
    let mut game = game();
    for sides in [2, 6] {
        let scripts = game.apply(HOST, &Action::Roll { sides, count: 1 });
        assert_eq!(notices(&scripts[0]), notices(&scripts[1]));
        assert!(matches!(&notices(&scripts[0])[..],
            [Notice::Roll { sides: die, outcomes }] if *die == sides && outcomes.len() == 1 && outcomes[0] < sides
        ));
    }
    for _ in 0..2 {
        for script in game.apply(
            HOST,
            &Action::Phase {
                phase: Phase::FirstMain,
            },
        ) {
            assert_eq!(
                notices(&script),
                [&Notice::Phase {
                    phase: TablePhase::FirstMain
                }]
            );
        }
    }
    game.apply(
        HOST,
        &Action::Look {
            seat: HOST,
            portion: Portion::All,
        },
    );
    let old_orders = [
        game.views()[0].view().players.you.library.clone(),
        game.views()[1].view().players.opponent.library.clone(),
    ];
    let scripts = game.apply(
        HOST,
        &Action::Shuffle {
            seat: HOST,
            portion: Portion::All,
        },
    );
    for (script, old_order) in scripts.iter().zip(old_orders) {
        assert_eq!(notices(script), [&Notice::Shuffle]);
        assert!(matches!(
            script.beats[0][0].act,
            Act::Lift {
                span: Span::End,
                ..
            }
        ));
        let shuffled = acts(script)
            .find_map(|act| match act {
                Act::Set {
                    fact: Fact::ShuffledLibrary { cards, .. },
                } => Some(cards),
                _ => None,
            })
            .expect("the shuffle sets a new library order");
        assert_eq!(shuffled.len(), old_order.len());
        assert!(
            shuffled
                .iter()
                .all(|card| card.name.is_none() && !old_order.contains(&card.slot))
        );
    }
}

#[test]
fn top_settings_use_set_and_keep_private_names_private() {
    let mut game = game();
    for (action, visible) in [
        (
            Action::AlwaysLookAtTop {
                seat: HOST,
                on: true,
            },
            [true, false],
        ),
        (
            Action::AlwaysRevealTop {
                seat: HOST,
                on: true,
            },
            [true, true],
        ),
        (
            Action::AlwaysRevealTop {
                seat: HOST,
                on: false,
            },
            [true, true],
        ),
        (
            Action::AlwaysLookAtTop {
                seat: HOST,
                on: false,
            },
            [true, true],
        ),
    ] {
        for (script, sees_top) in game.apply(HOST, &action).iter().zip(visible) {
            assert!(acts(script).all(|act| act.verb() == Verb::Set));
            assert!(acts(script).any(
                |act| matches!(act, Act::Set { fact: Fact::Object { object } }
                    if object.name.as_deref() == if sees_top { Some("Top 0") } else { None }
                )
            ));
        }
    }
}
