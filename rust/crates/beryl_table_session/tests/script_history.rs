use std::collections::{BTreeMap, BTreeSet};

use beryl_session::protocol::{Why, ZoneName};
use beryl_session::script::{Act, Mark, Notice, Script, Span, Verb};
use beryl_table::action::{Destination, Position};
use beryl_table::state::{Assist, Card, Face, Line, LineVisibility, ObjectId, Proposal, Zone};
use beryl_table::testing::{ASSIST_AND_LOG, CORE, Generator, Kind};
use beryl_table::{Action, Refusal, Table};
use beryl_table_session::script::proposal_marks;

mod script_common;
use script_common::{FIELD, GUEST, Game, HOST, add, select};

const LIBRARY: Zone = Zone::Library(HOST);
const HAND: Zone = Zone::Hand(HOST);

fn acts(script: &Script) -> impl Iterator<Item = &Act> {
    script.beats.iter().flatten().map(|step| &step.act)
}

#[test]
fn a_nested_batch_plays_its_member_verbs_and_undo_reverses_the_draw() {
    let mut table = Table::new(2);
    add(&mut table, 1, "<private face>", FIELD, &[]);
    table.objects.get_mut(&ObjectId(1)).unwrap().face = Face::Down;
    add(&mut table, 2, "<drawn card>", LIBRARY, &[]);
    let mut game = Game::new(table);
    let batch = Action::Batch {
        actions: vec![
            Action::Peek {
                selection: select(FIELD, &[1]),
            },
            Action::Batch {
                actions: vec![
                    Action::Draw {
                        seat: HOST,
                        count: 1,
                    },
                    Action::Tap {
                        selection: select(FIELD, &[1]),
                        tapped: true,
                    },
                ],
            },
        ],
    };
    for script in game.apply(HOST, &batch) {
        assert!(script.reset.is_none());
        assert_eq!(
            acts(&script)
                .filter(|act| act.verb() != Verb::Set)
                .map(Act::verb)
                .collect::<Vec<_>>(),
            [Verb::Flip, Verb::Fly, Verb::Tap],
        );
        assert!(acts(&script).any(|act| matches!(act, Act::Fly { why: Why::Draw, .. })));
    }
    for script in game.apply(GUEST, &Action::Undo { entry: Some(1) }) {
        assert!(script.reset.is_none());
        assert_eq!(
            acts(&script)
                .filter(|act| act.verb() != Verb::Set)
                .map(Act::verb)
                .collect::<Vec<_>>(),
            [Verb::Tap, Verb::Fly],
        );
        assert!(acts(&script).any(|act| matches!(act, Act::Fly { from: ZoneName::Hand, object, .. } if object.zone == ZoneName::Library)));
    }
}

