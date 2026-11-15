//! The timing rules on hand-built steps. `just timing-parity` holds them to the web
//! interface's over every recording; these say what a case is made of where a reader looks.

use std::collections::BTreeSet;

use super::conventions::DATA;
use super::{case_of, time_script, timing_of};
use crate::protocol::{
    CardHandling, Counter, Mana, ObjectId, ObjectView, Side, StackItem, Step as TurnStep, Why,
    ZoneName,
};
use crate::script::{
    Act, Arrow, Entry, Fact, Mark, Notice, Script, Span, Step, Stream, Subject, Table, Tally,
};

fn card(slot: u16, zone: ZoneName, controller: Side) -> ObjectView {
    ObjectView::card(ObjectId(slot), "Card", zone, controller, controller)
}

fn object(slot: u16) -> Subject {
    Subject::Object {
        slot: ObjectId(slot),
    }
}

fn player(player: Side) -> Subject {
    Subject::Player { player }
}

fn fly(from: ZoneName, why: Why, object: ObjectView) -> Act {
    Act::Fly {
        from,
        why,
        object,
        at: None,
        item: None,
        origin: None,
    }
}

fn vanish(subject: Subject, why: Why, object: Option<ObjectView>) -> Act {
    Act::Vanish {
        subject,
        from: ZoneName::Battlefield,
        why,
        object,
        at: None,
    }
}

fn ability(triggered: bool) -> StackItem {
    StackItem::Ability {
        source: Some(ObjectId(1)),
        ability: 0,
        granted: false,
        name: "Ability".to_owned(),
        triggered,
        taps_to_pay: None,
        nth: 0,
        text: None,
        controller: Side::You,
        manual: None,
        targets: Vec::new(),
        x: 0,
    }
}

fn point(span: Span, arrow: Arrow, to: Subject) -> Act {
    Act::Point {
        span,
        arrow,
        from: object(1),
        to,
    }
}

fn mark(span: Span, mark: Mark) -> Act {
    Act::Mark {
        span,
        mark,
        on: object(1),
    }
}

/// The data's case names of every verb, as `verb/case`.
fn data_cases() -> BTreeSet<String> {
    let data: serde_json::Value = serde_json::from_str(DATA).unwrap();
    let mut cases = BTreeSet::new();
    for verb in data["verbs"].as_array().unwrap() {
        let name = verb["name"].as_str().unwrap();
        for case in verb["cases"].as_object().into_iter().flat_map(|c| c.keys()) {
            cases.insert(format!("{name}/{case}"));
        }
    }
    cases
}

