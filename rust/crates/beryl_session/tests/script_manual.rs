use std::collections::BTreeMap;

use beryl_session::protocol::{
    AssistsView, Counter, CounterView, FaceName, ManaPool, ManualOrders, ManualPiles,
    MarkerPreview, NamedCounter, ObjectId, ObjectView, PlayerState, PlayerView, PowerToughness,
    Side, StackItem, TablePhase, TableView, Why, ZoneName,
};
use beryl_session::script::{
    Act, Notice, Reset, Script, Step, Subject, Table, Tally, failure, replay,
};

fn round_trip(view: &PlayerView) -> PlayerView {
    serde_json::from_value(serde_json::to_value(view).unwrap()).unwrap()
}

fn named(name: &str, count: i32) -> NamedCounter {
    NamedCounter {
        name: name.to_owned(),
        count,
    }
}

#[test]
fn manual_numbers_replay_through_each_count() {
    let slot = ObjectId(1);
    let mut object = ObjectView::hidden(slot, ZoneName::Battlefield, Side::You, Side::You);
    let mut before = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
    before.battlefield.push(slot);
    before.objects.push(object.clone());
    let mut after = before.clone();
    let you = &mut after.players.you;
    you.life = 40_000;
    you.mana = ManaPool {
        blue: 2_000_000,
        ..ManaPool::default()
    };
    you.named_counters = vec![named("Energy", i32::MAX)];
    let mut counts = vec![
        Tally::Life {
            player: Side::You,
            life: you.life,
        },
        Tally::Pool {
            player: Side::You,
            mana: you.mana,
        },
        Tally::PlayerCounters {
            player: Side::You,
            counters: you.named_counters.clone(),
        },
    ];
    object.pt = Some(PowerToughness {
        power: 200,
        toughness: -200,
    });
    counts.push(Tally::Stats {
        object: object.clone(),
    });
    object.damage = 300;
    counts.push(Tally::Damage {
        object: object.clone(),
    });
    object.counters.push(CounterView {
        kind: Counter::PlusOne,
        count: 999,
    });
    counts.push(Tally::Counters {
        counter: Counter::PlusOne,
        object: object.clone(),
    });
    object.named_counters.push(named("Page", 999));
    counts.push(Tally::NamedCounters {
        name: "Page".to_owned(),
        object: object.clone(),
    });
    after.objects = vec![object];
    after.script = Some(Script {
        reset: None,
        beats: counts
            .into_iter()
            .map(|tally| {
                vec![Step {
                    act: Act::Count { tally },
                    cause: None,
                }]
            })
            .collect(),
        timing: Vec::new(),
    });

    let decoded = round_trip(&after);
    assert_eq!(decoded, after);
    assert_eq!(decoded.script, after.script);
    let start = Table::of(&before);
    let mut script = decoded.script.clone().unwrap();
    assert!(
        replay(start.clone(), &script)
            .unwrap()
            .shows(&Table::of(&after))
    );
    script.beats.remove(2);
    let mismatch = failure(&start, &script, &decoded).unwrap();
    assert!(mismatch.contains("table.you.named_counters"), "{mismatch}");
}

#[test]
fn manual_notices_round_trip_and_replay_without_a_card_source() {
    let before = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
    for notice in [
        Notice::Roll {
            sides: 6,
            outcomes: vec![3],
        },
        Notice::Shuffle,
        Notice::Rewind { to: 41 },
    ] {
        let mut after = before.clone();
        let reset = matches!(notice, Notice::Rewind { .. }).then(Reset::default);
        if reset.is_some() {
            after.players.you.life = 10;
        }
        let script = Script {
            reset,
            beats: vec![vec![Step {
                act: Act::Announce { notice },
                cause: None,
            }]],
            timing: Vec::new(),
        };
        after.script = Some(script.clone());
        let decoded = round_trip(&after);
        assert_eq!(decoded.script.as_ref(), Some(&script));
        assert_eq!(failure(&Table::of(&before), &script, &decoded), None);
    }
}

fn ordered_card(slot: u16, zone: ZoneName) -> ObjectView {
    ObjectView::hidden(ObjectId(slot), zone, Side::You, Side::You)
}

fn slots(ids: &[u16]) -> Vec<ObjectId> {
    ids.iter().copied().map(ObjectId).collect()
}

fn manual_view_with_ordered_zones() -> PlayerView {
    let mut before = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
    before.objects = vec![
        ordered_card(1, ZoneName::Hand),
        ordered_card(2, ZoneName::Hand),
        ordered_card(3, ZoneName::Hand),
        ordered_card(4, ZoneName::Exile),
        ordered_card(5, ZoneName::Sideboard),
    ];
    before.table = Some(TableView {
        log: vec![],
        proposals: vec![],
        labels: vec![],
        assists: AssistsView::default(),
        phase: TablePhase::FirstMain,
        seats: 2,
        seat: None,
        orders: ManualOrders {
            you: ManualPiles {
                hand: slots(&[1, 2, 3]),
                exile: slots(&[4]),
                sideboard: slots(&[5]),
            },
            opponent: ManualPiles::default(),
        },
        arrows: vec![],
        acting: vec![],
        lent_hand: false,
        object_actions: vec![],
        card_commands: BTreeMap::new(),
        top_revealed: false,
        top_known: false,
        manual_stats: vec![],
        last_draw: None,
        deck_tokens: Vec::new(),
        prompt: None,
    });

    before
}