#[test]
fn a_batched_mulligan_keeps_its_draw_motion_and_undo_restores_the_hand() {
    let mut table = Table::new(2);
    add(&mut table, 1, "<hand one>", HAND, &[HOST]);
    add(&mut table, 2, "<hand two>", HAND, &[HOST]);
    add(&mut table, 3, "<library one>", LIBRARY, &[]);
    add(&mut table, 4, "<library two>", LIBRARY, &[]);
    let mut game = Game::new(table);
    let batch = Action::Batch {
        actions: vec![Action::Mulligan {
            seat: HOST,
            count: 2,
        }],
    };
    for script in game.apply(HOST, &batch) {
        assert!(script.reset.is_none());
        let flights = acts(&script)
            .filter_map(|act| match act {
                Act::Fly { from, why, .. } => Some((*from, *why)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            flights,
            [
                (ZoneName::Hand, Why::Effect),
                (ZoneName::Hand, Why::Effect),
                (ZoneName::Library, Why::Draw),
                (ZoneName::Library, Why::Draw),
            ]
        );
        assert!(!acts(&script).any(|act| act.verb() == Verb::Announce));
    }
    for script in game.apply(HOST, &Action::Undo { entry: None }) {
        assert!(script.reset.is_none());
        assert_eq!(
            acts(&script).filter(|act| act.verb() == Verb::Fly).count(),
            4
        );
    }
    assert_eq!(game.table.zone(HAND).unwrap(), [ObjectId(1), ObjectId(2)]);
}

#[test]
fn game_and_rewind_resets_have_one_banner() {
    let mut table = Table::new(2);
    table.game_match.seats[0].deck = vec![("Island".into(), 8)];
    table.game_match.seats[1].deck = vec![("Plains".into(), 8)];
    let mut game = Game::new(table);
    for (action, expected) in [
        (Action::Start { first: HOST }, Notice::Game { number: 1 }),
        (
            Action::NextGame { winner: None },
            Notice::Game { number: 2 },
        ),
        (Action::Start { first: GUEST }, Notice::Game { number: 2 }),
        (Action::Rewind { to: 1 }, Notice::Rewind { to: 1 }),
        (Action::Undo { entry: None }, Notice::Rewind { to: 3 }),
    ] {
        for script in game.apply(HOST, &action) {
            assert!(script.reset.is_some());
            assert_eq!(
                acts(&script).collect::<Vec<_>>(),
                [&Act::Announce {
                    notice: expected.clone()
                }]
            );
        }
    }
    for script in game.apply(HOST, &Action::Concede {}) {
        assert!(script.beats.is_empty());
    }
}

#[test]
fn proposal_marks_deduplicate_and_end_before_apply_or_on_withdrawal() {
    let mut table = Table::new(2);
    add(&mut table, 1, "Attacker", FIELD, &[HOST, GUEST]);
    add(&mut table, 2, "Blocker", FIELD, &[HOST, GUEST]);
    let mut game = Game::new(table);
    let proposal = Proposal {
        assist: Assist::Combat,
        lines: [
            (
                true,
                Action::Tap {
                    selection: select(FIELD, &[1]),
                    tapped: true,
                },
            ),
            (
                true,
                Action::Move {
                    selection: select(FIELD, &[1]),
                    to: Destination::Zone(HAND),
                    position: Position::Top,
                    face: None,
                    tapped: None,
                },
            ),
            (
                false,
                Action::Tap {
                    selection: select(FIELD, &[2]),
                    tapped: true,
                },
            ),
        ]
        .into_iter()
        .map(|(on, action)| Line {
            text: "Combat".into(),
            visibility: LineVisibility::Public,
            whose: HOST,
            on,
            actions: vec![action],
        })
        .collect(),
    };
    let marks = game.views.each_ref().map(|(view, slots)| {
        let marks = proposal_marks(std::slice::from_ref(&proposal), view, slots);
        assert_eq!(marks, [slots.get(ObjectId(1)).unwrap()]);
        marks
    });
    let quiet = Action::SetAssist {
        assist: Assist::Combat,
        on: true,
    };
    for _ in 0..2 {
        for script in game.try_apply_marked(HOST, &quiet, marks.clone()).unwrap() {
            assert!(matches!(
                acts(&script).collect::<Vec<_>>()[..],
                [Act::Mark {
                    mark: Mark::Suggested,
                    span: Span::Begin,
                    ..
                }]
            ));
        }
        for script in game.try_apply_marked(HOST, &quiet, marks.clone()).unwrap() {
            assert!(script.beats.is_empty());
        }
        for script in game.apply(HOST, &quiet) {
            assert!(matches!(
                acts(&script).collect::<Vec<_>>()[..],
                [Act::Mark {
                    mark: Mark::Suggested,
                    span: Span::End,
                    ..
                }]
            ));
        }
    }
    game.try_apply_marked(HOST, &quiet, marks).unwrap();
    for script in game.apply(HOST, &beryl_table::assist::apply_proposal(&proposal)) {
        assert!(matches!(
            script.beats[0][0].act,
            Act::Mark {
                mark: Mark::Suggested,
                span: Span::End,
                ..
            }
        ));
        assert!(acts(&script).any(|act| act.verb() == Verb::Fly));
    }
}

#[test]
fn every_script_replays_and_names_nothing_hidden() {
    let kinds: Vec<_> = CORE.iter().chain(ASSIST_AND_LOG).copied().collect();
    let mut generator = Generator::new(227);
    let mut covered = BTreeSet::new();
    let mut resets = BTreeMap::new();
    // Fresh logs bound the cost of rebuilding the history for both viewers.
    for round in 0..12 {
        let mut table = Table::new(2);
        if round % 2 == 0 {
            for (seat, loaded) in table.game_match.seats.iter_mut().enumerate() {
                loaded.deck = (0..12)
                    .map(|card| (format!("<deck-{seat}-{card}>"), 1))
                    .collect();
            }
        }
        add(&mut table, 1, "<host permanent>", FIELD, &[HOST, GUEST]);
        add(&mut table, 2, "<host library card>", LIBRARY, &[]);
        let mut game = Game::new(table);
        for turn in 0..200 {
            let (kind, by, action) = if turn % 19 == 0 {
                (None, HOST, Action::CloseLook {})
            } else {
                let (kind, by, action) = generator.next(&game.table, &kinds);
                (Some(kind), by, action)
            };
            let scripts = match game.try_apply(by, &action) {
                Ok(scripts) => scripts,
                // Flow is reserved for the host-held library prompt. It
                // produces no table entry or script through this entry point.
                Err(error) if kind == Some(Kind::Flow) => {
                    assert_eq!(error, Refusal::INVALID);
                    covered.insert(Kind::Flow);
                    continue;
                }
                Err(_) => continue,
            };
            covered.extend(kind);
            let entries = game.log.entries();
            let entry = entries.last().unwrap();
            let undone = entry.undoes.map(|target| {
                &entries
                    .iter()
                    .find(|entry| entry.n == target)
                    .unwrap()
                    .action
            });
            if let Some(reason) = check_resets(&action, undone, &scripts) {
                *resets.entry((undone.is_some(), reason)).or_insert(0) += scripts.len();
            }
            for seat in game.table.seats() {
                let index = usize::from(seat.0);
                let (view, slots) = &game.views[index];
                let json = serde_json::to_string(&(view, &scripts[index])).unwrap();
                let visible: BTreeSet<_> = view
                    .objects
                    .iter()
                    .filter_map(|record| record.name.as_deref())
                    .collect();
                for object in game
                    .table
                    .objects
                    .values()
                    .filter(|object| !object.known_to.contains(&seat))
                {
                    let name = match &object.card {
                        Card::Named(name) | Card::Custom { name, .. } => name,
                        Card::Hidden => unreachable!("host card"),
                    };
                    let slot = slots.get(object.id).unwrap();
                    assert!(
                        view.objects
                            .iter()
                            .any(|record| record.slot == slot && record.name.is_none())
                    );
                    assert!(
                        acts(&scripts[index]).all(|act| {
                            act.object()
                                .is_none_or(|record| record.slot != slot || record.name.is_none())
                        }),
                        "{action:?} named hidden slot {slot:?}"
                    );
                    assert!(
                        visible.contains(name.as_str()) || !json.contains(name),
                        "{action:?} leaked {name} to {seat:?}"
                    );
                }
            }
        }
    }
    let missing: Vec<_> = kinds
        .into_iter()
        .filter(|kind| !covered.contains(kind))
        .collect();
    assert!(missing.is_empty(), "seed missed {missing:?}");
    eprintln!("Reset scripts by (is undo, action): {resets:?}");
}

fn check_resets(
    action: &Action,
    undone: Option<&Action>,
    scripts: &[Script; 2],
) -> Option<&'static str> {
    let reason = match undone.unwrap_or(action) {
        Action::Start { .. } => Some("start"),
        Action::NextGame { .. } => Some("next game"),
        Action::Rewind { .. } => Some("rewind"),
        Action::Concede {} => Some("concede"),
        _ => None,
    };
    for script in scripts {
        assert_eq!(
            script.reset.is_some(),
            reason.is_some(),
            "{action:?}, undo of {undone:?}"
        );
        if let Some(reason) = reason {
            assert_eq!(
                acts(script)
                    .filter(|act| act.verb() == Verb::Announce)
                    .count(),
                usize::from(reason != "concede"),
                "{action:?}, undo of {undone:?}",
            );
        }
        if matches!(
            action,
            Action::Concede {}
                | Action::LoadDeck { .. }
                | Action::Sideboard { .. }
                | Action::Ready { .. }
                | Action::SetAssist { .. }
        ) {
            assert!(script.beats.is_empty(), "{action:?}");
        }
    }
    reason
}