/// One hand-built act for each case the data names, each found as that case; the cases of
/// `lift` are the web stage's own steps, of the cards a prompt holds up, which no script has.
#[test]
#[allow(clippy::too_many_lines)] // One act per case.
fn each_case_of_the_data_is_found_on_one_act() {
    let table = Table::empty(20);
    let mut stack = Table::empty(20);
    stack
        .objects
        .insert(ObjectId(2), card(2, ZoneName::Stack, Side::You));
    stack.stack.push(Entry {
        item: StackItem::Spell {
            slot: ObjectId(2),
            targets: Vec::new(),
            x: 0,
            paid: Vec::new(),
        },
        id: None,
    });
    let (you, them) = (Side::You, Side::Opponent);
    let tapped = ObjectView {
        tapped: true,
        ..card(1, ZoneName::Battlefield, you)
    };
    let drawn = fly(ZoneName::Library, Why::Draw, card(1, ZoneName::Hand, you));
    let other = fly(ZoneName::Library, Why::Draw, card(2, ZoneName::Hand, you));
    let strike = |to| Act::Strike {
        source: ObjectId(1),
        to,
        amount: 2,
        object: None,
    };
    let streak = |stream| Act::Streak {
        from: Some(object(1)),
        to: object(2),
        stream,
        amount: 1,
        object: None,
    };
    let life = |life| Act::Count {
        tally: Tally::Life { player: you, life },
    };
    let counters = |counter| Act::Count {
        tally: Tally::Counters {
            counter,
            object: card(1, ZoneName::Battlefield, you),
        },
    };
    let appear = |from, why| Act::Appear {
        from,
        why,
        object: card(1, ZoneName::Battlefield, you),
        at: None,
        item: None,
    };
    let emerge = |triggered| Act::Emerge {
        id: 1,
        item: ability(triggered),
        at: None,
    };
    let gone = |zone| Some(card(1, zone, you));
    let set = |fact| Act::Set { fact };
    let announce = |notice| Act::Announce { notice };
    let cases: Vec<(&str, Act, &Table, Vec<&Act>)> = vec![
        (
            "land played",
            fly(
                ZoneName::Hand,
                Why::Play,
                card(1, ZoneName::Battlefield, you),
            ),
            &table,
            vec![],
        ),
        (
            "opponent land played",
            fly(
                ZoneName::Hand,
                Why::Play,
                card(1, ZoneName::Battlefield, them),
            ),
            &table,
            vec![],
        ),
        (
            "permanent resolves",
            fly(
                ZoneName::Stack,
                Why::Resolve,
                card(1, ZoneName::Battlefield, you),
            ),
            &table,
            vec![],
        ),
        (
            "discard",
            fly(
                ZoneName::Hand,
                Why::Discard,
                card(1, ZoneName::Graveyard, you),
            ),
            &table,
            vec![],
        ),
        (
            "own spell cast",
            fly(ZoneName::Hand, Why::Cast, card(1, ZoneName::Stack, you)),
            &table,
            vec![],
        ),
        (
            "opponent spell cast",
            fly(ZoneName::Hand, Why::Cast, card(1, ZoneName::Stack, them)),
            &table,
            vec![],
        ),
        ("draw", drawn.clone(), &table, vec![]),
        ("draw of several", drawn.clone(), &table, vec![&other]),
        (
            "opponent draw",
            fly(
                ZoneName::Library,
                Why::Draw,
                ObjectView::hidden(ObjectId(1), ZoneName::Hand, them, them),
            ),
            &table,
            vec![],
        ),
        (
            "token",
            appear(ZoneName::Nowhere, Why::Created),
            &table,
            vec![],
        ),
        (
            "opening hand",
            appear(ZoneName::Library, Why::Draw),
            &table,
            vec![],
        ),
        (
            "destroyed",
            vanish(object(1), Why::Destroy, gone(ZoneName::Graveyard)),
            &table,
            vec![],
        ),
        (
            "countered",
            vanish(object(1), Why::Counter, gone(ZoneName::Graveyard)),
            &table,
            vec![],
        ),
        (
            "resolved instant or sorcery",
            vanish(object(1), Why::Resolve, gone(ZoneName::Graveyard)),
            &table,
            vec![],
        ),
        (
            "exiled",
            vanish(object(1), Why::Exile, gone(ZoneName::Exile)),
            &table,
            vec![],
        ),
        (
            "bounced",
            vanish(object(1), Why::Return, gone(ZoneName::Hand)),
            &table,
            vec![],
        ),
        (
            "sacrificed",
            vanish(object(1), Why::Sacrifice, gone(ZoneName::Graveyard)),
            &table,
            vec![],
        ),
        (
            "ability finished",
            vanish(Subject::Ability { id: 1 }, Why::Resolve, None),
            &table,
            vec![],
        ),
        ("triggered ability", emerge(true), &table, vec![]),
        ("activated ability", emerge(false), &table, vec![]),
        ("tap", Act::Tap { object: tapped }, &table, vec![]),
        (
            "untap",
            Act::Tap {
                object: card(1, ZoneName::Battlefield, you),
            },
            &table,
            vec![],
        ),
        ("blocked attacker", strike(object(2)), &table, vec![]),
        ("unblocked attacker", strike(player(them)), &table, vec![]),
        (
            "mana",
            streak(Stream::Mana { color: Mana::Blue }),
            &table,
            vec![],
        ),
        ("damage", streak(Stream::Damage), &table, vec![]),
        ("life loss", life(17), &table, vec![]),
        ("life gain", life(23), &table, vec![]),
        ("counter", counters(Counter::PlusOne), &table, vec![]),
        ("loyalty", counters(Counter::Loyalty), &table, vec![]),
        (
            "step begins",
            set(Fact::Turn {
                number: 2,
                own_number: 1,
                active: them,
                step: TurnStep::Upkeep,
            }),
            &table,
            vec![],
        ),
        (
            "library shuffled",
            set(Fact::Library {
                player: you,
                order: Vec::new(),
            }),
            &table,
            vec![],
        ),
        (
            "game's end",
            Act::Dwell {
                subject: player(you),
            },
            &table,
            vec![],
        ),
        (
            "turn banner",
            announce(Notice::Turn { active: them }),
            &table,
            vec![],
        ),
        (
            "scry pill",
            announce(Notice::Scry {
                player: them,
                top: 1,
                bottom: 0,
            }),
            &table,
            vec![],
        ),
        (
            "coin",
            announce(Notice::Coin {
                player: you,
                won: true,
                source: ObjectId(1),
            }),
            &table,
            vec![],
        ),
        (
            "source line",
            point(Span::Begin, Arrow::Source, object(2)),
            &table,
            vec![],
        ),
        (
            "target arrow",
            point(Span::Begin, Arrow::HostileTarget, object(2)),
            &table,
            vec![],
        ),
        (
            "stack arrow",
            point(Span::Begin, Arrow::HostileTarget, object(2)),
            &stack,
            vec![],
        ),
        (
            "block line",
            point(Span::Begin, Arrow::Block, object(2)),
            &table,
            vec![],
        ),
        (
            "attack line",
            point(Span::Begin, Arrow::Attack, player(them)),
            &table,
            vec![],
        ),
        (
            "end",
            point(Span::End, Arrow::HostileTarget, object(2)),
            &table,
            vec![],
        ),
        (
            "source line end",
            point(Span::End, Arrow::Source, object(2)),
            &table,
            vec![],
        ),
        (
            "eligible",
            mark(Span::Begin, Mark::Eligible),
            &table,
            vec![],
        ),
        (
            "selected",
            mark(Span::Begin, Mark::Selected),
            &table,
            vec![],
        ),
        (
            "target",
            mark(Span::Begin, Mark::FriendlyTarget),
            &table,
            vec![],
        ),
        ("paying", mark(Span::Begin, Mark::Paying), &table, vec![]),
        ("end", mark(Span::End, Mark::Eligible), &table, vec![]),
    ];
    let mut found = BTreeSet::new();
    for (case, act, table, others) in &cases {
        let mut beside = vec![act];
        beside.extend(others.iter().copied());
        assert_eq!(case_of(act, table, &beside), Some(*case), "{act:?}");
        found.insert(format!("{}/{case}", act.verb().name()));
    }
    let wanted: BTreeSet<String> = data_cases()
        .into_iter()
        .filter(|case| !case.starts_with("lift/"))
        .collect();
    assert_eq!(found, wanted);
}