#[test]
fn manual_order_replays_without_renumbering_slots() {
    let before = manual_view_with_ordered_zones();
    let mut moved_exile = ordered_card(4, ZoneName::Exile);
    moved_exile.zone_side = Some(Side::Opponent);
    let moved_sideboard = ordered_card(5, ZoneName::Exile);
    let sideboard = ordered_card(7, ZoneName::Sideboard);
    let acts = [
        Act::Fly {
            from: ZoneName::Hand,
            why: Why::Effect,
            object: ordered_card(3, ZoneName::Hand),
            at: Some(0),
            item: None,
            origin: None,
        },
        Act::Appear {
            from: ZoneName::Nowhere,
            why: Why::Created,
            object: ordered_card(6, ZoneName::Exile),
            at: Some(0),
            item: None,
        },
        Act::Fly {
            from: ZoneName::Exile,
            why: Why::Effect,
            object: moved_exile.clone(),
            at: None,
            item: None,
            origin: None,
        },
        Act::Appear {
            from: ZoneName::Nowhere,
            why: Why::Created,
            object: sideboard.clone(),
            at: Some(0),
            item: None,
        },
        Act::Vanish {
            subject: Subject::Object { slot: ObjectId(5) },
            from: ZoneName::Sideboard,
            why: Why::Exile,
            object: Some(moved_sideboard.clone()),
            at: Some(0),
        },
        Act::Vanish {
            subject: Subject::Object { slot: ObjectId(6) },
            from: ZoneName::Exile,
            why: Why::Effect,
            object: None,
            at: None,
        },
    ];

    let mut after = before.clone();
    let orders = &mut after.table.as_mut().unwrap().orders;
    orders.you.hand = slots(&[3, 1, 2]);
    orders.you.exile = slots(&[5]);
    orders.you.sideboard = slots(&[7]);
    orders.opponent.exile = slots(&[4]);
    after.objects.truncate(3);
    after
        .objects
        .extend([moved_exile, moved_sideboard, sideboard]);
    after.script = Some(Script {
        reset: None,
        beats: acts
            .into_iter()
            .map(|act| vec![Step { act, cause: None }])
            .collect(),
        timing: Vec::new(),
    });

    let decoded = round_trip(&after);
    assert_eq!(decoded, after);
    assert_eq!(decoded.script, after.script);
    let mut script = decoded.script.clone().unwrap();
    let start = Table::of(&before);
    let reordered = replay(
        start.clone(),
        &Script {
            reset: None,
            beats: script.beats[..1].to_vec(),
            timing: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(
        reordered.order(ZoneName::Hand, Side::You),
        Some(&slots(&[3, 1, 2]))
    );
    assert_eq!(reordered.objects, start.objects);
    assert_eq!(failure(&start, &script, &decoded), None);

    script.beats.remove(0);
    let mismatch = failure(&start, &script, &decoded).unwrap();
    assert!(mismatch.contains("table.you.manual.hand"), "{mismatch}");
}

#[test]
fn manual_marker_replay_removes_only_the_named_marker_after_reset() {
    // The manual id is wider than the engine's object slot and survives a wire round trip.
    let (first, second) = (65_537, 65_538);
    let marker = |id| {
        StackItem::ability(
            id,
            None,
            Some(MarkerPreview {
                name: "Borrowed Token".into(),
                face: FaceName::Back,
                text: "Ability".into(),
            }),
            Side::Opponent,
        )
    };
    let mut view = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
    view.stack = vec![marker(first), marker(second)];
    let wire = serde_json::to_value(&view).unwrap();
    assert_eq!(wire["stack"][1]["controller"], "opponent");
    assert_eq!(wire["stack"][1]["source"], serde_json::Value::Null);
    assert_eq!(wire["stack"][1]["manual"]["face"], "back");
    assert_eq!(wire["stack"][1]["text"], "Ability");

    let before = Table::of(&round_trip(&view));
    let mut reset = Script {
        reset: Some(Reset {
            abilities: vec![first, second],
        }),
        beats: vec![],
        timing: Vec::new(),
    };
    assert_eq!(failure(&before, &reset, &view), None);
    reset.reset.as_mut().unwrap().abilities.swap(0, 1);
    assert!(
        failure(&before, &reset, &view)
            .unwrap()
            .contains("numbers marker")
    );

    let vanish = |id| Script {
        reset: None,
        beats: vec![vec![Step {
            act: Act::Vanish {
                subject: Subject::Ability { id },
                from: ZoneName::Stack,
                why: Why::Resolve,
                object: None,
                at: None,
            },
            cause: None,
        }]],
        timing: Vec::new(),
    };
    view.stack.pop();
    assert_eq!(failure(&before, &vanish(second), &view), None);
    assert!(failure(&before, &vanish(first), &view).is_some());
    let after = replay(before, &vanish(second)).unwrap();
    assert_eq!(after.stack[0].subject(), Subject::Ability { id: first });

    let hidden = StackItem::ability(first, Some(ObjectId(1)), None, Side::Opponent);
    let wire = serde_json::to_value(&hidden).unwrap();
    assert_eq!(wire["manual"]["id"], first);
    assert_eq!(wire["manual"]["face"], serde_json::Value::Null);
    assert_eq!(wire["source"], serde_json::Value::Null);
    assert_eq!(wire["name"], "");
    assert_eq!(wire["text"], "");
    assert_eq!(wire["controller"], "opponent");
}
