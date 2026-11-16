use std::collections::HashSet;

use beryl_inspect::stress::stress;
use beryl_protocol::script::{Table, failure};

const BOARDS: [(&str, usize); 2] = [("stress-255", 255), ("stress-1024", 1024)];

#[test]
fn two_calls_are_equal() {
    for name in beryl_inspect::stress::NAMES {
        assert_eq!(stress(name), stress(name), "{name}");
    }
}

#[test]
fn unknown_names_have_no_board() {
    assert!(stress("stress-1").is_none());
}

#[test]
fn each_board_has_its_objects_with_names_of_their_own() {
    for (name, count) in BOARDS {
        let board = stress(name).unwrap();
        for view in &board.views {
            assert_eq!(view.objects.len(), count, "{name}");
            assert_eq!(view.battlefield.len(), count, "{name}");
            let names: HashSet<_> = view.objects.iter().map(|object| &object.name).collect();
            assert_eq!(names.len(), count, "{name}");
        }
    }
}

#[test]
fn the_second_view_moves_forty_permanents_with_a_timing_each() {
    for (name, _) in BOARDS {
        let board = stress(name).unwrap();
        assert_eq!(board.views.len(), 2);
        assert!(board.views[0].script.as_ref().unwrap().reset.is_some());
        let script = board.views[1].script.as_ref().unwrap();
        assert_eq!(script.beats.len(), 1);
        assert_eq!(script.beats[0].len(), 40, "{name}");
        assert_eq!(board.timings.len(), 1);
        assert_eq!(board.timings[0].len(), 40, "{name}");
    }
}

#[test]
fn the_script_replays_to_the_second_view() {
    for (name, _) in BOARDS {
        let board = stress(name).unwrap();
        let script = board.views[1].script.as_ref().unwrap();
        let start = Table::of(&board.views[0]);
        assert_eq!(failure(&start, script, &board.views[1]), None, "{name}");
    }
}

#[test]
fn the_board_is_mixed() {
    let board = stress("stress-1024").unwrap();
    let objects = &board.views[0].objects;
    let tapped = objects.iter().filter(|object| object.tapped).count();
    let creatures = objects.iter().filter(|object| object.pt.is_some()).count();
    let yours = objects
        .iter()
        .filter(|object| object.controller == beryl_protocol::protocol::Side::You)
        .count();
    assert_eq!(creatures, 512);
    assert_eq!(yours, 512);
    assert!(tapped.abs_diff(1024 / 3) <= 1, "{tapped}");
}

#[test]
fn verbs_play_in_one_beat_with_live_timings() {
    use beryl_protocol::protocol::Why;
    use beryl_protocol::script::{Act, Verb, time_script};
    use beryl_scene::agent::Rule;
    use beryl_scene::driver::Mode;
    let board = stress("stress-verbs").unwrap();
    let [before, after] = board.views.as_slice() else {
        panic!("one beat follows the reset");
    };
    assert_eq!(before.objects.len(), 100);
    assert_eq!(after.objects.len(), 100);
    let script = after.script.as_ref().unwrap();
    let [beat] = script.beats.as_slice() else {
        panic!("one beat");
    };
    let expected = [30, 10, 10, 5, 5, 10, 5, 5, 5, 5, 3, 1, 1, 5, 5, 1];
    for (&verb, count) in Verb::ALL.iter().zip(expected) {
        assert_eq!(
            beat.iter().filter(|step| step.act.verb() == verb).count(),
            count,
            "{verb:?}"
        );
    }
    assert_eq!(
        beat.iter()
            .filter(|step| matches!(step.act, Act::Fly { why: Why::Draw, .. }))
            .count(),
        10,
    );
    let table = Table::of(before);
    for (timing, live) in script.timing.iter().zip(time_script(script, &table)) {
        assert_eq!(timing.case, live.case);
        assert_eq!(timing.duration.to_bits(), live.duration.to_bits());
        assert_eq!(timing.curve, live.curve);
        assert_eq!(timing.phases, live.phases);
        assert_eq!(timing.effects, live.effects);
    }
    assert_eq!(failure(&table, script, after), None);
    let mut page = beryl_inspect::pages::page(
        "stress-verbs",
        beryl_protocol::protocol::Side::You,
        beryl_scene::agent::Layout::Desktop,
        beryl_inspect::pages::DESKTOP_WINDOWS[0],
    )
    .unwrap();
    let steps = page.timeline();
    assert_eq!(steps.len(), beat.len());
    assert!(steps.iter().all(|step| step.from == steps[0].from));
    for step in steps.iter().filter(|step| step.verb == Verb::Streak) {
        assert_eq!(step.to - step.from, 250_000_000);
    }
    let faults: Vec<_> = page
        .check_all(Mode::AtCheckTimes)
        .faults
        .into_iter()
        .filter(|fault| matches!(fault.rule, Rule::Jump | Rule::RestPlacement))
        .collect();
    assert!(faults.is_empty(), "{faults:?}");
}

