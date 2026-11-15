//! The frames `rank` picks for the instruction-count gate, and a playback's shape.

use beryl_inspect::gate::{BOARDS, Phase, REST_FRAMES, play, rank};

#[test]
fn rank_picks_the_frames_with_the_most_work() {
    // Every frame of a flight writes each card and evaluates each card's segment, and the rest
    // frames before it draw only the cards the rows show, so the first frames of the flight
    // rank highest, the earliest of equals first.
    let first: [usize; 8] = std::array::from_fn(|i| REST_FRAMES + i);
    assert_eq!(rank("stress-255"), Some(first));
    assert_eq!(rank("stress-1024"), Some(first));
    assert_eq!(rank("no-such-board"), None);
}

#[test]
fn a_board_rests_then_flies_until_its_script_ends() {
    for name in BOARDS {
        let mut phases = Vec::new();
        assert!(play(name, |moment, frame| {
            phases.push(moment.phase);
            if moment.phase == Phase::Rest {
                assert_eq!(frame.work.segments, 0, "{name} frame {}", moment.frame);
            }
        }));
        assert_eq!(phases[..REST_FRAMES], [Phase::Rest; REST_FRAMES], "{name}");
        // The 40 flights take 39 staggers of 30 ms and one flight of 420 ms: 1,590 ms, or 96
        // frames at 60 a second, and the frame that shows the board at rest after them.
        assert_eq!(phases.len() - REST_FRAMES, 97, "{name}");
    }
}
