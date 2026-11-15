use std::collections::HashSet;

use beryl_inspect::stress::stress;
use beryl_protocol::script::{Table, failure};

const BOARDS: [(&str, usize); 2] = [("stress-255", 255), ("stress-1024", 1024)];

#[test]
fn two_calls_are_equal() {
    for (name, _) in BOARDS {
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