/// A session's look at the top of a library is no case: it plays with the verb's values.
#[test]
fn a_look_at_a_library_is_no_case_and_lifts_each_card_in_turn() {
    let look = Act::Lift {
        span: Span::Begin,
        handling: CardHandling::Library {
            side: Side::Opponent,
            count: Some(3),
            scry: None,
        },
    };
    let table = Table::empty(20);
    assert_eq!(case_of(&look, &table, &[&look]), None);
    let timing = timing_of(&look, &table, None, &[&look]);
    let each = timing.each.unwrap();
    assert!((timing.duration - (each + 2.0 * timing.stagger)).abs() < 1e-9);
}

/// One timing for each step, in playing order, whatever the beats: the second beat's tap is
/// timed as the second step and the third beat's count as the last.
#[test]
fn a_script_of_three_beats_has_one_timing_per_step() {
    let step = |act| Step { act, cause: None };
    let mut lands = card(1, ZoneName::Battlefield, Side::You);
    let script = Script {
        reset: None,
        beats: vec![
            vec![step(fly(ZoneName::Hand, Why::Play, lands.clone()))],
            vec![step(Act::Tap {
                object: {
                    lands.tapped = true;
                    lands.clone()
                },
            })],
            vec![
                step(Act::Count {
                    tally: Tally::Life {
                        player: Side::You,
                        life: 18,
                    },
                }),
                step(Act::Count {
                    tally: Tally::Life {
                        player: Side::You,
                        life: 18,
                    },
                }),
            ],
        ],
    };
    let timings = time_script(&script, &Table::empty(20));
    let cases: Vec<_> = timings.iter().map(|t| t.case.as_deref()).collect();
    // The second count finds the table the first left: the life total no longer changes.
    assert_eq!(
        cases,
        [Some("land played"), Some("tap"), Some("life loss"), None]
    );
}