#[test]
fn the_pointer_crosses_the_hand_and_back() {
    use beryl_inspect::gate;
    use beryl_protocol::protocol::{Side, ZoneName};
    use beryl_scene::frame::{Place, Role, StateBits};

    let mut first = None;
    let mut middle = None;
    let mut last = None;
    let mut distinct = HashSet::new();
    assert!(gate::play("stress-hand", |moment, frame| {
        let hand: Vec<_> = frame
            .resolved
            .iter()
            .filter(|element| {
                element.role == Role::Card
                    && matches!(
                        element.place,
                        Place::Card {
                            zone: ZoneName::Hand,
                            side: Side::You,
                            ..
                        }
                    )
            })
            .collect();
        assert_eq!(hand.len(), 60);
        let hovered: Vec<_> = hand
            .iter()
            .filter(|element| element.state.has(StateBits::HOVERED))
            .collect();
        if moment.frame == 0 {
            assert!(hovered.is_empty());
        } else {
            assert_eq!(hovered.len(), 1, "frame {}", moment.frame);
            let slot = hovered[0].id.0;
            distinct.insert(slot);
            match moment.frame {
                1 => first = Some(slot),
                120 => middle = Some(slot),
                240 => last = Some(slot),
                _ => {}
            }
        }
    }));
    assert!(distinct.len() > 30, "{} cards hovered", distinct.len());
    assert_ne!(first, middle);
    assert_eq!(first, last);
}

#[test]
fn fifty_arrows_hold_for_two_seconds_then_the_top_ability_resolves() {
    use beryl_inspect::gate::{self, Phase};
    use beryl_scene::frame::Role;

    let mut at_rest = 0;
    let mut final_counts = (0, 0);
    assert!(gate::play("stress-stack", |moment, frame| {
        let arrows = frame
            .resolved
            .iter()
            .filter(|element| element.role == Role::Arrow)
            .count();
        let abilities = frame
            .resolved
            .iter()
            .filter(|element| element.role == Role::Ability)
            .count();
        if arrows == 50 && moment.phase == Phase::Rest {
            assert_eq!(abilities, 25);
            at_rest += 1;
        }
        final_counts = (arrows, abilities);
    }));
    assert_eq!(
        at_rest, 119,
        "the first still frame ends the preceding script"
    );
    assert_eq!(final_counts, (49, 24));
}

/// Every frame of the board that plays every verb at once is a function of its time: drawn
/// after stepping every tick to a moment, or straight at that moment with every frame before
/// it skipped, it is the same frame. Only the held card's spring, the open spans, the running
/// effects and notices and the scroll offsets may be kept from one frame to the next, and each
/// of those must still give the same picture however many frames came before.
#[test]
fn a_frame_of_every_verb_is_the_same_however_many_frames_came_before() {
    use beryl_scene::driver::{Duration, Page};
    use beryl_scene::frame::Frame;
    let open = || -> Page {
        beryl_inspect::pages::page(
            "stress-verbs",
            beryl_protocol::protocol::Side::You,
            beryl_scene::agent::Layout::Desktop,
            beryl_inspect::pages::DESKTOP_WINDOWS[0],
        )
        .unwrap()
    };
    let mut every = open();
    let start = every.now();
    // Every 35 ms through the script and a little past its end. 35 ms falls on no tick, so
    // the skipping page draws times the ticking one only reaches as the end of a step.
    let shown = |frame: &Frame| {
        (
            frame.draws.clone(),
            frame.resolved.clone(),
            frame.names.clone(),
            frame.shake,
            frame.opaque,
            frame.layer,
        )
    };
    let mut changes = 0;
    let mut previous = None;
    for at in (1..=60).map(|n| n * 35_000_000) {
        every.step(Duration(start + at - every.now()));
        let mut skipping = open();
        skipping.set_tick(Duration(at));
        skipping.step(Duration(at));
        assert_eq!(every.now(), skipping.now());
        let (a, b) = (shown(every.frame()), shown(skipping.frame()));
        if a != b {
            let first =
                a.0.iter()
                    .zip(&b.0)
                    .position(|(x, y)| x != y)
                    .map(|i| format!("draw {i}: {:?} vs {:?}", a.0[i], b.0[i]));
            panic!(
                "at {} ms the frame drawn every tick differs from the one drawn once: {} draws vs {}, {} elements vs {}, shake {:?} vs {:?}; first differing {first:?}",
                at / 1_000_000,
                a.0.len(),
                b.0.len(),
                a.1.len(),
                b.1.len(),
                a.3,
                b.3,
            );
        }
        if previous.as_ref().is_some_and(|previous| *previous != a.0) {
            changes += 1;
        }
        previous = Some(a.0);
    }
    assert!(
        changes > 30,
        "the verbs move on most of the sampled frames: {changes}"
    );
}
