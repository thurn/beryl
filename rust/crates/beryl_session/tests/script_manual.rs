use beryl_session::protocol::{
    Counter, CounterView, ManaPool, NamedCounter, ObjectId, ObjectView, PlayerState, PlayerView,
    PowerToughness, Side, ZoneName,
};
use beryl_session::script::{Act, Notice, Reset, Script, Step, Table, Tally, failure, replay};

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
        };
        after.script = Some(script.clone());
        let decoded = round_trip(&after);
        assert_eq!(decoded.script.as_ref(), Some(&script));
        assert_eq!(failure(&Table::of(&before), &script, &decoded), None);
    }
}
